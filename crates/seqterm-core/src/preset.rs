//! Universal instrument/effect presets.
//!
//! A [`Preset`] is a portable snapshot of an instrument's state: metadata, the
//! normalised parameter values, and the [`ModulationSystem`] (routes + macros,
//! which also carries learned MIDI-CC mappings). Presets serialize to JSON for
//! import/export and are stored in the project's preset library.

use serde::{Deserialize, Serialize};

use crate::modulation::ModulationSystem;

/// Descriptive metadata for a preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PresetMetadata {
    pub name: String,
    #[serde(default)]
    pub author: String,
    /// Identifier of the instrument/effect this preset targets (plugin id,
    /// SF2 path, "builtin", …). Empty = generic.
    #[serde(default)]
    pub instrument_id: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub notes: String,
}

/// A single stored parameter value (normalised `[0, 1]`), keyed by stable id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetParam {
    pub id: String,
    pub value: f64,
}

/// A portable instrument/effect state snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Preset {
    pub metadata: PresetMetadata,
    #[serde(default)]
    pub parameters: Vec<PresetParam>,
    #[serde(default)]
    pub modulation: ModulationSystem,
}

impl Preset {
    /// Build a preset from a name and `(parameter_id, normalised_value)` pairs.
    pub fn from_param_pairs(
        name: impl Into<String>,
        params: impl IntoIterator<Item = (String, f64)>,
        modulation: ModulationSystem,
    ) -> Self {
        Self {
            metadata: PresetMetadata { name: name.into(), ..Default::default() },
            parameters: params.into_iter().map(|(id, value)| PresetParam { id, value }).collect(),
            modulation,
        }
    }

    /// The `(id, value)` pairs as a borrowing iterator.
    pub fn param_pairs(&self) -> impl Iterator<Item = (&str, f64)> {
        self.parameters.iter().map(|p| (p.id.as_str(), p.value))
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Parse from JSON.
    pub fn from_json(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }

    /// A duplicate with a new name (e.g. "Bass" → "Bass copy").
    pub fn duplicate(&self, new_name: impl Into<String>) -> Self {
        let mut p = self.clone();
        p.metadata.name = new_name.into();
        p
    }

    /// Parameter ids whose stored value differs from `other` by more than `eps`
    /// — used by the A/B snapshot comparison.
    pub fn diff_params(&self, other: &Preset, eps: f64) -> Vec<String> {
        let mut diffs = Vec::new();
        for p in &self.parameters {
            let ov = other.parameters.iter().find(|q| q.id == p.id).map(|q| q.value);
            match ov {
                Some(v) if (v - p.value).abs() <= eps => {}
                _ => diffs.push(p.id.clone()),
            }
        }
        // Parameters present only in `other`.
        for q in &other.parameters {
            if !self.parameters.iter().any(|p| p.id == q.id) {
                diffs.push(q.id.clone());
            }
        }
        diffs
    }
}

/// An A/B snapshot pair for quick comparison while sound-designing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotAB {
    pub a: Option<Preset>,
    pub b: Option<Preset>,
    /// Which slot is currently active (false = A, true = B).
    #[serde(default)]
    pub b_active: bool,
}

impl SnapshotAB {
    pub fn store_a(&mut self, p: Preset) { self.a = Some(p); }
    pub fn store_b(&mut self, p: Preset) { self.b = Some(p); }
    pub fn toggle(&mut self) { self.b_active = !self.b_active; }
    /// The currently active snapshot.
    pub fn active(&self) -> Option<&Preset> {
        if self.b_active { self.b.as_ref() } else { self.a.as_ref() }
    }
    /// Parameter ids that differ between A and B.
    pub fn diff(&self, eps: f64) -> Vec<String> {
        match (&self.a, &self.b) {
            (Some(a), Some(b)) => a.diff_params(b, eps),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modulation::{ModulationRoute, ModulationSource, ModulationSystem};

    fn sample_preset() -> Preset {
        let mut modu = ModulationSystem::default();
        modu.routes.push(ModulationRoute::new(ModulationSource::MidiCc(74), "cutoff", 0.5));
        Preset::from_param_pairs(
            "Lead",
            [("cutoff".to_string(), 0.7), ("res".to_string(), 0.2)],
            modu,
        )
    }

    #[test]
    fn json_roundtrip() {
        let p = sample_preset();
        let json = p.to_json().unwrap();
        let back = Preset::from_json(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn duplicate_renames() {
        let p = sample_preset();
        let d = p.duplicate("Lead copy");
        assert_eq!(d.metadata.name, "Lead copy");
        assert_eq!(d.parameters, p.parameters);
    }

    #[test]
    fn ab_compare_diff() {
        let a = sample_preset();
        let mut b = a.clone();
        b.parameters[0].value = 0.1; // change cutoff
        let mut ab = SnapshotAB::default();
        ab.store_a(a);
        ab.store_b(b);
        assert_eq!(ab.diff(1e-6), vec!["cutoff".to_string()]);
        assert!(!ab.b_active);
        ab.toggle();
        assert!(ab.b_active);
    }
}
