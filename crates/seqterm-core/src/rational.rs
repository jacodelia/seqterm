//! Exact rational musical time.
//!
//! Phase 2 of `01_patternUpdate` introduces an exact, drift-free representation
//! of musical time. All stored positions and durations are measured in **beats**
//! (quarter notes) as a reduced fraction [`RationalTime`] — never `f32`/`f64`.
//!
//! Arbitrary subdivisions, odd denominators (`1/7`, `5/7`, …), tuplets
//! (`3:2`, `5:4`, `7:4`, …) and polyrhythms are all representable exactly.
//! An LCM grid is computed **only** for rendering a shared step grid across
//! patterns of different resolutions; it is never the internal representation.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// Greatest common divisor (Euclid), always non-negative.
pub fn gcd(a: i64, b: i64) -> i64 {
    let mut a = a.abs();
    let mut b = b.abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Least common multiple, always non-negative. `lcm(0, x) == 0`.
pub fn lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        return 0;
    }
    (a / gcd(a, b)).abs().saturating_mul(b.abs())
}

/// An exact musical time/position in **beats** (quarter notes), stored as a
/// reduced fraction `num/den` with `den > 0`.
///
/// Always normalized on construction: the fraction is reduced by its gcd and the
/// sign is carried in `num`. `0` is canonicalized to `0/1`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RationalTime {
    num: i64,
    den: i64,
}

impl RationalTime {
    /// Zero beats (`0/1`).
    pub const ZERO: RationalTime = RationalTime { num: 0, den: 1 };
    /// One beat (`1/1`).
    pub const ONE: RationalTime = RationalTime { num: 1, den: 1 };

    /// Construct from a numerator/denominator, reducing to lowest terms.
    ///
    /// A zero (or, defensively, negative) denominator is treated as `1` rather
    /// than panicking — musical time is never legitimately `n/0`, and a hard
    /// panic in the audio/persistence path is worse than a clamp.
    pub fn new(num: i64, den: i64) -> Self {
        if den == 0 {
            return RationalTime { num, den: 1 };
        }
        let (mut num, mut den) = (num, den);
        if den < 0 {
            num = -num;
            den = -den;
        }
        if num == 0 {
            return RationalTime { num: 0, den: 1 };
        }
        let g = gcd(num, den);
        RationalTime {
            num: num / g,
            den: den / g,
        }
    }

    /// A whole number of beats (`n/1`).
    pub const fn whole(n: i64) -> Self {
        RationalTime { num: n, den: 1 }
    }

    pub fn num(&self) -> i64 {
        self.num
    }

    pub fn den(&self) -> i64 {
        self.den
    }

    pub fn is_zero(&self) -> bool {
        self.num == 0
    }

    pub fn is_negative(&self) -> bool {
        self.num < 0
    }

    /// Decimal beats — **display / scheduling-boundary use only**, never stored.
    pub fn to_f64(&self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Alias for [`to_f64`] reading more naturally at call sites that mean
    /// "value in beats".
    pub fn to_beats(&self) -> f64 {
        self.to_f64()
    }

    /// Absolute value.
    pub fn abs(&self) -> Self {
        RationalTime {
            num: self.num.abs(),
            den: self.den,
        }
    }

    /// Floor to a whole number of beats.
    pub fn floor(&self) -> i64 {
        if self.num >= 0 {
            self.num / self.den
        } else {
            -((-self.num + self.den - 1) / self.den)
        }
    }

    /// Fractional part in `[0, 1)` beats (always non-negative).
    pub fn frac(&self) -> Self {
        *self - RationalTime::whole(self.floor())
    }

    /// Best rational approximation of a decimal beat value, bounding the
    /// denominator to `max_den` (Stern–Brocot mediant search). Used only when
    /// importing/converting external decimal timing into the rational model.
    pub fn from_beats_decimal(beats: f64, max_den: i64) -> Self {
        if !beats.is_finite() {
            return RationalTime::ZERO;
        }
        let max_den = max_den.max(1);
        let negative = beats < 0.0;
        let x = beats.abs();
        let whole = x.floor() as i64;
        let frac = x - whole as f64;

        // Stern–Brocot search for the best p/q approximating `frac` with q<=max_den.
        let (mut lo_n, mut lo_d) = (0i64, 1i64);
        let (mut hi_n, mut hi_d) = (1i64, 1i64);
        let (mut best_n, mut best_d) = (0i64, 1i64);
        let mut best_err = frac;
        for _ in 0..64 {
            let med_n = lo_n + hi_n;
            let med_d = lo_d + hi_d;
            if med_d > max_den {
                break;
            }
            let med = med_n as f64 / med_d as f64;
            let err = (med - frac).abs();
            if err < best_err {
                best_err = err;
                best_n = med_n;
                best_d = med_d;
            }
            if med < frac {
                lo_n = med_n;
                lo_d = med_d;
            } else if med > frac {
                hi_n = med_n;
                hi_d = med_d;
            } else {
                best_n = med_n;
                best_d = med_d;
                break;
            }
        }
        let r = RationalTime::new(whole * best_d + best_n, best_d);
        if negative {
            -r
        } else {
            r
        }
    }

    /// `self mod m`, result in `[0, m)` for positive `m` (musical wrap, e.g.
    /// looping a position within a pattern length). Returns `self` if `m<=0`.
    pub fn rem_euclid(&self, m: RationalTime) -> RationalTime {
        if !m.is_negative() && m.is_zero() {
            return *self;
        }
        if m.is_negative() || m.is_zero() {
            return *self;
        }
        let mut r = *self;
        while r.is_negative() {
            r = r + m;
        }
        while r >= m {
            r = r - m;
        }
        r
    }

    /// Number of whole `step` spans that fit in `self` (floor of `self/step`).
    /// Returns `0` if `step` is zero/negative.
    pub fn div_floor(&self, step: RationalTime) -> i64 {
        if step.is_zero() || step.is_negative() {
            return 0;
        }
        (*self / step).floor()
    }
}

impl Default for RationalTime {
    fn default() -> Self {
        RationalTime::ZERO
    }
}

impl PartialEq for RationalTime {
    fn eq(&self, other: &Self) -> bool {
        // Both are always reduced with den>0, so component-wise equality holds.
        self.num == other.num && self.den == other.den
    }
}
impl Eq for RationalTime {}

impl PartialOrd for RationalTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RationalTime {
    fn cmp(&self, other: &Self) -> Ordering {
        // a/b ? c/d  ->  a*d ? c*b  (den>0 so direction is preserved).
        // Use i128 to avoid overflow on cross-multiplication.
        let lhs = self.num as i128 * other.den as i128;
        let rhs = other.num as i128 * self.den as i128;
        lhs.cmp(&rhs)
    }
}

impl Add for RationalTime {
    type Output = RationalTime;
    fn add(self, rhs: RationalTime) -> RationalTime {
        RationalTime::new(
            self.num * rhs.den + rhs.num * self.den,
            self.den * rhs.den,
        )
    }
}

impl Sub for RationalTime {
    type Output = RationalTime;
    fn sub(self, rhs: RationalTime) -> RationalTime {
        RationalTime::new(
            self.num * rhs.den - rhs.num * self.den,
            self.den * rhs.den,
        )
    }
}

impl Mul for RationalTime {
    type Output = RationalTime;
    fn mul(self, rhs: RationalTime) -> RationalTime {
        RationalTime::new(self.num * rhs.num, self.den * rhs.den)
    }
}

impl Div for RationalTime {
    type Output = RationalTime;
    fn div(self, rhs: RationalTime) -> RationalTime {
        RationalTime::new(self.num * rhs.den, self.den * rhs.num)
    }
}

impl Mul<i64> for RationalTime {
    type Output = RationalTime;
    fn mul(self, rhs: i64) -> RationalTime {
        RationalTime::new(self.num * rhs, self.den)
    }
}

impl Div<i64> for RationalTime {
    type Output = RationalTime;
    // Dividing a fraction by an integer scales the denominator — the `*` here is
    // correct, not the typo clippy's heuristic warns about.
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn div(self, rhs: i64) -> RationalTime {
        RationalTime::new(self.num, self.den * rhs)
    }
}

impl Neg for RationalTime {
    type Output = RationalTime;
    fn neg(self) -> RationalTime {
        RationalTime {
            num: -self.num,
            den: self.den,
        }
    }
}

/// A tuplet ratio: `num` notes played in the time normally occupied by `den`
/// (e.g. a triplet is `3:2` — three notes in the span of two).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tuplet {
    pub num: i64,
    pub den: i64,
}

impl Tuplet {
    /// `num:den` tuplet. Degenerate values clamp to `1:1` (no tuplet).
    pub fn new(num: i64, den: i64) -> Self {
        if num <= 0 || den <= 0 {
            Tuplet { num: 1, den: 1 }
        } else {
            Tuplet { num, den }
        }
    }

    /// The "straight" 1:1 (no tuplet) ratio.
    pub const NONE: Tuplet = Tuplet { num: 1, den: 1 };

    pub fn is_none(&self) -> bool {
        self.num == self.den
    }

    /// The factor a base note value is scaled by under this tuplet: `den/num`.
    /// (e.g. triplet `3:2` → each note is `2/3` of the straight value.)
    pub fn scale(&self) -> RationalTime {
        RationalTime::new(self.den, self.num)
    }
}

impl Default for Tuplet {
    fn default() -> Self {
        Tuplet::NONE
    }
}

/// A musical edit resolution — the duration of one step on the grid, expressed
/// as a fraction of a whole note (`1/den` of a whole note).
///
/// `den = 4` is a quarter note (one beat). Non-power-of-two denominators
/// (`3, 5, 6, 7, 12, 24, 48, 96`) give triplet/quintuplet/… grids directly,
/// and arbitrary denominators are supported via [`Resolution::Custom`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "den", rename_all = "snake_case")]
pub enum Resolution {
    /// `1/den` of a whole note. `den` must be > 0.
    Whole(i64),
    /// Arbitrary `1/den` of a whole note (escape hatch for odd grids).
    Custom(i64),
}

impl Resolution {
    /// Common presets, including non-powers-of-two for tuplet grids.
    pub const PRESETS: [i64; 17] = [
        1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 24, 32, 48, 64, 96, 128, 1,
    ];

    /// The denominator (of a whole note). Always >= 1.
    pub fn den(&self) -> i64 {
        match self {
            Resolution::Whole(d) | Resolution::Custom(d) => (*d).max(1),
        }
    }

    /// Duration of one step at this resolution, in **beats** (quarter notes).
    /// `1/16` note → `1/4` beat; `1/4` note → `1` beat.
    pub fn step_beats(&self) -> RationalTime {
        // 1/den of a whole note = (4/den) beats.
        RationalTime::new(4, self.den())
    }

    /// Default editing resolution for migrated/new patterns: a 1/16 note.
    pub fn default_edit() -> Self {
        Resolution::Whole(16)
    }
}

impl Default for Resolution {
    fn default() -> Self {
        Resolution::default_edit()
    }
}

/// Map a `step` index under a `resolution` (optionally inside a `tuplet`) to its
/// absolute start position in beats, measured from `origin`.
///
/// Without a tuplet this is `origin + step * step_beats`. With a tuplet the step
/// span is scaled by `den/num`, so N tuplet steps fit in the straight span.
pub fn step_to_beats(
    origin: RationalTime,
    step: i64,
    resolution: Resolution,
    tuplet: Tuplet,
) -> RationalTime {
    let base = resolution.step_beats() * step;
    origin + base * tuplet.scale()
}

/// Subdivide the span `[start, start+span)` into `parts` equal pieces, returning
/// the absolute boundary positions `start, start + span/parts, …` (length
/// `parts`, the trailing `start+span` boundary excluded). Exact for any `parts`.
pub fn subdivide(start: RationalTime, span: RationalTime, parts: i64) -> Vec<RationalTime> {
    let parts = parts.max(1);
    let step = span / parts;
    (0..parts).map(|i| start + step * i).collect()
}

/// The LCM of a set of denominators — the fineness of a shared step grid that
/// can represent every supplied resolution/position exactly. **Rendering only.**
pub fn lcm_grid_den(dens: &[i64]) -> i64 {
    dens.iter().copied().filter(|d| *d != 0).fold(1, lcm).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: i64, d: i64) -> RationalTime {
        RationalTime::new(n, d)
    }

    #[test]
    fn reduces_on_construct() {
        let x = r(2, 4);
        assert_eq!((x.num(), x.den()), (1, 2));
        let y = r(6, 3);
        assert_eq!((y.num(), y.den()), (2, 1));
    }

    #[test]
    fn sign_carried_in_numerator() {
        let x = r(1, -2);
        assert_eq!((x.num(), x.den()), (-1, 2));
        assert!(x.is_negative());
    }

    #[test]
    fn zero_canonical() {
        let x = r(0, 5);
        assert_eq!((x.num(), x.den()), (0, 1));
        assert!(x.is_zero());
        assert_eq!(RationalTime::ZERO, x);
    }

    #[test]
    fn zero_denominator_does_not_panic() {
        let x = r(3, 0);
        assert_eq!(x.den(), 1);
    }

    #[test]
    fn arithmetic_exact() {
        assert_eq!(r(1, 3) + r(1, 6), r(1, 2));
        assert_eq!(r(1, 2) - r(1, 3), r(1, 6));
        assert_eq!(r(2, 3) * r(3, 4), r(1, 2));
        assert_eq!(r(1, 2) / r(1, 4), r(2, 1));
        assert_eq!(r(1, 7) * 3, r(3, 7));
        assert_eq!(r(3, 7) / 3, r(1, 7));
        assert_eq!(-r(1, 4), r(-1, 4));
    }

    #[test]
    fn ordering_odd_denominators() {
        assert!(r(1, 7) < r(1, 6));
        assert!(r(5, 7) > r(2, 3));
        assert!(r(11, 8) > r(1, 1));
        let mut v = vec![r(1, 3), r(1, 7), r(1, 2), r(5, 7), r(1, 1)];
        v.sort();
        assert_eq!(v, vec![r(1, 7), r(1, 3), r(1, 2), r(5, 7), r(1, 1)]);
    }

    #[test]
    fn no_drift_over_million_ops() {
        // Summing 1/7 a million times must be exactly 1_000_000/7.
        let mut acc = RationalTime::ZERO;
        for _ in 0..1_000_000 {
            acc = acc + r(1, 7);
        }
        assert_eq!(acc, r(1_000_000, 7));
        // And subtracting back returns to exactly zero.
        for _ in 0..1_000_000 {
            acc = acc - r(1, 7);
        }
        assert!(acc.is_zero());
    }

    #[test]
    fn floor_and_frac() {
        assert_eq!(r(7, 2).floor(), 3);
        assert_eq!(r(7, 2).frac(), r(1, 2));
        assert_eq!(r(-7, 2).floor(), -4);
        assert_eq!(r(-7, 2).frac(), r(1, 2));
        assert_eq!(r(4, 1).floor(), 4);
        assert!(r(4, 1).frac().is_zero());
    }

    #[test]
    fn rem_euclid_wraps() {
        // Position 5.5 beats in a 4-beat loop -> 1.5 beats.
        assert_eq!(r(11, 2).rem_euclid(r(4, 1)), r(3, 2));
        // Negative wraps forward.
        assert_eq!(r(-1, 2).rem_euclid(r(4, 1)), r(7, 2));
        // Exact multiple wraps to zero.
        assert_eq!(r(8, 1).rem_euclid(r(4, 1)), RationalTime::ZERO);
    }

    #[test]
    fn div_floor_counts_steps() {
        assert_eq!(r(7, 4).div_floor(r(1, 4)), 7);
        assert_eq!(r(15, 8).div_floor(r(1, 4)), 7); // 7.5 steps -> 7
        assert_eq!(r(7, 8).div_floor(r(1, 4)), 3); // 3.5 steps -> 3
        assert_eq!(r(1, 1).div_floor(r(0, 1)), 0);
    }

    #[test]
    fn from_beats_decimal_recovers_simple_fractions() {
        assert_eq!(RationalTime::from_beats_decimal(0.5, 64), r(1, 2));
        assert_eq!(RationalTime::from_beats_decimal(0.25, 64), r(1, 4));
        assert_eq!(RationalTime::from_beats_decimal(1.0 / 3.0, 64), r(1, 3));
        assert_eq!(RationalTime::from_beats_decimal(5.0 / 7.0, 64), r(5, 7));
        assert_eq!(RationalTime::from_beats_decimal(2.75, 64), r(11, 4));
        assert_eq!(RationalTime::from_beats_decimal(-0.5, 64), r(-1, 2));
    }

    #[test]
    fn gcd_lcm_basics() {
        assert_eq!(gcd(12, 18), 6);
        assert_eq!(gcd(7, 13), 1);
        assert_eq!(lcm(4, 6), 12);
        assert_eq!(lcm(0, 5), 0);
    }

    #[test]
    fn resolution_step_beats() {
        assert_eq!(Resolution::Whole(4).step_beats(), r(1, 1)); // quarter = 1 beat
        assert_eq!(Resolution::Whole(16).step_beats(), r(1, 4)); // 1/16 = 1/4 beat
        assert_eq!(Resolution::Whole(8).step_beats(), r(1, 2));
        assert_eq!(Resolution::Whole(7).step_beats(), r(4, 7)); // septuplet-of-whole
        assert_eq!(Resolution::default_edit().step_beats(), r(1, 4));
    }

    #[test]
    fn tuplet_scale_triplet() {
        // Triplet 3:2 — three notes in the span of two; each scaled by 2/3.
        let t = Tuplet::new(3, 2);
        assert_eq!(t.scale(), r(2, 3));
        // Three 1/8-triplet steps (base 1/8 note = 1/2 beat) span one beat.
        let base = Resolution::Whole(8).step_beats(); // 1/2 beat
        let span = base * t.scale() * 3;
        assert_eq!(span, r(1, 1));
        assert!(!t.is_none());
        assert!(Tuplet::NONE.is_none());
        assert_eq!(Tuplet::new(0, 5), Tuplet::NONE);
    }

    #[test]
    fn step_to_beats_straight_and_tuplet() {
        // Straight 1/16 grid: step 4 -> 1 beat.
        assert_eq!(
            step_to_beats(RationalTime::ZERO, 4, Resolution::Whole(16), Tuplet::NONE),
            r(1, 1)
        );
        // 1/8 triplet: step 3 (one full triplet group) -> 1 beat.
        assert_eq!(
            step_to_beats(RationalTime::ZERO, 3, Resolution::Whole(8), Tuplet::new(3, 2)),
            r(1, 1)
        );
        // Offset from a non-zero origin.
        assert_eq!(
            step_to_beats(r(2, 1), 2, Resolution::Whole(16), Tuplet::NONE),
            r(5, 2)
        );
    }

    #[test]
    fn subdivide_seven() {
        // 4 beats divided into 7 equal parts.
        let parts = subdivide(RationalTime::ZERO, r(4, 1), 7);
        assert_eq!(parts.len(), 7);
        assert_eq!(parts[0], RationalTime::ZERO);
        assert_eq!(parts[1], r(4, 7));
        assert_eq!(parts[6], r(24, 7));
        // Spacing is uniform and exact.
        for w in parts.windows(2) {
            assert_eq!(w[1] - w[0], r(4, 7));
        }
    }

    #[test]
    fn polyrhythm_lcm_grid() {
        // 4 vs 5 vs 7 divisions across a bar -> grid of 140 slots.
        assert_eq!(lcm_grid_den(&[4, 5, 7]), 140);
        // 3 vs 4 -> 12. 11 vs 13 -> 143.
        assert_eq!(lcm_grid_den(&[3, 4]), 12);
        assert_eq!(lcm_grid_den(&[11, 13]), 143);
        // Each pattern's steps land exactly on the shared grid.
        let grid = lcm_grid_den(&[4, 5]); // 20
        let a = r(1, 4); // 5/20
        let b = r(1, 5); // 4/20
        assert_eq!(a.den() * (grid / a.den()), grid);
        assert_eq!(b.den() * (grid / b.den()), grid);
    }

    #[test]
    fn serde_roundtrip() {
        let x = r(5, 7);
        let j = serde_json::to_string(&x).unwrap();
        let back: RationalTime = serde_json::from_str(&j).unwrap();
        assert_eq!(x, back);

        let res = Resolution::Whole(12);
        let j = serde_json::to_string(&res).unwrap();
        let back: Resolution = serde_json::from_str(&j).unwrap();
        assert_eq!(res, back);
    }
}
