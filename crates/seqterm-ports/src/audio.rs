//! Audio backend port — controls the audio I/O system (non-realtime side).

use anyhow::Result;

/// Configuration for the audio backend.
#[derive(Debug, Clone)]
pub struct AudioEngineConfig {
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub output_device: Option<String>,
    pub sf2_max_voices: u32,
    pub sf2_cache_mb: u32,
    /// Use JACK host instead of default system audio (requires jack-backend feature).
    pub use_jack: bool,
}

impl Default for AudioEngineConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            buffer_size: 256,
            output_device: None,
            sf2_max_voices: 128,
            sf2_cache_mb: 256,
            use_jack: false,
        }
    }
}

/// Describes a physical or virtual audio device.
#[derive(Debug, Clone)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub max_output_channels: u16,
    pub supported_sample_rates: Vec<u32>,
}

/// Port: controls the audio output subsystem.
/// Implemented by CpalAudioBackend, JackAudioBackend, etc.
///
/// No `Send + Sync` requirement — backends own platform streams that may be
/// thread-bound. Callers that need thread-mobility wrap in `Arc<Mutex<Box<...>>>`.
pub trait AudioBackendPort {
    /// Open the audio output stream with the given configuration.
    fn open(&mut self, config: AudioEngineConfig) -> Result<()>;

    /// Close the audio output stream cleanly.
    fn close(&mut self);

    /// Whether the stream is currently running.
    fn is_running(&self) -> bool;

    /// List available output devices.
    fn list_devices(&self) -> Vec<AudioDeviceInfo>;

    /// Current DSP CPU load in percent (0-100).
    fn dsp_load(&self) -> f32;

    /// Total xrun count since stream opened.
    fn xrun_count(&self) -> u32;

    /// Current sample rate.
    fn sample_rate(&self) -> u32;

    /// Current buffer size in frames.
    fn buffer_size(&self) -> u32;
}
