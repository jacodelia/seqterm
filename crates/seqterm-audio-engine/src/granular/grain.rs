//! Individual grain — one windowed fragment of a source buffer.

use seqterm_core::{GrainDirection, GrainEnvelope};

/// A single active grain.
#[derive(Clone)]
pub struct Grain {
    /// Read position in the source buffer (fractional sample index).
    pub read_pos:   f64,
    /// Per-sample increment (controls pitch + direction + speed).
    pub increment:  f64,
    /// Total duration of this grain in source samples.
    pub duration:   f64,
    /// Samples rendered so far (for envelope phase).
    pub elapsed:    f64,
    /// Grain amplitude at spawn time (velocity scaling).
    pub amplitude:  f32,
    /// Pan: −1.0 (L) to +1.0 (R).
    pub pan:        f32,
    /// Envelope shape.
    pub envelope:   GrainEnvelope,
    /// Direction (cached from spawn).
    pub direction:  GrainDirection,
    /// Source buffer length in samples.
    pub buf_len:    usize,
    pub active:     bool,
}

impl Default for Grain {
    fn default() -> Self {
        Self {
            read_pos:  0.0,
            increment: 1.0,
            duration:  1024.0,
            elapsed:   0.0,
            amplitude: 1.0,
            pan:       0.0,
            envelope:  GrainEnvelope::Hann,
            direction: GrainDirection::Forward,
            buf_len:   0,
            active:    false,
        }
    }
}

impl Grain {
    /// Spawn this grain.
    pub fn spawn(
        &mut self,
        start_pos: f64,
        duration_samples: f64,
        pitch_ratio: f64,
        amplitude: f32,
        pan: f32,
        envelope: GrainEnvelope,
        direction: GrainDirection,
        buf_len: usize,
    ) {
        self.read_pos  = start_pos;
        self.increment = match direction {
            GrainDirection::Forward  => pitch_ratio,
            GrainDirection::Backward => -pitch_ratio,
            GrainDirection::Random   => if rand_bool() { pitch_ratio } else { -pitch_ratio },
        };
        self.duration  = duration_samples.max(1.0);
        self.elapsed   = 0.0;
        self.amplitude = amplitude;
        self.pan       = pan.clamp(-1.0, 1.0);
        self.envelope  = envelope;
        self.direction = direction;
        self.buf_len   = buf_len;
        self.active    = true;
    }

    /// Render one sample pair (L, R) from the source buffer and advance.
    /// Returns `(left, right)`. Deactivates the grain when done.
    #[inline]
    pub fn render_sample(&mut self, src: &[f32], env_sample: f32) -> (f32, f32) {
        if !self.active || self.buf_len == 0 { return (0.0, 0.0); }

        // Linear interpolation read
        let pos = self.read_pos.rem_euclid(self.buf_len as f64);
        let i0  = pos as usize;
        let i1  = (i0 + 1) % self.buf_len;
        let frac = (pos - i0 as f64) as f32;
        let sample = src[i0] * (1.0 - frac) + src[i1] * frac;

        let amp  = sample * env_sample * self.amplitude;
        let pan_r = (self.pan + 1.0) * 0.5;
        let pan_l = 1.0 - pan_r;
        let out = (amp * pan_l, amp * pan_r);

        self.read_pos += self.increment;
        // Bounce off buffer edges for bidirectional playback
        if self.read_pos < 0.0 { self.read_pos = 0.0; self.increment = self.increment.abs(); }
        if self.read_pos >= self.buf_len as f64 { self.read_pos = (self.buf_len - 1) as f64; self.increment = -self.increment.abs(); }

        self.elapsed += 1.0;
        if self.elapsed >= self.duration {
            self.active = false;
        }
        out
    }

    /// Envelope phase in [0, 1] based on elapsed / duration.
    #[inline]
    pub fn env_phase(&self) -> f32 {
        (self.elapsed / self.duration).clamp(0.0, 1.0) as f32
    }
}

// Minimal LCG for grain direction randomisation (no std thread_rng allocation).
static GRAIN_RNG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0xABCD_EF01);

fn rand_bool() -> bool {
    let v = GRAIN_RNG.fetch_add(0x9E37_79B9, std::sync::atomic::Ordering::Relaxed);
    v & 1 == 0
}
