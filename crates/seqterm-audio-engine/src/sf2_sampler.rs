//! SeqTerm's own SF2 sampler.
//!
//! Plays an editable [`Sf2Instrument`] (loaded via [`crate::sf2_loader`]) by
//! resampling each zone's PCM with per-voice ADSR, a one-pole filter and forward
//! / ping-pong looping. This is the playback path that makes EDITOR edits to an
//! SF2 zone *audible* — it replaces fluidsynth for SF2 sources the user is
//! editing, so changes to envelope/filter/loop/tune/gain take effect live.
//!
//! REALTIME CONTRACT: `render`/`note_on`/`note_off`/`control_change` are
//! allocation- and lock-free. The instrument + sample pool are swapped in from a
//! non-RT loader thread via [`Sf2Sampler::set_instrument`].

use std::any::Any;

use seqterm_core::{Sf2Instrument, Sf2LoopMode};
use seqterm_ports::{AudioSource, AudioSynthPort};

use crate::sf2_loader::{LoadedSf2, Sf2SampleData};

const MAX_VOICES: usize = 32;

#[derive(Clone, Copy, PartialEq)]
enum Env { Idle, Attack, Hold, Decay, Sustain, Release }

#[derive(Clone, Copy)]
struct Voice {
    env: Env,
    channel: u8,
    note: u8,
    /// Index into the sampler's `samples` pool.
    sample_idx: usize,
    /// Fractional read position, in source frames.
    pos: f64,
    /// Read increment per output frame (pitch × resample ratio).
    step: f64,
    /// Loop window in source frames (end == 0 disables looping).
    loop_start: f64,
    loop_end: f64,
    loop_mode: Sf2LoopMode,
    /// True while a ping-pong loop is travelling backwards.
    reverse: bool,
    // ── ADSR (seconds + sustain level) ──
    attack_s: f32,
    hold_s: f32,
    decay_s: f32,
    sustain: f32,
    release_s: f32,
    level: f32,
    hold_left: f32, // seconds remaining in hold
    // ── Per-voice filter ──
    cutoff: f32,
    filter_kind: u8, // 0=LPF, 1=HPF, 2=BPF
    lp: f32,
    bp: f32,
    // ── Amplitude ──
    gain: f32, // zone gain × velocity (linear)
}

impl Voice {
    const fn silent() -> Self {
        Self {
            env: Env::Idle, channel: 0, note: 0, sample_idx: 0,
            pos: 0.0, step: 1.0, loop_start: 0.0, loop_end: 0.0,
            loop_mode: Sf2LoopMode::None, reverse: false,
            attack_s: 0.001, hold_s: 0.0, decay_s: 0.1, sustain: 1.0, release_s: 0.1,
            level: 0.0, hold_left: 0.0,
            cutoff: 20_000.0, filter_kind: 0, lp: 0.0, bp: 0.0, gain: 1.0,
        }
    }
}

/// Polyphonic SF2 sampler driven by an editable [`Sf2Instrument`].
pub struct Sf2Sampler {
    inst: Sf2Instrument,
    samples: Vec<Sf2SampleData>,
    /// Resolved sample index per zone (aligned with `inst.zones`).
    zone_sample: Vec<Option<usize>>,
    voices: [Voice; MAX_VOICES],
    chan_vol: [f32; 16],
    gain: f32,
    fading: bool,
    fade: f32,
}

impl Sf2Sampler {
    pub fn new(loaded: LoadedSf2) -> Self {
        let mut s = Self {
            inst: Sf2Instrument::default(),
            samples: Vec::new(),
            zone_sample: Vec::new(),
            voices: [Voice::silent(); MAX_VOICES],
            chan_vol: [100.0 / 127.0; 16],
            gain: 0.7,
            fading: false,
            fade: 1.0,
        };
        s.set_instrument(loaded);
        s
    }

    /// Swap in a freshly loaded instrument + sample pool (non-RT). Silences
    /// sounding voices since their sample indices may no longer be valid.
    pub fn set_instrument(&mut self, loaded: LoadedSf2) {
        self.inst = loaded.instrument;
        self.samples = loaded.samples;
        self.rebuild_zone_map();
        for v in self.voices.iter_mut() { v.env = Env::Idle; v.level = 0.0; }
    }

    /// Replace just the editable instrument params (e.g. after an EDITOR edit),
    /// keeping the sample pool. Sounding voices keep playing; new notes pick up
    /// the edited zones.
    pub fn update_instrument(&mut self, inst: Sf2Instrument) {
        self.inst = inst;
        self.rebuild_zone_map();
    }

    pub fn instrument(&self) -> &Sf2Instrument { &self.inst }

    fn rebuild_zone_map(&mut self) {
        self.zone_sample = self.inst.zones.iter()
            .map(|z| self.samples.iter().position(|s| s.name == z.sample_name))
            .collect();
    }

    fn alloc_voice(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| v.env == Env::Idle) {
            return i;
        }
        let mut quietest = 0usize;
        let mut min_level = f32::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if v.level < min_level { min_level = v.level; quietest = i; }
        }
        quietest
    }

    fn start_voice(&mut self, zone_idx: usize, channel: u8, note: u8, velocity: u8) {
        let Some(Some(sample_idx)) = self.zone_sample.get(zone_idx).copied() else { return };
        // Snapshot everything we need from the (immutably-borrowed) zone + sample
        // into a small descriptor, so we can then take a mutable voice borrow
        // without overlapping borrows of `self`.
        let zone = &self.inst.zones[zone_idx];
        let sample = &self.samples[sample_idx];
        let native_sr = sample.sample_rate as f64;

        let semis = (note as f32 - zone.root_key as f32)
            + zone.coarse_tune as f32
            + zone.fine_tune as f32 / 100.0
            + sample.pitch_correction as f32 / 100.0;
        let pitch_ratio = 2f32.powf(semis / 12.0) as f64;

        let loop_start = zone.loop_start as f64;
        let loop_end = if zone.loop_end > zone.loop_start { zone.loop_end as f64 } else { 0.0 };
        let attack_s = zone.attack.max(0.0005);
        let hold_s = zone.hold.max(0.0);
        let decay_s = zone.decay.max(0.0005);
        let sustain = zone.sustain.clamp(0.0, 1.0);
        let release_s = zone.release.max(0.0005);
        let loop_mode = zone.loop_mode;
        let cutoff = zone.cutoff.clamp(20.0, 20_000.0);
        let filter_kind = match zone.filter_type {
            seqterm_core::Sf2FilterType::LowPass => 0u8,
            seqterm_core::Sf2FilterType::HighPass => 1,
            seqterm_core::Sf2FilterType::BandPass => 2,
        };
        let vel = (velocity as f32 / 127.0).clamp(0.0, 1.0);
        let gain = 10f32.powf(zone.gain_db / 20.0) * vel;

        let i = self.alloc_voice();
        let v = &mut self.voices[i];
        *v = Voice {
            env: Env::Attack,
            channel: channel & 0x0F,
            note,
            sample_idx,
            pos: 0.0,
            // step is pitch × native rate; render divides by the engine rate.
            step: pitch_ratio * native_sr,
            loop_start,
            loop_end,
            loop_mode,
            reverse: false,
            attack_s, hold_s, decay_s, sustain, release_s,
            level: 0.0,
            hold_left: hold_s,
            cutoff,
            filter_kind,
            lp: 0.0,
            bp: 0.0,
            gain,
        };
    }
}

impl AudioSource for Sf2Sampler {
    fn render(&mut self, output: &mut [f32], sample_rate: u32) -> usize {
        let frames = output.len() / 2;
        if frames == 0 { return 0; }
        for s in output.iter_mut() { *s = 0.0; }
        let sr = sample_rate.max(1) as f32;
        let sr64 = sr as f64;

        let Self { voices, samples, chan_vol, gain, .. } = self;

        for v in voices.iter_mut() {
            if v.env == Env::Idle { continue; }
            let Some(sample) = samples.get(v.sample_idx) else { v.env = Env::Idle; continue };
            let pcm = &sample.pcm;
            if pcm.is_empty() { v.env = Env::Idle; continue; }

            // Per-voice ADSR increments for this block.
            let atk = 1.0 / (v.attack_s * sr).max(1.0);
            let dec = (1.0 - v.sustain) / (v.decay_s * sr).max(1.0);
            let rel_rate = 1.0 / (v.release_s * sr).max(1.0);
            // One-pole filter coefficient from cutoff.
            let fc = (1.0 - (-2.0 * std::f32::consts::PI * v.cutoff / sr).exp()).clamp(0.0, 1.0);
            let step = v.step / sr64; // complete the resample ratio
            let cv = chan_vol[(v.channel & 0x0F) as usize];

            for frame in 0..frames {
                // ── ADSR ──
                match v.env {
                    Env::Attack => { v.level += atk; if v.level >= 1.0 { v.level = 1.0; v.env = if v.hold_s > 0.0 { Env::Hold } else { Env::Decay }; } }
                    Env::Hold => { v.hold_left -= 1.0 / sr; if v.hold_left <= 0.0 { v.env = Env::Decay; } }
                    Env::Decay => { v.level -= dec; if v.level <= v.sustain { v.level = v.sustain; v.env = Env::Sustain; } }
                    Env::Sustain => {}
                    Env::Release => { v.level -= rel_rate; if v.level <= 0.0 { v.level = 0.0; v.env = Env::Idle; break; } }
                    Env::Idle => break,
                }

                // ── Sample read (linear interpolation) ──
                let p = v.pos;
                let i0 = p as usize;
                if i0 + 1 >= pcm.len() && v.loop_end <= 0.0 { v.env = Env::Idle; break; }
                let i1 = (i0 + 1).min(pcm.len() - 1);
                let frac = (p - i0 as f64) as f32;
                let s = pcm[i0] + (pcm[i1] - pcm[i0]) * frac;

                // ── Filter ──
                v.lp += fc * (s - v.lp);
                let filtered = match v.filter_kind {
                    1 => s - v.lp,          // HPF
                    2 => { v.bp += fc * (v.lp - v.bp); v.lp - v.bp } // BPF (crude)
                    _ => v.lp,              // LPF
                };

                let out = filtered * v.level * v.gain * cv;
                output[frame * 2]     += out;
                output[frame * 2 + 1] += out;

                // ── Advance position with looping ──
                if v.loop_mode == Sf2LoopMode::PingPong && v.reverse {
                    v.pos -= step;
                    if v.pos <= v.loop_start { v.pos = v.loop_start; v.reverse = false; }
                } else {
                    v.pos += step;
                    if v.loop_end > 0.0 && v.pos >= v.loop_end {
                        match v.loop_mode {
                            Sf2LoopMode::Forward => { v.pos = v.loop_start + (v.pos - v.loop_end); }
                            Sf2LoopMode::PingPong => { v.pos = v.loop_end; v.reverse = true; }
                            Sf2LoopMode::None => {}
                        }
                    }
                }
            }
        }

        // Master gain + optional fade-out.
        for frame in 0..frames {
            let mut g = *gain;
            if self.fading {
                g *= self.fade;
                self.fade = (self.fade - 1.0 / frames as f32).max(0.0);
            }
            output[frame * 2]     *= g;
            output[frame * 2 + 1] *= g;
        }
        if self.fading && self.fade <= 0.0 {
            for v in self.voices.iter_mut() { v.env = Env::Idle; v.level = 0.0; }
            self.fading = false;
            self.fade = 1.0;
        }
        frames
    }

    fn is_active(&self) -> bool { true } // persistent instrument slot

    fn stop(&mut self) { self.fading = true; self.fade = 1.0; }

    fn as_any_mut(&mut self) -> &mut dyn Any { self }

    fn as_synth(&mut self) -> Option<&mut dyn AudioSynthPort> { Some(self) }
}

impl AudioSynthPort for Sf2Sampler {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        if velocity == 0 { self.note_off(channel, note); return; }
        // Trigger every zone matching (note, velocity) — supports velocity layers
        // and key splits playing together. Collect matches into a fixed stack
        // buffer (no heap alloc) to keep note_on realtime-safe.
        let mut hits = [0usize; 8];
        let mut n = 0;
        for (i, z) in self.inst.zones.iter().enumerate() {
            if z.matches(note, velocity) {
                hits[n] = i;
                n += 1;
                if n == hits.len() { break; }
            }
        }
        for &zi in &hits[..n] { self.start_voice(zi, channel, note, velocity); }
    }

    fn note_off(&mut self, channel: u8, note: u8) {
        let ch = channel & 0x0F;
        for v in self.voices.iter_mut() {
            if v.channel == ch && v.note == note
                && v.env != Env::Idle && v.env != Env::Release
            {
                v.env = Env::Release;
            }
        }
    }

    fn control_change(&mut self, channel: u8, cc: u8, value: u8) {
        let ch = (channel & 0x0F) as usize;
        match cc {
            7 => self.chan_vol[ch] = (value as f32 / 127.0).clamp(0.0, 1.0),
            120 | 123 => for v in self.voices.iter_mut() {
                if (v.channel & 0x0F) as usize == ch { v.env = Env::Release; }
            },
            _ => {}
        }
    }

    fn pitch_bend(&mut self, _channel: u8, _value: i16) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use seqterm_core::Sf2Zone;

    fn loaded_sine() -> LoadedSf2 {
        // One zone over the whole keyboard, root C4, mapped to a short sine.
        let sr = 48_000u32;
        let freq = 261.63f32; // C4
        let pcm: Vec<f32> = (0..sr as usize / 4)
            .map(|i| (i as f32 / sr as f32 * freq * std::f32::consts::TAU).sin() * 0.5)
            .collect();
        let sample = Sf2SampleData {
            name: "sine".into(),
            pcm: Arc::from(pcm),
            sample_rate: sr,
            loop_start: 0,
            loop_end: 0,
            root_key: 60,
            pitch_correction: 0,
        };
        let mut z = Sf2Zone::new("sine");
        z.attack = 0.001; z.decay = 0.05; z.sustain = 0.8; z.release = 0.05;
        let inst = Sf2Instrument { name: "T".into(), zones: vec![z], selected: 0 };
        LoadedSf2 { instrument: inst, samples: vec![sample] }
    }

    #[test]
    fn note_on_produces_audio() {
        let mut s = Sf2Sampler::new(loaded_sine());
        s.note_on(0, 60, 110);
        let mut buf = vec![0.0f32; 512 * 2];
        s.render(&mut buf, 48_000);
        let peak = buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak > 0.001, "sf2 sampler should produce sound, peak={peak}");
    }

    #[test]
    fn note_off_releases_to_silence() {
        let mut s = Sf2Sampler::new(loaded_sine());
        s.note_on(0, 60, 110);
        let mut buf = vec![0.0f32; 256 * 2];
        s.render(&mut buf, 48_000);
        s.note_off(0, 60);
        let mut last_peak = 1.0f32;
        for _ in 0..400 {
            buf.iter_mut().for_each(|x| *x = 0.0);
            s.render(&mut buf, 48_000);
            last_peak = buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        }
        assert!(last_peak < 1e-3, "voice should release toward silence, peak={last_peak}");
    }

    #[test]
    fn unmatched_velocity_is_silent() {
        let mut loaded = loaded_sine();
        loaded.instrument.zones[0].vel_low = 100; // note at vel 50 won't match
        let mut s = Sf2Sampler::new(loaded);
        s.note_on(0, 60, 50);
        let mut buf = vec![0.0f32; 256 * 2];
        s.render(&mut buf, 48_000);
        let peak = buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak < 1e-6, "out-of-range velocity must not sound, peak={peak}");
    }
}
