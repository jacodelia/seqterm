use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Note;

// ─── PatternSource ────────────────────────────────────────────────────────────

/// Defines what drives a clip's audio output.
///
/// - `Midi`: classic mode — MIDI notes routed to an external port/synth.
/// - `Sf2`: built-in SF2 synthesis via seqterm-audio-engine (oxisynth).
/// - `AudioFile`: sample playback (WAV/FLAC/MP3/OGG).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PatternSource {
    /// Route MIDI to an external port (default, backwards-compatible).
    Midi,
    /// Use an SF2 SoundFont bank+preset for synthesis.
    Sf2 {
        /// Path to the .sf2 file (stored relative to project root on save).
        path: PathBuf,
        /// SF2 bank number (0-127).
        bank: u8,
        /// SF2 preset / program number (0-127).
        preset: u8,
        /// Human-readable preset name (cached, not authoritative).
        #[serde(default)]
        preset_name: String,
    },
    /// Play back an audio file (WAV, FLAC, MP3, OGG).
    AudioFile {
        /// Path to the audio file (stored relative to project root on save).
        path: PathBuf,
        /// Whether to loop the sample.
        #[serde(default)]
        looping: bool,
        /// BPM hint for time-stretch sync (0.0 = no stretch).
        #[serde(default)]
        original_bpm: f64,
        /// Volume gain multiplier (1.0 = unity).
        #[serde(default = "default_audio_gain")]
        gain: f32,
    },
}

fn default_audio_gain() -> f32 { 1.0 }

impl Default for PatternSource {
    fn default() -> Self {
        PatternSource::Midi
    }
}

impl PatternSource {
    /// Short display label for UI cells.
    pub fn kind_label(&self) -> &'static str {
        match self {
            PatternSource::Midi => "MIDI",
            PatternSource::Sf2 { .. } => "SF2",
            PatternSource::AudioFile { .. } => "AUDIO",
        }
    }

    /// Icon character for matrix cell display.
    pub fn icon(&self) -> char {
        match self {
            PatternSource::Midi => ' ',
            PatternSource::Sf2 { .. } => '♪',
            PatternSource::AudioFile { .. } => '▶',
        }
    }

    pub fn is_midi(&self) -> bool { matches!(self, PatternSource::Midi) }
    pub fn is_sf2(&self) -> bool { matches!(self, PatternSource::Sf2 { .. }) }
    pub fn is_audio(&self) -> bool { matches!(self, PatternSource::AudioFile { .. }) }
}

fn default_euclid_fill() -> usize { 3 }
fn default_euclid_len() -> usize { 16 }
fn default_humanization() -> u8 { 0 }
fn default_time_sig_num() -> u8 { 4 }
fn default_time_sig_den() -> u8 { 4 }

/// A sequence of up to 128 steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub name: String,
    /// The actual step data.
    pub steps: Vec<Note>,
    /// Active step count (1-128).
    pub length: usize,
    /// Swing percentage (50 = no swing, 54 = light swing).
    pub swing: u8,
    /// Random variation amount (0-100).
    pub random: u8,
    /// Pattern trigger probability (0-100).
    pub prob: u8,
    /// Euclidean rhythm: number of active pulses.
    #[serde(default = "default_euclid_fill")]
    pub euclid_fill: usize,
    /// Euclidean rhythm: total step count for the pattern.
    #[serde(default = "default_euclid_len")]
    pub euclid_len: usize,
    /// Humanization amount 0-100%.
    #[serde(default = "default_humanization")]
    pub humanization: u8,
    /// Evolution speed: 0=off, 1=slow, 2=medium, 3=fast.
    #[serde(default)]
    pub evolution: u8,
    /// Probability lock (freezes randomization).
    #[serde(default)]
    pub prob_lock: bool,
    /// Pattern-level microshift (-99..+99 ticks).
    #[serde(default)]
    pub microshift: i8,
    /// Time signature numerator (beats per measure, 1-128).
    #[serde(default = "default_time_sig_num")]
    pub time_sig_num: u8,
    /// Time signature denominator (beat unit, 1-128).
    #[serde(default = "default_time_sig_den")]
    pub time_sig_den: u8,
    /// Beat grouping within a measure, e.g. [3,2,2] for 7/8 → 3+2+2.
    /// Empty means one undivided group equal to time_sig_num.
    #[serde(default)]
    pub beat_groups: Vec<u8>,
}

impl Pattern {
    /// Maximum pattern length (steps). Long enough for full-piece MIDI imports
    /// while keeping alloc bounded.
    pub const MAX_STEPS: usize = 8192;

    pub fn new(name: impl Into<String>, length: usize) -> Self {
        let length = length.clamp(1, Self::MAX_STEPS);
        Self {
            name: name.into(),
            steps: vec![Note::default(); length],
            length,
            swing: 50,
            random: 0,
            prob: 0,
            euclid_fill: 3,
            euclid_len: length.min(16).max(2),
            humanization: 0,
            evolution: 0,
            prob_lock: false,
            microshift: 0,
            time_sig_num: 4,
            time_sig_den: 4,
            beat_groups: vec![],
        }
    }

    /// Return an immutable reference to the step at `index`, if in range.
    pub fn step(&self, index: usize) -> Option<&Note> {
        self.steps.get(index)
    }

    /// Return a mutable reference to the step at `index`, if in range.
    pub fn step_mut(&mut self, index: usize) -> Option<&mut Note> {
        self.steps.get_mut(index)
    }

    /// Set a note at the given step index.
    pub fn set_step(&mut self, index: usize, note: Note) {
        if index < self.steps.len() {
            self.steps[index] = note;
        }
    }

    /// Clear (silence) a step.
    pub fn clear_step(&mut self, index: usize) {
        if index < self.steps.len() {
            self.steps[index] = Note::default();
        }
    }

    /// Calculate the swing offset in ticks for a given step.
    /// `ppqn` is the pulse-per-quarter-note resolution.
    pub fn swing_offset(&self, step: usize, ppqn: u32) -> i32 {
        if step % 2 == 1 {
            ((self.swing as i32 - 50) * ppqn as i32) / 100
        } else {
            0
        }
    }

    /// Rounded-up step count to fit complete measures of the pattern's time signature.
    /// e.g. length=32, time_sig_num=7 → ceil(32/7)*7 = 35.
    pub fn effective_length(&self) -> usize {
        let num = self.time_sig_num.max(1) as usize;
        ((self.length + num - 1) / num) * num
    }

    /// Quantize all note `micro` fields toward zero.
    ///
    /// - `strength` 0-100: how much to pull micro toward the grid (100 = snap fully).
    /// - `grid_divs` 1-32: subdivision of each step (1 = step-level, 2 = half-step, 4 = quarter-step…).
    ///   At grid_divs > 1 each step is divided into `grid_divs` slots; micro is snapped to the
    ///   nearest slot boundary.
    /// - `swing_aware`: if true and `self.swing != 50`, even steps are left untouched (preserving swing groove).
    pub fn quantize(&mut self, strength: u8, grid_divs: usize, swing_aware: bool) {
        let s = (strength as f32 / 100.0).clamp(0.0, 1.0);
        let divs = grid_divs.max(1) as i8;
        for (step_idx, note) in self.steps.iter_mut().enumerate() {
            if note.is_empty() { continue; }
            // Skip even steps when swing-aware (swing offsets them intentionally).
            if swing_aware && self.swing != 50 && step_idx % 2 == 1 { continue; }

            if divs <= 1 {
                // Simple: pull micro straight to zero.
                note.micro = (note.micro as f32 * (1.0 - s)).round() as i8;
            } else {
                // Snap to nearest 1/divs boundary within [-99, 99].
                let slot_size = 100i8 / divs;
                let nearest_slot = ((note.micro as f32 / slot_size as f32).round() as i8)
                    .clamp(-divs + 1, divs - 1);
                let target = nearest_slot * slot_size;
                note.micro = (note.micro as f32 + (target - note.micro) as f32 * s).round() as i8;
            }
        }
    }

    /// Humanize timing: add random micro-offsets up to `amount` percent.
    /// Preserves existing micro values by adding to them (clamped to ±99).
    pub fn humanize_timing(&mut self, amount: u8) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let range = amount.min(99) as i8;
        for (i, note) in self.steps.iter_mut().enumerate() {
            if note.is_empty() { continue; }
            // Deterministic pseudo-random from step index + current micro.
            let mut h = DefaultHasher::new();
            (i, note.micro).hash(&mut h);
            let r = ((h.finish() as i8).wrapping_add(note.micro)) % (range + 1);
            note.micro = (note.micro + r).clamp(-99, 99);
        }
    }

    /// Returns the active beat grouping, validating that groups sum to time_sig_num.
    /// Falls back to [time_sig_num] if unset or the sum is inconsistent.
    pub fn effective_groups(&self) -> Vec<u8> {
        let n = self.time_sig_num.max(1);
        let sum: u8 = self.beat_groups.iter().copied().fold(0u8, |a, b| a.saturating_add(b));
        if self.beat_groups.is_empty() || sum != n {
            vec![n]
        } else {
            self.beat_groups.clone()
        }
    }
}

impl Default for Pattern {
    fn default() -> Self {
        Self::new("EMPTY", 64)
    }
}

/// All ordered compositions of `n` into parts of 2 and 3 (plus the trivial [n]).
/// For n > 16 only [[n]] is returned to avoid combinatorial explosion.
pub fn musical_groupings(n: u8) -> Vec<Vec<u8>> {
    let n = n.max(1);
    if n > 16 {
        return vec![vec![n]];
    }
    let mut comps: Vec<Vec<u8>> = Vec::new();
    fn compose(rem: u8, cur: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        if rem == 0 { out.push(cur.clone()); return; }
        for p in [2u8, 3u8] {
            if p <= rem { cur.push(p); compose(rem - p, cur, out); cur.pop(); }
        }
    }
    let mut buf = Vec::new();
    compose(n, &mut buf, &mut comps);
    comps.sort();
    comps.dedup();
    comps.retain(|g| g.as_slice() != [n]);
    let mut result = vec![vec![n]];
    result.extend(comps);
    result
}

fn default_clip_enabled() -> bool { true }
fn default_midi_channel() -> u8 { 1 }

/// A clip is a (row, col) slot in the session matrix that references a pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    /// The pattern key this clip references (e.g. "KCK01").
    pub pattern_key: Option<String>,
    /// Whether the clip is currently scheduled to play.
    pub playing: bool,
    /// Current playback step within the pattern.
    pub current_step: usize,
    /// Matrix row (0-7, A-H).
    pub row: usize,
    /// Matrix column (0-7).
    pub col: usize,
    /// Whether the clip is enabled (false = muted/disabled).
    #[serde(default = "default_clip_enabled")]
    pub enabled: bool,
    /// MIDI output port name to route this clip's pattern to (None = unrouted).
    #[serde(default)]
    pub midi_out: Option<String>,
    /// MIDI output channel (1-16).
    #[serde(default = "default_midi_channel")]
    pub midi_channel: u8,
    /// Audio source type: MIDI (default), SF2, or AudioFile.
    #[serde(default)]
    pub source: PatternSource,
    /// When Some, MIDI output uses MPE channel allocation for this clip.
    #[serde(default)]
    pub mpe_zone: Option<crate::mpe::MpeZone>,
    /// Frozen state: when true, this clip is playing a rendered audio stem.
    /// The original source (MIDI/SF2) is preserved in `freeze_source` for unfreeze.
    #[serde(default)]
    pub frozen: bool,
    /// The original source saved before freezing. Restored on unfreeze.
    #[serde(default)]
    pub freeze_source: Option<Box<PatternSource>>,
}

impl Clip {
    pub fn new(name: impl Into<String>, row: usize, col: usize) -> Self {
        Self {
            name: name.into(),
            pattern_key: None,
            playing: false,
            current_step: 0,
            row,
            col,
            enabled: true,
            midi_out: None,
            midi_channel: 1,
            source: PatternSource::default(),
            mpe_zone: None,
            frozen: false,
            freeze_source: None,
        }
    }

    /// Assign an SF2 source to this clip.
    pub fn with_sf2(mut self, path: impl Into<PathBuf>, bank: u8, preset: u8) -> Self {
        self.source = PatternSource::Sf2 {
            path: path.into(),
            bank,
            preset,
            preset_name: String::new(),
        };
        self
    }

    /// Assign an audio file source to this clip.
    pub fn with_audio(mut self, path: impl Into<PathBuf>, looping: bool) -> Self {
        self.source = PatternSource::AudioFile {
            path: path.into(),
            looping,
            original_bpm: 0.0,
            gain: 1.0,
        };
        self
    }

    pub fn with_pattern(mut self, key: impl Into<String>) -> Self {
        self.pattern_key = Some(key.into());
        self
    }

    pub fn with_channel(mut self, channel: u8) -> Self {
        self.midi_channel = channel.clamp(1, 16);
        self
    }

    pub fn row_label(&self) -> char {
        (b'A' + self.row as u8) as char
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::Note;

    fn make_pattern(len: usize) -> Pattern {
        Pattern::new("TEST", len)
    }

    #[test]
    fn pattern_default_steps_empty() {
        let p = make_pattern(16);
        assert_eq!(p.length, 16);
        assert!(p.steps.iter().all(|s| s.is_empty()));
    }

    #[test]
    fn set_and_get_step() {
        let mut p = make_pattern(8);
        let note = Note::from_midi(60, 100).unwrap();
        p.set_step(0, note.clone());
        assert_eq!(p.step(0).unwrap().to_midi(), Some(60));
    }

    #[test]
    fn clear_step_makes_it_empty() {
        let mut p = make_pattern(8);
        let note = Note::from_midi(64, 80).unwrap();
        p.set_step(3, note);
        assert!(!p.step(3).unwrap().is_empty());
        p.clear_step(3);
        assert!(p.step(3).unwrap().is_empty());
    }

    #[test]
    fn step_out_of_bounds_returns_none() {
        let p = make_pattern(8);
        assert!(p.step(8).is_none());
        assert!(p.step(100).is_none());
    }

    #[test]
    fn effective_length_defaults_to_length() {
        let p = make_pattern(16);
        assert_eq!(p.effective_length(), 16);
    }

    #[test]
    fn pattern_polymeter_phase() {
        let p = make_pattern(6);
        // The scheduler uses `global_step % pat.length`.
        for global_step in 0..18usize {
            let pos = global_step % p.length;
            assert!(pos < p.length);
        }
    }

    #[test]
    fn clip_row_label() {
        let c = Clip::new("PAT1", 0, 0);
        assert_eq!(c.row_label(), 'A');
        let c2 = Clip::new("PAT2", 7, 0);
        assert_eq!(c2.row_label(), 'H');
    }

    #[test]
    fn clip_with_sf2_builder() {
        let c = Clip::new("X", 0, 0)
            .with_sf2("/usr/share/sounds/bank.sf2", 0, 1);
        assert!(c.source.is_sf2());
    }

    #[test]
    fn clip_with_audio_builder() {
        let c = Clip::new("X", 0, 0)
            .with_audio("/tmp/loop.wav", true);
        assert!(c.source.is_audio());
    }
}

