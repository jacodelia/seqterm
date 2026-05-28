//! CPAL audio backend — implements AudioBackendPort via CPAL.
//!
//! REALTIME CONTRACT: The CPAL callback is the innermost hot path.
//! It MUST only:
//!   - Read from `rtrb::Consumer<AudioCommand>` (lock-free)
//!   - Call `Mixer::mix()` (pre-allocated buffers only)
//!   - Write f32 samples to the output buffer
//! No allocation. No mutex. No blocking.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, anyhow};
use cpal::{
    Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rtrb::{Producer, RingBuffer};
use tracing::{info, error};

use seqterm_ports::{AudioBackendPort, AudioDeviceInfo, AudioEngineConfig, AudioSynthPort};

use crate::{
    events::{AudioCommand, AudioEngineEvent},
    mixer::Mixer,
    sf2_synth::SoundFontSynth,
    audio_clip::AudioClipPlayer,
    granular::engine::GranularEngine,
    assets::AssetCache,
    skip_back::SkipBackBuffer,
};

/// Number of commands the ring buffer can hold.
const CMD_RING_CAPACITY: usize = 1024;
/// Maximum audio block size (frames).
const MAX_BLOCK_FRAMES: usize = 4096;

/// Shared atomic stats readable from non-RT thread.
struct AudioStats {
    xrun_count:   AtomicU32,
    dsp_load_ppm: AtomicU32, // parts per million → / 10000.0 = percent
    is_running:   AtomicBool,
    /// Per-slot peak level (f32 bits, 0.0–1.0+).
    slot_peaks:   Box<[AtomicU32]>,
    /// Master output peak (f32 bits).
    master_peak:  AtomicU32,
}

impl AudioStats {
    fn new() -> Arc<Self> {
        use crate::mixer::MAX_SLOTS;
        let slot_peaks: Box<[AtomicU32]> = (0..MAX_SLOTS)
            .map(|_| AtomicU32::new(0))
            .collect();
        Arc::new(Self {
            xrun_count:   AtomicU32::new(0),
            dsp_load_ppm: AtomicU32::new(0),
            is_running:   AtomicBool::new(false),
            slot_peaks,
            master_peak:  AtomicU32::new(0),
        })
    }
}

/// CPAL-backed audio output stream.
///
/// Non-RT side: sends commands via `cmd_tx` (Producer).
/// RT side: audio callback drains `cmd_rx` (Consumer) each block.
pub struct CpalAudioBackend {
    config: AudioEngineConfig,
    stats: Arc<AudioStats>,
    /// Sender half of the command ring buffer (non-RT side).
    cmd_tx: Option<Producer<AudioCommand>>,
    /// Event channel back to the application layer.
    event_tx: flume::Sender<AudioEngineEvent>,
    /// Active CPAL stream (kept alive for the lifetime of playback).
    _stream: Option<Stream>,
    /// Asset cache for background SF2/audio loading (used by load_sf2_background).
    _asset_cache: AssetCache,
    /// Skip-back circular buffer — shared with the RT callback (try_write, no blocking).
    skip_back: Option<Arc<parking_lot::RwLock<SkipBackBuffer>>>,
}

impl CpalAudioBackend {
    pub fn new(event_tx: flume::Sender<AudioEngineEvent>) -> Self {
        Self {
            config: AudioEngineConfig::default(),
            stats: AudioStats::new(),
            cmd_tx: None,
            event_tx,
            _stream: None,
            _asset_cache: AssetCache::new(),
            skip_back: None,
        }
    }

    /// Access the skip-back buffer (available after `open()`).
    pub fn skip_back(&self) -> Option<Arc<parking_lot::RwLock<SkipBackBuffer>>> {
        self.skip_back.as_ref().map(Arc::clone)
    }

    /// Start capturing mixed audio output to a WAV file (non-RT side).
    pub fn start_capture(&mut self, path: PathBuf) -> Result<()> {
        let sample_rate = self.config.sample_rate;
        // 8 seconds of stereo f32 capture ring buffer.
        let cap_capacity = sample_rate as usize * 2 * 8;
        let (cap_tx, cap_rx) = rtrb::RingBuffer::<f32>::new(cap_capacity);
        let done = Arc::new(AtomicBool::new(false));
        let done_writer = Arc::clone(&done);
        let event_tx = self.event_tx.clone();
        let cap_path = path.clone();

        // Background WAV writer thread.
        std::thread::Builder::new()
            .name("seqterm-capture-writer".to_string())
            .spawn(move || {
                write_capture_wav(cap_rx, done_writer, cap_path, sample_rate, event_tx);
            })
            .expect("failed to spawn capture writer thread");

        let _ = self.event_tx.send(AudioEngineEvent::CaptureStarted(path));
        self.send_command(AudioCommand::StartCapture { capture_tx: cap_tx, done })
    }

    /// Stop capturing and finalize the WAV file.
    pub fn stop_capture(&mut self) -> Result<()> {
        self.send_command(AudioCommand::StopCapture)
    }

    /// Send a command to the audio callback thread (lock-free).
    pub fn send_command(&mut self, cmd: AudioCommand) -> Result<()> {
        match &mut self.cmd_tx {
            Some(tx) => {
                tx.push(cmd).map_err(|_| anyhow!("audio command ring buffer full"))
            }
            None => Err(anyhow!("audio stream not started")),
        }
    }

    /// Resolve SF2 load: this happens on the background asset thread.
    pub fn load_sf2_background(
        &self,
        slot_id: u32,
        path: std::path::PathBuf,
        bank: u8,
        preset: u8,
        cmd_tx_clone: Producer<AudioCommand>,
        event_tx: flume::Sender<AudioEngineEvent>,
    ) {
        // Spawn background thread — safe, not in audio callback.
        std::thread::spawn(move || {
            match SoundFontSynth::load(&path, bank, preset, 48000) {
                Ok(_synth) => {
                    let preset_name = format!("Bank:{bank} Preset:{preset}");
                    let _ = event_tx.send(AudioEngineEvent::Sf2Loaded {
                        slot_id,
                        preset_name,
                    });
                    // The synth is actually installed via InstallSf2 command (not in this fn).
                }
                Err(e) => {
                    let _ = event_tx.send(AudioEngineEvent::LoadFailed {
                        slot_id,
                        error: e.to_string(),
                    });
                }
            }
        });
        let _ = cmd_tx_clone; // moved in
    }
}

impl AudioBackendPort for CpalAudioBackend {
    fn open(&mut self, config: AudioEngineConfig) -> Result<()> {
        self.config = config.clone();

        // Select JACK host when explicitly requested and available.
        #[cfg(feature = "jack-backend")]
        let host = if config.use_jack {
            cpal::host_from_id(cpal::HostId::Jack)
                .unwrap_or_else(|_| cpal::default_host())
        } else {
            cpal::default_host()
        };
        #[cfg(not(feature = "jack-backend"))]
        let host = cpal::default_host();

        let device = match &config.output_device {
            Some(name) => host
                .output_devices()?
                .find(|d| d.name().map(|n| &n == name).unwrap_or(false))
                .ok_or_else(|| anyhow!("audio device '{}' not found", name))?,
            None => host
                .default_output_device()
                .ok_or_else(|| anyhow!("no default audio output device"))?,
        };

        let device_name = device.name().unwrap_or_else(|_| "unknown".to_string());
        info!("Opening audio device: {}", device_name);

        let _supported = device.default_output_config()?;
        let sample_rate = cpal::SampleRate(config.sample_rate);
        let buffer_size = cpal::BufferSize::Fixed(config.buffer_size);

        let stream_config = StreamConfig {
            channels: 2,
            sample_rate,
            buffer_size,
        };

        // Create the lock-free command ring buffer.
        let (cmd_tx, mut cmd_rx) = RingBuffer::<AudioCommand>::new(CMD_RING_CAPACITY);

        // Build mixer (pre-allocate all buffers now, before callback starts).
        let mut mixer = Mixer::new(MAX_BLOCK_FRAMES);

        // Skip-back buffer: 30 seconds of stereo at the configured sample rate.
        let sb = Arc::new(parking_lot::RwLock::new(
            SkipBackBuffer::new(30, config.sample_rate),
        ));
        self.skip_back = Some(Arc::clone(&sb));
        let skip_back_rt = sb;

        let stats = Arc::clone(&self.stats);
        let actual_sample_rate = config.sample_rate;
        let actual_buffer_size = config.buffer_size;

        // Capture state — managed by StartCapture / StopCapture commands.
        // `cap_tx`: lock-free ring producer; written from the RT callback.
        // `cap_done`: set true when StopCapture received so the writer thread can exit.
        let mut cap_tx: Option<rtrb::Producer<f32>> = None;
        let mut cap_done: Option<Arc<AtomicBool>> = None;

        // The audio callback — called by CPAL on the realtime thread.
        // REALTIME SAFE: no alloc, no mutex, no blocking.
        let callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let t0 = Instant::now();

            // Drain command ring (lock-free).
            while let Ok(cmd) = cmd_rx.pop() {
                match cmd {
                    AudioCommand::InstallSource { slot_id, source } => {
                        mixer.set_slot(slot_id as usize, source, 1.0);
                    }
                    AudioCommand::NoteOn { slot_id, channel, note, velocity } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            // Ensure the slot is active so the mixer renders it.
                            slot.active = true;
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(synth) = src.as_any_mut()
                                    .downcast_mut::<SoundFontSynth>()
                                {
                                    synth.note_on(channel, note, velocity);
                                }
                            }
                        }
                    }
                    AudioCommand::NoteOff { slot_id, channel, note } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(synth) = src.as_any_mut()
                                    .downcast_mut::<SoundFontSynth>()
                                {
                                    synth.note_off(channel, note);
                                }
                            }
                        }
                    }
                    AudioCommand::ControlChange { slot_id, channel, cc, value } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(synth) = src.as_any_mut()
                                    .downcast_mut::<SoundFontSynth>()
                                {
                                    synth.control_change(channel, cc, value);
                                }
                            }
                        }
                    }
                    AudioCommand::PlayAudioClip { slot_id } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(clip) = src.as_any_mut()
                                    .downcast_mut::<AudioClipPlayer>()
                                {
                                    clip.play();
                                }
                            }
                            slot.active = true;
                        }
                    }
                    AudioCommand::StopAudioClip { slot_id } => {
                        mixer.clear_slot(slot_id as usize);
                    }
                    AudioCommand::SetMasterVolume(v) => {
                        mixer.master_volume = v;
                    }
                    AudioCommand::SetSlotVolume { slot_id, volume } => {
                        mixer.set_slot_volume(slot_id as usize, volume);
                    }
                    AudioCommand::SetSlotSends { slot_id, send_a, send_b } => {
                        mixer.set_slot_sends(slot_id as usize, send_a, send_b);
                    }
                    AudioCommand::SetBusVolume { bus_idx, volume } => {
                        mixer.set_bus_volume(bus_idx, volume);
                    }
                    AudioCommand::SetBusMuted { bus_idx, muted } => {
                        mixer.set_bus_muted(bus_idx, muted);
                    }
                    AudioCommand::StartCapture { capture_tx, done } => {
                        cap_tx = Some(capture_tx);
                        cap_done = Some(done);
                    }
                    AudioCommand::StopCapture => {
                        if let Some(done) = cap_done.take() {
                            done.store(true, Ordering::Relaxed);
                        }
                        cap_tx = None;
                    }
                    AudioCommand::SetSlotFxChain { slot_id, chain } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            slot.fx_chain = chain;
                        }
                    }
                    AudioCommand::ClearSlotFx { slot_id } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            slot.fx_chain.clear();
                        }
                    }
                    AudioCommand::SetLoopPoints { slot_id, start_frac, end_frac } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(clip) = src.as_any_mut().downcast_mut::<AudioClipPlayer>() {
                                    clip.set_loop_region(start_frac, end_frac);
                                }
                            }
                        }
                    }
                    AudioCommand::FreezeGranular { slot_id } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(eng) = src.as_any_mut().downcast_mut::<GranularEngine>() {
                                    eng.freeze();
                                }
                            }
                        }
                    }
                    AudioCommand::UnfreezeGranular { slot_id } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(eng) = src.as_any_mut().downcast_mut::<GranularEngine>() {
                                    eng.set_frozen(false);
                                }
                            }
                        }
                    }
                    AudioCommand::SetGranularParams { slot_id, params } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(eng) = src.as_any_mut().downcast_mut::<GranularEngine>() {
                                    eng.set_params(params);
                                }
                            }
                        }
                    }
                    AudioCommand::SetGranularZone { slot_id, zone } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(eng) = src.as_any_mut().downcast_mut::<GranularEngine>() {
                                    eng.set_zone(zone);
                                }
                            }
                        }
                    }
                    AudioCommand::SetReverse { slot_id, reverse } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(player) = src.as_any_mut().downcast_mut::<AudioClipPlayer>() {
                                    player.set_reverse(reverse);
                                }
                            }
                        }
                    }
                    AudioCommand::SetPitchSt { slot_id, semitones } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(player) = src.as_any_mut().downcast_mut::<AudioClipPlayer>() {
                                    player.set_pitch_st(semitones);
                                }
                            }
                        }
                    }
                    AudioCommand::SetPlaybackRange { slot_id, start_frac, end_frac } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(player) = src.as_any_mut().downcast_mut::<AudioClipPlayer>() {
                                    player.set_playback_range(start_frac, end_frac);
                                }
                            }
                        }
                    }
                    AudioCommand::SetMasterFxChain { chain } => {
                        mixer.master_fx = chain;
                    }
                    AudioCommand::ClearMasterFx => {
                        mixer.master_fx.clear();
                    }
                    AudioCommand::Shutdown => {
                        // The stream will be dropped by the non-RT side.
                    }
                    // LoadSf2 / LoadAudioFile / UnloadSlot are handled outside the callback.
                    _ => {}
                }
            }

            // Mix all active sources into the CPAL output buffer.
            mixer.mix(data, actual_sample_rate);

            // Publish per-slot and master peaks to stats (Relaxed: updated every block).
            for (i, peak) in mixer.slot_peaks.iter().enumerate() {
                if i < stats.slot_peaks.len() {
                    stats.slot_peaks[i].store(peak.to_bits(), Ordering::Relaxed);
                }
            }
            stats.master_peak.store(
                mixer.master_peak[0].max(mixer.master_peak[1]).to_bits(),
                Ordering::Relaxed,
            );

            // Skip-back: try_write is lock-free if no reader holds the lock.
            // If a capture is ongoing (read lock held), we silently skip this block.
            if let Some(mut sb) = skip_back_rt.try_write() {
                sb.push_block(data);
            }

            // Write captured audio to the lock-free ring (RT-safe: no alloc, no block).
            if let Some(tx) = &mut cap_tx {
                for &sample in data.iter() {
                    let _ = tx.push(sample); // silently drop if ring is full
                }
            }

            // Measure DSP load: callback_time / block_duration.
            let elapsed_us = t0.elapsed().as_micros() as u64;
            let block_dur_us = (actual_buffer_size as u64 * 1_000_000) / actual_sample_rate as u64;
            let load_ppm = if block_dur_us > 0 {
                ((elapsed_us * 1_000_000) / block_dur_us).min(1_000_000) as u32
            } else {
                0
            };
            stats.dsp_load_ppm.store(load_ppm, Ordering::Relaxed);
            if elapsed_us > block_dur_us {
                stats.xrun_count.fetch_add(1, Ordering::Relaxed);
            }
        };

        let err_fn = {
            let event_tx = self.event_tx.clone();
            move |e: cpal::StreamError| {
                error!("CPAL stream error: {}", e);
                let _ = event_tx.send(AudioEngineEvent::Xrun);
            }
        };

        let stream = device.build_output_stream(&stream_config, callback, err_fn, None)?;
        stream.play()?;

        self.stats.is_running.store(true, Ordering::Relaxed);
        self.cmd_tx = Some(cmd_tx);
        self._stream = Some(stream);

        let _ = self.event_tx.send(AudioEngineEvent::StreamStarted {
            sample_rate: actual_sample_rate,
            buffer_size: actual_buffer_size,
        });

        info!(
            "Audio stream started: {}Hz / {} frames ({:.1}ms latency)",
            actual_sample_rate,
            actual_buffer_size,
            actual_buffer_size as f64 * 1000.0 / actual_sample_rate as f64
        );

        Ok(())
    }

    fn close(&mut self) {
        self._stream = None;
        self.cmd_tx = None;
        self.stats.is_running.store(false, Ordering::Relaxed);
        let _ = self.event_tx.send(AudioEngineEvent::StreamStopped);
        info!("Audio stream closed");
    }

    fn is_running(&self) -> bool {
        self.stats.is_running.load(Ordering::Relaxed)
    }

    fn list_devices(&self) -> Vec<AudioDeviceInfo> {
        let host = cpal::default_host();
        host.output_devices()
            .map(|devs| {
                devs.filter_map(|d| {
                    let name = d.name().ok()?;
                    let default_dev = host.default_output_device();
                    let is_default = default_dev
                        .and_then(|dd| dd.name().ok())
                        .map(|dn| dn == name)
                        .unwrap_or(false);
                    let supported = d.supported_output_configs().ok()?;
                    let sample_rates: Vec<u32> = supported
                        .flat_map(|r| {
                            [r.min_sample_rate().0, r.max_sample_rate().0]
                        })
                        .collect();
                    let max_ch = d.default_output_config()
                        .map(|c| c.channels())
                        .unwrap_or(2);
                    Some(AudioDeviceInfo {
                        name,
                        is_default,
                        max_output_channels: max_ch,
                        supported_sample_rates: sample_rates,
                    })
                })
                .collect()
            })
            .unwrap_or_default()
    }

    fn dsp_load(&self) -> f32 {
        self.stats.dsp_load_ppm.load(Ordering::Relaxed) as f32 / 10000.0
    }

    fn xrun_count(&self) -> u32 {
        self.stats.xrun_count.load(Ordering::Relaxed)
    }

    fn sample_rate(&self) -> u32 { self.config.sample_rate }
    fn buffer_size(&self) -> u32 { self.config.buffer_size }
}

impl CpalAudioBackend {
    /// Per-slot peak levels (0.0–1.0+). Sampled at block rate; includes decay.
    pub fn slot_peaks(&self) -> Vec<f32> {
        self.stats.slot_peaks.iter()
            .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
            .collect()
    }

    /// Master output peak (mono-max of L/R). Sampled at block rate; includes decay.
    pub fn master_peak(&self) -> f32 {
        f32::from_bits(self.stats.master_peak.load(Ordering::Relaxed))
    }
}

/// Reads stereo f32 samples from `cap_rx` and writes them to a WAV file at `path`.
/// Exits when `done` is set to `true` AND the ring is empty.
fn write_capture_wav(
    mut cap_rx: rtrb::Consumer<f32>,
    done: Arc<AtomicBool>,
    path: PathBuf,
    sample_rate: u32,
    event_tx: flume::Sender<AudioEngineEvent>,
) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = match hound::WavWriter::create(&path, spec) {
        Ok(w) => w,
        Err(e) => {
            let _ = event_tx.send(AudioEngineEvent::CaptureFailed(e.to_string()));
            return;
        }
    };

    let mut total_frames: u64 = 0;

    loop {
        let is_done = done.load(Ordering::Relaxed);
        let available = cap_rx.slots();

        if available == 0 {
            if is_done {
                break;
            }
            // Wait a bit before checking again (non-RT thread, sleeping is fine).
            std::thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }

        for _ in 0..available {
            if let Ok(sample) = cap_rx.pop() {
                let _ = writer.write_sample(sample);
            }
        }
        // Stereo: frames = samples / 2.
        total_frames += available as u64 / 2;
    }

    let duration_secs = total_frames as f64 / sample_rate as f64;
    match writer.finalize() {
        Ok(()) => {
            let _ = event_tx.send(AudioEngineEvent::CaptureStopped { path, duration_secs });
        }
        Err(e) => {
            let _ = event_tx.send(AudioEngineEvent::CaptureFailed(e.to_string()));
        }
    }
}
