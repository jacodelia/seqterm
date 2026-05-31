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
    atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering},
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
/// Number of waveform samples captured per slot for the live oscilloscope.
pub const WAVE_LEN: usize = 1024;

struct AudioStats {
    xrun_count:   AtomicU32,
    dsp_load_ppm: AtomicU32, // parts per million → / 10000.0 = percent
    is_running:   AtomicBool,
    /// Per-slot peak level (f32 bits, 0.0–1.0+).
    slot_peaks:   Box<[AtomicU32]>,
    /// Per-slot RMS level (f32 bits, exponential moving average).
    slot_rms:     Box<[AtomicU32]>,
    /// Master output peak (f32 bits).
    master_peak:  AtomicU32,
    /// Master RMS L and R (f32 bits each).
    master_rms_l: AtomicU32,
    master_rms_r: AtomicU32,
    /// Slot id to capture for live oscilloscope (-1 = none).
    waveform_slot_id:  AtomicI32,
    /// Ring buffer of WAVE_LEN captured samples (f32 as u32 bits, L channel).
    waveform_buf:      Box<[AtomicU32]>,
    /// Monotonically increasing write counter — UI uses this to detect new data.
    waveform_write_pos: AtomicUsize,
}

impl AudioStats {
    fn new() -> Arc<Self> {
        use crate::mixer::MAX_SLOTS;
        let slot_peaks: Box<[AtomicU32]> = (0..MAX_SLOTS)
            .map(|_| AtomicU32::new(0))
            .collect();
        let slot_rms: Box<[AtomicU32]> = (0..MAX_SLOTS)
            .map(|_| AtomicU32::new(0))
            .collect();
        let waveform_buf: Box<[AtomicU32]> = (0..WAVE_LEN)
            .map(|_| AtomicU32::new(0))
            .collect();
        Arc::new(Self {
            xrun_count:   AtomicU32::new(0),
            dsp_load_ppm: AtomicU32::new(0),
            is_running:   AtomicBool::new(false),
            slot_peaks,
            slot_rms,
            master_peak:  AtomicU32::new(0),
            master_rms_l: AtomicU32::new(0),
            master_rms_r: AtomicU32::new(0),
            waveform_slot_id:   AtomicI32::new(-1),
            waveform_buf,
            waveform_write_pos: AtomicUsize::new(0),
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
    /// Native JACK async client (held alive when using JACK backend).
    /// Stored as Any+Send to avoid generic type parameters on this struct.
    #[cfg(feature = "jack-backend")]
    _jack_client: Option<Box<dyn std::any::Any + Send>>,
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
            #[cfg(feature = "jack-backend")]
            _jack_client: None,
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

        // Use native JACK backend when requested — gives correct client name
        // "SeqTerm" in Carla/PipeWire and auto-connects to system:playback_*.
        //
        // When use_jack=true and PipeWire is running, set PIPEWIRE_QUANTUM so PipeWire
        // uses the exact buffer size requested, minimising round-trip latency.
        #[cfg(feature = "jack-backend")]
        if config.use_jack {
            if pipewire_is_running() {
                let quantum = if config.pipewire_quantum > 0 {
                    config.pipewire_quantum
                } else {
                    config.buffer_size
                };
                // PIPEWIRE_QUANTUM = frames/rate tells PipeWire the desired quantum.
                // MUST be set before the JACK client is created.
                // Safety: single-threaded at this point (no other threads read env).
                unsafe {
                    std::env::set_var(
                        "PIPEWIRE_QUANTUM",
                        format!("{}/{}", quantum, config.sample_rate),
                    );
                }
                info!(
                    "PipeWire detected — setting PIPEWIRE_QUANTUM={}/{}",
                    quantum, config.sample_rate
                );
            }
            return self.open_jack(config);
        }

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
                    AudioCommand::AllNotesOff { slot_id } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(synth) = src.as_any_mut()
                                    .downcast_mut::<SoundFontSynth>()
                                {
                                    synth.all_notes_off();
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
                    AudioCommand::SetGranularMod { slot_id, mod_matrix } => {
                        if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(eng) = src.as_any_mut().downcast_mut::<GranularEngine>() {
                                    eng.set_mod_matrix(mod_matrix);
                                }
                            }
                        }
                    }
                    AudioCommand::SetGranularLiveSource { granular_slot_id, source_slot_id } => {
                        let gran_idx = granular_slot_id as usize;
                        // Remove any existing link for this granular slot.
                        mixer.live_links.retain(|&(_, g)| g != gran_idx);
                        // Enable/disable live mode on the engine.
                        if let Some(slot) = mixer.slots.get_mut(gran_idx) {
                            if let Some(src) = slot.source.as_mut() {
                                if let Some(eng) = src.as_any_mut().downcast_mut::<GranularEngine>() {
                                    eng.set_live_active(source_slot_id.is_some());
                                    // Ensure the slot stays active in live mode.
                                    if source_slot_id.is_some() { slot.active = true; }
                                }
                            }
                        }
                        // Register the new link.
                        if let Some(src_id) = source_slot_id {
                            mixer.live_links.push((src_id as usize, gran_idx));
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
                    AudioCommand::UnloadSlot { slot_id } => {
                        mixer.unload_slot(slot_id as usize);
                    }
                    // LoadSf2 / LoadAudioFile are handled outside the callback.
                    _ => {}
                }
            }

            // Set the waveform capture target from stats (non-RT writes this via atomic).
            mixer.waveform_slot = stats.waveform_slot_id.load(Ordering::Relaxed);

            // Mix all active sources into the CPAL output buffer.
            mixer.mix(data, actual_sample_rate);

            // Publish per-slot peaks and RMS to stats (Relaxed: updated every block).
            for (i, peak) in mixer.slot_peaks.iter().enumerate() {
                if i < stats.slot_peaks.len() {
                    stats.slot_peaks[i].store(peak.to_bits(), Ordering::Relaxed);
                }
            }
            for (i, rms) in mixer.slot_rms.iter().enumerate() {
                if i < stats.slot_rms.len() {
                    stats.slot_rms[i].store(rms.to_bits(), Ordering::Relaxed);
                }
            }
            stats.master_peak.store(
                mixer.master_peak[0].max(mixer.master_peak[1]).to_bits(),
                Ordering::Relaxed,
            );
            stats.master_rms_l.store(mixer.master_rms[0].to_bits(), Ordering::Relaxed);
            stats.master_rms_r.store(mixer.master_rms[1].to_bits(), Ordering::Relaxed);

            // Publish waveform buffer (L-channel samples, ring view).
            if mixer.waveform_slot >= 0 {
                for (i, &s) in mixer.waveform_buf.iter().enumerate() {
                    if i < stats.waveform_buf.len() {
                        stats.waveform_buf[i].store(s.to_bits(), Ordering::Relaxed);
                    }
                }
                stats.waveform_write_pos.store(mixer.waveform_pos, Ordering::Relaxed);
            }

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

        // Auto-connect JACK/PipeWire output ports to system playback.
        #[cfg(feature = "jack-backend")]
        if config.use_jack {
            jack_autoconnect_to_system(device_name);
        }

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

    /// Per-slot RMS levels (0.0–1.0). Exponential moving average updated each block.
    pub fn slot_rms(&self) -> Vec<f32> {
        self.stats.slot_rms.iter()
            .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
            .collect()
    }

    /// Master RMS [L, R].
    pub fn master_rms(&self) -> [f32; 2] {
        [
            f32::from_bits(self.stats.master_rms_l.load(Ordering::Relaxed)),
            f32::from_bits(self.stats.master_rms_r.load(Ordering::Relaxed)),
        ]
    }

    /// Set the audio slot id to capture for live oscilloscope. Pass `None` to disable.
    pub fn set_waveform_slot(&self, slot_id: Option<u32>) {
        let id = slot_id.map(|id| id as i32).unwrap_or(-1);
        self.stats.waveform_slot_id.store(id, Ordering::Relaxed);
    }

    /// Read the current waveform ring buffer as an ordered Vec<f32> (WAVE_LEN samples, L ch).
    /// Returns empty vec when no slot is being captured.
    pub fn waveform_samples(&self) -> Vec<f32> {
        if self.stats.waveform_slot_id.load(Ordering::Relaxed) < 0 {
            return Vec::new();
        }
        let pos = self.stats.waveform_write_pos.load(Ordering::Relaxed);
        let start = pos % WAVE_LEN;
        (0..WAVE_LEN)
            .map(|i| {
                let idx = (start + i) % WAVE_LEN;
                f32::from_bits(self.stats.waveform_buf[idx].load(Ordering::Relaxed))
            })
            .collect()
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

// ─── Backend detection ────────────────────────────────────────────────────────

/// Returns true when a PipeWire daemon is reachable on this session.
///
/// Checks the canonical PipeWire socket path `$XDG_RUNTIME_DIR/pipewire-0`
/// (fallback: `/run/user/<uid>/pipewire-0` resolved via `id -u`). No client
/// connection is opened — filesystem existence check only.
pub fn pipewire_is_running() -> bool {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        // Fallback: ask `id -u` for the effective UID without pulling libc.
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| format!("/run/user/{}", s.trim()))
            .unwrap_or_default()
    });
    !runtime_dir.is_empty()
        && std::path::Path::new(&runtime_dir).join("pipewire-0").exists()
}

// ─── JACK / PipeWire auto-connection ─────────────────────────────────────────

/// Spawn a background thread that waits for CPAL to register its JACK ports
/// and then connects them to `system:playback_1` / `system:playback_2`.
///
/// Works with native JACK and PipeWire (via pipewire-jack compatibility layer).
/// `cpal_device_name` is used as a hint to identify the correct client.
#[cfg(feature = "jack-backend")]
fn jack_autoconnect_to_system(cpal_device_name: String) {
    std::thread::Builder::new()
        .name("jack-autoconnect".to_string())
        .spawn(move || {
            // Allow CPAL to register its ports with JACK/PipeWire.
            std::thread::sleep(std::time::Duration::from_millis(400));

            let (client, _status) = match jack::Client::new(
                "seqterm_autoconn",
                jack::ClientOptions::NO_START_SERVER,
            ) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("JACK auto-connect: could not create helper client: {e:?}");
                    return;
                }
            };

            // Find physical playback (input to system) ports.
            let playback_ports = client.ports(
                Some("system:playback_"),
                None,
                jack::PortFlags::IS_INPUT | jack::PortFlags::IS_PHYSICAL,
            );
            if playback_ports.is_empty() {
                tracing::warn!("JACK auto-connect: no system:playback_* ports found");
                return;
            }

            // Find CPAL's output ports. CPAL names its client after the binary or
            // the JACK device name. Try the device name first, then common fallbacks.
            let binary_name = std::env::current_exe()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default();

            let candidates: Vec<String> = {
                let mut c = Vec::new();
                // The JACK device name shown in the list (may contain the client name).
                // cpal_device_name for PipeWire looks like "seqterm-app (PipeWire)" or similar.
                let dev_stem: String = cpal_device_name
                    .split_whitespace()
                    .next()
                    .unwrap_or(&cpal_device_name)
                    .to_string();
                c.push(dev_stem);
                c.push(binary_name);
                c.push("seqterm-app".into());
                c.push("seqterm".into());
                c.push("cpal_client_out".into());
                c
            };

            // Try each candidate client name — look for output ports.
            let mut connected = 0usize;
            'outer: for client_name in &candidates {
                let our_outputs = client.ports(
                    Some(&format!("{}:", client_name)),
                    None,
                    jack::PortFlags::IS_OUTPUT,
                );
                if our_outputs.is_empty() { continue; }

                for (out_port, in_port) in our_outputs.iter().zip(playback_ports.iter()) {
                    match client.connect_ports_by_name(out_port, in_port) {
                        Ok(()) => {
                            tracing::info!("JACK auto-connect: {} → {}", out_port, in_port);
                            connected += 1;
                        }
                        Err(jack::Error::PortAlreadyConnected(..)) => {
                            tracing::info!("JACK auto-connect: {} → {} (already connected)", out_port, in_port);
                            connected += 1;
                        }
                        Err(e) => {
                            tracing::warn!("JACK auto-connect: {} → {} failed: {e:?}", out_port, in_port);
                        }
                    }
                }
                if connected > 0 { break 'outer; }
            }

            if connected == 0 {
                tracing::warn!(
                    "JACK auto-connect: could not connect SeqTerm ports. \
                     Tried clients: {:?}. Connect manually in your patchbay.",
                    candidates
                );
            }
        })
        .ok();
}

// ─── Native JACK backend (replaces CPAL when use_jack=true) ──────────────────
//
// Creates a "SeqTerm" JACK client with ports "out_L" / "out_R", runs the full
// Mixer in the JACK process callback, and auto-connects to system:playback_*.
// Works with native JACK and PipeWire's JACK compatibility layer.

#[cfg(feature = "jack-backend")]
struct JackProcessHandler {
    cmd_rx: rtrb::Consumer<AudioCommand>,
    mixer:  Mixer,
    out_l:  jack::Port<jack::AudioOut>,
    out_r:  jack::Port<jack::AudioOut>,
    stats:  Arc<AudioStats>,
    sample_rate: u32,
    /// Pre-allocated interleaved scratch buffer — avoids RT allocation.
    scratch: Vec<f32>,
    event_tx: flume::Sender<AudioEngineEvent>,
}

#[cfg(feature = "jack-backend")]
impl jack::ProcessHandler for JackProcessHandler {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        let t0 = Instant::now();
        let n = ps.n_frames() as usize;

        // Drain command ring (lock-free, no alloc).
        while let Ok(cmd) = self.cmd_rx.pop() {
            dispatch_audio_command(cmd, &mut self.mixer);
        }

        // Mix into pre-allocated interleaved buffer.
        let needed = n * 2;
        let cap = self.scratch.len();
        let scratch = &mut self.scratch[..needed.min(cap)];
        for s in scratch.iter_mut() { *s = 0.0; }
        self.mixer.mix(scratch, self.sample_rate);

        // Publish peaks to stats (lock-free reads from UI).
        for (i, peak) in self.mixer.slot_peaks.iter().enumerate() {
            if i < self.stats.slot_peaks.len() {
                self.stats.slot_peaks[i].store(peak.to_bits(), Ordering::Relaxed);
            }
        }
        let master = self.mixer.master_peak[0].max(self.mixer.master_peak[1]);
        self.stats.master_peak.store(master.to_bits(), Ordering::Relaxed);

        // De-interleave into JACK output ports.
        let frames = n.min(cap / 2);
        let out_l = self.out_l.as_mut_slice(ps);
        let out_r = self.out_r.as_mut_slice(ps);
        for i in 0..frames {
            out_l[i] = self.scratch[i * 2];
            out_r[i] = self.scratch[i * 2 + 1];
        }

        // DSP load measurement.
        let elapsed_us = t0.elapsed().as_micros() as u64;
        let block_dur_us = (n as u64 * 1_000_000) / self.sample_rate.max(1) as u64;
        let load_ppm = if block_dur_us > 0 {
            ((elapsed_us * 1_000_000) / block_dur_us).min(1_000_000) as u32
        } else { 0 };
        self.stats.dsp_load_ppm.store(load_ppm, Ordering::Relaxed);
        if elapsed_us > block_dur_us {
            let _ = self.event_tx.try_send(AudioEngineEvent::Xrun);
        }

        jack::Control::Continue
    }
}

/// Dispatch one AudioCommand into the Mixer — shared by both CPAL and JACK callbacks.
/// Capture commands (StartCapture/StopCapture) are CPAL-only and handled separately.
#[cfg(feature = "jack-backend")]
fn dispatch_audio_command(cmd: AudioCommand, mixer: &mut Mixer) {
    match cmd {
        AudioCommand::InstallSource { slot_id, source } => {
            mixer.set_slot(slot_id as usize, source, 1.0);
        }
        AudioCommand::NoteOn { slot_id, channel, note, velocity } => {
            if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                slot.active = true;
                if let Some(src) = slot.source.as_mut() {
                    if let Some(synth) = src.as_any_mut().downcast_mut::<SoundFontSynth>() {
                        synth.note_on(channel, note, velocity);
                    }
                }
            }
        }
        AudioCommand::NoteOff { slot_id, channel, note } => {
            if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                if let Some(src) = slot.source.as_mut() {
                    if let Some(synth) = src.as_any_mut().downcast_mut::<SoundFontSynth>() {
                        synth.note_off(channel, note);
                    }
                }
            }
        }
        AudioCommand::AllNotesOff { slot_id } => {
            if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                if let Some(src) = slot.source.as_mut() {
                    if let Some(synth) = src.as_any_mut().downcast_mut::<SoundFontSynth>() {
                        synth.all_notes_off();
                    }
                }
            }
        }
        AudioCommand::ControlChange { slot_id, channel, cc, value } => {
            if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                if let Some(src) = slot.source.as_mut() {
                    if let Some(synth) = src.as_any_mut().downcast_mut::<SoundFontSynth>() {
                        synth.control_change(channel, cc, value);
                    }
                }
            }
        }
        AudioCommand::PlayAudioClip { slot_id } => {
            if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                if let Some(src) = slot.source.as_mut() {
                    if let Some(clip) = src.as_any_mut().downcast_mut::<AudioClipPlayer>() {
                        clip.play();
                    }
                }
                slot.active = true;
            }
        }
        AudioCommand::StopAudioClip { slot_id } => {
            mixer.clear_slot(slot_id as usize);
        }
        AudioCommand::UnloadSlot { slot_id } => {
            mixer.unload_slot(slot_id as usize);
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
        AudioCommand::SetGranularMod { slot_id, mod_matrix } => {
            if let Some(slot) = mixer.slots.get_mut(slot_id as usize) {
                if let Some(src) = slot.source.as_mut() {
                    if let Some(eng) = src.as_any_mut().downcast_mut::<GranularEngine>() {
                        eng.set_mod_matrix(mod_matrix);
                    }
                }
            }
        }
        AudioCommand::SetGranularLiveSource { granular_slot_id, source_slot_id } => {
            let gran_idx = granular_slot_id as usize;
            mixer.live_links.retain(|&(_, g)| g != gran_idx);
            if let Some(slot) = mixer.slots.get_mut(gran_idx) {
                if let Some(src) = slot.source.as_mut() {
                    if let Some(eng) = src.as_any_mut().downcast_mut::<GranularEngine>() {
                        eng.set_live_active(source_slot_id.is_some());
                        if source_slot_id.is_some() { slot.active = true; }
                    }
                }
            }
            if let Some(src_id) = source_slot_id {
                mixer.live_links.push((src_id as usize, gran_idx));
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
        _ => {} // StartCapture/StopCapture handled by CPAL only; LoadSf2/LoadAudioFile non-RT
    }
}

#[cfg(feature = "jack-backend")]
impl CpalAudioBackend {
    /// Open a native JACK client named "SeqTerm" and auto-connect to system playback.
    fn open_jack(&mut self, _config: AudioEngineConfig) -> Result<()> {

        let (client, _status) = jack::Client::new(
            "SeqTerm",
            jack::ClientOptions::empty(),
        ).map_err(|e| anyhow!("JACK client init failed: {e:?}"))?;

        let sample_rate = client.sample_rate() as u32;
        let buffer_size = client.buffer_size();

        let out_l = client.register_port("out_L", jack::AudioOut::default())
            .map_err(|e| anyhow!("JACK register out_L: {e:?}"))?;
        let out_r = client.register_port("out_R", jack::AudioOut::default())
            .map_err(|e| anyhow!("JACK register out_R: {e:?}"))?;

        let (cmd_tx, cmd_rx) = RingBuffer::<AudioCommand>::new(CMD_RING_CAPACITY);

        let handler = JackProcessHandler {
            cmd_rx,
            mixer:   Mixer::new(MAX_BLOCK_FRAMES),
            out_l,
            out_r,
            stats:   Arc::clone(&self.stats),
            sample_rate,
            scratch: vec![0.0f32; MAX_BLOCK_FRAMES * 2],
            event_tx: self.event_tx.clone(),
        };

        let active = client.activate_async((), handler)
            .map_err(|e| anyhow!("JACK activate failed: {e:?}"))?;

        self.stats.is_running.store(true, Ordering::Relaxed);
        self.cmd_tx = Some(cmd_tx);
        self._jack_client = Some(Box::new(active));

        let _ = self.event_tx.send(AudioEngineEvent::StreamStarted {
            sample_rate,
            buffer_size,
        });

        info!(
            "JACK stream started: SeqTerm @ {}Hz / {} frames",
            sample_rate, buffer_size,
        );

        // Auto-connect our output ports to physical playback in a background thread.
        // We delay slightly so PipeWire/JACK has time to announce system ports.
        jack_connect_our_outputs();

        Ok(())
    }
}

/// Spawn a thread that connects SeqTerm's JACK output ports to the system
/// playback inputs. Works with native JACK, PipeWire-JACK, and PulseAudio-JACK.
#[cfg(feature = "jack-backend")]
fn jack_connect_our_outputs() {
    std::thread::Builder::new()
        .name("jack-conn".into())
        .spawn(|| {
            // Wait for the session manager (WirePlumber / jackdbus) to wire things up.
            std::thread::sleep(std::time::Duration::from_millis(500));

            let (client, _) = match jack::Client::new(
                "seqterm_conn",
                jack::ClientOptions::NO_START_SERVER,
            ) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("JACK conn helper: {e:?}");
                    return;
                }
            };

            // Find our output ports (registered by open_jack).
            let our_outs = client.ports(
                Some("SeqTerm:"),
                None,
                jack::PortFlags::IS_OUTPUT,
            );
            if our_outs.is_empty() {
                tracing::warn!("JACK conn: no SeqTerm output ports found");
                return;
            }

            // Find physical playback (sink) input ports.
            // Try several naming conventions used by JACK, PipeWire, and Pulse.
            let playback_candidates = [
                "system:playback_",
                "_system:playback_",
                "PulseAudio JACK Sink:front-",
                "PulseAudio JACK Sink:playback_",
            ];
            let mut sinks: Vec<String> = Vec::new();
            for prefix in &playback_candidates {
                sinks = client.ports(
                    Some(prefix),
                    None,
                    jack::PortFlags::IS_INPUT | jack::PortFlags::IS_PHYSICAL,
                );
                if !sinks.is_empty() { break; }
            }
            // Fallback: any physical input port that isn't one of our own.
            if sinks.is_empty() {
                sinks = client.ports(None, None, jack::PortFlags::IS_INPUT | jack::PortFlags::IS_PHYSICAL)
                    .into_iter()
                    .filter(|p| !p.starts_with("SeqTerm:"))
                    .collect();
            }
            if sinks.is_empty() {
                tracing::warn!("JACK conn: no physical playback ports found");
                return;
            }

            tracing::info!("JACK conn: found sinks: {:?}", &sinks[..sinks.len().min(4)]);

            for (out_port, in_port) in our_outs.iter().zip(sinks.iter()) {
                match client.connect_ports_by_name(out_port, in_port) {
                    Ok(()) => tracing::info!("JACK: {} → {}", out_port, in_port),
                    Err(jack::Error::PortAlreadyConnected(..)) => {
                        tracing::info!("JACK: {} → {} (already connected)", out_port, in_port);
                    }
                    Err(e) => tracing::warn!("JACK: {} → {} failed: {e:?}", out_port, in_port),
                }
            }
        })
        .ok();
}
