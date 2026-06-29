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
// ── New processors ───────────────────────────────────────────────────────────
pub mod expander;
pub mod pan;
// ── Creative time/texture ─────────────────────────────────────────────────────
pub mod protocosmos;
pub mod reverse;
pub mod space_echo;

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
// New
pub use expander::Expander;
pub use pan::Pan;
// Creative time/texture
pub use protocosmos::Protocosmos;
pub use reverse::ReverseDelay;
pub use space_echo::SpaceEcho;

/// A single automatable parameter descriptor.
#[derive(Debug, Clone)]
pub struct FxParam {
    /// Short display name (e.g. "Threshold", "Rate", "Wet").
    pub name: &'static str,
    /// Current value normalised 0.0–1.0.
    pub value: f32,
    /// Minimum native value (for display scaling; may equal 0.0).
    pub min: f32,
    /// Maximum native value (for display scaling; may equal 1.0).
    pub max: f32,
    /// Unit label shown next to the value (e.g. "dB", "Hz", "%", "").
    pub unit: &'static str,
}

impl FxParam {
    pub const fn new(name: &'static str, value: f32, min: f32, max: f32, unit: &'static str) -> Self {
        Self { name, value, min, max, unit }
    }

    /// Convert normalised value back to native range: `min + value * (max - min)`.
    pub fn native(&self) -> f32 {
        self.min + self.value * (self.max - self.min)
    }
}

/// Common interface for all FX processors.
pub trait FxProcessor: Send {
    /// Process one stereo block in place. `buf` is interleaved L/R.
    fn process_block(&mut self, buf: &mut [f32], sample_rate: u32);

    /// Reset internal state (e.g. clear delay buffers, reset filter states).
    fn reset(&mut self);

    /// Dry/wet mix (0.0 = dry, 1.0 = fully wet).
    fn set_mix(&mut self, wet: f32);

    /// Human-readable name of this processor (e.g. "Compressor").
    fn name(&self) -> &str { "FX" }

    /// Return the current automatable parameter list.
    /// Each entry describes one parameter with its current normalised value.
    /// Default: empty — no automatable parameters.
    fn params(&self) -> Vec<FxParam> { Vec::new() }

    /// Set a parameter by index to a normalised 0.0–1.0 value.
    /// Out-of-range indices are silently ignored.
    fn set_param(&mut self, _index: usize, _value: f32) {}
}
