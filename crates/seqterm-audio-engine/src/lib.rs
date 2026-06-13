//! # SeqTerm Audio Engine
//!
//! Realtime audio output, SF2 synthesis, and sample playback for SeqTerm.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │  AudioEngine (non-RT control handle)                     │
//! │    start() / stop() / send_command()                     │
//! ├──────────────────────────────────────────────────────────┤
//! │  AudioThread (realtime — no alloc, no mutex)             │
//! │    AudioCallback                                         │
//! │    ├── reads from rtrb::Consumer<AudioCommand>           │
//! │    ├── routes to active AudioSource slots:               │
//! │    │     SoundFontSynth (oxisynth)                       │
//! │    │     AudioClipPlayer (symphonia decode)              │
//! │    └── writes mixed output to CPAL stream buffer         │
//! └──────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Realtime Contract
//!
//! Everything inside `AudioCallback::process()` must be:
//! - Allocation-free (pre-allocated buffers only)
//! - Lock-free (rtrb ring buffers, no Mutex)
//! - Non-blocking (no sleep, no I/O)

pub mod assets;
pub mod audio_clip;
pub mod built_synth;
pub mod engine;
pub mod events;
pub mod fx;
pub mod fx_chain;
pub mod granular;
pub mod lufs;
pub mod mixer;
pub mod spectrum;
pub mod waveform_cache;
pub mod offline;
pub mod sf2_synth;
pub mod sf2_loader;
pub mod sf2_sampler;
pub mod skip_back;

#[cfg(feature = "cpal-backend")]
pub mod cpal_backend;

pub use built_synth::BuiltinSynth;
pub use fx_chain::{build_chain_from_specs, build_processor};
pub use engine::{AudioEngine, AudioEngineHandle};
#[cfg(feature = "cpal-backend")]
pub use cpal_backend::pipewire_is_running;
pub use events::{AudioCommand, AudioEngineEvent};
pub use offline::{
    render_offline_mixdown, render_offline_mixdown_with,
    render_offline_stem, render_offline_stem_with,
    PluginSourceFactory,
};
pub use sf2_synth::{
    SoundFontSynth, enumerate_sf2_presets,
    set_sf2_prefer_fluidsynth, sf2_prefer_fluidsynth, fluidsynth_available,
};
pub use sf2_loader::{load_sf2_instrument, LoadedSf2, Sf2SampleData};
pub use sf2_sampler::Sf2Sampler;
pub use audio_clip::{AudioClipPlayer, LoadedClip, write_wav};
pub use assets::AssetCache;
pub use skip_back::SkipBackBuffer;
pub use lufs::LufsIntegrator;
pub use granular::GranularEngine;
pub use fx::{
    FxProcessor, Bitcrusher, Svf, SvfMode, DelayLine, VinylSim, Reverb,
    // Dynamics
    Compressor, Gate,
    // EQ
    ParametricEq,
    // Modulation
    Chorus, Flanger, Phaser,
    // Spatial
    StereoWidener,
    // Utility
    Gain, PhaseInvert, MonoMaker, SoftClipper, TubeSaturation,
};

use seqterm_ports::AudioEngineConfig;

/// Convenience: create and start an audio engine with default CPAL backend.
pub fn start_default(config: AudioEngineConfig) -> anyhow::Result<AudioEngineHandle> {
    let mut engine = AudioEngine::new(config);
    engine.start()?;
    Ok(engine.handle())
}

/// Scan an audio file and return `bands` amplitude peaks in `[0.0, 1.0]`.
/// Decodes the full file — call from a background thread.
pub fn scan_waveform(path: &std::path::Path, bands: usize) -> anyhow::Result<Vec<f32>> {
    let clip = audio_clip::LoadedClip::load(path)?;
    Ok(clip.peak_bands(bands))
}

/// Decode just enough of an audio file to report its duration in seconds.
/// Used to size arrangement audio clips at the project tempo (Milestone C).
pub fn audio_duration_secs(path: &std::path::Path) -> anyhow::Result<f64> {
    Ok(audio_clip::LoadedClip::load(path)?.duration_secs)
}
