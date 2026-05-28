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
pub mod engine;
pub mod events;
pub mod fx;
pub mod granular;
pub mod mixer;
pub mod offline;
pub mod sf2_synth;
pub mod skip_back;

#[cfg(feature = "cpal-backend")]
pub mod cpal_backend;

pub use engine::{AudioEngine, AudioEngineHandle};
pub use events::{AudioCommand, AudioEngineEvent};
pub use offline::{render_offline_mixdown, render_offline_stem};
pub use sf2_synth::{SoundFontSynth, enumerate_sf2_presets};
pub use audio_clip::AudioClipPlayer;
pub use assets::AssetCache;
pub use skip_back::SkipBackBuffer;
pub use granular::GranularEngine;
pub use fx::{FxProcessor, Bitcrusher, Svf, SvfMode, DelayLine, VinylSim, Reverb};

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
