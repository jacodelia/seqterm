use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const NOTE_NAMES: [&str; 12] = [
    "C-", "C#", "D-", "D#", "E-", "F-", "F#", "G-", "G#", "A-", "A#", "B-",
];

#[derive(Debug, Clone, Error)]
pub enum NoteError {
    #[error("Invalid note name: {0}")]
    InvalidNoteName(String),
    #[error("MIDI note out of range: {0}")]
    MidiOutOfRange(u8),
}

/// A single sequencer step / note event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Note {
    /// Primary note name like "C-4" or "---" for empty step.
    pub note: String,
    /// Instrument/patch index (0-based).
    pub instrument: u8,
    /// Note velocity 0-127.
    pub velocity: u8,
    /// Effect command 1 (two hex chars or "--").
    pub fx1: String,
    /// Effect command 2.
    pub fx2: String,
    /// MIDI CC 01 (modulation) value 0-127.
    pub cc01: u8,
    /// MIDI CC 74 (filter cutoff) value 0-127.
    pub cc74: u8,
    /// Gate time 0-400 (100 = 100% of step length).
    pub gate: u16,
    /// Microtiming offset -99 to +99.
    pub micro: i8,
    /// Trigger probability 0-100.
    pub prob: u8,

    // ── Per-step automation (editable in Track Modulation panel) ──────────────
    /// Gain / volume automation 0-127 (default 100).
    #[serde(default = "default_gain")]
    pub gain: u8,
    /// Stereo pan 0-127 (64 = center, 0 = full left, 127 = full right).
    #[serde(default = "default_pan")]
    pub pan: u8,
    /// Low-pass filter cutoff 0-127 (default 127 = open).
    #[serde(default = "default_lp")]
    pub lp: u8,
    /// High-pass filter cutoff 0-127 (default 0 = off).
    #[serde(default)]
    pub hp: u8,
    /// LFO depth 0-127 (default 0 = off).
    #[serde(default)]
    pub lfo: u8,
    /// Playback speed 0-127 (default 64 = normal).
    #[serde(default = "default_speed")]
    pub speed: u8,
    /// Amplitude envelope 0-127 (default 127 = full).
    #[serde(default = "default_amp")]
    pub amp: u8,

    /// Pitch bend at this step: -8192 (full down) to +8191 (full up), 0 = none.
    #[serde(default)]
    pub pitch_bend: i16,
    /// Per-note channel pressure (aftertouch) 0-127 for MPE, 0 = none.
    #[serde(default)]
    pub pressure: u8,
    /// Per-note timbre (MPE Dimension Y, maps to CC 74) 0-127, 64 = default.
    #[serde(default = "default_timbre")]
    pub timbre: u8,

    // ── Polyphony ─────────────────────────────────────────────────────────────
    /// Additional chord notes (up to 127 extra voices), parallel to chord_velocities.
    #[serde(default)]
    pub chord_notes: Vec<String>,
    /// Per-voice velocity for chord_notes (index i → velocity for chord_notes[i]).
    /// If shorter than chord_notes, missing entries default to `velocity`.
    #[serde(default)]
    pub chord_velocities: Vec<u8>,
}

fn default_gain() -> u8 { 100 }
fn default_pan() -> u8 { 64 }
fn default_lp() -> u8 { 127 }
fn default_speed() -> u8 { 64 }
fn default_amp() -> u8 { 127 }
fn default_timbre() -> u8 { 64 }

impl Default for Note {
    fn default() -> Self {
        Self {
            note: "---".to_string(),
            instrument: 0,
            velocity: 100,
            fx1: "--".to_string(),
            fx2: "--".to_string(),
            cc01: 64,
            cc74: 32,
            gate: 100,
            micro: 0,
            prob: 100,
            gain: 100,
            pan: 64,
            lp: 127,
            hp: 0,
            lfo: 0,
            speed: 64,
            amp: 127,
            pitch_bend: 0,
            pressure: 0,
            timbre: 64,
            chord_notes: Vec::new(),
            chord_velocities: Vec::new(),
        }
    }
}

impl Note {
    /// Returns `true` if this step is empty (no note triggered).
    pub fn is_empty(&self) -> bool {
        self.note == "---"
    }

    /// Construct a Note from a MIDI note number (0-127).
    pub fn from_midi(midi: u8, velocity: u8) -> Result<Self, NoteError> {
        if midi > 127 {
            return Err(NoteError::MidiOutOfRange(midi));
        }
        let octave = midi / 12;
        let semitone = (midi % 12) as usize;
        let name = format!("{}{}", NOTE_NAMES[semitone], octave);
        Ok(Self {
            note: name,
            velocity,
            ..Default::default()
        })
    }

    /// Convert this note's name to a MIDI note number, returning `None` for empty steps.
    pub fn to_midi(&self) -> Option<u8> {
        if self.is_empty() { return None; }
        parse_note_name(&self.note)
    }

    /// Return a note-on event tuple `(midi_note, velocity)` if the step is active.
    pub fn note_on(&self) -> Option<(u8, u8)> {
        self.to_midi().map(|n| (n, self.velocity))
    }

    /// Return note-on tuples for all voices (primary + chord notes).
    /// chord_velocities provides per-voice velocity; falls back to primary velocity.
    pub fn all_note_ons(&self) -> Vec<(u8, u8)> {
        let mut out = Vec::with_capacity(1 + self.chord_notes.len());
        if let Some((midi, vel)) = self.note_on() {
            out.push((midi, vel));
        }
        for (i, cn) in self.chord_notes.iter().enumerate() {
            if let Some(midi) = parse_note_name(cn) {
                let vel = self.chord_velocities.get(i).copied().unwrap_or(self.velocity);
                out.push((midi, vel));
            }
        }
        out
    }

    /// Return the velocity for chord voice `i` (0 = primary).
    pub fn voice_velocity(&self, i: usize) -> u8 {
        if i == 0 {
            self.velocity
        } else {
            self.chord_velocities.get(i - 1).copied().unwrap_or(self.velocity)
        }
    }

    /// Total number of active voices (1 = monophonic, >1 = polyphonic chord).
    pub fn voice_count(&self) -> usize {
        if self.is_empty() { 0 } else { 1 + self.chord_notes.len() }
    }
}

/// Parse a note string like "C-4" or "A#5" into a MIDI number.
pub fn parse_note_name(name: &str) -> Option<u8> {
    if name == "---" || name.len() < 3 {
        return None;
    }
    let (note_part, octave_part) = name.split_at(2);
    let octave: u8 = octave_part.parse().ok()?;
    let semitone = NOTE_NAMES.iter().position(|&n| n == note_part)?;
    Some(octave * 12 + semitone as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_midi_c4() {
        let note = Note::from_midi(60, 100).unwrap();
        assert_eq!(note.note, "C-5");
    }

    #[test]
    fn test_to_midi_empty() {
        let note = Note::default();
        assert!(note.to_midi().is_none());
    }

    #[test]
    fn test_roundtrip() {
        let note = Note::from_midi(69, 80).unwrap();
        assert_eq!(note.to_midi(), Some(69));
    }

    #[test]
    fn test_polyphonic_voice_count() {
        let mut note = Note::from_midi(60, 100).unwrap();
        assert_eq!(note.voice_count(), 1);
        note.chord_notes.push("E-5".to_string());
        assert_eq!(note.voice_count(), 2);
    }

    #[test]
    fn empty_note_is_empty() {
        let n = Note::default();
        assert!(n.is_empty());
    }

    #[test]
    fn midi_note_is_not_empty() {
        let n = Note::from_midi(60, 100).unwrap();
        assert!(!n.is_empty());
    }

    #[test]
    fn all_note_ons_single() {
        let n = Note::from_midi(48, 90).unwrap();
        let ons = n.all_note_ons();
        assert_eq!(ons.len(), 1);
        assert_eq!(ons[0], (48, 90));
    }

    #[test]
    fn all_note_ons_chord() {
        let mut n = Note::from_midi(60, 100).unwrap();
        n.chord_notes.push("E-5".to_string()); // E5 = MIDI 76
        n.chord_notes.push("G-5".to_string()); // G5 = MIDI 79
        let ons = n.all_note_ons();
        assert_eq!(ons.len(), 3);
        assert_eq!(ons[0].0, 60);
    }

    #[test]
    fn from_midi_range_check() {
        assert!(Note::from_midi(127, 100).is_ok());
        // MIDI note 128 is out of range.
        assert!(Note::from_midi(128, 100).is_err());
    }

    #[test]
    fn parse_note_name_various() {
        assert_eq!(parse_note_name("C-0"), Some(0));
        assert_eq!(parse_note_name("C-5"), Some(60));
        assert_eq!(parse_note_name("A-4"), Some(57));
        assert_eq!(parse_note_name("---"), None);
        assert_eq!(parse_note_name(""), None);
    }

    #[test]
    fn note_name_roundtrip_all_midi() {
        for midi in 0u8..=127 {
            let n = Note::from_midi(midi, 100).unwrap();
            let back = n.to_midi().unwrap();
            assert_eq!(back, midi, "roundtrip failed for MIDI {midi}");
        }
    }

    #[test]
    fn velocity_defaults() {
        let n = Note::from_midi(60, 64).unwrap();
        assert_eq!(n.velocity, 64);
        assert_eq!(n.prob, 100); // default probability
        assert_eq!(n.gate, 100); // default gate
    }
}
