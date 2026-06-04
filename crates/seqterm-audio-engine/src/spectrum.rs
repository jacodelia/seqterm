//! Real-time spectrum analyzer.
//!
//! Accumulates samples, applies a Hanning window, FFT-transforms them,
//! and publishes power in N logarithmic frequency bands.
//!
//! REALTIME CONTRACT: `process_frame` must be allocation-free.
//! All buffers are pre-allocated in `new()`.

use rustfft::{FftPlanner, num_complex::Complex};

/// Number of frequency bands (logarithmic, covering ~20 Hz – 20 kHz).
pub const SPECTRUM_BANDS: usize = 32;

/// FFT size (power of 2 ≥ 1024 for reasonable frequency resolution).
const FFT_SIZE: usize = 2048;

pub struct SpectrumAnalyzer {
    /// Ring buffer of the last FFT_SIZE mono samples.
    buf: Vec<f32>,
    write_pos: usize,
    /// How many new samples since the last FFT computation.
    new_samples: usize,
    /// Hop size: how many new samples before the next FFT (50% overlap).
    hop: usize,
    /// Pre-computed Hanning window coefficients.
    window: Vec<f32>,
    /// Pre-allocated FFT scratch buffer.
    fft_buf: Vec<Complex<f32>>,
    /// FFT plan (stateless after creation).
    planner_output: std::sync::Arc<dyn rustfft::Fft<f32>>,
    /// Smoothed power per band (exponential moving average).
    pub bands: [f32; SPECTRUM_BANDS],
    /// Sample rate, used to map FFT bins to frequencies.
    sample_rate: f32,
}

impl SpectrumAnalyzer {
    pub fn new(sample_rate: u32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos())
            })
            .collect();
        Self {
            buf: vec![0.0f32; FFT_SIZE],
            write_pos: 0,
            new_samples: 0,
            hop: FFT_SIZE / 2,
            window,
            fft_buf: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            planner_output: fft,
            bands: [0.0f32; SPECTRUM_BANDS],
            sample_rate: sample_rate as f32,
        }
    }

    /// Feed one stereo interleaved frame. Returns true when a new FFT result is ready.
    #[inline]
    pub fn process_frame(&mut self, l: f32, r: f32) -> bool {
        let mono = (l + r) * 0.5;
        self.buf[self.write_pos % FFT_SIZE] = mono;
        self.write_pos += 1;
        self.new_samples += 1;

        if self.new_samples >= self.hop {
            self.new_samples = 0;
            self.compute_fft();
            return true;
        }
        false
    }

    fn compute_fft(&mut self) {
        // Fill FFT buffer with windowed samples from the ring buffer.
        // The most recent FFT_SIZE samples end at write_pos - 1.
        // Use wrapping_add to avoid overflow when start is near usize::MAX.
        let start = self.write_pos.wrapping_sub(FFT_SIZE);
        for i in 0..FFT_SIZE {
            let idx = start.wrapping_add(i) % FFT_SIZE;
            let s = self.buf[idx];
            let w = self.window[i];
            self.fft_buf[i] = Complex::new(s * w, 0.0);
        }

        self.planner_output.process(&mut self.fft_buf);

        // Map FFT bins to frequency bands (logarithmic).
        let bin_hz = self.sample_rate / FFT_SIZE as f32;
        let lo_hz = 20.0f32;
        let hi_hz = (self.sample_rate / 2.0).min(20000.0);
        let log_lo = lo_hz.log2();
        let log_hi = hi_hz.log2();

        const DECAY: f32 = 0.85;

        for b in 0..SPECTRUM_BANDS {
            let f_lo = 2.0f32.powf(log_lo + (b as f32) * (log_hi - log_lo) / SPECTRUM_BANDS as f32);
            let f_hi = 2.0f32.powf(log_lo + (b as f32 + 1.0) * (log_hi - log_lo) / SPECTRUM_BANDS as f32);
            let bin_lo = ((f_lo / bin_hz) as usize).max(1);
            let bin_hi = ((f_hi / bin_hz) as usize + 1).min(FFT_SIZE / 2);

            let power: f32 = if bin_hi > bin_lo {
                let sum: f32 = (bin_lo..bin_hi)
                    .map(|i| self.fft_buf[i].norm_sqr())
                    .sum();
                (sum / (bin_hi - bin_lo) as f32).sqrt() / (FFT_SIZE as f32 / 2.0)
            } else {
                0.0
            };

            // EMA smoothing.
            self.bands[b] = self.bands[b] * DECAY + power * (1.0 - DECAY);
        }
    }

    pub fn reset(&mut self) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.write_pos = 0;
        self.new_samples = 0;
        self.bands = [0.0f32; SPECTRUM_BANDS];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectrum_sine_shows_peak_in_correct_band() {
        let sr = 48000u32;
        let mut sa = SpectrumAnalyzer::new(sr);
        // 1 kHz sine for 4096 frames.
        for i in 0..(sr * 2) {
            let t = i as f32 / sr as f32;
            let s = (2.0 * std::f32::consts::PI * 1000.0 * t).sin();
            sa.process_frame(s, s);
        }
        // Find the band with the highest magnitude.
        let max_band = sa.bands.iter().enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        // 1 kHz should fall in roughly band 16-22 out of 32.
        assert!(max_band >= 12 && max_band <= 26,
            "1 kHz peak expected in bands 12-26, got {max_band}");
    }
}
