//! S4Texture — live granular "texture engine" inspired by the *workflow* of
//! gesture/granular performance boxes (e.g. the Torso S-4 family): a continuously
//! written circular buffer scanned by a pool of windowed grains around an
//! independent **scrub** read head, with **Freeze** (hold the buffer = drones),
//! **Feedback** (resample its own grain output for layering), and **Stretch**
//! (the scrub head drifts slower/faster/backwards than real time).
//!
//! NOT a clone of any product's DSP — original implementation. It shares the
//! grain/circular-buffer skeleton with [`super::Protocosmos`] (so the two stay
//! consistent and cheap), but exposes live-performance controls instead of the
//! reverb/diffusion ones.
//!
//! ```text
//!   in ──┬───────────────────────────────────────────► dry
//!        ▼ (skipped when Freeze)
//!     [ circular buffer ] ◄── Feedback (grain output recirculated)
//!        ▲
//!   grains: spawn around the SCRUB head ± Scatter, Hann-windowed, ± random pitch
//!        │ sum
//!        ▼
//!     stereo grain cloud ──► wet
//!   scrub head drifts by (Stretch-0.5) each sample (ping-pong at the edges)
//! ```
//!
//! Source = whatever audio is already on the slot's FX chain, i.e. the Pattern
//! voice (sample / synth / tracker). No global input is read. All buffers are
//! preallocated; the audio callback never allocates.

use super::{FxParam, FxProcessor};

const MAX_BUF_S: f32 = 4.0;
const MAX_GRAINS: usize = 12;

struct Grain {
    pos: f64,    // fractional read index into the buffer
    speed: f64,  // signed playback rate (negative = reverse)
    age: u32,
    life: u32,
    gain: f32,
    active: bool,
}

/// Named starting points (param sets) — usable as future preset picks. Order =
/// `[Size, Density, Pitch, Scatter, Feedback, Freeze, Stretch, Wet]`.
pub const S4_PRESETS: &[(&str, [f32; 8])] = &[
    ("Frozen Pad",      [0.70, 0.50, 0.50, 0.20, 0.30, 1.00, 0.50, 0.60]),
    ("Cloud",           [0.45, 0.65, 0.50, 0.40, 0.25, 0.00, 0.50, 0.55]),
    ("Broken Tape",     [0.30, 0.40, 0.45, 0.55, 0.45, 0.00, 0.30, 0.60]),
    ("Microloop",       [0.12, 0.80, 0.50, 0.10, 0.55, 1.00, 0.55, 0.70]),
    ("Reverse Rain",    [0.25, 0.70, 0.55, 0.60, 0.30, 0.00, 0.20, 0.60]),
    ("Particle Swarm",  [0.18, 0.90, 0.60, 0.75, 0.35, 0.00, 0.65, 0.65]),
    ("Digital Mist",    [0.55, 0.55, 0.50, 0.35, 0.20, 1.00, 0.50, 0.50]),
    ("Infinite Drone",  [0.80, 0.45, 0.50, 0.15, 0.60, 1.00, 0.50, 0.70]),
    ("Granular Delay",  [0.35, 0.50, 0.50, 0.30, 0.50, 0.00, 0.50, 0.55]),
    ("Glitch Clouds",   [0.15, 0.85, 0.55, 0.85, 0.40, 0.00, 0.70, 0.60]),
];

/// Live granular texture processor.
pub struct S4Texture {
    sample_rate: u32,
    // Normalised 0..1 controls.
    size: f32,      // grain length
    density: f32,   // grains/sec
    pitch: f32,     // 0.5 = unison; ±12 st
    scatter: f32,   // position spray + random pitch
    feedback: f32,  // grain output recirculated into the buffer
    freeze: f32,    // >0.5 = hold buffer (stop writing)
    stretch: f32,   // scrub-head drift: 0.5 static, <0.5 reverse, >0.5 forward
    wet: f32,

    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write: usize,
    scrub: f64,     // independent read head the grains cluster around
    pingpong: f64,  // ±1 drift direction for ping-pong at the buffer edges
    grains: [Grain; MAX_GRAINS],
    spawn_timer: f64,
    rng: u64,
}

impl S4Texture {
    #[allow(clippy::too_many_arguments)]
    pub fn new(sr: u32, size: f32, density: f32, pitch: f32, scatter: f32,
               feedback: f32, freeze: f32, stretch: f32) -> Self {
        let sr = sr.max(8000);
        let len = (MAX_BUF_S * sr as f32) as usize + 4;
        const DEAD: Grain = Grain { pos: 0.0, speed: 1.0, age: 0, life: 1, gain: 0.0, active: false };
        Self {
            sample_rate: sr,
            size, density, pitch, scatter, feedback, freeze, stretch, wet: 0.6,
            buf_l: vec![0.0; len],
            buf_r: vec![0.0; len],
            write: 0,
            scrub: 0.0,
            pingpong: 1.0,
            grains: [DEAD; MAX_GRAINS],
            spawn_timer: 0.0,
            rng: 0x5EED_4D1C_7E27_0001,
        }
    }

    #[inline]
    fn rand(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.rng >> 40) as f32 / (1u32 << 24) as f32
    }

    fn spawn(&mut self) {
        let Some(idx) = self.grains.iter().position(|g| !g.active) else { return };
        let len = self.buf_l.len();
        let sr = self.sample_rate as f32;
        // Cluster grains around the SCRUB head, scattered back by up to 0.5 s.
        let spray_samps = self.scatter * 0.5 * sr;
        let offset = (self.rand() as f64) * spray_samps as f64;
        let pos = (self.scrub - offset).rem_euclid(len as f64);
        // Pitch: ±12 st from the knob, plus ± up to 4 st of random scatter.
        let jitter = (self.rand() * 2.0 - 1.0) * self.scatter * 4.0;
        let st = (self.pitch - 0.5) * 24.0 + jitter;
        let speed = 2.0_f64.powf(st as f64 / 12.0);
        // Grain length 20..200 ms scaled by `size`.
        let grain_ms = 20.0 + self.size * 180.0;
        let life = ((grain_ms / 1000.0) * sr).max(2.0) as u32;
        self.grains[idx] = Grain { pos, speed, age: 0, life, gain: 0.9, active: true };
    }
}

impl FxProcessor for S4Texture {
    fn process_block(&mut self, buf: &mut [f32], sample_rate: u32) {
        if sample_rate != self.sample_rate {
            // Preserve current controls; rebuild buffers at the new rate.
            *self = S4Texture::new(sample_rate, self.size, self.density, self.pitch,
                self.scatter, self.feedback, self.freeze, self.stretch);
        }
        let len = self.buf_l.len();
        let sr = self.sample_rate as f32;
        let frozen = self.freeze > 0.5;
        let density = 1.0 + self.density * 79.0; // 1..80 grains/sec
        let inter_spawn = (sr / density) as f64;
        // Scrub drift in samples/sample: 0.5→0 (static), 0→-1 (reverse), 1→+1.
        let drift = ((self.stretch - 0.5) * 2.0) as f64;
        let fb = self.feedback.clamp(0.0, 0.95);
        let frames = buf.len() / 2;

        for i in 0..frames {
            let dry_l = buf[i * 2];
            let dry_r = buf[i * 2 + 1];

            // Advance the scrub head; when static, follow just behind the write
            // head so fresh audio is always under the grains.
            if drift.abs() < 1e-4 && !frozen {
                self.scrub = (self.write as f64 - 1.0).rem_euclid(len as f64);
            } else {
                self.scrub = (self.scrub + drift * self.pingpong).rem_euclid(len as f64);
            }

            // Spawn grains on schedule.
            if self.spawn_timer <= 0.0 {
                self.spawn();
                self.spawn_timer += inter_spawn;
            }
            self.spawn_timer -= 1.0;

            // Sum active grains (Hann-windowed, fractional read).
            let mut gl = 0.0f32;
            let mut gr = 0.0f32;
            for g in self.grains.iter_mut() {
                if !g.active { continue; }
                let p0 = g.pos as usize % len;
                let p1 = (p0 + 1) % len;
                let frac = g.pos.fract() as f32;
                let sl = self.buf_l[p0] * (1.0 - frac) + self.buf_l[p1] * frac;
                let sr_ = self.buf_r[p0] * (1.0 - frac) + self.buf_r[p1] * frac;
                let env = (std::f32::consts::PI * g.age as f32 / g.life as f32).sin().powi(2);
                let w = env * g.gain;
                gl += sl * w;
                gr += sr_ * w;
                g.pos = (g.pos + g.speed).rem_euclid(len as f64);
                g.age += 1;
                if g.age >= g.life { g.active = false; }
            }

            // Write input (+ Feedback of the grain cloud) unless Freeze holds it.
            if !frozen {
                self.buf_l[self.write] = (dry_l + gl * fb).clamp(-4.0, 4.0);
                self.buf_r[self.write] = (dry_r + gr * fb).clamp(-4.0, 4.0);
            }
            self.write = (self.write + 1) % len;

            buf[i * 2]     = dry_l + self.wet * (gl - dry_l);
            buf[i * 2 + 1] = dry_r + self.wet * (gr - dry_r);
        }
    }

    fn reset(&mut self) {
        self.buf_l.iter_mut().for_each(|v| *v = 0.0);
        self.buf_r.iter_mut().for_each(|v| *v = 0.0);
        self.write = 0;
        self.scrub = 0.0;
        self.pingpong = 1.0;
        self.grains.iter_mut().for_each(|g| g.active = false);
        self.spawn_timer = 0.0;
    }

    fn set_mix(&mut self, wet: f32) { self.wet = wet.clamp(0.0, 1.0); }
    fn name(&self) -> &str { "S4Texture" }

    fn params(&self) -> Vec<FxParam> {
        vec![
            FxParam::new("Size", self.size, 0.0, 1.0, ""),
            FxParam::new("Density", self.density, 0.0, 1.0, ""),
            FxParam::new("Pitch", self.pitch, 0.0, 1.0, ""),
            FxParam::new("Scatter", self.scatter, 0.0, 1.0, ""),
            FxParam::new("Feedbk", self.feedback, 0.0, 1.0, ""),
            FxParam::new("Freeze", self.freeze, 0.0, 1.0, ""),
            FxParam::new("Stretch", self.stretch, 0.0, 1.0, ""),
            FxParam::new("Wet", self.wet, 0.0, 1.0, ""),
        ]
    }

    /// Live update — preserves the buffer + grains (toggling Freeze must NOT
    /// rebuild, or the held audio would be wiped).
    fn set_param(&mut self, index: usize, value: f32) {
        let v = value.clamp(0.0, 1.0);
        match index {
            0 => self.size = v,
            1 => self.density = v,
            2 => self.pitch = v,
            3 => self.scatter = v,
            4 => self.feedback = v,
            5 => self.freeze = v,
            6 => self.stretch = v,
            7 => self.wet = v,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s4_is_finite_and_bounded() {
        let mut fx = S4Texture::new(48000, 0.4, 0.6, 0.5, 0.5, 0.4, 0.0, 0.5);
        fx.set_mix(0.8);
        let mut block: Vec<f32> = (0..2048).map(|i| 0.4 * (i as f32 * 0.05).sin()).collect();
        for _ in 0..20 {
            fx.process_block(&mut block, 48000);
            assert!(block.iter().all(|s| s.is_finite()));
            assert!(block.iter().all(|s| s.abs() < 8.0));
        }
    }

    #[test]
    fn freeze_holds_after_silence() {
        // Fill the buffer with audio, engage Freeze live (no rebuild), then go
        // silent — grains must keep scanning the held audio.
        let mut fx = S4Texture::new(48000, 0.5, 0.8, 0.5, 0.3, 0.3, 0.0, 0.5);
        fx.set_mix(1.0);
        let mut prime: Vec<f32> = (0..8192).map(|i| 0.6 * (i as f32 * 0.07).sin()).collect();
        fx.process_block(&mut prime, 48000);
        fx.set_param(5, 1.0); // Freeze ON
        let mut silence = vec![0.0f32; 8192];
        fx.process_block(&mut silence, 48000);
        let energy: f32 = silence.iter().map(|s| s.abs()).sum();
        assert!(energy > 0.0, "frozen buffer should still emit grains on silence");
    }

    #[test]
    fn presets_have_eight_params() {
        assert!(S4_PRESETS.iter().all(|(_, p)| p.len() == 8));
    }
}
