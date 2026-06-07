//! Universal Instrument Engine — format-agnostic parameter model.
//!
//! This is the backbone abstraction of SeqTerm's instrument/plugin subsystem.
//! The UI and the modulation / automation / preset engines talk **only** to
//! these types; concrete formats (SF2, SFZ, LV2, VST2/3, CLAP, internal synths)
//! expose their controls by implementing [`ParameterProvider`] in an adapter.
//!
//! Design rules (see `01_editorUpdate2.md`):
//! - No format-specific logic leaks into the UI.
//! - Every concrete parameter is mapped onto the universal [`Parameter`] model.
//! - Values are stored in native range; helpers convert to/from 0–1 normalised.

use serde::{Deserialize, Serialize};

/// The kind of a universal parameter, used by editors to pick a control widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParameterType {
    /// Continuous real value (slider / knob).
    Float,
    /// Discrete integer value (spin box).
    Integer,
    /// On/off (switch).
    Boolean,
    /// One of [`Parameter::enum_values`] (selector).
    Enum,
    /// Momentary action with no persistent value (button).
    Trigger,
    /// Free-form text.
    String,
}

impl ParameterType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Float => "Float",
            Self::Integer => "Integer",
            Self::Boolean => "Boolean",
            Self::Enum => "Enum",
            Self::Trigger => "Trigger",
            Self::String => "String",
        }
    }
}

/// A format-agnostic instrument/effect parameter: descriptor + current value.
///
/// Mirrors the C++ `Parameter` struct in the design doc. `value`, `minimum`,
/// `maximum` and `default` are in the parameter's **native** range (e.g. an
/// "Attack" parameter might range 0–5000 ms). Use [`Parameter::normalized`] /
/// [`Parameter::denormalize`] to convert to/from the 0–1 range hosts often use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    /// Stable identifier, unique within a provider (e.g. "attack", or "12").
    pub id: String,
    /// Human-readable name (e.g. "Attack").
    pub name: String,
    pub kind: ParameterType,

    pub value: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub default: f64,

    /// Unit label (e.g. "ms", "%", "Hz"); empty if unitless.
    pub unit: String,

    pub automatable: bool,
    pub modulatable: bool,
    pub read_only: bool,

    /// For [`ParameterType::Enum`]: the ordered choice labels.
    #[serde(default)]
    pub enum_values: Vec<String>,
}

impl Parameter {
    /// Build a continuous float parameter.
    pub fn float(id: impl Into<String>, name: impl Into<String>, value: f64, min: f64, max: f64) -> Self {
        Self {
            id: id.into(), name: name.into(), kind: ParameterType::Float,
            value, minimum: min, maximum: max, default: value,
            unit: String::new(), automatable: true, modulatable: true,
            read_only: false, enum_values: Vec::new(),
        }
    }

    /// Build a boolean (switch) parameter.
    pub fn boolean(id: impl Into<String>, name: impl Into<String>, value: bool) -> Self {
        Self {
            id: id.into(), name: name.into(), kind: ParameterType::Boolean,
            value: if value { 1.0 } else { 0.0 }, minimum: 0.0, maximum: 1.0,
            default: if value { 1.0 } else { 0.0 }, unit: String::new(),
            automatable: true, modulatable: false, read_only: false, enum_values: Vec::new(),
        }
    }

    /// Build an enum (selector) parameter; `value` is the selected index.
    pub fn enumerated(id: impl Into<String>, name: impl Into<String>, value: usize, choices: Vec<String>) -> Self {
        let max = choices.len().saturating_sub(1) as f64;
        Self {
            id: id.into(), name: name.into(), kind: ParameterType::Enum,
            value: value as f64, minimum: 0.0, maximum: max, default: value as f64,
            unit: String::new(), automatable: true, modulatable: false,
            read_only: false, enum_values: choices,
        }
    }

    /// Fluent: set the unit label.
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self { self.unit = unit.into(); self }
    /// Fluent: set the default value.
    pub fn with_default(mut self, d: f64) -> Self { self.default = d; self }
    /// Fluent: mark as non-modulatable.
    pub fn non_modulatable(mut self) -> Self { self.modulatable = false; self }
    /// Fluent: mark as read-only (display only).
    pub fn read_only(mut self) -> Self { self.read_only = true; self.automatable = false; self.modulatable = false; self }

    /// The span of the native range (always ≥ 0).
    pub fn span(&self) -> f64 { (self.maximum - self.minimum).max(0.0) }

    /// Current value mapped to 0.0–1.0. Degenerate (zero-span) ranges map to 0.
    pub fn normalized(&self) -> f64 {
        let span = self.span();
        if span <= f64::EPSILON { 0.0 } else { ((self.value - self.minimum) / span).clamp(0.0, 1.0) }
    }

    /// Map a 0.0–1.0 value into this parameter's native range.
    pub fn denormalize(&self, norm: f64) -> f64 {
        self.minimum + norm.clamp(0.0, 1.0) * self.span()
    }

    /// Clamp + snap an incoming native value to the parameter's domain
    /// (integers/enums/booleans round to the nearest valid step).
    pub fn sanitize(&self, v: f64) -> f64 {
        let c = v.clamp(self.minimum, self.maximum);
        match self.kind {
            ParameterType::Integer | ParameterType::Enum => c.round(),
            ParameterType::Boolean => if c >= 0.5 { 1.0 } else { 0.0 },
            _ => c,
        }
    }

    /// Human-readable current value (e.g. "1.20 ms", "On", "Sine").
    pub fn display(&self) -> String {
        match self.kind {
            ParameterType::Boolean => if self.value >= 0.5 { "On".into() } else { "Off".into() },
            ParameterType::Enum => self.enum_values
                .get(self.value.round() as usize)
                .cloned()
                .unwrap_or_else(|| format!("{}", self.value.round() as i64)),
            ParameterType::Integer => {
                if self.unit.is_empty() { format!("{}", self.value.round() as i64) }
                else { format!("{} {}", self.value.round() as i64, self.unit) }
            }
            ParameterType::Trigger => "—".into(),
            ParameterType::String => String::new(),
            ParameterType::Float => {
                if self.unit.is_empty() { format!("{:.3}", self.value) }
                else { format!("{:.3} {}", self.value, self.unit) }
            }
        }
    }
}

/// A source of universal [`Parameter`]s. Implemented by adapters wrapping each
/// concrete instrument/effect format. Indices are stable for the provider's
/// lifetime; `id`s are stable identifiers usable for automation/modulation
/// targets and preset storage.
pub trait ParameterProvider {
    /// Number of parameters exposed.
    fn parameter_count(&self) -> usize;

    /// Snapshot of the parameter at `index` (descriptor + current value).
    fn parameter(&self, index: usize) -> Option<Parameter>;

    /// Set the parameter at `index` to a native value (implementations should
    /// sanitize/clamp). No-op if the index is out of range or read-only.
    fn set_parameter(&mut self, index: usize, value: f64);

    /// Trigger a momentary [`ParameterType::Trigger`] parameter. Default no-op.
    fn trigger_parameter(&mut self, _index: usize) {}

    // ── Provided helpers ─────────────────────────────────────────────────────

    /// Snapshot of all parameters in order.
    fn parameters(&self) -> Vec<Parameter> {
        (0..self.parameter_count()).filter_map(|i| self.parameter(i)).collect()
    }

    /// Set a parameter from a normalised 0.0–1.0 value.
    fn set_parameter_normalized(&mut self, index: usize, norm: f64) {
        if let Some(p) = self.parameter(index) {
            self.set_parameter(index, p.denormalize(norm));
        }
    }

    /// Find a parameter (index + snapshot) by its stable id.
    fn parameter_by_id(&self, id: &str) -> Option<(usize, Parameter)> {
        (0..self.parameter_count())
            .filter_map(|i| self.parameter(i).map(|p| (i, p)))
            .find(|(_, p)| p.id == id)
    }

    /// Set a parameter by its stable id; returns true if found and writable.
    fn set_parameter_by_id(&mut self, id: &str, value: f64) -> bool {
        match self.parameter_by_id(id) {
            Some((index, p)) if !p.read_only => { self.set_parameter(index, value); true }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_roundtrip() {
        let p = Parameter::float("cut", "Cutoff", 1000.0, 20.0, 20000.0).with_unit("Hz");
        let n = p.normalized();
        assert!((p.denormalize(n) - 1000.0).abs() < 1e-6);
        assert_eq!(p.unit, "Hz");
    }

    #[test]
    fn enum_display_and_sanitize() {
        let p = Parameter::enumerated("wave", "Waveform", 1, vec!["Sine".into(), "Saw".into(), "Square".into()]);
        assert_eq!(p.display(), "Saw");
        assert_eq!(p.sanitize(1.4), 1.0);
        assert_eq!(p.sanitize(9.0), 2.0); // clamped to max index then rounded
    }

    #[test]
    fn boolean_display() {
        let mut p = Parameter::boolean("loop", "Loop", false);
        assert_eq!(p.display(), "Off");
        p.value = p.sanitize(0.9);
        assert_eq!(p.display(), "On");
    }

    /// A tiny in-memory provider to exercise the trait's default helpers.
    struct MemProvider(Vec<Parameter>);
    impl ParameterProvider for MemProvider {
        fn parameter_count(&self) -> usize { self.0.len() }
        fn parameter(&self, i: usize) -> Option<Parameter> { self.0.get(i).cloned() }
        fn set_parameter(&mut self, i: usize, v: f64) {
            if let Some(p) = self.0.get_mut(i) { if !p.read_only { p.value = p.sanitize(v); } }
        }
    }

    #[test]
    fn provider_by_id_and_normalized() {
        let mut prov = MemProvider(vec![
            Parameter::float("gain", "Gain", 0.0, -60.0, 6.0).with_unit("dB"),
            Parameter::boolean("mute", "Mute", false),
        ]);
        assert!(prov.set_parameter_by_id("gain", 3.0));
        assert_eq!(prov.parameter_by_id("gain").unwrap().1.value, 3.0);
        // normalized set: 0.5 over [-60, 6] = -27.0
        prov.set_parameter_normalized(0, 0.5);
        assert!((prov.parameter(0).unwrap().value - (-27.0)).abs() < 1e-6);
        // unknown id
        assert!(!prov.set_parameter_by_id("nope", 1.0));
    }
}
