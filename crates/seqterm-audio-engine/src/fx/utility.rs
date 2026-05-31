/// Utility processors: Gain, Phase Invert, Mono Maker, Soft Clipper, Tube Saturation.

// ─── Gain ────────────────────────────────────────────────────────────────────

/// Simple gain stage (dB).
pub struct Gain {
    pub gain_db: f32, // -60..+24
    mix: f32,
}

impl Gain {
    pub fn new() -> Self { Self { gain_db: 0.0, mix: 1.0 } }
}

impl Default for Gain { fn default() -> Self { Self::new() } }

impl super::FxProcessor for Gain {
    fn process_block(&mut self, buf: &mut [f32], _sr: u32) {
        let g = 10.0f32.powf(self.gain_db / 20.0);
        for s in buf.iter_mut() { *s *= g; }
    }
    fn reset(&mut self) {}
    fn set_mix(&mut self, wet: f32) { self.mix = wet.clamp(0.0, 1.0); }
}

// ─── Phase Invert ─────────────────────────────────────────────────────────────

/// Polarity inversion per channel.
pub struct PhaseInvert {
    pub invert_l: bool,
    pub invert_r: bool,
}

impl PhaseInvert {
    pub fn new() -> Self { Self { invert_l: true, invert_r: false } }
    pub fn both() -> Self { Self { invert_l: true, invert_r: true } }
}

impl Default for PhaseInvert { fn default() -> Self { Self::new() } }

impl super::FxProcessor for PhaseInvert {
    fn process_block(&mut self, buf: &mut [f32], _sr: u32) {
        let frames = buf.len() / 2;
        for i in 0..frames {
            if self.invert_l { buf[i * 2]     = -buf[i * 2]; }
            if self.invert_r { buf[i * 2 + 1] = -buf[i * 2 + 1]; }
        }
    }
    fn reset(&mut self) {}
    fn set_mix(&mut self, _wet: f32) {}
}

// ─── Mono Maker ───────────────────────────────────────────────────────────────

/// Sum L+R to mono on both channels.
pub struct MonoMaker {
    mix: f32,
}

impl MonoMaker {
    pub fn new() -> Self { Self { mix: 1.0 } }
}

impl Default for MonoMaker { fn default() -> Self { Self::new() } }

impl super::FxProcessor for MonoMaker {
    fn process_block(&mut self, buf: &mut [f32], _sr: u32) {
        let frames = buf.len() / 2;
        for i in 0..frames {
            let l = buf[i * 2];
            let r = buf[i * 2 + 1];
            let m = (l + r) * 0.5;
            buf[i * 2]     = l + self.mix * (m - l);
            buf[i * 2 + 1] = r + self.mix * (m - r);
        }
    }
    fn reset(&mut self) {}
    fn set_mix(&mut self, wet: f32) { self.mix = wet.clamp(0.0, 1.0); }
}

// ─── Soft Clipper ─────────────────────────────────────────────────────────────

/// Smooth soft-clip saturation using `tanh` (same as the master bus limiter).
pub struct SoftClipper {
    pub drive: f32, // 1.0..10.0 — amount of gain into the clipper
    mix: f32,
}

impl SoftClipper {
    pub fn new() -> Self { Self { drive: 2.0, mix: 1.0 } }
}

impl Default for SoftClipper { fn default() -> Self { Self::new() } }

impl super::FxProcessor for SoftClipper {
    fn process_block(&mut self, buf: &mut [f32], _sr: u32) {
        let drive = self.drive;
        let inv_drive = 1.0 / drive;
        let frames = buf.len() / 2;
        for i in 0..frames {
            let l = buf[i * 2];
            let r = buf[i * 2 + 1];
            let wl = (l * drive).tanh() * inv_drive;
            let wr = (r * drive).tanh() * inv_drive;
            buf[i * 2]     = l + self.mix * (wl - l);
            buf[i * 2 + 1] = r + self.mix * (wr - r);
        }
    }
    fn reset(&mut self) {}
    fn set_mix(&mut self, wet: f32) { self.mix = wet.clamp(0.0, 1.0); }
}

// ─── Tube Saturation ──────────────────────────────────────────────────────────

/// Warm asymmetric tube-style saturation using a polynomial waveshaper.
///
/// Combines soft-clip for positive half-waves with a slightly harder
/// negative-half characteristic, reproducing the asymmetry of a triode stage.
pub struct TubeSaturation {
    pub drive: f32,  // 1.0..20.0
    pub tone:  f32,  // 0.0..1.0 — amount of 1-pole HP to remove mud
    mix: f32,
    hp_state_l: f32,
    hp_state_r: f32,
}

impl TubeSaturation {
    pub fn new() -> Self {
        Self { drive: 3.0, tone: 0.3, mix: 0.6,
               hp_state_l: 0.0, hp_state_r: 0.0 }
    }

    #[inline]
    fn waveshape(x: f32, drive: f32) -> f32 {
        let x_d = x * drive;
        if x_d >= 0.0 {
            // Soft positive half
            x_d.tanh() / drive
        } else {
            // Slightly harder negative half (asymmetry)
            let clamped = x_d.max(-1.0);
            (clamped - clamped * clamped * clamped / 3.0) / drive
        }
    }
}

impl Default for TubeSaturation { fn default() -> Self { Self::new() } }

impl super::FxProcessor for TubeSaturation {
    fn process_block(&mut self, buf: &mut [f32], sample_rate: u32) {
        if buf.len() < 2 { return; }
        let sr = sample_rate as f32;
        // 1-pole HP to remove DC / low-frequency mud; fc ≈ 80 Hz
        let hp_coeff = 1.0 - (-2.0 * std::f32::consts::PI * 80.0 / sr).exp();
        let drive = self.drive;

        let frames = buf.len() / 2;
        for i in 0..frames {
            let l = buf[i * 2];
            let r = buf[i * 2 + 1];

            let sat_l = Self::waveshape(l, drive);
            let sat_r = Self::waveshape(r, drive);

            // Tone HP (removes muddiness introduced by saturation)
            self.hp_state_l += hp_coeff * (sat_l - self.hp_state_l) * self.tone;
            self.hp_state_r += hp_coeff * (sat_r - self.hp_state_r) * self.tone;
            let out_l = sat_l - self.hp_state_l;
            let out_r = sat_r - self.hp_state_r;

            buf[i * 2]     = l + self.mix * (out_l - l);
            buf[i * 2 + 1] = r + self.mix * (out_r - r);
        }
    }

    fn reset(&mut self) {
        self.hp_state_l = 0.0;
        self.hp_state_r = 0.0;
    }
    fn set_mix(&mut self, wet: f32) { self.mix = wet.clamp(0.0, 1.0); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::FxProcessor;

    #[test]
    fn phase_invert_l_flips_left() {
        let mut p = PhaseInvert::new(); // invert_l=true, invert_r=false
        let mut buf = vec![0.5f32, 0.3f32, 0.5f32, 0.3f32];
        p.process_block(&mut buf, 48000);
        assert!((buf[0] - (-0.5)).abs() < 1e-6, "L should be inverted");
        assert!((buf[1] - 0.3).abs()   < 1e-6, "R should be unchanged");
    }

    #[test]
    fn mono_maker_sums_to_mono() {
        let mut m = MonoMaker::new();
        let mut buf = vec![1.0f32, 0.0f32, 1.0f32, 0.0f32]; // L=1, R=0
        m.process_block(&mut buf, 48000);
        assert!((buf[0] - 0.5).abs() < 1e-6, "L should be 0.5");
        assert!((buf[1] - 0.5).abs() < 1e-6, "R should be 0.5");
    }

    #[test]
    fn gain_unity_at_zero_db() {
        let mut g = Gain::new(); // 0.0 dB
        let mut buf = vec![0.4f32; 8];
        let before = buf[0];
        g.process_block(&mut buf, 48000);
        assert!((buf[0] - before).abs() < 1e-5, "0 dB gain should be unity");
    }

    #[test]
    fn soft_clipper_reduces_hot_signal() {
        let mut s = SoftClipper::new();
        s.drive = 5.0;
        let mut buf = vec![2.0f32; 64];
        s.process_block(&mut buf, 48000);
        assert!(buf[0] < 1.5, "soft clipper should reduce amplitude");
    }

    #[test]
    fn rms_fields_initialise_to_zero() {
        use crate::mixer::Mixer;
        let mixer = Mixer::new(512);
        assert_eq!(mixer.slot_rms[0], 0.0, "slot_rms should start at zero");
        assert_eq!(mixer.master_rms[0], 0.0, "master_rms[L] should start at zero");
        assert_eq!(mixer.master_rms[1], 0.0, "master_rms[R] should start at zero");
    }
}
