//! Grain envelope shapes — precomputed lookup tables for zero-alloc rendering.

use seqterm_core::GrainEnvelope;

/// Size of every envelope lookup table (power of 2 for fast wrapping).
pub const ENV_TABLE_SIZE: usize = 1024;

/// Precomputed lookup tables for all grain envelope shapes.
pub struct EnvelopeTables {
    pub hann:        [f32; ENV_TABLE_SIZE],
    pub gaussian:    [f32; ENV_TABLE_SIZE],
    pub triangle:    [f32; ENV_TABLE_SIZE],
    pub exponential: [f32; ENV_TABLE_SIZE],
}

impl EnvelopeTables {
    pub fn build() -> Self {
        let n = ENV_TABLE_SIZE;
        let mut hann        = [0.0f32; ENV_TABLE_SIZE];
        let mut gaussian    = [0.0f32; ENV_TABLE_SIZE];
        let mut triangle    = [0.0f32; ENV_TABLE_SIZE];
        let mut exponential = [0.0f32; ENV_TABLE_SIZE];

        for i in 0..n {
            let t = i as f32 / (n - 1) as f32; // 0.0 – 1.0
            hann[i] = (std::f32::consts::PI * t).sin().powi(2);
            // Gaussian: σ = 0.4, centred at 0.5
            let sigma = 0.4_f32;
            let x = t - 0.5;
            gaussian[i] = (-0.5 * (x / sigma).powi(2)).exp();
            triangle[i] = if t < 0.5 { 2.0 * t } else { 2.0 * (1.0 - t) };
            exponential[i] = if t < 0.5 {
                (std::f32::consts::E.powf(2.0 * t) - 1.0) / (std::f32::consts::E - 1.0)
            } else {
                let u = 1.0 - t;
                (std::f32::consts::E.powf(2.0 * u) - 1.0) / (std::f32::consts::E - 1.0)
            };
        }

        Self { hann, gaussian, triangle, exponential }
    }

    /// Sample an envelope at normalised phase [0.0, 1.0].
    #[inline]
    pub fn sample(&self, kind: GrainEnvelope, phase: f32) -> f32 {
        let idx = ((phase.clamp(0.0, 1.0) * (ENV_TABLE_SIZE - 1) as f32) as usize)
            .min(ENV_TABLE_SIZE - 1);
        match kind {
            GrainEnvelope::Hann        => self.hann[idx],
            GrainEnvelope::Gaussian    => self.gaussian[idx],
            GrainEnvelope::Triangle    => self.triangle[idx],
            GrainEnvelope::Exponential => self.exponential[idx],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_zero_at_boundaries() {
        let t = EnvelopeTables::build();
        assert!(t.hann[0] < 1e-6);
        assert!(t.hann[ENV_TABLE_SIZE - 1] < 1e-6);
    }

    #[test]
    fn hann_peak_near_centre() {
        let t = EnvelopeTables::build();
        let centre = t.hann[ENV_TABLE_SIZE / 2];
        assert!(centre > 0.99, "Hann peak should be ~1.0, got {centre}");
    }

    #[test]
    fn triangle_symmetric() {
        let t = EnvelopeTables::build();
        let half = ENV_TABLE_SIZE / 2;
        assert!((t.triangle[half / 2] - t.triangle[half + half / 2]).abs() < 0.02);
    }

    #[test]
    fn all_shapes_in_unit_range() {
        let t = EnvelopeTables::build();
        for i in 0..ENV_TABLE_SIZE {
            assert!((0.0..=1.0001).contains(&t.hann[i]));
            assert!((0.0..=1.0001).contains(&t.gaussian[i]));
            assert!((0.0..=1.0001).contains(&t.triangle[i]));
            assert!((0.0..=1.0001).contains(&t.exponential[i]));
        }
    }
}
