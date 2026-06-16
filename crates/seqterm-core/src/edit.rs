//! Shared rational-time editing state and snap engine (Phase 3).
//!
//! [`EditState`] is the single source of truth for the **current edit
//! resolution**, an optional [`Tuplet`], the [`SnapMode`], and free-time mode.
//! The Tracker, Pattern, and Piano-Roll views all consume it so their cursor,
//! insertion, movement, selection, snap and resize behave identically.
//!
//! The snap logic is pure and exact (rational), so odd resolutions and tuplets
//! snap without floating-point drift.

use serde::{Deserialize, Serialize};

use crate::rational::{RationalTime, Resolution, Tuplet};

/// How positions/durations are snapped while editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SnapMode {
    /// Snap to the current edit resolution (under the active tuplet, if any).
    #[default]
    Grid,
    /// Snap to the nearest half of the current grid cell (finer placement).
    Fine,
    /// No snapping — placement is at the exact rational time (same as free-time).
    Off,
}

impl SnapMode {
    pub fn label(self) -> &'static str {
        match self {
            SnapMode::Grid => "grid",
            SnapMode::Fine => "fine",
            SnapMode::Off => "off",
        }
    }

    /// Cycle Grid → Fine → Off → Grid.
    pub fn next(self) -> Self {
        match self {
            SnapMode::Grid => SnapMode::Fine,
            SnapMode::Fine => SnapMode::Off,
            SnapMode::Off => SnapMode::Grid,
        }
    }
}

/// The shared editing state for all rational-time editors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditState {
    /// The grid one step/cell represents while editing.
    pub resolution: Resolution,
    /// Active tuplet applied to the grid (`None` = straight).
    pub tuplet: Option<Tuplet>,
    /// How positions snap.
    pub snap: SnapMode,
    /// When true, all snapping is bypassed (place at exact `RationalTime`).
    pub free_time: bool,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            resolution: Resolution::default_edit(), // 1/16
            tuplet: None,
            snap: SnapMode::Grid,
            free_time: false,
        }
    }
}

/// Common edit-resolution denominators offered by the UI when cycling.
/// Includes non-powers-of-two for triplet/quintuplet/septuplet grids.
pub const RESOLUTION_LADDER: [i64; 12] =
    [1, 2, 4, 8, 16, 32, 64, 3, 6, 12, 24, 48];

impl EditState {
    /// The beat span of one grid cell at the current resolution and tuplet.
    pub fn grid_beats(&self) -> RationalTime {
        let base = self.resolution.step_beats();
        match self.tuplet {
            Some(t) => base * t.scale(),
            None => base,
        }
    }

    /// The effective snap unit in beats, honoring [`SnapMode`].
    /// Returns `None` when no snapping applies (free-time, or `SnapMode::Off`).
    pub fn snap_unit(&self) -> Option<RationalTime> {
        if self.free_time {
            return None;
        }
        match self.snap {
            SnapMode::Off => None,
            SnapMode::Grid => Some(self.grid_beats()),
            SnapMode::Fine => Some(self.grid_beats() / 2),
        }
    }

    /// Snap an absolute position (beats) to the active grid. In free-time or
    /// `SnapMode::Off` the position is returned unchanged. Exact rational
    /// rounding to the nearest grid line.
    pub fn snap_pos(&self, t: RationalTime) -> RationalTime {
        match self.snap_unit() {
            None => t,
            Some(unit) if unit.is_zero() => t,
            Some(unit) => {
                let q = t / unit;
                // round(q) = floor(q + 1/2)
                let n = (q + RationalTime::new(1, 2)).floor();
                unit * n
            }
        }
    }

    /// Snap a duration to at least one snap unit (never snaps to zero length).
    pub fn snap_duration(&self, d: RationalTime) -> RationalTime {
        match self.snap_unit() {
            None => d,
            Some(unit) if unit.is_zero() => d,
            Some(unit) => {
                let q = d / unit;
                let mut n = (q + RationalTime::new(1, 2)).floor();
                if n < 1 {
                    n = 1;
                }
                unit * n
            }
        }
    }

    /// Step the resolution along [`RESOLUTION_LADDER`]. `dir > 0` moves to a
    /// finer grid in ladder order, `dir < 0` coarser; saturates at the ends.
    pub fn cycle_resolution(&mut self, dir: i32) {
        let cur = self.resolution.den();
        let idx = RESOLUTION_LADDER.iter().position(|&d| d == cur).unwrap_or(4);
        let next = (idx as i32 + dir).clamp(0, RESOLUTION_LADDER.len() as i32 - 1) as usize;
        self.resolution = Resolution::Whole(RESOLUTION_LADDER[next]);
    }

    /// Toggle a triplet (`3:2`) tuplet on/off. Distinct tuplets are set directly.
    pub fn toggle_triplet(&mut self) {
        self.tuplet = match self.tuplet {
            Some(t) if t == Tuplet::new(3, 2) => None,
            _ => Some(Tuplet::new(3, 2)),
        };
    }

    /// The beat span of one grid cell at the current resolution, **ignoring** any
    /// active tuplet. Complex rhythms are local to a selection/region, so the
    /// displayed grid (ticks, GRID readout) must stay on the straight resolution
    /// regardless of the tuplet used to lay out a figure.
    pub fn display_grid_beats(&self) -> RationalTime {
        self.resolution.step_beats()
    }

    /// Grid-only readout for the `RHYTHM :: GRID` tab and view titles: the base
    /// resolution plus snap/free state, **never** the tuplet factor. (The tuplet is
    /// per-selection state and must not appear as if it changed the global grid —
    /// e.g. this never renders `1/32·6:4`.)
    pub fn grid_label(&self) -> String {
        let mut s = format!("1/{}", self.resolution.den());
        if self.free_time {
            s.push_str(" · free");
        } else {
            s.push_str(&format!(" · snap {}", self.snap.label()));
        }
        s
    }

    /// A compact human-readable summary for the status bar, e.g.
    /// `1/16` or `1/8·3:2 · snap fine` or `1/16 · free`.
    pub fn summary(&self) -> String {
        let mut s = format!("1/{}", self.resolution.den());
        if let Some(t) = self.tuplet.filter(|t| !t.is_none()) {
            s.push_str(&format!("·{}:{}", t.num, t.den));
        }
        if self.free_time {
            s.push_str(" · free");
        } else {
            s.push_str(&format!(" · snap {}", self.snap.label()));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: i64, d: i64) -> RationalTime {
        RationalTime::new(n, d)
    }

    #[test]
    fn default_is_sixteenth_grid_snap() {
        let e = EditState::default();
        assert_eq!(e.resolution, Resolution::Whole(16));
        assert_eq!(e.grid_beats(), r(1, 4));
        assert_eq!(e.snap_unit(), Some(r(1, 4)));
    }

    #[test]
    fn snap_pos_to_sixteenth() {
        let e = EditState::default();
        // 0.3 beat snaps to 1/4 (0.25), 0.4 snaps to 1/2.
        assert_eq!(e.snap_pos(r(3, 10)), r(1, 4));
        assert_eq!(e.snap_pos(r(2, 5)), r(1, 2));
        // Exactly on a line stays put.
        assert_eq!(e.snap_pos(r(3, 4)), r(3, 4));
    }

    #[test]
    fn fine_snap_halves_the_grid() {
        let mut e = EditState::default();
        e.snap = SnapMode::Fine;
        assert_eq!(e.snap_unit(), Some(r(1, 8))); // half of 1/4
        // 0.2 beat → nearest 1/8 (0.125 or 0.25): 0.2 rounds to 0.25.
        assert_eq!(e.snap_pos(r(1, 5)), r(1, 4));
    }

    #[test]
    fn free_time_and_off_bypass_snap() {
        let mut e = EditState::default();
        e.free_time = true;
        assert_eq!(e.snap_unit(), None);
        assert_eq!(e.snap_pos(r(7, 13)), r(7, 13));
        e.free_time = false;
        e.snap = SnapMode::Off;
        assert_eq!(e.snap_pos(r(7, 13)), r(7, 13));
    }

    #[test]
    fn triplet_grid_snaps_to_thirds() {
        let mut e = EditState::default();
        e.resolution = Resolution::Whole(8); // 1/8 = 1/2 beat
        e.toggle_triplet(); // 3:2 → cell = 1/2 * 2/3 = 1/3 beat
        assert_eq!(e.grid_beats(), r(1, 3));
        // 0.3 beat snaps to 1/3; 0.6 snaps to 2/3.
        assert_eq!(e.snap_pos(r(3, 10)), r(1, 3));
        assert_eq!(e.snap_pos(r(3, 5)), r(2, 3));
        e.toggle_triplet(); // back to straight
        assert_eq!(e.tuplet, None);
    }

    #[test]
    fn snap_duration_never_zero() {
        let e = EditState::default();
        // A tiny duration snaps up to one grid unit, not zero.
        assert_eq!(e.snap_duration(r(1, 100)), r(1, 4));
        assert_eq!(e.snap_duration(r(1, 3)), r(1, 4)); // 0.333 → 1 unit
        assert_eq!(e.snap_duration(r(3, 4)), r(3, 4)); // 3 units stays
    }

    #[test]
    fn cycle_resolution_saturates() {
        let mut e = EditState::default(); // 16, ladder idx 4
        e.cycle_resolution(1);
        assert_eq!(e.resolution.den(), 32);
        e.cycle_resolution(-1);
        assert_eq!(e.resolution.den(), 16);
        // Saturate at the coarse end.
        for _ in 0..10 {
            e.cycle_resolution(-1);
        }
        assert_eq!(e.resolution.den(), 1);
    }

    #[test]
    fn snap_mode_cycles() {
        assert_eq!(SnapMode::Grid.next(), SnapMode::Fine);
        assert_eq!(SnapMode::Fine.next(), SnapMode::Off);
        assert_eq!(SnapMode::Off.next(), SnapMode::Grid);
    }

    #[test]
    fn summary_text() {
        let mut e = EditState::default();
        assert_eq!(e.summary(), "1/16 · snap grid");
        e.resolution = Resolution::Whole(8);
        e.toggle_triplet();
        assert_eq!(e.summary(), "1/8·3:2 · snap grid");
        e.free_time = true;
        assert_eq!(e.summary(), "1/8·3:2 · free");
    }

    #[test]
    fn grid_label_never_shows_the_tuplet() {
        // The grid readout is independent of the per-selection tuplet — it must
        // never render something like `1/32·6:4`.
        let mut e = EditState::default();
        e.resolution = Resolution::Whole(32);
        e.tuplet = Some(Tuplet::new(6, 4));
        assert_eq!(e.grid_label(), "1/32 · snap grid");
        assert!(!e.grid_label().contains(':'), "no tuplet ratio in the grid label");
        // `summary()` still carries the tuplet (used in transient status toasts).
        assert!(e.summary().contains("6:4"));
        // display_grid_beats ignores the tuplet (1/32 = 1/8 beat).
        assert_eq!(e.display_grid_beats(), r(1, 8));
    }

    #[test]
    fn serde_roundtrip() {
        let mut e = EditState::default();
        e.resolution = Resolution::Whole(12);
        e.snap = SnapMode::Fine;
        let j = serde_json::to_string(&e).unwrap();
        let back: EditState = serde_json::from_str(&j).unwrap();
        assert_eq!(e, back);
    }
}
