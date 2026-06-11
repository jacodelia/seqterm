//! SP-404-style sampler pad domain types.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::granular::PadEditorPreset;

/// How a pad triggers its sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TriggerMode {
    /// Play from start to end once; NoteOff ignored.
    #[default]
    OneShot,
    /// Loop between loop_start and loop_end while gate is held.
    Loop,
    /// Play only while gate is held; stop on NoteOff.
    Gate,
    /// Retriggered from start on every NoteOn.
    Retrigger,
}

/// Mute group (0 = no group, 1-8 = exclusive mute group).
/// When a pad in group N plays, all other playing pads in group N are stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MuteGroup(pub u8);

/// Choke group (0 = no group, 1-8).
/// When a pad in group N plays, all pads *in the same choke group* are silenced immediately (no fade).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChokeGroup(pub u8);

/// A single pad slot — sample assignment + playback parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PadSlot {
    pub path:        PathBuf,
    pub trigger:     TriggerMode,
    pub mute_group:  MuteGroup,
    pub choke_group: ChokeGroup,

    /// Pitch shift in semitones (-24.0 to +24.0).
    pub pitch_st:    f32,
    /// Amplitude gain (linear, 1.0 = unity).
    pub gain:        f32,
    /// Pan (-1.0 = L, 0.0 = C, +1.0 = R).
    pub pan:         f32,
    /// Play reversed.
    pub reverse:     bool,

    /// Trim: start offset as fraction of sample length (0.0–1.0).
    pub trim_start:  f32,
    /// Trim: end offset as fraction of sample length (0.0–1.0).
    pub trim_end:    f32,

    /// Loop start as fraction of sample length (only used in Loop mode).
    pub loop_start:  f32,
    /// Loop end as fraction of sample length.
    pub loop_end:    f32,

    /// Velocity scaling: 0.0 = no scaling, 1.0 = full velocity-to-volume mapping.
    pub vel_to_vol:  f32,
    /// Peak-normalize to 0 dBFS on load when true.
    pub normalize:   bool,
    /// Number of retriggles within one step (1 = single trigger, 2–8 = retrigger at even sub-step intervals).
    pub retrigger:   u8,

    /// Audio Source Editor preset for this pad: holds the editor-only parameters
    /// (fine-tune, loop mode, ADSR envelope, filter, waveform markers) that have
    /// no canonical `PadSlot` field. The overlapping params (gain, pan, pitch,
    /// reverse, trim) remain authoritative in the fields above and are mirrored
    /// into `editor.sample` on save. `#[serde(default)]` keeps old projects loadable.
    #[serde(default)]
    pub editor:      PadEditorPreset,
}

impl PadSlot {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            trigger:     TriggerMode::OneShot,
            mute_group:  MuteGroup(0),
            choke_group: ChokeGroup(0),
            pitch_st:    0.0,
            gain:        1.0,
            pan:         0.0,
            reverse:     false,
            trim_start:  0.0,
            trim_end:    1.0,
            loop_start:  0.0,
            loop_end:    1.0,
            vel_to_vol:  0.8,
            normalize:   false,
            retrigger:   1,
            editor:      PadEditorPreset::default(),
        }
    }
}

/// 16 pads in one bank.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PadBank {
    pub name:  String,
    pub slots: [Option<PadSlot>; 16],
}

impl PadBank {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            slots: Default::default(),
        }
    }

    pub fn assign(&mut self, pad: usize, slot: PadSlot) {
        if pad < 16 { self.slots[pad] = Some(slot); }
    }

    pub fn clear(&mut self, pad: usize) {
        if pad < 16 { self.slots[pad] = None; }
    }
}

/// Full sampler configuration stored in the project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplerConfig {
    /// Up to 16 banks (A–P).
    pub banks:       Vec<PadBank>,
    pub active_bank: usize,
    /// Skip-back buffer duration in seconds.
    pub skip_back_secs: u32,
}

impl Default for SamplerConfig {
    fn default() -> Self {
        let banks = (0..4)
            .map(|i| PadBank::new(format!("{}", (b'A' + i) as char)))
            .collect();
        Self { banks, active_bank: 0, skip_back_secs: 8 }
    }
}

impl SamplerConfig {
    pub fn active_bank(&self) -> Option<&PadBank> {
        self.banks.get(self.active_bank)
    }

    pub fn active_bank_mut(&mut self) -> Option<&mut PadBank> {
        self.banks.get_mut(self.active_bank)
    }

    /// Find active pads in a mute group (used for choke/mute logic in the engine).
    pub fn pads_in_mute_group(&self, bank: usize, group: MuteGroup) -> Vec<usize> {
        if group.0 == 0 { return Vec::new(); }
        self.banks.get(bank)
            .map(|b| b.slots.iter().enumerate()
                .filter(|(_, s)| s.as_ref().map_or(false, |p| p.mute_group == group))
                .map(|(i, _)| i)
                .collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_bank_assign_and_clear() {
        let mut bank = PadBank::new("A");
        bank.assign(0, PadSlot::new(PathBuf::from("kick.wav")));
        assert!(bank.slots[0].is_some());
        bank.clear(0);
        assert!(bank.slots[0].is_none());
    }

    #[test]
    fn sampler_config_default_has_four_banks() {
        let cfg = SamplerConfig::default();
        assert_eq!(cfg.banks.len(), 4);
        assert_eq!(cfg.banks[0].name, "A");
    }

    #[test]
    fn mute_group_zero_returns_empty() {
        let cfg = SamplerConfig::default();
        assert!(cfg.pads_in_mute_group(0, MuteGroup(0)).is_empty());
    }

    #[test]
    fn legacy_padslot_without_editor_field_deserializes() {
        // A PadSlot serialized before the `editor` field existed must still load,
        // defaulting `editor` to PadEditorPreset::default() (serde(default)).
        let legacy = r#"{
            "path": "kick.wav",
            "trigger": "OneShot",
            "mute_group": 0,
            "choke_group": 0,
            "pitch_st": 0.0,
            "gain": 1.0,
            "pan": 0.0,
            "reverse": false,
            "trim_start": 0.0,
            "trim_end": 1.0,
            "loop_start": 0.0,
            "loop_end": 1.0,
            "vel_to_vol": 0.8,
            "normalize": false,
            "retrigger": 1
        }"#;
        let slot: PadSlot = serde_json::from_str(legacy).expect("legacy PadSlot must deserialize");
        assert_eq!(slot.gain, 1.0);
        assert!(slot.editor.markers.is_empty());
    }

    #[test]
    fn padslot_editor_roundtrips() {
        let mut slot = PadSlot::new(PathBuf::from("snare.wav"));
        slot.editor.sample.fine_tune = 25.0;
        slot.editor.markers.push(crate::EditorMarker::new(crate::MarkerKind::Slice, 0.5));
        // New EDITOR sections: amplitude / frequency / layers.
        slot.editor.amplitude.level = 0.5;
        slot.editor.amplitude.lfo_enabled = true;
        slot.editor.frequency.octave = -2;
        slot.editor.frequency.harmonics = 7;
        slot.editor.layers.layers[1].enabled = true;
        slot.editor.layers.layers[1].pitch_st = 12.0;
        let json = serde_json::to_string(&slot).unwrap();
        let back: PadSlot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.editor.sample.fine_tune, 25.0);
        assert_eq!(back.editor.markers.len(), 1);
        assert_eq!(back.editor.amplitude.level, 0.5);
        assert!(back.editor.amplitude.lfo_enabled);
        assert_eq!(back.editor.frequency.octave, -2);
        assert_eq!(back.editor.frequency.harmonics, 7);
        assert!(back.editor.layers.layers[1].enabled);
        assert_eq!(back.editor.layers.layers[1].pitch_st, 12.0);
    }
}
