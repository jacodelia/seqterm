//! LUFS (Loudness Units Full Scale) measurement per ITU-R BS.1770-4.
//!
//! Implements K-weighting filter + 400 ms block gating + 3 s short-term window.
//! Designed to run inside the audio callback — allocation-free after construction.

/// A single transposed-direct-form II biquad section.
struct Biquad {
    b0: f64, b1: f64, b2: f64,
    a1: f64, a2: f64,
    z1: f64, z2: f64,
}

impl Biquad {
    fn new(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Self {
        Self { b0, b1, b2, a1, a2, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Two-stage K-weighting filter for one audio channel.
struct KWeightFilter {
    pre: Biquad,  // pre-filter / shelving stage
    rlb: Biquad,  // RLB weighting (high-pass)
}

impl KWeightFilter {
    /// Compute K-weighting coefficients for `sample_rate` using bilinear transform.
    /// Reference: ITU-R BS.1770-4 Annex 1.
    fn for_sample_rate(sr: f64) -> Self {
        // Pre-filter (shelf): Fc ≈ 1681.97 Hz, boost +4.0 dB
        // Prototype: f0=1681.97, Q=1/sqrt(2), dBgain=+3.999843853
        let f0 = 1681.974450955533;
        let g  = 10.0f64.powf(3.999843853 / 20.0);
        let q  = 0.7071752369554196;
        let k  = (std::f64::consts::PI * f0 / sr).tan();
        let v0 = g.sqrt();
        let a0_pre = 1.0 + k / q + k * k;
        let b0_pre = (v0 + v0 * k / q + k * k) / a0_pre;
        let b1_pre = (2.0 * (k * k - v0)) / a0_pre;
        let b2_pre = (v0 - v0 * k / q + k * k) / a0_pre;
        let a1_pre = (2.0 * (k * k - 1.0)) / a0_pre;
        let a2_pre = (1.0 - k / q + k * k) / a0_pre;

        // RLB high-pass filter: Fc ≈ 38.13 Hz, Q=0.5003270373
        let f1 = 38.13547087613982;
        let q1 = 0.5003270373238773;
        let k1 = (std::f64::consts::PI * f1 / sr).tan();
        let a0_rlb = 1.0 + k1 / q1 + k1 * k1;
        let b0_rlb = 1.0 / a0_rlb;
        let b1_rlb = -2.0 / a0_rlb;
        let b2_rlb = 1.0 / a0_rlb;
        let a1_rlb = (2.0 * (k1 * k1 - 1.0)) / a0_rlb;
        let a2_rlb = (1.0 - k1 / q1 + k1 * k1) / a0_rlb;

        Self {
            pre: Biquad::new(b0_pre, b1_pre, b2_pre, a1_pre, a2_pre),
            rlb: Biquad::new(b0_rlb, b1_rlb, b2_rlb, a1_rlb, a2_rlb),
        }
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        self.rlb.process(self.pre.process(x))
    }
}

/// Full-program LUFS integrator per ITU-R BS.1770-4.
///
/// Realtime-safe: no allocation after construction.
pub struct LufsIntegrator {
    // Per-channel K-weighting filters.
    kw_l: KWeightFilter,
    kw_r: KWeightFilter,
    // 400 ms block accumulator.
    block_sum: f64,
    block_samples: u32,
    block_size: u32, // samples per 400 ms block
    // Short-term (3 s) sliding window: 7 overlapping 400 ms blocks → 8 slots.
    short_window: [f64; 8],
    short_idx: usize,
    // Integrated loudness gating state.
    gated_sum: f64,
    gated_count: u64,
    // Absolute gate threshold: −70 LUFS = 10^((-70+0.691)/10) ≈ 1.17e-7
    // We store as mean-square threshold.
    abs_gate_ms: f64,
    // Published outputs (updated each 400 ms block).
    pub momentary_lufs: f32,
    pub short_term_lufs: f32,
    pub integrated_lufs: f32,
}

impl LufsIntegrator {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f64;
        let block_size = (sr * 0.4).round() as u32;
        // Absolute gate: -70 LUFS → mean-square = 10^((-70+0.691)/10)
        let abs_gate_ms = 10.0f64.powf((-70.0 + 0.691) / 10.0);
        Self {
            kw_l: KWeightFilter::for_sample_rate(sr),
            kw_r: KWeightFilter::for_sample_rate(sr),
            block_sum: 0.0,
            block_samples: 0,
            block_size,
            short_window: [0.0; 8],
            short_idx: 0,
            gated_sum: 0.0,
            gated_count: 0,
            abs_gate_ms,
            momentary_lufs: -f32::INFINITY,
            short_term_lufs: -f32::INFINITY,
            integrated_lufs: -f32::INFINITY,
        }
    }

    /// Process one stereo interleaved frame (2 samples: L, R).
    #[inline]
    pub fn process_frame(&mut self, l: f32, r: f32) {
        let wl = self.kw_l.process(l as f64);
        let wr = self.kw_r.process(r as f64);
        self.block_sum += wl * wl + wr * wr;
        self.block_samples += 1;
        if self.block_samples >= self.block_size {
            self.commit_block();
        }
    }

    fn commit_block(&mut self) {
        if self.block_samples == 0 { return; }
        // Mean-square for this block (sum of both channels / (2 * N)).
        let ms = self.block_sum / (2.0 * self.block_samples as f64);
        self.block_sum = 0.0;
        self.block_samples = 0;

        // Momentary loudness: single block.
        self.momentary_lufs = lufs_from_ms(ms) as f32;

        // Short-term: average of current sliding window (up to 7.5 blocks = 3 s).
        self.short_window[self.short_idx % 8] = ms;
        self.short_idx += 1;
        let n_short = self.short_idx.min(8);
        let short_ms: f64 = self.short_window.iter().take(n_short).sum::<f64>() / n_short as f64;
        self.short_term_lufs = lufs_from_ms(short_ms) as f32;

        // Integrated: absolute gate only (full BS.1770 also uses relative gate, omitted for simplicity).
        if ms > self.abs_gate_ms {
            self.gated_sum += ms;
            self.gated_count += 1;
            if self.gated_count > 0 {
                let int_ms = self.gated_sum / self.gated_count as f64;
                self.integrated_lufs = lufs_from_ms(int_ms) as f32;
            }
        }
    }

    pub fn reset(&mut self) {
        self.block_sum = 0.0;
        self.block_samples = 0;
        self.short_window = [0.0; 8];
        self.short_idx = 0;
        self.gated_sum = 0.0;
        self.gated_count = 0;
        self.momentary_lufs = -f32::INFINITY;
        self.short_term_lufs = -f32::INFINITY;
        self.integrated_lufs = -f32::INFINITY;
    }
}

/// Convert mean-square to LUFS. Formula: LUFS = -0.691 + 10 * log10(ms).
#[inline]
fn lufs_from_ms(ms: f64) -> f64 {
    if ms < 1e-30 { return -f64::INFINITY; }
    -0.691 + 10.0 * ms.log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lufs_1khz_sine_near_minus_3() {
        let sr = 48000u32;
        let mut meter = LufsIntegrator::new(sr);
        // 0 dBFS sine at 1 kHz for 3 seconds.
        for i in 0..(sr * 3) {
            let t = i as f32 / sr as f32;
            let s = (2.0 * std::f32::consts::PI * 1000.0 * t).sin();
            meter.process_frame(s, s);
        }
        let lufs = meter.integrated_lufs;
        assert!(lufs.is_finite(), "LUFS should be finite for 1 kHz sine");
        // 0 dBFS full-scale sine → approximately -3 LUFS (varies with K-weighting at 1 kHz).
        assert!(lufs > -8.0 && lufs < 2.0,
            "1 kHz 0 dBFS sine should be near -3 LUFS, got {lufs:.1}");
    }

    #[test]
    fn lufs_silence_is_infinite() {
        let mut meter = LufsIntegrator::new(48000);
        for _ in 0..48000 {
            meter.process_frame(0.0, 0.0);
        }
        assert!(!meter.integrated_lufs.is_finite() || meter.integrated_lufs < -60.0);
    }
}
