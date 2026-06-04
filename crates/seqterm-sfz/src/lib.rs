//! SFZ format support for SeqTerm.
//!
//! Implements a minimal SFZ parser (sections, opcodes) and an
//! [`InstrumentBackend`] adapter that maps MIDI note-on events to WAV samples.
//!
//! ## SFZ Overview
//!
//! SFZ is a plain-text sampler format:
//! ```text
//! <group>
//! <region> sample=kick.wav lokey=36 hikey=36
//! <region> sample=snare.wav lokey=38 hikey=38
//! ```
//!
//! Each `<region>` maps a sample file to a pitch range. On note-on the
//! nearest region is selected and the sample played back.

pub mod parser;
pub mod backend;

pub use backend::SfzBackend;

/// Load an SFZ file and return a ready-to-use backend.
/// Returns an error if the file cannot be parsed or any required sample is missing.
pub fn load(path: &std::path::Path) -> anyhow::Result<SfzBackend> {
    let instrument = parser::parse(path)?;
    Ok(SfzBackend::new(instrument))
}

/// Parsed SFZ instrument — a list of regions.
#[derive(Debug, Clone)]
pub struct SfzInstrument {
    /// All regions in definition order (already resolved to absolute paths).
    pub regions: Vec<SfzRegion>,
    /// Absolute path to the .sfz file (used for resolving relative sample paths).
    pub base_dir: std::path::PathBuf,
}

/// One SFZ region (a sample + its key/velocity mapping).
#[derive(Debug, Clone)]
pub struct SfzRegion {
    /// Absolute path to the sample WAV/FLAC file.
    pub sample:   std::path::PathBuf,
    /// Lowest MIDI note this region responds to.
    pub lo_key:   u8,
    /// Highest MIDI note this region responds to.
    pub hi_key:   u8,
    /// Root pitch of the sample in semitones (for transposition).
    pub pitch_key_center: u8,
    /// Lowest velocity this region responds to.
    pub lo_vel:   u8,
    /// Highest velocity this region responds to.
    pub hi_vel:   u8,
    /// Amplitude scaling 0.0–1.0 (from `volume` opcode in dB).
    pub gain:     f32,
}

impl SfzRegion {
    /// Returns true if this region should respond to (note, vel).
    pub fn matches(&self, note: u8, vel: u8) -> bool {
        note >= self.lo_key && note <= self.hi_key
            && vel >= self.lo_vel && vel <= self.hi_vel
    }

    /// Playback rate multiplier to transpose sample to the requested note.
    pub fn rate_for_note(&self, note: u8) -> f32 {
        let semitones = note as i32 - self.pitch_key_center as i32;
        2.0_f32.powf(semitones as f32 / 12.0)
    }
}
