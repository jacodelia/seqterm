//! Universal modulation matrix + macros — format-agnostic.
//!
//! Part of SeqTerm's Universal Instrument Engine (`01_editorUpdate2.md`). This
//! module is **pure domain logic**: it maps modulation sources (MIDI CC, LFOs,
//! envelopes, macros, …) onto parameter destinations (identified by stable
//! string ids), independent of any plugin format. Realtime engines feed it a
//! [`SourceValues`] snapshot and apply the resulting per-destination offsets to
//! the matching universal parameters.
//!
//! All types are serializable so they persist in the project.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Maximum number of macro controls (Macro 1–16).
pub const MACRO_COUNT: usize = 16;

/// A modulation source. Index-bearing variants (LFO/Env/Step/Macro) reference a
/// slot in the corresponding bank of [`SourceValues`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ModulationSource {
    MidiCc(u8),
    Velocity,
    Aftertouch,
    PolyAftertouch,
    PitchBend,
    Lfo(usize),
    Envelope(usize),
    StepSequencer(usize),
    Macro(usize),
    AudioEnvelope,
}

impl ModulationSource {
    pub fn label(&self) -> String {
        match self {
            Self::MidiCc(n) => format!("CC{n}"),
            Self::Velocity => "Velocity".into(),
            Self::Aftertouch => "Aftertouch".into(),
            Self::PolyAftertouch => "PolyAT".into(),
            Self::PitchBend => "PitchBend".into(),
            Self::Lfo(i) => format!("LFO{}", i + 1),
            Self::Envelope(i) => format!("Env{}", i + 1),
            Self::StepSequencer(i) => format!("Step{}", i + 1),
            Self::Macro(i) => format!("Macro{}", i + 1),
            Self::AudioEnvelope => "AudioEnv".into(),
        }
    }
}

/// Response curve applied to a normalised source value before scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ModulationCurve {
    #[default]
    Linear,
    Exponential,
    Logarithmic,
    /// Smoothstep (eased in/out).
    SCurve,
}

impl ModulationCurve {
    /// Apply the curve to `x` in `[0, 1]`, returning `[0, 1]`.
    pub fn apply(self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Linear => x,
            Self::Exponential => x * x,
            Self::Logarithmic => x.sqrt(),
            Self::SCurve => x * x * (3.0 - 2.0 * x),
        }
    }
}

/// Whether a source modulates one-sided (`[0, +amt]`) or centred (`[-amt, +amt]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Polarity {
    #[default]
    Unipolar,
    Bipolar,
}

/// One modulation route: source → destination with depth, curve and polarity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModulationRoute {
    pub source: ModulationSource,
    /// Destination parameter id (stable within its provider).
    pub destination: String,
    /// Modulation depth in `[-1, 1]` applied to the normalised parameter.
    pub amount: f64,
    #[serde(default)]
    pub curve: ModulationCurve,
    #[serde(default)]
    pub polarity: Polarity,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

impl ModulationRoute {
    pub fn new(source: ModulationSource, destination: impl Into<String>, amount: f64) -> Self {
        Self {
            source, destination: destination.into(), amount,
            curve: ModulationCurve::Linear, polarity: Polarity::Unipolar, enabled: true,
        }
    }

    /// Contribution of this route to its destination, given a raw source value
    /// in `[0, 1]`. Bipolar routes recentre the (curved) source around 0.
    pub fn contribution(&self, raw: f64) -> f64 {
        if !self.enabled { return 0.0; }
        let shaped = self.curve.apply(raw);
        let signed = match self.polarity {
            Polarity::Unipolar => shaped,
            Polarity::Bipolar => shaped * 2.0 - 1.0,
        };
        signed * self.amount
    }
}

/// A direct target of a macro control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacroTarget {
    pub destination: String,
    /// Depth in `[-1, 1]`.
    pub amount: f64,
}

/// One macro control: a single knob (`value` in `[0, 1]`) wired to many
/// destinations, each with its own depth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacroControl {
    pub name: String,
    #[serde(default)]
    pub value: f64,
    #[serde(default)]
    pub targets: Vec<MacroTarget>,
}

impl MacroControl {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), value: 0.0, targets: Vec::new() }
    }
}

/// Current values of every modulation source, fed to [`ModulationSystem::resolve`].
/// Continuous sources are normalised `[0, 1]` except `pitch_bend` (`[-1, 1]`).
#[derive(Debug, Clone)]
pub struct SourceValues {
    pub midi_cc: [f64; 128],
    pub velocity: f64,
    pub aftertouch: f64,
    pub poly_aftertouch: f64,
    pub pitch_bend: f64,
    pub lfo: Vec<f64>,
    pub envelope: Vec<f64>,
    pub step_seq: Vec<f64>,
    pub macros: Vec<f64>,
    pub audio_envelope: f64,
}

impl Default for SourceValues {
    fn default() -> Self {
        Self {
            midi_cc: [0.0; 128],
            velocity: 0.0,
            aftertouch: 0.0,
            poly_aftertouch: 0.0,
            pitch_bend: 0.0,
            lfo: Vec::new(),
            envelope: Vec::new(),
            step_seq: Vec::new(),
            macros: vec![0.0; MACRO_COUNT],
            audio_envelope: 0.0,
        }
    }
}

impl SourceValues {
    /// Raw value for a source in `[0, 1]` (pitch-bend is mapped from `[-1,1]`).
    pub fn get(&self, source: &ModulationSource) -> f64 {
        let v = match *source {
            ModulationSource::MidiCc(n) => self.midi_cc.get(n as usize).copied().unwrap_or(0.0),
            ModulationSource::Velocity => self.velocity,
            ModulationSource::Aftertouch => self.aftertouch,
            ModulationSource::PolyAftertouch => self.poly_aftertouch,
            ModulationSource::PitchBend => (self.pitch_bend + 1.0) * 0.5,
            ModulationSource::Lfo(i) => self.lfo.get(i).copied().unwrap_or(0.0),
            ModulationSource::Envelope(i) => self.envelope.get(i).copied().unwrap_or(0.0),
            ModulationSource::StepSequencer(i) => self.step_seq.get(i).copied().unwrap_or(0.0),
            ModulationSource::Macro(i) => self.macros.get(i).copied().unwrap_or(0.0),
            ModulationSource::AudioEnvelope => self.audio_envelope,
        };
        v.clamp(0.0, 1.0)
    }
}

/// The full modulation system for one instrument: routes + macro bank. Persisted
/// in the project; resolved each block into per-destination offsets.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModulationSystem {
    #[serde(default)]
    pub routes: Vec<ModulationRoute>,
    #[serde(default)]
    pub macros: Vec<MacroControl>,
}

impl ModulationSystem {
    /// A system with `MACRO_COUNT` empty, named macros and no routes.
    pub fn with_default_macros() -> Self {
        Self {
            routes: Vec::new(),
            macros: (0..MACRO_COUNT).map(|i| MacroControl::new(format!("Macro {}", i + 1))).collect(),
        }
    }

    /// Ensure the macro bank holds exactly `MACRO_COUNT` controls, appending
    /// default-named empty macros for any missing slots. Existing macros (and
    /// their assigned targets/values) are preserved. Idempotent — call before
    /// surfacing the 16-macro bank in the EDITOR on a project that predates it.
    pub fn ensure_macros(&mut self) {
        for i in self.macros.len()..MACRO_COUNT {
            self.macros.push(MacroControl::new(format!("Macro {}", i + 1)));
        }
        self.macros.truncate(MACRO_COUNT);
    }

    /// Unified MIDI Learn: bind (or rebind) a MIDI CC to a destination parameter
    /// with a response curve. If a route already exists for this `(cc,
    /// destination)` pair it is updated in place; otherwise one is appended.
    /// Returns a mutable reference to the route for further tweaking.
    pub fn learn_cc(
        &mut self,
        cc: u8,
        destination: impl Into<String>,
        amount: f64,
        curve: ModulationCurve,
    ) -> &mut ModulationRoute {
        let dest = destination.into();
        let pos = self.routes.iter().position(|r| {
            matches!(r.source, ModulationSource::MidiCc(n) if n == cc) && r.destination == dest
        });
        let idx = match pos {
            Some(i) => i,
            None => {
                self.routes.push(ModulationRoute::new(ModulationSource::MidiCc(cc), dest, amount));
                self.routes.len() - 1
            }
        };
        let r = &mut self.routes[idx];
        r.amount = amount;
        r.curve = curve;
        r.enabled = true;
        r
    }

    /// Remove all routes targeting `destination` whose source is a MIDI CC.
    pub fn forget_cc(&mut self, destination: &str) {
        self.routes.retain(|r| {
            !(matches!(r.source, ModulationSource::MidiCc(_)) && r.destination == destination)
        });
    }

    /// Total normalised modulation offset per destination id. Sums all enabled
    /// routes plus every macro's direct targets (scaled by the macro value).
    /// Apply each offset to the corresponding parameter's normalised value, then
    /// clamp to `[0, 1]`.
    pub fn resolve(&self, sv: &SourceValues) -> HashMap<String, f64> {
        let mut out: HashMap<String, f64> = HashMap::new();

        for route in &self.routes {
            if !route.enabled { continue; }
            let raw = sv.get(&route.source);
            let c = route.contribution(raw);
            if c != 0.0 {
                *out.entry(route.destination.clone()).or_insert(0.0) += c;
            }
        }

        for m in &self.macros {
            if m.value == 0.0 { continue; }
            for t in &m.targets {
                let c = m.value.clamp(0.0, 1.0) * t.amount;
                if c != 0.0 {
                    *out.entry(t.destination.clone()).or_insert(0.0) += c;
                }
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_endpoints() {
        for c in [ModulationCurve::Linear, ModulationCurve::Exponential, ModulationCurve::Logarithmic, ModulationCurve::SCurve] {
            assert!((c.apply(0.0)).abs() < 1e-9, "{c:?} at 0");
            assert!((c.apply(1.0) - 1.0).abs() < 1e-9, "{c:?} at 1");
        }
        assert!(ModulationCurve::Exponential.apply(0.5) < 0.5); // concave up
        assert!(ModulationCurve::Logarithmic.apply(0.5) > 0.5); // concave down
    }

    #[test]
    fn unipolar_vs_bipolar() {
        let uni = ModulationRoute::new(ModulationSource::Velocity, "cut", 1.0);
        assert!((uni.contribution(0.0) - 0.0).abs() < 1e-9);
        assert!((uni.contribution(1.0) - 1.0).abs() < 1e-9);

        let mut bip = uni.clone();
        bip.polarity = Polarity::Bipolar;
        assert!((bip.contribution(0.5) - 0.0).abs() < 1e-9); // centre = 0
        assert!((bip.contribution(0.0) + 1.0).abs() < 1e-9); // -amount
        assert!((bip.contribution(1.0) - 1.0).abs() < 1e-9); // +amount
    }

    #[test]
    fn disabled_route_contributes_nothing() {
        let mut r = ModulationRoute::new(ModulationSource::MidiCc(1), "cut", 1.0);
        r.enabled = false;
        assert_eq!(r.contribution(1.0), 0.0);
    }

    #[test]
    fn learn_cc_binds_then_rebinds_in_place() {
        let mut sys = ModulationSystem::default();
        sys.learn_cc(74, "cutoff", 0.8, ModulationCurve::Exponential);
        assert_eq!(sys.routes.len(), 1);
        // Re-learning the same (cc, dest) updates rather than duplicating.
        sys.learn_cc(74, "cutoff", 0.4, ModulationCurve::Linear);
        assert_eq!(sys.routes.len(), 1);
        assert!((sys.routes[0].amount - 0.4).abs() < 1e-9);
        assert_eq!(sys.routes[0].curve, ModulationCurve::Linear);
        // A different destination adds a new route.
        sys.learn_cc(74, "reso", 0.5, ModulationCurve::Linear);
        assert_eq!(sys.routes.len(), 2);
        sys.forget_cc("cutoff");
        assert_eq!(sys.routes.len(), 1);
        assert_eq!(sys.routes[0].destination, "reso");
    }

    #[test]
    fn resolve_sums_routes_and_macros() {
        let mut sys = ModulationSystem::default();
        sys.routes.push(ModulationRoute::new(ModulationSource::MidiCc(74), "cutoff", 0.5));
        sys.routes.push(ModulationRoute::new(ModulationSource::Macro(0), "cutoff", 0.2));
        let mut m = MacroControl::new("Brightness");
        m.value = 1.0;
        m.targets.push(MacroTarget { destination: "cutoff".into(), amount: 0.3 });
        m.targets.push(MacroTarget { destination: "reverb".into(), amount: -0.4 });
        sys.macros.push(m);

        let mut sv = SourceValues::default();
        sv.midi_cc[74] = 1.0; // full
        sv.macros = vec![1.0]; // Macro(0) source = 1.0

        let offs = sys.resolve(&sv);
        // cutoff: 0.5 (cc) + 0.2 (macro-as-source) + 0.3 (macro target) = 1.0
        assert!((offs["cutoff"] - 1.0).abs() < 1e-9, "cutoff={}", offs["cutoff"]);
        assert!((offs["reverb"] + 0.4).abs() < 1e-9);
    }

    #[test]
    fn ensure_macros_pads_and_preserves() {
        // Empty system → padded to the full bank.
        let mut sys = ModulationSystem::default();
        sys.ensure_macros();
        assert_eq!(sys.macros.len(), MACRO_COUNT);

        // Existing macro (value + targets) is preserved; the rest are appended.
        let mut sys = ModulationSystem::default();
        let mut m = MacroControl::new("Brightness");
        m.value = 0.7;
        m.targets.push(MacroTarget { destination: "cutoff".into(), amount: 0.5 });
        sys.macros.push(m);
        sys.ensure_macros();
        assert_eq!(sys.macros.len(), MACRO_COUNT);
        assert_eq!(sys.macros[0].name, "Brightness");
        assert_eq!(sys.macros[0].value, 0.7);
        assert_eq!(sys.macros[0].targets.len(), 1);
    }
}
