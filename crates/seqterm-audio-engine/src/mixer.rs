//! In-engine audio mixer: sums N AudioSource slots into one stereo output.
//!
//! REALTIME RULE: no allocation, no mutex inside `mix()`.

use seqterm_ports::realtime::AudioSource;
use crate::fx::FxProcessor;
use crate::granular::engine::GranularEngine;
use crate::lufs::LufsIntegrator;
use crate::spectrum::SpectrumAnalyzer;

/// Maximum concurrent audio slots (SF2 synths + audio clips).
pub const MAX_SLOTS: usize = 32;

/// Waveform ring buffer length (L-channel samples) for the live oscilloscope.
pub const WAVE_LEN: usize = 1024;

/// Exponential smoothing coefficient for the correlation meter (~100 ms at 48 kHz/512 block).
const CORR_DECAY: f32 = 0.85;

/// Number of aux send buses (Bus A + Bus B).
pub const MAX_BUSES: usize = 2;

/// Number of group routing buses (1-8, index 0-7).
pub const MAX_GROUP_BUSES: usize = 8;

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
    /// Group bus routing: 0 = master mix, 1-8 = group bus 1-8.
    pub group_bus: u8,
}

impl MixerSlot {
    pub fn empty() -> Self {
        Self { source: None, volume: 1.0, active: false, send_a: 0.0, send_b: 0.0, fx_chain: Vec::new(), group_bus: 0 }
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
    /// Per-group-bus accumulation buffers (stereo interleaved, index = group_bus-1).
    group_scratch: Vec<Vec<f32>>,
    /// Per-group-bus return volume (linear, 0.0 = silent, 1.0 = unity).
    pub group_vols: Vec<f32>,
    /// Per-group-bus mute flag.
    pub group_muted: Vec<bool>,
    /// Peak level per group bus (L/R), decays like slot peaks.
    pub group_peaks: Vec<[f32; 2]>,
    /// Peak level per slot (updated each block, decays when slot is silent).
    pub slot_peaks: [f32; MAX_SLOTS],
    /// RMS level per slot (exponential moving average, updated each block).
    pub slot_rms: [f32; MAX_SLOTS],
    /// Master output peak L/R (updated each block after master FX + soft-clip).
    pub master_peak: [f32; 2],
    /// Master RMS L/R.
    pub master_rms: [f32; 2],
    /// Live-input links: (source_slot_idx, granular_slot_idx).
    /// After source_slot renders, its scratch is fed into the granular engine's live ring.
    pub live_links: Vec<(usize, usize)>,
    /// Per-slot tap buffers (stereo interleaved); populated each block for live-link feeding.
    /// Pre-allocated: MAX_SLOTS × max_block*2.
    tap_buffers: Vec<Vec<f32>>,
    /// Slot id to capture for the live oscilloscope (-1 = none).
    pub waveform_slot: i32,
    /// Ring buffer of L-channel samples (pre-allocated, WAVE_LEN elements).
    pub waveform_buf: Vec<f32>,
    /// Monotonic write counter; index into ring = waveform_pos % WAVE_LEN.
    pub waveform_pos: usize,
    /// M/S stereo correlation coefficient (-1 = anti-phase, 0 = uncorrelated, +1 = mono).
    pub master_correlation: f32,
    /// LUFS integrator for the master output.
    pub lufs: LufsIntegrator,
    /// Spectrum analyzer for the master output (SPECTRUM_BANDS logarithmic bands).
    pub spectrum: SpectrumAnalyzer,
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
            group_scratch: (0..MAX_GROUP_BUSES).map(|_| vec![0.0f32; max_block * 2]).collect(),
            group_vols:   vec![1.0f32; MAX_GROUP_BUSES],
            group_muted:  vec![false; MAX_GROUP_BUSES],
            group_peaks:  vec![[0.0f32; 2]; MAX_GROUP_BUSES],
            slot_peaks:   [0.0; MAX_SLOTS],
            slot_rms:     [0.0; MAX_SLOTS],
            master_peak:  [0.0; 2],
            master_rms:   [0.0; 2],
            live_links:   Vec::new(),
            tap_buffers:  (0..MAX_SLOTS).map(|_| vec![0.0f32; max_block * 2]).collect(),
            waveform_slot: -1,
            waveform_buf:  vec![0.0f32; WAVE_LEN],
            waveform_pos:  0,
            master_correlation: 0.0,
            lufs: LufsIntegrator::new(48000),
            spectrum: SpectrumAnalyzer::new(48000),
        }
    }

    /// Mix all active slots into `output` (interleaved stereo).
    /// REALTIME SAFE: no allocation, no mutex.
    pub fn mix(&mut self, output: &mut [f32], sample_rate: u32) {
        let n = output.len();

        // Zero output, aux bus, and group bus accumulators.
        for s in output.iter_mut() { *s = 0.0; }
        for bus in self.bus_scratch.iter_mut() {
            for s in bus[..n].iter_mut() { *s = 0.0; }
        }
        for gb in self.group_scratch.iter_mut() {
            for s in gb[..n].iter_mut() { *s = 0.0; }
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

            // Compute slot peak and RMS (post-FX, pre-fader).
            let frames_f = (n / 2) as f32;
            let block_peak = scratch[..n].iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            self.slot_peaks[slot_idx] = block_peak.max(self.slot_peaks[slot_idx] * PEAK_DECAY);
            // RMS: sqrt(mean(x^2)) with exponential smoothing (τ ≈ 300 ms)
            const RMS_DECAY: f32 = 0.95;
            let sum_sq: f32 = scratch[..n].iter().map(|s| s * s).sum();
            let block_rms = (sum_sq / (frames_f * 2.0)).sqrt();
            self.slot_rms[slot_idx] = self.slot_rms[slot_idx] * RMS_DECAY
                + block_rms * (1.0 - RMS_DECAY);

            // Capture L-channel samples into waveform ring for live oscilloscope.
            if self.waveform_slot >= 0 && slot_idx == self.waveform_slot as usize {
                let frames = n / 2;
                for f in 0..frames {
                    let pos = self.waveform_pos % WAVE_LEN;
                    self.waveform_buf[pos] = scratch[f * 2]; // L channel
                    self.waveform_pos = self.waveform_pos.wrapping_add(1);
                }
            }

            let vol = slot.volume * self.master_volume;

            // Route post-fader signal to master mix or a group bus.
            if slot.group_bus == 0 {
                mix_accumulate(output, scratch, vol);
            } else {
                let gb_idx = (slot.group_bus as usize - 1).min(MAX_GROUP_BUSES - 1);
                mix_accumulate(&mut self.group_scratch[gb_idx][..n], scratch, vol);
            }

            // Post-fader aux sends (independent of group bus routing).
            if slot.send_a > 0.0 {
                mix_accumulate(&mut self.bus_scratch[0][..n], scratch, slot.send_a * vol);
            }
            if slot.send_b > 0.0 {
                mix_accumulate(&mut self.bus_scratch[1][..n], scratch, slot.send_b * vol);
            }

            // Copy rendered audio into the tap buffer for live-link feeding (second pass below).
            if self.live_links.iter().any(|&(src, _)| src == slot_idx) {
                self.tap_buffers[slot_idx][..n].copy_from_slice(&self.scratch[..n]);
            }
        }

        // Live-input second pass: feed tap buffers into linked granular engines.
        // Done outside the main loop to avoid borrow-checker conflicts.
        for &(src_idx, gran_idx) in &self.live_links {
            if let Some(gran_slot) = self.slots.get_mut(gran_idx) {
                if let Some(gran_src) = gran_slot.source.as_mut() {
                    if let Some(eng) = gran_src.as_any_mut().downcast_mut::<GranularEngine>() {
                        eng.push_live_samples(&self.tap_buffers[src_idx][..n]);
                    }
                }
            }
        }

        // Add aux bus returns to the main output.
        for bus_idx in 0..MAX_BUSES {
            if self.bus_muted[bus_idx] { continue; }
            let bvol = self.bus_volumes[bus_idx];
            if bvol == 0.0 { continue; }
            mix_accumulate(output, &self.bus_scratch[bus_idx][..n], bvol);
        }

        // Add group bus returns to the main output; update per-group peaks.
        for gb_idx in 0..MAX_GROUP_BUSES {
            let gvol = self.group_vols[gb_idx];
            if self.group_muted[gb_idx] || gvol == 0.0 { continue; }
            let has_signal = self.group_scratch[gb_idx][..n].iter().any(|&s| s != 0.0);
            if has_signal {
                mix_accumulate(output, &self.group_scratch[gb_idx][..n], gvol);
                let frames = n / 2;
                let pk_l = (0..frames).map(|i| self.group_scratch[gb_idx][i*2].abs()).fold(0.0f32, f32::max);
                let pk_r = (0..frames).map(|i| self.group_scratch[gb_idx][i*2+1].abs()).fold(0.0f32, f32::max);
                self.group_peaks[gb_idx][0] = pk_l.max(self.group_peaks[gb_idx][0] * PEAK_DECAY);
                self.group_peaks[gb_idx][1] = pk_r.max(self.group_peaks[gb_idx][1] * PEAK_DECAY);
            } else {
                self.group_peaks[gb_idx][0] *= PEAK_DECAY;
                self.group_peaks[gb_idx][1] *= PEAK_DECAY;
            }
        }

        // Master bus insert FX chain.
        for fx in self.master_fx.iter_mut() {
            fx.process_block(&mut output[..n], sample_rate);
        }

        // Soft clip to prevent distortion on sum overflow.
        for s in output.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }

        // No specific slot selected → capture the master output (post-FX) into the
        // waveform ring so the WAVE oscilloscope tracks the general audio output.
        if self.waveform_slot < 0 {
            let frames = n / 2;
            for f in 0..frames {
                let pos = self.waveform_pos % WAVE_LEN;
                self.waveform_buf[pos] = output[f * 2]; // L channel
                self.waveform_pos = self.waveform_pos.wrapping_add(1);
            }
        }

        // Master peak and RMS (post soft-clip): track L and R separately.
        let frames = n / 2;
        let peak_l = (0..frames).map(|i| output[i * 2].abs()).fold(0.0f32, f32::max);
        let peak_r = (0..frames).map(|i| output[i * 2 + 1].abs()).fold(0.0f32, f32::max);
        self.master_peak[0] = peak_l.max(self.master_peak[0] * PEAK_DECAY);
        self.master_peak[1] = peak_r.max(self.master_peak[1] * PEAK_DECAY);
        // Master RMS
        const RMS_DECAY: f32 = 0.95;
        let rms_l = ((0..frames).map(|i| output[i*2].powi(2)).sum::<f32>() / frames as f32).sqrt();
        let rms_r = ((0..frames).map(|i| output[i*2+1].powi(2)).sum::<f32>() / frames as f32).sqrt();
        self.master_rms[0] = self.master_rms[0] * RMS_DECAY + rms_l * (1.0 - RMS_DECAY);
        self.master_rms[1] = self.master_rms[1] * RMS_DECAY + rms_r * (1.0 - RMS_DECAY);

        // Correlation meter: Pearson correlation of L and R channels.
        let (mut sum_lr, mut sum_l2, mut sum_r2) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..frames {
            let l = output[i * 2] as f64;
            let r = output[i * 2 + 1] as f64;
            sum_lr += l * r;
            sum_l2 += l * l;
            sum_r2 += r * r;
        }
        let denom = (sum_l2 * sum_r2).sqrt();
        let corr_block = if denom > 1e-12 { (sum_lr / denom) as f32 } else { 0.0 };
        self.master_correlation = self.master_correlation * CORR_DECAY + corr_block * (1.0 - CORR_DECAY);

        // LUFS metering + Spectrum analysis.
        for i in 0..frames {
            let l = output[i * 2];
            let r = output[i * 2 + 1];
            self.lufs.process_frame(l, r);
            self.spectrum.process_frame(l, r);
        }
    }

    /// Set the sample rate (must be called when the audio stream starts or changes).
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.lufs = LufsIntegrator::new(sample_rate);
        self.spectrum = SpectrumAnalyzer::new(sample_rate);
    }

    /// Install a source into a slot. Takes ownership.
    pub fn set_slot(&mut self, slot_id: usize, source: Box<dyn AudioSource>, volume: f32) {
        if slot_id >= self.slots.len() { return; }
        self.slots[slot_id].source = Some(source);
        self.slots[slot_id].volume = volume;
        self.slots[slot_id].active = true;
    }

    /// Deactivate a slot (stops rendering; source stays in memory for possible restart).
    pub fn clear_slot(&mut self, slot_id: usize) {
        if slot_id >= self.slots.len() { return; }
        if let Some(src) = self.slots[slot_id].source.as_mut() {
            src.stop();
        }
        self.slots[slot_id].active = false;
    }

    /// Unload a slot completely: drop the source and free its memory.
    pub fn unload_slot(&mut self, slot_id: usize) {
        if slot_id >= self.slots.len() { return; }
        self.slots[slot_id].source = None;
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

    /// Set the group bus routing for a slot (0 = master, 1-8 = group bus).
    pub fn set_slot_group_bus(&mut self, slot_id: usize, group_bus: u8) {
        if slot_id < self.slots.len() {
            self.slots[slot_id].group_bus = group_bus;
        }
    }

    /// Set return volume for a group bus (linear gain).
    pub fn set_group_vol(&mut self, gb_idx: usize, volume: f32) {
        if gb_idx < MAX_GROUP_BUSES {
            self.group_vols[gb_idx] = volume;
        }
    }

    /// Mute or unmute a group bus.
    pub fn set_group_muted(&mut self, gb_idx: usize, muted: bool) {
        if gb_idx < MAX_GROUP_BUSES {
            self.group_muted[gb_idx] = muted;
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

    #[test]
    fn slot_rms_converges_to_signal_amplitude() {
        // A DC source of amplitude 0.5 has RMS = 0.5.
        // After many blocks the EMA (decay=0.95) should converge within 2%.
        let buf_size = 512;
        let mut mixer = Mixer::new(buf_size);
        mixer.set_slot(0, Box::new(DcSource::new(0.5)), 1.0);

        let mut out = vec![0.0f32; buf_size];
        // 200 blocks is well past the EMA time constant (τ = 20 blocks at decay=0.95).
        for _ in 0..200 {
            mixer.mix(&mut out, 48000);
        }
        let rms = mixer.slot_rms[0];
        assert!((rms - 0.5).abs() < 0.02, "slot_rms should converge to 0.5, got {rms}");
    }

    #[test]
    fn master_rms_tracks_output_level() {
        let buf_size = 512;
        let mut mixer = Mixer::new(buf_size);
        // Two equal DC sources sum to 1.0, which the soft-clip clamps back to 1.0.
        mixer.set_slot(0, Box::new(DcSource::new(0.3)), 1.0);

        let mut out = vec![0.0f32; buf_size];
        for _ in 0..200 {
            mixer.mix(&mut out, 48000);
        }
        let rms_l = mixer.master_rms[0];
        let rms_r = mixer.master_rms[1];
        // DC 0.3 → master RMS ≈ 0.3 (within 2%).
        assert!((rms_l - 0.3).abs() < 0.02, "master_rms[L] should be ~0.3, got {rms_l}");
        assert!((rms_r - 0.3).abs() < 0.02, "master_rms[R] should be ~0.3, got {rms_r}");
    }
}
