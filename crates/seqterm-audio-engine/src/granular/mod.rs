//! Granular synthesis engine — realtime-safe grain cloud processor.

pub mod envelopes;
pub mod grain;
pub mod engine;

pub use engine::GranularEngine;
pub use envelopes::EnvelopeTables;
