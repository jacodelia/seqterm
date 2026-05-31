//! AudioEngine — the non-RT control handle.
//!
//! Provides start/stop, command dispatch, and background asset loading.
//! The realtime work happens inside the CPAL callback via Mixer + AudioSources.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tracing::warn;

use seqterm_ports::AudioEngineConfig;
use seqterm_ports::realtime::AudioSource;

use crate::{
    assets::AssetCache,
    audio_clip::{AudioClipPlayer, LoopMode},
    events::{AudioCommand, AudioEngineEvent},
    mixer::MAX_SLOTS,
    skip_back::SkipBackBuffer,
};

#[cfg(feature = "cpal-backend")]
use crate::cpal_backend::CpalAudioBackend;
use seqterm_ports::AudioBackendPort;

/// Slot assignment info (used for command routing).
#[derive(Debug, Clone)]
pub struct SlotInfo {
    pub slot_id: u32,
    pub kind: SlotKind,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Sf2,
    AudioClip,
}

/// Non-realtime engine controller.
/// Owns the CPAL backend and the asset cache.
/// The `AudioEngineHandle` is a cheaply-cloneable sender used by the application layer.
pub struct AudioEngine {
    config: AudioEngineConfig,
    #[cfg(feature = "cpal-backend")]
    backend: CpalAudioBackend,
    asset_cache: Arc<AssetCache>,
    event_rx: flume::Receiver<AudioEngineEvent>,
    event_tx: flume::Sender<AudioEngineEvent>,
    slots: Vec<Option<SlotInfo>>,
    /// Non-RT → RT installation channel: background threads send loaded sources here.
    /// `drain_events()` pumps them to the ring buffer so the callback can install them.
    install_rx: flume::Receiver<(u32, Box<dyn AudioSource>)>,
    install_tx: flume::Sender<(u32, Box<dyn AudioSource>)>,
}

impl AudioEngine {
    pub fn new(config: AudioEngineConfig) -> Self {
        let (event_tx, event_rx) = flume::unbounded();
        let (install_tx, install_rx) = flume::unbounded::<(u32, Box<dyn AudioSource>)>();
        let asset_cache = Arc::new(AssetCache::new());

        #[cfg(feature = "cpal-backend")]
        let backend = CpalAudioBackend::new(event_tx.clone());

        let mut slots = Vec::with_capacity(MAX_SLOTS);
        slots.resize_with(MAX_SLOTS, || None);

        Self {
            config,
            #[cfg(feature = "cpal-backend")]
            backend,
            asset_cache,
            event_rx,
            event_tx,
            slots,
            install_rx,
            install_tx,
        }
    }

    /// Start the audio output stream.
    pub fn start(&mut self) -> Result<()> {
        #[cfg(feature = "cpal-backend")]
        self.backend.open(self.config.clone())?;
        Ok(())
    }

    /// Stop and close the audio stream.
    pub fn stop(&mut self) {
        #[cfg(feature = "cpal-backend")]
        self.backend.close();
    }

    /// Stop and restart with a new configuration (e.g. after settings change).
    pub fn restart(&mut self, config: AudioEngineConfig) -> Result<()> {
        self.stop();
        self.config = config;
        self.start()
    }

    /// Drain all pending events from the audio engine.
    /// Also pumps any pending source installations to the RT callback.
    pub fn drain_events(&mut self) -> Vec<AudioEngineEvent> {
        self.pump_installs();
        let mut out = Vec::new();
        while let Ok(ev) = self.event_rx.try_recv() {
            out.push(ev);
        }
        out
    }

    /// Forward any sources that finished background loading into the RT ring buffer.
    fn pump_installs(&mut self) {
        while let Ok((slot_id, source)) = self.install_rx.try_recv() {
            self.send(AudioCommand::InstallSource { slot_id, source });
        }
    }

    /// DSP CPU load (0-100%).
    pub fn dsp_load(&self) -> f32 {
        #[cfg(feature = "cpal-backend")]
        return self.backend.dsp_load();
        #[allow(unreachable_code)]
        0.0
    }

    /// Total xrun count.
    pub fn xrun_count(&self) -> u32 {
        #[cfg(feature = "cpal-backend")]
        return self.backend.xrun_count();
        #[allow(unreachable_code)]
        0
    }

    /// Whether the stream is running.
    pub fn is_running(&self) -> bool {
        #[cfg(feature = "cpal-backend")]
        return self.backend.is_running();
        #[allow(unreachable_code)]
        false
    }

    /// Load an SF2 file into a slot (background thread).
    /// Returns the slot_id for subsequent NoteOn/NoteOff commands.
    /// The synth is installed into the RT mixer automatically once loading completes.
    pub fn load_sf2(&mut self, path: PathBuf, bank: u8, preset: u8) -> u32 {
        let slot_id = self.allocate_slot();
        self.slots[slot_id as usize] = Some(SlotInfo {
            slot_id,
            kind: SlotKind::Sf2,
            path: Some(path.clone()),
        });

        let cache = Arc::clone(&self.asset_cache);
        let event_tx  = self.event_tx.clone();
        let install_tx = self.install_tx.clone();
        let sr = self.config.sample_rate;

        std::thread::spawn(move || {
            match cache.load_sf2(&path, bank, preset, sr) {
                Ok(synth) => {
                    let preset_name = format!("Bank:{bank} Prog:{preset}");
                    let _ = event_tx.send(AudioEngineEvent::Sf2Loaded { slot_id, preset_name });
                    // Transfer the synth to the RT mixer via the install channel.
                    let _ = install_tx.send((slot_id, Box::new(synth) as Box<dyn AudioSource>));
                }
                Err(e) => {
                    let _ = event_tx.send(AudioEngineEvent::LoadFailed {
                        slot_id,
                        error: e.to_string(),
                    });
                }
            }
        });

        slot_id
    }

    /// Release a slot: unload its source from the RT mixer and free the slot for reuse.
    pub fn release_slot(&mut self, slot_id: u32) {
        self.send(AudioCommand::UnloadSlot { slot_id });
        if (slot_id as usize) < self.slots.len() {
            self.slots[slot_id as usize] = None;
        }
    }

    /// Load one SF2 file and configure multiple MIDI channels in a single synth instance.
    /// `channels` is a list of `(midi_channel_0based, bank, preset)`.
    /// Returns the slot_id shared by all clips using this SF2 path.
    pub fn load_sf2_multi(&mut self, path: PathBuf, channels: Vec<(u8, u8, u8)>) -> u32 {
        let slot_id = self.allocate_slot();
        self.slots[slot_id as usize] = Some(SlotInfo {
            slot_id,
            kind: SlotKind::Sf2,
            path: Some(path.clone()),
        });

        let cache     = Arc::clone(&self.asset_cache);
        let event_tx  = self.event_tx.clone();
        let install_tx = self.install_tx.clone();
        let sr = self.config.sample_rate;

        std::thread::spawn(move || {
            match cache.load_sf2_multi(&path, &channels, sr) {
                Ok(synth) => {
                    let preset_name = format!("{} channels", channels.len());
                    let _ = event_tx.send(AudioEngineEvent::Sf2Loaded { slot_id, preset_name });
                    let _ = install_tx.send((slot_id, Box::new(synth) as Box<dyn AudioSource>));
                }
                Err(e) => {
                    let _ = event_tx.send(AudioEngineEvent::LoadFailed {
                        slot_id,
                        error: e.to_string(),
                    });
                }
            }
        });

        slot_id
    }

    /// Load an audio file into a slot (background decode).
    /// The player is installed into the RT mixer automatically once loading completes.
    pub fn load_audio_file(&mut self, path: PathBuf, looping: bool, _original_bpm: f64) -> u32 {
        self.load_audio_file_ex(path, looping, false)
    }

    pub fn load_audio_file_ex(&mut self, path: PathBuf, looping: bool, normalize: bool) -> u32 {
        let slot_id = self.allocate_slot();
        self.slots[slot_id as usize] = Some(SlotInfo {
            slot_id,
            kind: SlotKind::AudioClip,
            path: Some(path.clone()),
        });

        let cache = Arc::clone(&self.asset_cache);
        let event_tx  = self.event_tx.clone();
        let install_tx = self.install_tx.clone();
        let sr = self.config.sample_rate;

        std::thread::spawn(move || {
            match cache.get_or_load_audio(&path) {
                Ok(loaded) => {
                    let duration_secs  = loaded.duration_secs;
                    let native_sr      = loaded.native_sample_rate;
                    let mut player = AudioClipPlayer::new(loaded.clone(), sr);
                    if looping {
                        player.set_loop_mode(LoopMode::Loop);
                    }
                    if normalize {
                        let peak = loaded.samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
                        if peak > 1e-6 { player.set_gain(1.0 / peak); }
                    }
                    let _ = event_tx.send(AudioEngineEvent::AudioFileLoaded {
                        slot_id,
                        duration_secs,
                        sample_rate: native_sr,
                    });
                    let _ = install_tx.send((slot_id, Box::new(player) as Box<dyn AudioSource>));
                }
                Err(e) => {
                    let _ = event_tx.send(AudioEngineEvent::LoadFailed {
                        slot_id,
                        error: e.to_string(),
                    });
                }
            }
        });

        slot_id
    }

    /// Start capturing mixed output to a WAV file at `path`.
    pub fn start_capture(&mut self, path: std::path::PathBuf) {
        #[cfg(feature = "cpal-backend")]
        if let Err(e) = self.backend.start_capture(path) {
            warn!("Failed to start capture: {e}");
        }
    }

    /// Stop the current capture and finalize the WAV file.
    pub fn stop_capture(&mut self) {
        #[cfg(feature = "cpal-backend")]
        if let Err(e) = self.backend.stop_capture() {
            warn!("Failed to stop capture: {e}");
        }
    }

    /// Send a command to the RT audio callback (lock-free ring buffer).
    pub fn send(&mut self, cmd: AudioCommand) {
        #[cfg(feature = "cpal-backend")]
        if let Err(e) = self.backend.send_command(cmd) {
            warn!("Audio command dropped: {e}");
        }
    }

    /// Per-slot peak levels (post-FX, pre-fader) updated each audio block.
    pub fn slot_peak_levels(&self) -> Vec<f32> {
        #[cfg(feature = "cpal-backend")]
        { return self.backend.slot_peaks(); }
        #[cfg(not(feature = "cpal-backend"))]
        { vec![0.0; MAX_SLOTS] }
    }

    /// Master output peak (mono-max of L/R) updated each audio block.
    pub fn master_peak_level(&self) -> f32 {
        #[cfg(feature = "cpal-backend")]
        return self.backend.master_peak();
        #[allow(unreachable_code)]
        0.0
    }

    /// Per-slot RMS levels updated each audio block.
    pub fn slot_rms_levels(&self) -> Vec<f32> {
        #[cfg(feature = "cpal-backend")]
        { return self.backend.slot_rms(); }
        #[cfg(not(feature = "cpal-backend"))]
        { vec![0.0; MAX_SLOTS] }
    }

    /// Master RMS [L, R] updated each audio block.
    pub fn master_rms_levels(&self) -> [f32; 2] {
        #[cfg(feature = "cpal-backend")]
        return self.backend.master_rms();
        #[allow(unreachable_code)]
        [0.0; 2]
    }

    /// Set which audio slot to capture for the live oscilloscope. `None` disables capture.
    pub fn set_waveform_slot(&self, slot_id: Option<u32>) {
        #[cfg(feature = "cpal-backend")]
        self.backend.set_waveform_slot(slot_id);
        #[cfg(not(feature = "cpal-backend"))]
        let _ = slot_id;
    }

    /// Read the current waveform ring buffer (WAVE_LEN L-channel samples, signed -1..1).
    /// Returns an empty Vec when capture is disabled or the engine is not running.
    pub fn waveform_samples(&self) -> Vec<f32> {
        #[cfg(feature = "cpal-backend")]
        return self.backend.waveform_samples();
        #[cfg(not(feature = "cpal-backend"))]
        Vec::new()
    }

    /// Access the skip-back circular buffer (available after `start()`).
    /// Returns `None` before the stream is opened.
    pub fn skip_back(&self) -> Option<std::sync::Arc<parking_lot::RwLock<SkipBackBuffer>>> {
        #[cfg(feature = "cpal-backend")]
        return self.backend.skip_back();
        #[allow(unreachable_code)]
        None
    }

    /// List available audio output devices.
    pub fn list_devices(&self) -> Vec<seqterm_ports::AudioDeviceInfo> {
        #[cfg(feature = "cpal-backend")]
        { return self.backend.list_devices(); }
        #[cfg(not(feature = "cpal-backend"))]
        { vec![] }
    }

    /// Produce a cheap cloneable handle for use by the application layer.
    pub fn handle(&self) -> AudioEngineHandle {
        AudioEngineHandle {
            event_tx: self.event_tx.clone(),
        }
    }

    fn allocate_slot(&self) -> u32 {
        for (i, s) in self.slots.iter().enumerate() {
            if s.is_none() { return i as u32; }
        }
        warn!("All audio slots occupied — reusing slot 0");
        0
    }
}

/// Lightweight handle passed to the scheduler/application layer.
/// Can signal the audio engine via the event channel.
#[derive(Clone)]
pub struct AudioEngineHandle {
    pub event_tx: flume::Sender<AudioEngineEvent>,
}

impl AudioEngineHandle {
    /// Send an audio event back to the engine (e.g. from scheduler thread).
    pub fn emit(&self, ev: AudioEngineEvent) {
        let _ = self.event_tx.send(ev);
    }
}
