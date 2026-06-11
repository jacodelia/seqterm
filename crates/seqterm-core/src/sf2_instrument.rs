//! Editable SoundFont (SF2) instrument model.
//!
//! A structured, serializable representation of an SF2 instrument's editable
//! generators — zone mapping, amplitude envelope, filter, LFO, loop points and
//! sample gain — matching the editor sections in `01_editorUpdate2.md`. The
//! values map onto the universal parameter model (see the `ParameterProvider`
//! impl in `seqterm-ports`) so the same auto-generated inspector edits them.

use serde::{Deserialize, Serialize};

/// Loop playback mode for a zone's sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Sf2LoopMode {
    #[default]
    None,
    Forward,
    PingPong,
}

impl Sf2LoopMode {
    pub const ALL: [Self; 3] = [Self::None, Self::Forward, Self::PingPong];
    pub fn label(self) -> &'static str {
        match self { Self::None => "None", Self::Forward => "Forward", Self::PingPong => "PingPong" }
    }
}

/// Filter type for a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Sf2FilterType {
    #[default]
    LowPass,
    HighPass,
    BandPass,
}

impl Sf2FilterType {
    pub const ALL: [Self; 3] = [Self::LowPass, Self::HighPass, Self::BandPass];
    pub fn label(self) -> &'static str {
        match self { Self::LowPass => "LPF", Self::HighPass => "HPF", Self::BandPass => "BPF" }
    }
}

/// LFO waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Sf2LfoWaveform {
    #[default]
    Sine,
    Triangle,
    Square,
    Saw,
}

impl Sf2LfoWaveform {
    pub const ALL: [Self; 4] = [Self::Sine, Self::Triangle, Self::Square, Self::Saw];
    pub fn label(self) -> &'static str {
        match self {
            Self::Sine => "Sine", Self::Triangle => "Triangle",
            Self::Square => "Square", Self::Saw => "Saw",
        }
    }
}

/// One editable SF2 zone: a sample mapped to a key/velocity region plus its
/// envelope, filter, LFO and loop generators.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sf2Zone {
    pub sample_name: String,

    // ── Zone mapping ─────────────────────────────────────────────────────────
    pub key_low: u8,
    pub key_high: u8,
    pub vel_low: u8,
    pub vel_high: u8,
    pub root_key: u8,
    /// Fine tune, cents (-100..=100).
    pub fine_tune: i32,
    /// Coarse tune, semitones (-64..=64).
    pub coarse_tune: i32,

    // ── Amplitude envelope (seconds; sustain 0..1) ───────────────────────────
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,

    // ── Filter ───────────────────────────────────────────────────────────────
    pub filter_type: Sf2FilterType,
    /// Cutoff, Hz.
    pub cutoff: f32,
    /// Resonance, 0..1.
    pub resonance: f32,
    /// Key tracking, 0..1.
    pub key_tracking: f32,

    // ── LFO ──────────────────────────────────────────────────────────────────
    pub lfo_waveform: Sf2LfoWaveform,
    /// LFO frequency, Hz.
    pub lfo_freq: f32,
    /// LFO delay before onset, seconds.
    pub lfo_delay: f32,
    /// LFO depth, 0..1.
    pub lfo_depth: f32,

    // ── Loop ─────────────────────────────────────────────────────────────────
    pub loop_mode: Sf2LoopMode,
    pub loop_start: u32,
    pub loop_end: u32,
    /// Loop crossfade, milliseconds.
    pub loop_crossfade: f32,

    // ── Sample ───────────────────────────────────────────────────────────────
    /// Output gain, dB.
    pub gain_db: f32,
}

impl Default for Sf2Zone {
    fn default() -> Self {
        Self {
            sample_name: String::new(),
            key_low: 0, key_high: 127, vel_low: 0, vel_high: 127, root_key: 60,
            fine_tune: 0, coarse_tune: 0,
            attack: 0.005, hold: 0.0, decay: 0.2, sustain: 0.8, release: 0.3,
            filter_type: Sf2FilterType::LowPass, cutoff: 20_000.0, resonance: 0.0, key_tracking: 0.0,
            lfo_waveform: Sf2LfoWaveform::Sine, lfo_freq: 5.0, lfo_delay: 0.0, lfo_depth: 0.0,
            loop_mode: Sf2LoopMode::None, loop_start: 0, loop_end: 0, loop_crossfade: 0.0,
            gain_db: 0.0,
        }
    }
}

impl Sf2Zone {
    pub fn new(sample_name: impl Into<String>) -> Self {
        Self { sample_name: sample_name.into(), ..Default::default() }
    }

    /// True if `(note, velocity)` falls within this zone's mapping.
    pub fn matches(&self, note: u8, velocity: u8) -> bool {
        note >= self.key_low && note <= self.key_high
            && velocity >= self.vel_low && velocity <= self.vel_high
    }
}

/// An editable SF2 instrument: a named set of zones (velocity layers / key
/// splits) plus a selected zone for editing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Sf2Instrument {
    pub name: String,
    pub zones: Vec<Sf2Zone>,
    /// Index of the zone currently being edited.
    #[serde(default)]
    pub selected: usize,
}

impl Sf2Instrument {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), zones: vec![Sf2Zone::default()], selected: 0 }
    }

    pub fn selected_zone(&self) -> Option<&Sf2Zone> { self.zones.get(self.selected) }
    pub fn selected_zone_mut(&mut self) -> Option<&mut Sf2Zone> { self.zones.get_mut(self.selected) }

    /// All zones whose mapping contains `(note, velocity)`, in order — supports
    /// velocity layering / round-robin selection by the player.
    pub fn zones_for(&self, note: u8, velocity: u8) -> impl Iterator<Item = (usize, &Sf2Zone)> {
        self.zones.iter().enumerate().filter(move |(_, z)| z.matches(note, velocity))
    }

    /// Auto-split the keyboard evenly across the current zones (mapping editor
    /// helper): zone *i* covers an equal slice of the 0–127 key range.
    pub fn auto_split_keys(&mut self) {
        let n = self.zones.len().max(1);
        let span = 128.0 / n as f32;
        for (i, z) in self.zones.iter_mut().enumerate() {
            z.key_low = (i as f32 * span).round() as u8;
            z.key_high = (((i + 1) as f32 * span).round() as i32 - 1).clamp(0, 127) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_matches_region() {
        let mut z = Sf2Zone::new("kick");
        z.key_low = 36; z.key_high = 36; z.vel_low = 0; z.vel_high = 127;
        assert!(z.matches(36, 100));
        assert!(!z.matches(37, 100));
    }

    #[test]
    fn auto_split_covers_keyboard() {
        let mut inst = Sf2Instrument::new("Split");
        inst.zones = vec![Sf2Zone::new("lo"), Sf2Zone::new("hi")];
        inst.auto_split_keys();
        assert_eq!(inst.zones[0].key_low, 0);
        assert_eq!(inst.zones[1].key_high, 127);
        // Contiguous, non-overlapping split.
        assert_eq!(inst.zones[0].key_high as u16 + 1, inst.zones[1].key_low as u16);
    }

    #[test]
    fn json_roundtrip() {
        let inst = Sf2Instrument::new("Piano");
        let json = serde_json::to_string(&inst).unwrap();
        let back: Sf2Instrument = serde_json::from_str(&json).unwrap();
        assert_eq!(inst, back);
    }
}
