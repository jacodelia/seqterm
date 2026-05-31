//! Real-time FX processors (zero allocation in the audio callback).
//!
//! All processors implement [`FxProcessor`]: a single `process_block` call
//! transforms a stereo (interleaved L/R) buffer in place.

// ── Original ──────────────────────────────────────────────────────────────────
pub mod bitcrusher;
pub mod cassette;
pub mod delay;
pub mod filter;
pub mod filterbank;
pub mod gran_delay;
pub mod isolator;
pub mod looper;
pub mod reverb;
pub mod sidechain;
pub mod vinyl;
// ── Dynamics ─────────────────────────────────────────────────────────────────
pub mod compressor;
pub mod gate;
// ── EQ ───────────────────────────────────────────────────────────────────────
pub mod parametric_eq;
// ── Modulation ───────────────────────────────────────────────────────────────
pub mod chorus;
pub mod flanger;
pub mod phaser;
// ── Spatial ──────────────────────────────────────────────────────────────────
pub mod widener;
// ── Utility ──────────────────────────────────────────────────────────────────
pub mod utility;

// ── Re-exports ────────────────────────────────────────────────────────────────
pub use bitcrusher::Bitcrusher;
pub use cassette::Cassette;
pub use delay::DelayLine;
pub use filter::{Svf, SvfMode};
pub use filterbank::FilterBankFx;
pub use gran_delay::GranularDelay;
pub use isolator::Isolator;
pub use looper::{Looper, LooperState};
pub use reverb::Reverb;
pub use sidechain::SidechainDuck;
pub use vinyl::VinylSim;
// Dynamics
pub use compressor::Compressor;
pub use gate::Gate;
// EQ
pub use parametric_eq::{EqBandKind, ParametricEq};
// Modulation
pub use chorus::Chorus;
pub use flanger::Flanger;
pub use phaser::Phaser;
// Spatial
pub use widener::StereoWidener;
// Utility
pub use utility::{Gain, MonoMaker, PhaseInvert, SoftClipper, TubeSaturation};

/// Common interface for all FX processors.
pub trait FxProcessor: Send {
    /// Process one stereo block in place. `buf` is interleaved L/R.
    fn process_block(&mut self, buf: &mut [f32], sample_rate: u32);

    /// Reset internal state (e.g. clear delay buffers, reset filter states).
    fn reset(&mut self);

    /// Dry/wet mix (0.0 = dry, 1.0 = fully wet).
    fn set_mix(&mut self, wet: f32);
}
