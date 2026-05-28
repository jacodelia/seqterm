//! In-engine audio mixer: sums N AudioSource slots into one stereo output.
//!
//! REALTIME RULE: no allocation, no mutex inside `mix()`.

use seqterm_ports::realtime::AudioSource;
use crate::fx::FxProcessor;

/// Maximum concurrent audio slots (SF2 synths + audio clips).
pub const MAX_SLOTS: usize = 32;

/// Number of aux send buses (Bus A + Bus B).
pub const MAX_BUSES: usize = 2;

/// One slot in the realtime mixer.
pub struct MixerSlot {
    pub source: Option<Box<dyn AudioSource>>,
    pub volume: f32,
    pub active: bool,
    /// Post-fader send level to Bus A (0.0 = off, 1.0 = full).
    pub send_a: f32,
    /// Post-fader send level to Bus B (0.0 = off, 1.0 = full).
    pub send_b: f32,
    /// Pre-fader insert FX chain. Applied after render, before volume scaling.
    /// Elements are pre-allocated; no heap alloc in the audio path.
    pub fx_chain: Vec<Box<dyn FxProcessor>>,
}

impl MixerSlot {
    pub fn empty() -> Self {
        Self { source: None, volume: 1.0, active: false, send_a: 0.0, send_b: 0.0, fx_chain: Vec::new() }
    }
}

/// Realtime mixer: holds up to MAX_SLOTS sources, mixes them each block.
pub struct Mixer {
    pub slots: Vec<MixerSlot>,
    pub master_volume: f32,
    /// Pre-allocated mix scratch buffer (stereo interleaved).
    scratch: Vec<f32>,
    /// Per-bus accumulation buffers (stereo interleaved, [bus_idx][sample]).
    bus_scratch: [Vec<f32>; MAX_BUSES],
    /// Per-bus return volume (linear, 0.0 = silent).
    pub bus_volumes: [f32; MAX_BUSES],
    /// Per-bus mute flag.
    pub bus_muted: [bool; MAX_BUSES],
    /// Master bus insert FX chain — applied to the summed stereo output before soft-clip.
    pub master_fx: Vec<Box<dyn FxProcessor>>,
    /// Peak level per slot (updated each block, decays when slot is silent).
    pub slot_peaks: [f32; MAX_SLOTS],
    /// Master output peak L/R (updated each block after master FX + soft-clip).
    pub master_peak: [f32; 2],
}

impl Mixer {
    pub fn new(max_block: usize) -> Self {
        let mut slots = Vec::with_capacity(MAX_SLOTS);
        for _ in 0..MAX_SLOTS {
            slots.push(MixerSlot::empty());
        }
        Self {
            slots,
            master_volume: 1.0,
            scratch: vec![0.0f32; max_block * 2],
            bus_scratch: [
                vec![0.0f32; max_block * 2],
                vec![0.0f32; max_block * 2],
            ],
            bus_volumes:  [1.0; MAX_BUSES],
            bus_muted:    [false; MAX_BUSES],
            master_fx:    Vec::new(),
            slot_peaks:   [0.0; MAX_SLOTS],
            master_peak:  [0.0; 2],
        }
    }

    /// Mix all active slots into `output` (interleaved stereo).
    /// REALTIME SAFE: no allocation, no mutex.
    pub fn mix(&mut self, output: &mut [f32], sample_rate: u32) {
        let n = output.len();

        // Zero output and bus accumulators.
        for s in output.iter_mut() { *s = 0.0; }
        for bus in self.bus_scratch.iter_mut() {
            for s in bus[..n].iter_mut() { *s = 0.0; }
        }

        const PEAK_DECAY: f32 = 0.98; // per-block decay factor (~-0.2 dB/block)

        for (slot_idx, slot) in self.slots.iter_mut().enumerate() {
            if !slot.active {
                // Decay peak toward zero when slot is inactive.
                self.slot_peaks[slot_idx] *= PEAK_DECAY;
                continue;
            }
            let src = match slot.source.as_mut() {
                Some(s) => s,
                None => { slot.active = false; continue; }
            };
            if !src.is_active() { slot.active = false; continue; }

            let scratch = &mut self.scratch[..n];
            for s in scratch.iter_mut() { *s = 0.0; }

            let frames = src.render(scratch, sample_rate);
            if frames == 0 { slot.active = false; continue; }

            // Pre-fader insert FX chain (zero-alloc — processors are pre-constructed).
            for fx in slot.fx_chain.iter_mut() {
                fx.process_block(scratch, sample_rate);
            }

            // Compute and store slot peak (post-FX, pre-fader).
            let block_peak = scratch[..n].iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            self.slot_peaks[slot_idx] = block_peak.max(self.slot_peaks[slot_idx] * PEAK_DECAY);

            let vol = slot.volume * self.master_volume;

            // Mix rendered samples into main output (SIMD on x86_64+AVX2).
            mix_accumulate(output, scratch, vol);

            // Post-fader sends (applied after volume scaling).
            if slot.send_a > 0.0 {
                mix_accumulate(&mut self.bus_scratch[0][..n], scratch, slot.send_a * vol);
            }
            if slot.send_b > 0.0 {
                mix_accumulate(&mut self.bus_scratch[1][..n], scratch, slot.send_b * vol);
            }
        }

        // Add bus returns to the main output.
        for bus_idx in 0..MAX_BUSES {
            if self.bus_muted[bus_idx] { continue; }
            let bvol = self.bus_volumes[bus_idx];
            if bvol == 0.0 { continue; }
            mix_accumulate(output, &self.bus_scratch[bus_idx][..n], bvol);
        }

        // Master bus insert FX chain.
        for fx in self.master_fx.iter_mut() {
            fx.process_block(&mut output[..n], sample_rate);
        }

        // Soft clip to prevent distortion on sum overflow.
        for s in output.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }

        // Master peak (post soft-clip): track L and R separately.
        let frames = n / 2;
        let peak_l = (0..frames).map(|i| output[i * 2].abs()).fold(0.0f32, f32::max);
        let peak_r = (0..frames).map(|i| output[i * 2 + 1].abs()).fold(0.0f32, f32::max);
        self.master_peak[0] = peak_l.max(self.master_peak[0] * PEAK_DECAY);
        self.master_peak[1] = peak_r.max(self.master_peak[1] * PEAK_DECAY);
    }

    /// Install a source into a slot. Takes ownership.
    pub fn set_slot(&mut self, slot_id: usize, source: Box<dyn AudioSource>, volume: f32) {
        if slot_id >= self.slots.len() { return; }
        self.slots[slot_id].source = Some(source);
        self.slots[slot_id].volume = volume;
        self.slots[slot_id].active = true;
    }

    /// Remove a slot's source.
    pub fn clear_slot(&mut self, slot_id: usize) {
        if slot_id >= self.slots.len() { return; }
        if let Some(src) = self.slots[slot_id].source.as_mut() {
            src.stop();
        }
        self.slots[slot_id].active = false;
    }

    /// Set per-slot volume.
    pub fn set_slot_volume(&mut self, slot_id: usize, volume: f32) {
        if slot_id < self.slots.len() {
            self.slots[slot_id].volume = volume;
        }
    }

    /// Set per-slot bus send levels (post-fader, linear gain).
    pub fn set_slot_sends(&mut self, slot_id: usize, send_a: f32, send_b: f32) {
        if slot_id < self.slots.len() {
            self.slots[slot_id].send_a = send_a;
            self.slots[slot_id].send_b = send_b;
        }
    }

    /// Set return volume for a bus.
    pub fn set_bus_volume(&mut self, bus_idx: usize, volume: f32) {
        if bus_idx < MAX_BUSES {
            self.bus_volumes[bus_idx] = volume;
        }
    }

    /// Mute or unmute a bus return.
    pub fn set_bus_muted(&mut self, bus_idx: usize, muted: bool) {
        if bus_idx < MAX_BUSES {
            self.bus_muted[bus_idx] = muted;
        }
    }
}

// ─── SIMD-accelerated accumulation ────────────────────────────────────────────

/// `output[i] += src[i] * gain` — uses AVX2 on x86_64 when available, scalar fallback.
#[inline]
fn mix_accumulate(output: &mut [f32], src: &[f32], gain: f32) {
    let n = output.len().min(src.len());

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            // Safety: feature detection confirmed both AVX2 and FMA at runtime.
            unsafe { mix_accumulate_avx2(&mut output[..n], &src[..n], gain); }
            return;
        }
    }

    // Scalar fallback.
    for (o, &s) in output[..n].iter_mut().zip(src[..n].iter()) {
        *o += s * gain;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn mix_accumulate_avx2(output: &mut [f32], src: &[f32], gain: f32) {
    use std::arch::x86_64::*;

    let n = output.len();
    let gain_v = _mm256_set1_ps(gain);
    let mut i = 0usize;

    // Process 8 f32 lanes at a time.
    while i + 8 <= n {
        unsafe {
            let o_ptr = output.as_mut_ptr().add(i);
            let s_ptr = src.as_ptr().add(i);
            let o_v = _mm256_loadu_ps(o_ptr);
            let s_v = _mm256_loadu_ps(s_ptr);
            // Fused multiply-add: o + s * gain
            let r_v = _mm256_fmadd_ps(s_v, gain_v, o_v);
            _mm256_storeu_ps(o_ptr, r_v);
        }
        i += 8;
    }

    // Scalar tail.
    while i < n {
        unsafe {
            *output.get_unchecked_mut(i) += *src.get_unchecked(i) * gain;
        }
        i += 1;
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use seqterm_ports::realtime::AudioSource;

    /// A trivial AudioSource that outputs a constant value (DC).
    struct DcSource {
        value: f32,
        active: bool,
    }
    impl DcSource {
        fn new(val: f32) -> Self { Self { value: val, active: true } }
    }
    impl AudioSource for DcSource {
        fn render(&mut self, buf: &mut [f32], _sr: u32) -> usize {
            if !self.active { return 0; }
            for s in buf.iter_mut() { *s = self.value; }
            buf.len()
        }
        fn is_active(&self) -> bool { self.active }
        fn stop(&mut self) { self.active = false; }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    #[test]
    fn empty_mixer_outputs_silence() {
        let mut mixer = Mixer::new(512);
        let mut out = vec![1.0f32; 64];
        mixer.mix(&mut out, 48000);
        assert!(out.iter().all(|&s| s == 0.0), "empty mixer should output silence");
    }

    #[test]
    fn single_slot_dc_source() {
        let mut mixer = Mixer::new(512);
        mixer.set_slot(0, Box::new(DcSource::new(0.5)), 1.0);
        let mut out = vec![0.0f32; 16];
        mixer.mix(&mut out, 48000);
        assert!(out.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn master_volume_scales_output() {
        let mut mixer = Mixer::new(512);
        mixer.set_slot(0, Box::new(DcSource::new(0.5)), 1.0);
        mixer.master_volume = 0.5;
        let mut out = vec![0.0f32; 16];
        mixer.mix(&mut out, 48000);
        assert!(out.iter().all(|&s| (s - 0.25).abs() < 1e-6));
    }

    #[test]
    fn slot_volume_scales_output() {
        let mut mixer = Mixer::new(512);
        mixer.set_slot(0, Box::new(DcSource::new(0.4)), 0.5);
        let mut out = vec![0.0f32; 16];
        mixer.mix(&mut out, 48000);
        assert!(out.iter().all(|&s| (s - 0.2).abs() < 1e-6));
    }

    #[test]
    fn soft_clip_clamps_overflow() {
        let mut mixer = Mixer::new(512);
        mixer.set_slot(0, Box::new(DcSource::new(1.0)), 1.0);
        mixer.set_slot(1, Box::new(DcSource::new(1.0)), 1.0);
        let mut out = vec![0.0f32; 16];
        mixer.mix(&mut out, 48000);
        assert!(out.iter().all(|&s| s <= 1.0));
    }

    #[test]
    fn inactive_source_deactivates_slot() {
        let mut mixer = Mixer::new(512);
        let mut src = DcSource::new(0.5);
        src.active = false;
        mixer.set_slot(0, Box::new(src), 1.0);
        let mut out = vec![0.0f32; 16];
        mixer.mix(&mut out, 48000);
        assert!(out.iter().all(|&s| s == 0.0));
        assert!(!mixer.slots[0].active);
    }

    #[test]
    fn clear_slot_stops_source() {
        let mut mixer = Mixer::new(512);
        mixer.set_slot(0, Box::new(DcSource::new(1.0)), 1.0);
        mixer.clear_slot(0);
        assert!(!mixer.slots[0].active);
        let mut out = vec![0.0f32; 16];
        mixer.mix(&mut out, 48000);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn set_slot_volume_updates_correctly() {
        let mut mixer = Mixer::new(512);
        mixer.set_slot(0, Box::new(DcSource::new(1.0)), 1.0);
        mixer.set_slot_volume(0, 0.25);
        let mut out = vec![0.0f32; 16];
        mixer.mix(&mut out, 48000);
        assert!(out.iter().all(|&s| (s - 0.25).abs() < 1e-6));
    }

    #[test]
    fn send_a_routes_to_bus() {
        let mut mixer = Mixer::new(512);
        // Slot 0: DC 0.5, volume 1.0, send_a = 1.0 (full send).
        mixer.set_slot(0, Box::new(DcSource::new(0.5)), 1.0);
        mixer.set_slot_sends(0, 1.0, 0.0);
        // Bus A return volume = 1.0.
        mixer.bus_volumes[0] = 1.0;
        let mut out = vec![0.0f32; 16];
        mixer.mix(&mut out, 48000);
        // Direct path: 0.5.  Bus return: 0.5 * 1.0 * 1.0 = 0.5.  Total = 1.0 (clamped).
        assert!(out.iter().all(|&s| (s - 1.0).abs() < 1e-5));
    }

    #[test]
    fn muted_bus_contributes_nothing() {
        let mut mixer = Mixer::new(512);
        mixer.set_slot(0, Box::new(DcSource::new(0.3)), 1.0);
        mixer.set_slot_sends(0, 1.0, 0.0);
        mixer.set_bus_muted(0, true);
        let mut out = vec![0.0f32; 16];
        mixer.mix(&mut out, 48000);
        // Only direct path: 0.3.
        assert!(out.iter().all(|&s| (s - 0.3).abs() < 1e-5));
    }
}
