//! Granular synthesis parameter types (stored in project; engine lives in seqterm-audio-engine).
//! Also contains the Audio Source Editor domain types (SampleParams, AdsrEnvelope, EditorFilter,
//! EditorMarker) shared by the EDITOR view.

use serde::{Deserialize, Serialize};

// ─── Audio Source Editor types ────────────────────────────────────────────────

/// Marker type in the waveform editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkerKind {
    /// Playback start point.
    Start,
    /// Playback end point.
    End,
    /// Loop start.
    LoopStart,
    /// Loop end.
    LoopEnd,
    /// Audio slice boundary.
    Slice,
    /// Grain region boundary.
    GrainRegion,
}

impl MarkerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Start       => "ST",
            Self::End         => "EN",
            Self::LoopStart   => "LS",
            Self::LoopEnd     => "LE",
            Self::Slice       => "SL",
            Self::GrainRegion => "GR",
        }
    }

    pub fn color_hint(self) -> u8 {
        match self {
            Self::Start       => 10,  // green
            Self::End         => 9,   // red
            Self::LoopStart   => 14,  // cyan
            Self::LoopEnd     => 14,
            Self::Slice       => 11,  // yellow
            Self::GrainRegion => 13,  // magenta
        }
    }
}

/// A positioned marker in the waveform timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorMarker {
    /// Normalised position in the buffer (0.0–1.0).
    pub position: f32,
    pub kind:     MarkerKind,
    pub label:    String,
}

impl EditorMarker {
    pub fn new(kind: MarkerKind, position: f32) -> Self {
        Self { position, kind, label: kind.label().to_string() }
    }
}

/// Loop playback mode for the sample player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SampleLoopMode {
    #[default]
    Off,
    Forward,
    PingPong,
    Backward,
}

impl SampleLoopMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off      => "Off",
            Self::Forward  => "Fwd",
            Self::PingPong => "Ping",
            Self::Backward => "Bwd",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Off      => Self::Forward,
            Self::Forward  => Self::PingPong,
            Self::PingPong => Self::Backward,
            Self::Backward => Self::Off,
        }
    }
}

/// Per-pad sample playback parameters (stored in the project, edited in EDITOR).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleParams {
    /// Start offset as fraction of clip length (0.0–1.0).
    pub start:      f32,
    /// End offset as fraction of clip length (0.0–1.0).
    pub end:        f32,
    /// Output gain (linear, 1.0 = unity).
    pub gain:       f32,
    /// Stereo pan (-1.0 = L, 0.0 = C, +1.0 = R).
    pub pan:        f32,
    /// Pitch in semitones (-24 to +24).
    pub pitch:      f32,
    /// Fine-tune in cents (-100 to +100).
    pub fine_tune:  f32,
    /// Reverse playback.
    pub reverse:    bool,
    /// Loop enabled.
    pub loop_on:    bool,
    /// Loop playback mode.
    pub loop_mode:  SampleLoopMode,
}

impl Default for SampleParams {
    fn default() -> Self {
        Self {
            start:     0.0,
            end:       1.0,
            gain:      1.0,
            pan:       0.0,
            pitch:     0.0,
            fine_tune: 0.0,
            reverse:   false,
            loop_on:   false,
            loop_mode: SampleLoopMode::Off,
        }
    }
}

/// ADSR+Hold envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdsrEnvelope {
    /// When false the envelope is bypassed (sample plays at unity). Off by
    /// default so existing pads are unaffected until the user enables it.
    #[serde(default)]
    pub enabled:    bool,
    /// Attack time in milliseconds (0–5000).
    pub attack_ms:  f32,
    /// Hold time in milliseconds (0–5000).
    pub hold_ms:    f32,
    /// Decay time in milliseconds (0–5000).
    pub decay_ms:   f32,
    /// Sustain level (0.0–1.0).
    pub sustain:    f32,
    /// Release time in milliseconds (0–10000).
    pub release_ms: f32,
}

impl Default for AdsrEnvelope {
    fn default() -> Self {
        Self {
            enabled:    false,
            attack_ms:  2.0,
            hold_ms:    0.0,
            decay_ms:   200.0,
            sustain:    0.8,
            release_ms: 100.0,
        }
    }
}

/// Filter type for the per-pad filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FilterKind {
    #[default]
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
    Off,
}

impl FilterKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lowpass  => "LP",
            Self::Highpass => "HP",
            Self::Bandpass => "BP",
            Self::Notch    => "Notch",
            Self::Off      => "Off",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Lowpass  => Self::Highpass,
            Self::Highpass => Self::Bandpass,
            Self::Bandpass => Self::Notch,
            Self::Notch    => Self::Off,
            Self::Off      => Self::Lowpass,
        }
    }
}

/// Per-pad filter parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorFilter {
    /// Cutoff frequency (0.0–1.0 normalised, mapped to 20–20000 Hz).
    pub cutoff:    f32,
    /// Resonance / Q (0.0–1.0 normalised).
    pub resonance: f32,
    pub kind:      FilterKind,
}

impl Default for EditorFilter {
    fn default() -> Self {
        Self { cutoff: 1.0, resonance: 0.1, kind: FilterKind::Off }
    }
}

/// Per-pad AMPLITUDE section: output level plus an amplitude envelope/LFO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmplitudeParams {
    /// Output level (linear, 1.0 = unity). Folded with sample gain.
    pub level:       f32,
    /// Shape amplitude with the ADSR envelope section.
    pub env_enabled: bool,
    /// Tremolo LFO on amplitude.
    pub lfo_enabled: bool,
    /// LFO rate in Hz (0.01–20).
    pub lfo_rate:    f32,
    /// LFO depth (0.0–1.0).
    pub lfo_depth:   f32,
    pub lfo_shape:   LfoShape,
}

impl Default for AmplitudeParams {
    fn default() -> Self {
        Self {
            level:       1.0,
            env_enabled: false,
            lfo_enabled: false,
            lfo_rate:    0.5,
            lfo_depth:   0.2,
            lfo_shape:   LfoShape::Sine,
        }
    }
}

/// Per-pad FREQUENCY section: detune, octave shift, harmonic count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrequencyParams {
    /// Fine detune in cents (-100 to +100), folded into pitch.
    pub detune_cents: f32,
    /// Octave shift (-4 to +4), folded into pitch as ±12 st.
    pub octave:       i32,
    /// Number of harmonic partials (1–16). Additive colour control.
    pub harmonics:    u8,
}

impl Default for FrequencyParams {
    fn default() -> Self {
        Self { detune_cents: 0.0, octave: 0, harmonics: 1 }
    }
}

/// One stacked sampler layer (voice offset relative to the base pad).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplerLayer {
    pub enabled:  bool,
    /// Layer gain (linear).
    pub gain:     f32,
    /// Pitch offset in semitones (-24 to +24).
    pub pitch_st: f32,
    /// Stereo pan (-1.0–1.0).
    pub pan:      f32,
}

impl Default for SamplerLayer {
    fn default() -> Self {
        Self { enabled: false, gain: 1.0, pitch_st: 0.0, pan: 0.0 }
    }
}

/// Maximum stacked layers per pad.
pub const MAX_LAYERS: usize = 4;

/// Per-pad LAYERS section: up to `MAX_LAYERS` stacked voices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayersParams {
    pub layers: [SamplerLayer; MAX_LAYERS],
}

impl Default for LayersParams {
    fn default() -> Self {
        Self { layers: Default::default() }
    }
}

/// Complete per-pad editor preset (sample params + envelope + filter + granular).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PadEditorPreset {
    pub sample:    SampleParams,
    pub envelope:  AdsrEnvelope,
    pub filter:    EditorFilter,
    pub markers:   Vec<EditorMarker>,
    #[serde(default)]
    pub amplitude: AmplitudeParams,
    #[serde(default)]
    pub frequency: FrequencyParams,
    #[serde(default)]
    pub layers:    LayersParams,
    /// Per-pad granular engine parameters (size/density/spray/pitch/…). Persisted
    /// so an edited granular sound reloads with the pad. `#[serde(default)]` keeps
    /// pre-granular projects loadable.
    #[serde(default)]
    pub grain:     GrainParams,
    /// Per-pad granular scan zone (position/range/scan mode/…).
    #[serde(default)]
    pub zone:      GranularZone,
}

// ─── Undo record for destructive audio edits ─────────────────────────────────

/// A destructive audio edit operation that can be undone.
#[derive(Debug, Clone)]
pub enum AudioEditOp {
    /// Silence a region [start_frac, end_frac].
    Silence  { start: f32, end: f32 },
    /// Reverse a region.
    Reverse  { start: f32, end: f32 },
    /// Normalize the entire clip.
    Normalize,
    /// Apply fade-in over [0, end_frac].
    FadeIn   { end: f32 },
    /// Apply fade-out over [start_frac, 1].
    FadeOut  { start: f32 },
    /// Delete a region [start_frac, end_frac].
    Delete   { start: f32, end: f32 },
    /// Trim: keep only [start_frac, end_frac].
    Trim     { start: f32, end: f32 },
}

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

impl GrainDirection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Forward  => "Forward",
            Self::Backward => "Backward",
            Self::Random   => "Random",
        }
    }
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

impl ScanMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear     => "Linear",
            Self::RandomWalk => "RandomWalk",
            Self::Freeze     => "Freeze",
        }
    }
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

    /// Unipolar `[0, 1]` value at the given phase (`0..1`). `cycle` indexes the
    /// current LFO period and selects the held random value for `SampleHold`.
    pub fn unipolar(self, phase: f64, cycle: u64) -> f64 {
        use std::f64::consts::TAU;
        let p = phase.rem_euclid(1.0);
        match self {
            Self::Sine => 0.5 + 0.5 * (p * TAU).sin(),
            Self::Triangle => 1.0 - (2.0 * p - 1.0).abs(),
            Self::Square => if p < 0.5 { 1.0 } else { 0.0 },
            Self::SampleHold => {
                // Deterministic per-cycle pseudo-random hold value in [0, 1].
                let h = cycle.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((h >> 11) as f64) / ((1u64 << 53) as f64)
            }
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
