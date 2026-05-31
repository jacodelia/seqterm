//! Granular synthesis engine.
//!
//! Maintains a fixed pool of [`Grain`]s and a source buffer; spawns new grains
//! according to density and position parameters; mixes all active grains
//! into the output block each tick.
//!
//! **Realtime-safe**: no allocation after construction, no mutex in the audio path.

use super::{envelopes::EnvelopeTables, grain::Grain};
use seqterm_core::{GrainParams, GranularMod, GranularZone, LfoShape, ModTarget, MOD_SLOTS, ScanMode};
use seqterm_ports::realtime::AudioSource;

const MAX_GRAINS: usize = 32;

/// Granular synthesis engine — owns source buffer and grain pool.
pub struct GranularEngine {
    /// Source audio buffer (mono, f32).
    source: Vec<f32>,
    /// Frozen copy of the source (used when `zone.frozen = true`).
    freeze_buf: Vec<f32>,
    /// Grain voice pool.
    grains: [Grain; MAX_GRAINS],
    /// Precomputed envelope tables.
    envs: EnvelopeTables,
    /// Samples until next grain spawn.
    next_spawn: f64,
    /// Internal LCG for spray / jitter randomisation.
    rng: u64,
    /// Linear scan playhead position as fraction of source (0.0–1.0).
    playhead: f64,
    /// RandomWalk current position as fraction of source (0.0–1.0).
    walk_pos: f64,
    /// Owned params for AudioSource rendering (updated via set_params).
    params: GrainParams,
    /// Owned zone for AudioSource rendering (updated via set_zone).
    zone: GranularZone,
    /// Modulation matrix (4 LFO slots).
    mod_matrix: GranularMod,
    /// LFO phase accumulators (0.0–1.0 per slot).
    lfo_phases: [f64; MOD_SLOTS],
    /// Last output value for S&H LFOs (updated at phase wrap).
    lfo_sh_vals: [f32; MOD_SLOTS],
    /// Whether this source is active (set by activate/deactivate or when source loaded).
    active: bool,

    // ── Live input mode ───────────────────────────────────────────────────────
    /// Pre-allocated circular buffer for live pattern input (mono, 2 s @ 48 kHz).
    live_ring:   Vec<f32>,
    /// Write head into `live_ring`.
    live_write:  usize,
    /// When true, `live_ring` is used as source instead of `source`.
    live_active: bool,
}

impl GranularEngine {
    pub fn new() -> Self {
        let grains = std::array::from_fn(|_| Grain::default());
        let zone = GranularZone::default();
        let pos = zone.position as f64;
        Self {
            source:     Vec::new(),
            freeze_buf: Vec::new(),
            grains,
            envs:       EnvelopeTables::build(),
            next_spawn: 0.0,
            rng:        0xDEAD_CAFE_1234_5678,
            playhead:   pos,
            walk_pos:   pos,
            params:      GrainParams::default(),
            zone,
            mod_matrix:  GranularMod::default(),
            lfo_phases:  [0.0; MOD_SLOTS],
            lfo_sh_vals: [0.0; MOD_SLOTS],
            active:      false,
            live_ring:   vec![0.0f32; 96_000], // 2 s @ 48 kHz mono
            live_write:  0,
            live_active: false,
        }
    }

    /// Update grain parameters (call from non-RT thread before activate).
    pub fn set_params(&mut self, params: GrainParams) { self.params = params; }

    /// Update zone settings (call from non-RT thread or via AudioCommand).
    /// Snaps scan positions to new zone.position when it shifts significantly.
    pub fn set_zone(&mut self, zone: GranularZone) {
        if (zone.position as f64 - self.zone.position as f64).abs() > 0.005 {
            self.playhead = zone.position as f64;
            self.walk_pos = zone.position as f64;
        }
        self.zone = zone;
    }

    /// Read current zone (for incremental updates via AudioCommand).
    pub fn zone(&self) -> &GranularZone { &self.zone }

    /// Set frozen flag without replacing other zone parameters.
    pub fn set_frozen(&mut self, frozen: bool) { self.zone.frozen = frozen; }

    /// Update the LFO modulation matrix.
    pub fn set_mod_matrix(&mut self, m: GranularMod) { self.mod_matrix = m; }
    pub fn mod_matrix(&self) -> &GranularMod { &self.mod_matrix }

    /// Mark this engine as active so the Mixer will render it.
    pub fn activate(&mut self) { self.active = true; }

    /// Silence and deactivate.
    pub fn deactivate(&mut self) { self.active = false; }

    /// Load (or replace) the source buffer. Not realtime-safe — call from non-RT thread.
    pub fn load_source(&mut self, samples: Vec<f32>) {
        self.source = samples;
        self.freeze_buf.clear();
    }

    /// Freeze: take a snapshot of the current source into the freeze buffer.
    pub fn freeze(&mut self) {
        self.freeze_buf = self.source.clone();
    }

    // ── Live input ────────────────────────────────────────────────────────────

    /// Enable or disable live ring-buffer mode.
    /// When enabled, the ring buffer is used as source; `source` is ignored.
    pub fn set_live_active(&mut self, active: bool) {
        self.live_active = active;
        if active {
            self.live_ring.iter_mut().for_each(|s| *s = 0.0);
            self.live_write = 0;
        }
    }

    pub fn is_live_active(&self) -> bool { self.live_active }

    /// Push a stereo interleaved block from another mixer slot into the live ring.
    /// Downmixes to mono by averaging L+R. REALTIME SAFE — no alloc.
    pub fn push_live_samples(&mut self, stereo: &[f32]) {
        let ring_len = self.live_ring.len();
        let frames = stereo.len() / 2;
        for i in 0..frames {
            let mono = (stereo[i * 2] + stereo[i * 2 + 1]) * 0.5;
            self.live_ring[self.live_write] = mono;
            self.live_write = (self.live_write + 1) % ring_len;
        }
        // Once the ring has been filled at least once, the engine can produce grains.
        if !self.live_active { return; }
        // Ensure activate is set so the mixer renders this slot.
        self.active = true;
    }

    /// Return true when a source buffer is loaded or live mode is active.
    pub fn has_source(&self) -> bool { self.live_active || !self.source.is_empty() }

    /// Render `frames` stereo samples into `out` (interleaved L/R).
    /// `params` and `zone` are read each block (no locking needed — update from same thread).
    pub fn render_block(&mut self, out: &mut [f32], frames: usize, params: &GrainParams, zone: &GranularZone, sample_rate: u32) {
        // Determine which buffer to read from — capture length before any mutable borrows.
        // Priority: freeze > live_ring > source.
        let frozen  = zone.frozen && !self.freeze_buf.is_empty();
        let use_live = self.live_active && !frozen;
        let buf_len = if frozen {
            self.freeze_buf.len()
        } else if use_live {
            self.live_ring.len()
        } else {
            self.source.len()
        };
        if buf_len == 0 { return; }

        let sr = sample_rate as f64;
        let inter_spawn = sr / params.density as f64;
        let grain_dur   = (params.size_ms as f64 / 1000.0 * sr).max(1.0);
        let pitch_ratio = semitones_to_ratio(params.pitch_st as f64);

        // Zone scan boundaries (clamped to [0, 1]).
        let zone_center = zone.position as f64;
        let half_range  = (zone.range as f64 * 0.5).clamp(0.0, 0.5);
        let zone_lo     = (zone_center - half_range).clamp(0.0, 1.0);
        let zone_hi     = (zone_center + half_range).clamp(0.0, 1.0);
        let zone_span   = (zone_hi - zone_lo).max(1e-6);

        // Per-frame advance for Linear scan: traverse zone span in ~2 s at scan_speed=1.
        let linear_step = zone.scan_speed as f64 * zone_span / (sr * 2.0);
        // Per-frame walk step for RandomWalk: small Brownian drift within the zone.
        let walk_step_scale = zone.scan_speed as f64 * zone_span * 0.003;

        for i in 0..frames {
            // Advance scan positions.
            match zone.scan_mode {
                ScanMode::Linear => {
                    self.playhead += linear_step;
                    if self.playhead > zone_hi  { self.playhead = zone_lo; }
                    if self.playhead < zone_lo  { self.playhead = zone_lo; }
                }
                ScanMode::RandomWalk => {
                    let step = walk_step_scale * self.rand_f64_signed();
                    self.walk_pos = (self.walk_pos + step).clamp(zone_lo, zone_hi);
                }
                ScanMode::Freeze => {}
            }

            // Effective spawn position for this frame.
            let effective_pos = match zone.scan_mode {
                ScanMode::Linear     => self.playhead,
                ScanMode::RandomWalk => self.walk_pos,
                ScanMode::Freeze     => zone_center,
            };

            // Spawn new grains when the counter expires.
            if self.next_spawn <= 0.0 {
                let jitter    = params.jitter as f64 * grain_dur * self.rand_f64();
                self.next_spawn = inter_spawn + jitter;
                self.try_spawn_grain(buf_len, params, zone, grain_dur, pitch_ratio, effective_pos);
            }
            self.next_spawn -= 1.0;

            // Mix all active grains.
            let mut l = 0.0f32;
            let mut r = 0.0f32;
            for grain in self.grains.iter_mut() {
                if !grain.active { continue; }
                let phase = grain.env_phase();
                let env   = self.envs.sample(grain.envelope, phase);
                let src: &[f32] = if frozen { &self.freeze_buf } else if use_live { &self.live_ring } else { &self.source };
                let (gl, gr) = grain.render_sample(src, env);
                l += gl;
                r += gr;
            }

            out[i * 2]     += l * params.gain;
            out[i * 2 + 1] += r * params.gain;
        }
    }

    fn try_spawn_grain(
        &mut self,
        buf_len: usize,
        params: &GrainParams,
        zone: &GranularZone,
        grain_dur: f64,
        pitch_ratio: f64,
        effective_pos: f64,
    ) {
        // Compute random values before taking the mutable grain borrow.
        let spray_rand  = self.rand_f64_signed();
        let spread_rand = self.rand_f64_signed();

        let spray_offset = params.spray as f64 * spray_rand * zone.range as f64 * buf_len as f64;
        let centre       = effective_pos * buf_len as f64;
        let start        = (centre + spray_offset).clamp(0.0, (buf_len - 1) as f64);
        let pan          = params.pan + params.stereo_spread * spread_rand as f32;

        // Find an inactive slot by index to avoid multiple-borrow.
        let slot_idx = self.grains.iter().position(|g| !g.active);
        let Some(idx) = slot_idx else { return };

        self.grains[idx].spawn(
            start,
            grain_dur,
            pitch_ratio,
            1.0,
            pan,
            params.envelope,
            params.direction,
            buf_len,
        );
    }

    /// Advance LFO phases by one block and add modulation to `params`.
    fn apply_lfo_block(&mut self, params: &mut GrainParams, frames: usize, sample_rate: u32) {
        use std::f64::consts::TAU;
        let sr = sample_rate as f64;
        let block_secs = frames as f64 / sr;

        for i in 0..MOD_SLOTS {
            // Copy fields to avoid holding a shared borrow when calling rand_f64.
            let (enabled, shape, rate_hz, depth_frac, target) = {
                let s = &self.mod_matrix.slots[i];
                (s.enabled, s.shape, s.rate_hz, s.depth, s.target)
            };
            if !enabled { continue; }

            let phase_advance = rate_hz as f64 * block_secs;
            let old_phase = self.lfo_phases[i];
            self.lfo_phases[i] = (old_phase + phase_advance).rem_euclid(1.0);

            // Compute LFO value in -1..+1.
            let lfo_val: f32 = match shape {
                LfoShape::Sine     => (TAU * old_phase).sin() as f32,
                LfoShape::Triangle => (if old_phase < 0.5 { 4.0 * old_phase - 1.0 } else { 3.0 - 4.0 * old_phase }) as f32,
                LfoShape::Square   => if old_phase < 0.5 { 1.0 } else { -1.0 },
                LfoShape::SampleHold => {
                    if self.lfo_phases[i] < old_phase {
                        self.lfo_sh_vals[i] = (self.rand_f64() * 2.0 - 1.0) as f32;
                    }
                    self.lfo_sh_vals[i]
                }
            };

            let depth = depth_frac * lfo_val;
            match target {
                ModTarget::Spray     => params.spray     = (params.spray     + depth * 1.0).clamp(0.0, 1.0),
                ModTarget::Density   => params.density   = (params.density   + depth * 50.0).clamp(0.1, 200.0),
                ModTarget::PitchSt   => params.pitch_st  =  params.pitch_st  + depth * 24.0,
                ModTarget::Pan       => params.pan       = (params.pan       + depth * 1.0).clamp(-1.0, 1.0),
                ModTarget::GrainSize => params.size_ms   = (params.size_ms   + depth * 200.0).clamp(1.0, 500.0),
                ModTarget::Overlap   => params.overlap   = (params.overlap   + depth * 1.0).clamp(0.0, 1.0),
                ModTarget::Jitter    => params.jitter    = (params.jitter    + depth * 1.0).clamp(0.0, 1.0),
            }
        }
    }

    fn rand_f64(&mut self) -> f64 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    fn rand_f64_signed(&mut self) -> f64 {
        self.rand_f64() * 2.0 - 1.0
    }

    /// Number of currently active grains.
    pub fn active_grain_count(&self) -> usize {
        self.grains.iter().filter(|g| g.active).count()
    }
}

impl Default for GranularEngine {
    fn default() -> Self { Self::new() }
}

impl AudioSource for GranularEngine {
    fn render(&mut self, output: &mut [f32], sample_rate: u32) -> usize {
        if !self.has_source() { return 0; }
        let frames = output.len() / 2;
        for s in output.iter_mut() { *s = 0.0; }
        // Apply LFO modulation to a params copy — one LFO update per block.
        let mut params = self.params.clone();
        let zone = self.zone.clone();
        self.apply_lfo_block(&mut params, frames, sample_rate);
        self.render_block(output, frames, &params, &zone, sample_rate);
        frames
    }

    fn is_active(&self) -> bool {
        self.active && self.has_source()
    }

    fn stop(&mut self) {
        self.active = false;
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

#[inline]
fn semitones_to_ratio(st: f64) -> f64 {
    2.0_f64.powf(st / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use seqterm_core::{GrainParams, GranularZone};

    #[test]
    fn engine_renders_silence_with_no_source() {
        let mut eng = GranularEngine::new();
        let params = GrainParams::default();
        let zone   = GranularZone::default();
        let mut out = vec![0.0f32; 256];
        eng.render_block(&mut out, 128, &params, &zone, 48000);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn engine_produces_output_with_source() {
        let mut eng = GranularEngine::new();
        let sr = 48000u32;
        let source: Vec<f32> = (0..sr as usize)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin())
            .collect();
        eng.load_source(source);

        let params = GrainParams { density: 20.0, size_ms: 50.0, ..GrainParams::default() };
        let zone   = GranularZone { position: 0.5, ..GranularZone::default() };
        let mut out = vec![0.0f32; 512];
        eng.render_block(&mut out, 256, &params, &zone, sr);

        let energy: f32 = out.iter().map(|&s| s * s).sum();
        assert!(energy > 0.0, "should produce non-zero output");
    }

    #[test]
    fn semitones_to_ratio_octave() {
        let r = semitones_to_ratio(12.0);
        assert!((r - 2.0).abs() < 1e-10);
    }

    #[test]
    fn freeze_copies_source() {
        let mut eng = GranularEngine::new();
        eng.load_source(vec![0.5f32; 100]);
        eng.freeze();
        assert_eq!(eng.freeze_buf.len(), 100);
        assert_eq!(eng.freeze_buf[0], 0.5);
    }

    #[test]
    fn linear_scan_advances_playhead() {
        let mut eng = GranularEngine::new();
        let sr = 48000u32;
        let source: Vec<f32> = vec![0.5f32; sr as usize];
        eng.load_source(source);
        let params = GrainParams { density: 20.0, size_ms: 50.0, ..GrainParams::default() };
        let zone = GranularZone {
            position: 0.5, range: 0.5, scan_speed: 1.0,
            scan_mode: seqterm_core::ScanMode::Linear, ..GranularZone::default()
        };
        eng.set_zone(zone.clone());
        let ph_before = eng.playhead;
        let mut out = vec![0.0f32; 512];
        eng.render_block(&mut out, 256, &params, &zone, sr);
        // Playhead should have advanced from its starting position.
        assert!(eng.playhead != ph_before || zone.scan_speed == 0.0,
            "linear scan should advance playhead: before={ph_before}, after={}", eng.playhead);
    }

    #[test]
    fn random_walk_stays_within_zone() {
        let mut eng = GranularEngine::new();
        let sr = 48000u32;
        eng.load_source(vec![0.5f32; sr as usize]);
        let params = GrainParams { density: 20.0, size_ms: 50.0, ..GrainParams::default() };
        let zone = GranularZone {
            position: 0.5, range: 0.4, scan_speed: 1.0,
            scan_mode: seqterm_core::ScanMode::RandomWalk, ..GranularZone::default()
        };
        eng.set_zone(zone.clone());
        let mut out = vec![0.0f32; 512];
        eng.render_block(&mut out, 256, &params, &zone, sr);
        // walk_pos should stay within [0.3, 0.7] zone range.
        assert!(eng.walk_pos >= 0.3 - 1e-6 && eng.walk_pos <= 0.7 + 1e-6,
            "walk_pos out of zone: {}", eng.walk_pos);
    }

    #[test]
    fn grain_count_increases_after_render() {
        let mut eng = GranularEngine::new();
        eng.load_source(vec![0.3f32; 48000]);
        let params = GrainParams { density: 100.0, size_ms: 200.0, ..GrainParams::default() };
        let zone   = GranularZone::default();
        let mut out = vec![0.0f32; 512];
        eng.render_block(&mut out, 256, &params, &zone, 48000);
        assert!(eng.active_grain_count() > 0);
    }
}
