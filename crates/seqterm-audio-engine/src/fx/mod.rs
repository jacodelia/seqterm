//! SP-404-style real-time FX processors.
//!
//! All processors implement [`FxProcessor`]: a single `process_block` call
//! transforms a stereo (interleaved L/R) buffer in place — zero allocation.

pub mod bitcrusher;
pub mod cassette;
pub mod delay;
pub mod filter;
pub mod filterbank;
pub mod isolator;
pub mod looper;
pub mod reverb;
pub mod sidechain;
pub mod vinyl;

pub use bitcrusher::Bitcrusher;
pub use cassette::Cassette;
pub use delay::DelayLine;
pub use filter::{Svf, SvfMode};
pub use filterbank::FilterBankFx;
pub use isolator::Isolator;
pub use looper::{Looper, LooperState};
pub use reverb::Reverb;
pub use sidechain::SidechainDuck;
pub use vinyl::VinylSim;

/// Common interface for all FX processors.
pub trait FxProcessor: Send {
    /// Process one stereo block in place. `buf` is interleaved L/R.
    fn process_block(&mut self, buf: &mut [f32], sample_rate: u32);

    /// Reset internal state (e.g. clear delay buffers, reset filter states).
    fn reset(&mut self);

    /// Dry/wet mix (0.0 = dry, 1.0 = fully wet).
    fn set_mix(&mut self, wet: f32);
}
