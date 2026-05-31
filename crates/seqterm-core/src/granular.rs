//! Granular synthesis parameter types (stored in project; engine lives in seqterm-audio-engine).

use serde::{Deserialize, Serialize};

/// Grain envelope shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GrainEnvelope {
    #[default]
    Hann,
    Gaussian,
    Triangle,
    Exponential,
}

impl GrainEnvelope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hann        => "Hann",
            Self::Gaussian    => "Gauss",
            Self::Triangle    => "Tri",
            Self::Exponential => "Exp",
        }
    }
}

/// Direction of grain playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GrainDirection {
    #[default]
    Forward,
    Backward,
    Random,
}

/// Per-grain voice parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrainParams {
    /// Grain size in milliseconds (1–500 ms).
    pub size_ms:    f32,
    /// Grains per second (density), 1–200.
    pub density:    f32,
    /// Position spray: random scatter around playhead, 0.0–1.0.
    pub spray:      f32,
    /// Overlap factor (0.0 = no overlap, 1.0 = full crossfade overlap).
    pub overlap:    f32,
    /// Pitch transpose in semitones (-24 to +24).
    pub pitch_st:   f32,
    /// Playback direction.
    pub direction:  GrainDirection,
    /// Stereo pan (-1.0–1.0).
    pub pan:        f32,
    /// Output gain (linear).
    pub gain:       f32,
    /// Timing jitter: random delay per grain, 0.0–1.0 fraction of grain size.
    pub jitter:     f32,
    /// Stereo spread (duplicate with slight detuning), 0.0–1.0.
    pub stereo_spread: f32,
    /// Envelope shape.
    pub envelope:   GrainEnvelope,
    /// Max simultaneous grain voices (1–32).
    pub max_voices: u8,
}

impl Default for GrainParams {
    fn default() -> Self {
        Self {
            size_ms:      80.0,
            density:      10.0,
            spray:        0.05,
            overlap:      0.5,
            pitch_st:     0.0,
            direction:    GrainDirection::Forward,
            pan:          0.0,
            gain:         1.0,
            jitter:       0.0,
            stereo_spread: 0.0,
            envelope:     GrainEnvelope::Hann,
            max_voices:   8,
        }
    }
}

/// Playhead scanning mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScanMode {
    /// Continuously advance through the buffer.
    #[default]
    Linear,
    /// Random walk around current position.
    RandomWalk,
    /// Freeze at a fixed position.
    Freeze,
}

/// Granular zone: which region of the source buffer to scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GranularZone {
    /// Playhead position as fraction of buffer length (0.0–1.0).
    pub position:   f32,
    /// Scan range: playhead wanders ±range/2 around position.
    pub range:      f32,
    /// Scan speed: how fast the playhead advances (0.0 = frozen, 1.0 = normal).
    pub scan_speed: f32,
    pub scan_mode:  ScanMode,
    /// Whether the freeze buffer is active.
    pub frozen:     bool,
}

impl Default for GranularZone {
    fn default() -> Self {
        Self {
            position:   0.0,
            range:      1.0,
            scan_speed: 0.5,
            scan_mode:  ScanMode::Linear,
            frozen:     false,
        }
    }
}

// ─── Modulation matrix ────────────────────────────────────────────────────────

/// LFO waveform shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Square,
    SampleHold,
}

impl LfoShape {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sine       => "Sine",
            Self::Triangle   => "Tri",
            Self::Square     => "Sqr",
            Self::SampleHold => "S&H",
        }
    }

    pub fn next(self) -> Self {
        match self { Self::Sine => Self::Triangle, Self::Triangle => Self::Square, Self::Square => Self::SampleHold, Self::SampleHold => Self::Sine }
    }

    pub fn prev(self) -> Self {
        match self { Self::Sine => Self::SampleHold, Self::Triangle => Self::Sine, Self::Square => Self::Triangle, Self::SampleHold => Self::Square }
    }
}

/// Granular parameter target for LFO modulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ModTarget {
    #[default]
    Spray,
    Density,
    PitchSt,
    Pan,
    GrainSize,
    Overlap,
    Jitter,
}

impl ModTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::Spray     => "spray",
            Self::Density   => "density",
            Self::PitchSt   => "pitch",
            Self::Pan       => "pan",
            Self::GrainSize => "grain_sz",
            Self::Overlap   => "overlap",
            Self::Jitter    => "jitter",
        }
    }

    pub fn next(self) -> Self {
        match self { Self::Spray => Self::Density, Self::Density => Self::PitchSt, Self::PitchSt => Self::Pan, Self::Pan => Self::GrainSize, Self::GrainSize => Self::Overlap, Self::Overlap => Self::Jitter, Self::Jitter => Self::Spray }
    }

    pub fn prev(self) -> Self {
        match self { Self::Spray => Self::Jitter, Self::Density => Self::Spray, Self::PitchSt => Self::Density, Self::Pan => Self::PitchSt, Self::GrainSize => Self::Pan, Self::Overlap => Self::GrainSize, Self::Jitter => Self::Overlap }
    }
}

/// One modulation slot: an LFO mapped to a granular parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LfoSlot {
    pub enabled:  bool,
    pub shape:    LfoShape,
    /// Modulation rate in Hz (0.01–20.0).
    pub rate_hz:  f32,
    /// Modulation depth as a fraction of the target's full range (0.0–1.0).
    pub depth:    f32,
    pub target:   ModTarget,
}

impl Default for LfoSlot {
    fn default() -> Self {
        Self { enabled: false, shape: LfoShape::Sine, rate_hz: 0.5, depth: 0.1, target: ModTarget::Spray }
    }
}

/// Number of LFO slots in the modulation matrix.
pub const MOD_SLOTS: usize = 4;

/// Per-engine modulation matrix (sent alongside or inside GrainParams).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GranularMod {
    pub slots: [LfoSlot; MOD_SLOTS],
}

/// Complete granular preset (stored per-clip or globally).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GranularPreset {
    pub name:   String,
    pub params: GrainParams,
    pub zone:   GranularZone,
}

impl GranularPreset {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grain_params_default_reasonable() {
        let p = GrainParams::default();
        assert!(p.size_ms > 0.0 && p.size_ms < 1000.0);
        assert!(p.density > 0.0);
        assert!((0.0..=1.0).contains(&p.spray));
        assert_eq!(p.max_voices, 8);
    }

    #[test]
    fn granular_preset_new() {
        let p = GranularPreset::new("ambient");
        assert_eq!(p.name, "ambient");
    }
}
