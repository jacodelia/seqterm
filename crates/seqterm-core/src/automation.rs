//! Universal automation engine.
//!
//! Part of SeqTerm's Universal Instrument Engine (`01_editorUpdate2.md`).
//! Format-agnostic: lanes are keyed by destination parameter id (the same stable
//! ids used by the modulation matrix), hold time-stamped breakpoints in beats,
//! and evaluate to a normalised `[0, 1]` value at any transport position. Modes
//! (Read/Write/Touch/Latch) govern how live edits are recorded.
//!
//! The realtime driver evaluates the engine at the current beat each control
//! block and applies the result to the matching universal parameters (e.g. the
//! pattern-FX and mixer-FX chains).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Automation write/playback mode for a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AutomationMode {
    /// Play back recorded points; ignore live edits.
    #[default]
    Read,
    /// Continuously overwrite with the live value.
    Write,
    /// Overwrite only while the control is being touched, then return to Read.
    Touch,
    /// Overwrite from first touch and hold the new value until stop.
    Latch,
}

/// Interpolation from one breakpoint to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AutomationCurve {
    #[default]
    Linear,
    Exponential,
    Logarithmic,
    /// Eased in/out (smoothstep) — a symmetric Bézier-like S.
    Bezier,
}

impl AutomationCurve {
    /// Shape a `[0, 1]` interpolation fraction.
    pub fn shape(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Exponential => t * t,
            Self::Logarithmic => t.sqrt(),
            Self::Bezier => t * t * (3.0 - 2.0 * t),
        }
    }
}

/// One automation breakpoint: a normalised value at a beat position, with the
/// curve used to reach the *next* point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    pub beat: f64,
    pub value: f64,
    #[serde(default)]
    pub curve: AutomationCurve,
}

/// A single automation lane for one destination parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    pub destination: String,
    #[serde(default)]
    pub mode: AutomationMode,
    /// Breakpoints, kept sorted by `beat`.
    #[serde(default)]
    pub points: Vec<AutomationPoint>,
    /// Transient: true while the user is actively touching the control.
    #[serde(skip)]
    pub touched: bool,
}

impl AutomationLane {
    pub fn new(destination: impl Into<String>) -> Self {
        Self { destination: destination.into(), mode: AutomationMode::Read, points: Vec::new(), touched: false }
    }

    /// Insert or replace a point at `beat` (points within `1e-6` beats merge).
    pub fn set_point(&mut self, beat: f64, value: f64, curve: AutomationCurve) {
        let value = value.clamp(0.0, 1.0);
        match self.points.iter().position(|p| (p.beat - beat).abs() < 1e-6) {
            Some(i) => { self.points[i].value = value; self.points[i].curve = curve; }
            None => {
                let idx = self.points.partition_point(|p| p.beat < beat);
                self.points.insert(idx, AutomationPoint { beat, value, curve });
            }
        }
    }

    /// Evaluate the lane at `beat`. Returns `None` if the lane has no points.
    /// Before the first / after the last point the value is held (clamped).
    pub fn value_at(&self, beat: f64) -> Option<f64> {
        if self.points.is_empty() { return None; }
        if beat <= self.points[0].beat { return Some(self.points[0].value); }
        let last = self.points.last().unwrap();
        if beat >= last.beat { return Some(last.value); }
        // Find the segment [a, b] containing `beat`.
        let i = self.points.partition_point(|p| p.beat <= beat) - 1;
        let a = &self.points[i];
        let b = &self.points[i + 1];
        let span = (b.beat - a.beat).max(1e-9);
        let t = a.curve.shape((beat - a.beat) / span);
        Some(a.value + t * (b.value - a.value))
    }
}

/// The automation engine: a set of lanes keyed by destination id.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutomationEngine {
    #[serde(default)]
    pub lanes: Vec<AutomationLane>,
}

impl AutomationEngine {
    pub fn lane(&self, destination: &str) -> Option<&AutomationLane> {
        self.lanes.iter().find(|l| l.destination == destination)
    }

    pub fn lane_mut(&mut self, destination: &str) -> Option<&mut AutomationLane> {
        self.lanes.iter_mut().find(|l| l.destination == destination)
    }

    /// Get or create a lane for `destination`.
    pub fn lane_or_default(&mut self, destination: &str) -> &mut AutomationLane {
        if let Some(i) = self.lanes.iter().position(|l| l.destination == destination) {
            &mut self.lanes[i]
        } else {
            self.lanes.push(AutomationLane::new(destination));
            self.lanes.last_mut().unwrap()
        }
    }

    /// Remove a lane entirely (clears automation for the destination).
    pub fn clear(&mut self, destination: &str) {
        self.lanes.retain(|l| l.destination != destination);
    }

    /// Evaluate every readable lane at `beat`, returning destination → value for
    /// lanes that should play back (Read, plus Touch/Latch when not touched).
    pub fn values_at(&self, beat: f64) -> HashMap<String, f64> {
        let mut out = HashMap::new();
        for lane in &self.lanes {
            let playback = match lane.mode {
                AutomationMode::Read => true,
                AutomationMode::Write => false,
                AutomationMode::Touch | AutomationMode::Latch => !lane.touched,
            };
            if !playback { continue; }
            if let Some(v) = lane.value_at(beat) {
                out.insert(lane.destination.clone(), v);
            }
        }
        out
    }

    /// Record a live value for `destination` at `beat` according to the lane's
    /// mode. Write always records; Touch/Latch record while touched. Returns true
    /// if a point was written.
    pub fn record(&mut self, destination: &str, beat: f64, value: f64, touched: bool) -> bool {
        let lane = self.lane_or_default(destination);
        lane.touched = touched;
        let should = match lane.mode {
            AutomationMode::Read => false,
            AutomationMode::Write => true,
            AutomationMode::Touch | AutomationMode::Latch => touched,
        };
        if should {
            lane.set_point(beat, value, AutomationCurve::Linear);
        }
        should
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_shapes_bounded() {
        for c in [AutomationCurve::Linear, AutomationCurve::Exponential, AutomationCurve::Logarithmic, AutomationCurve::Bezier] {
            assert!((c.shape(0.0)).abs() < 1e-9);
            assert!((c.shape(1.0) - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn lane_interpolates_and_holds() {
        let mut lane = AutomationLane::new("cutoff");
        lane.set_point(0.0, 0.0, AutomationCurve::Linear);
        lane.set_point(4.0, 1.0, AutomationCurve::Linear);
        assert_eq!(lane.value_at(-1.0), Some(0.0)); // held before first
        assert_eq!(lane.value_at(2.0), Some(0.5));  // midpoint
        assert_eq!(lane.value_at(9.0), Some(1.0));  // held after last
    }

    #[test]
    fn points_stay_sorted_and_merge() {
        let mut lane = AutomationLane::new("x");
        lane.set_point(4.0, 0.5, AutomationCurve::Linear);
        lane.set_point(0.0, 0.0, AutomationCurve::Linear);
        lane.set_point(2.0, 0.25, AutomationCurve::Linear);
        lane.set_point(2.0, 0.9, AutomationCurve::Linear); // replaces
        let beats: Vec<f64> = lane.points.iter().map(|p| p.beat).collect();
        assert_eq!(beats, vec![0.0, 2.0, 4.0]);
        assert_eq!(lane.points[1].value, 0.9);
    }

    #[test]
    fn modes_govern_playback_and_record() {
        let mut eng = AutomationEngine::default();
        // Read lane plays back.
        eng.lane_or_default("a").set_point(0.0, 0.3, AutomationCurve::Linear);
        // Write lane does not play back but records.
        eng.lane_or_default("b").mode = AutomationMode::Write;
        let vals = eng.values_at(0.0);
        assert_eq!(vals.get("a"), Some(&0.3));
        assert!(!vals.contains_key("b"));
        assert!(eng.record("b", 1.0, 0.7, false));   // Write records regardless
        assert!(!eng.record("a", 1.0, 0.9, false));  // Read ignores
    }
}
