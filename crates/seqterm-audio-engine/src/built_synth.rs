//! A tiny built-in polyphonic synthesizer.
//!
//! Used as the default *internal* instrument for note patterns that have no
//! assigned plugin/SF2 — so every channel produces audio that passes through
//! the mixer (and therefore the audio export), instead of being silent.
//!
//! REALTIME CONTRACT: `render`/`note_on`/`note_off`/`control_change` are
//! allocation- and lock-free; all state lives in fixed-size arrays.

use std::any::Any;

use seqterm_ports::{AudioSource, AudioSynthPort};

const MAX_VOICES: usize = 24;

/// ADSR segment (seconds) and sustain level for the built-in synth.
const ATTACK_S:  f32 = 0.004;
const DECAY_S:   f32 = 0.12;
const SUSTAIN:   f32 = 0.62;
const RELEASE_S: f32 = 0.18;

#[derive(Clone, Copy, PartialEq)]
enum Env { Idle, Attack, Decay, Sustain, Release }

#[derive(Clone, Copy)]
struct Voice {
    env:     Env,
    channel: u8,
    note:    u8,
    /// Oscillator phases in 0..1.
    phase1:  f32,
    phase2:  f32,
    freq:    f32,
    vel:     f32,
    level:   f32, // current ADSR envelope level 0..1
    lp:      f32, // one-pole lowpass state
}

impl Voice {
    const fn silent() -> Self {
        Self {
            env: Env::Idle, channel: 0, note: 0,
            phase1: 0.0, phase2: 0.0, freq: 0.0, vel: 0.0, level: 0.0, lp: 0.0,
        }
    }
}

/// Built-in subtractive-style polyphonic synth (two detuned PolyBLEP saws +
/// a gentle one-pole lowpass + ADSR), playable on all 16 MIDI channels.
pub struct BuiltinSynth {
    voices:   [Voice; MAX_VOICES],
    chan_vol: [f32; 16], // CC7 per channel, 0..1
    gain:     f32,
    fading:   bool,
    fade:     f32,
}

impl Default for BuiltinSynth {
    fn default() -> Self { Self::new() }
}

impl BuiltinSynth {
    pub fn new() -> Self {
        Self {
            voices:   [Voice::silent(); MAX_VOICES],
            chan_vol: [100.0 / 127.0; 16],
            gain:     0.28, // headroom for polyphony
            fading:   false,
            fade:     1.0,
        }
    }

    fn note_freq(note: u8) -> f32 {
        440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
    }

    /// Find a free voice, or steal the quietest one.
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
}

/// PolyBLEP residual for band-limited sawtooth synthesis.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 { return 0.0; }
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[inline]
fn saw(phase: f32, dt: f32) -> f32 {
    // Naive saw minus the PolyBLEP correction = band-limited saw.
    (2.0 * phase - 1.0) - poly_blep(phase, dt)
}

impl AudioSource for BuiltinSynth {
    fn render(&mut self, output: &mut [f32], sample_rate: u32) -> usize {
        let frames = output.len() / 2;
        if frames == 0 { return 0; }
        let sr = sample_rate.max(1) as f32;

        // Per-sample ADSR increments.
        let atk = 1.0 / (ATTACK_S * sr).max(1.0);
        let dec = (1.0 - SUSTAIN) / (DECAY_S * sr).max(1.0);
        let rel = SUSTAIN / (RELEASE_S * sr).max(1.0);
        // Lowpass coefficient (~3.5 kHz one-pole).
        let lp_c = (1.0 - (-2.0 * std::f32::consts::PI * 3500.0 / sr).exp()).clamp(0.0, 1.0);

        for frame in 0..frames {
            let mut mix = 0.0f32;
            for v in self.voices.iter_mut() {
                if v.env == Env::Idle { continue; }

                // ── ADSR ──
                match v.env {
                    Env::Attack  => { v.level += atk; if v.level >= 1.0 { v.level = 1.0; v.env = Env::Decay; } }
                    Env::Decay   => { v.level -= dec; if v.level <= SUSTAIN { v.level = SUSTAIN; v.env = Env::Sustain; } }
                    Env::Sustain => {}
                    Env::Release => { v.level -= rel; if v.level <= 0.0 { v.level = 0.0; v.env = Env::Idle; continue; } }
                    Env::Idle    => continue,
                }

                // ── Oscillators (two detuned saws) ──
                let dt1 = (v.freq / sr).clamp(0.0, 0.5);
                let dt2 = (v.freq * 1.0045 / sr).clamp(0.0, 0.5);
                let s = 0.5 * (saw(v.phase1, dt1) + saw(v.phase2, dt2));
                v.phase1 += dt1; if v.phase1 >= 1.0 { v.phase1 -= 1.0; }
                v.phase2 += dt2; if v.phase2 >= 1.0 { v.phase2 -= 1.0; }

                // ── Gentle lowpass ──
                v.lp += lp_c * (s - v.lp);

                let cv = self.chan_vol[(v.channel & 0x0F) as usize];
                mix += v.lp * v.level * v.vel * cv;
            }

            let mut out = mix * self.gain;
            if self.fading {
                out *= self.fade;
                self.fade = (self.fade - 1.0 / frames as f32).max(0.0);
            }
            output[frame * 2]     = out;
            output[frame * 2 + 1] = out;
        }

        if self.fading && self.fade <= 0.0 {
            for v in self.voices.iter_mut() { v.env = Env::Idle; v.level = 0.0; }
            self.fading = false;
            self.fade = 1.0;
        }
        frames
    }

    fn is_active(&self) -> bool { true } // persistent instrument slot

    fn stop(&mut self) {
        self.fading = true;
        self.fade = 1.0;
    }

    fn as_any_mut(&mut self) -> &mut dyn Any { self }

    fn as_synth(&mut self) -> Option<&mut dyn AudioSynthPort> { Some(self) }
}

impl AudioSynthPort for BuiltinSynth {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        if velocity == 0 { self.note_off(channel, note); return; }
        let i = self.alloc_voice();
        let v = &mut self.voices[i];
        v.env     = Env::Attack;
        v.channel = channel & 0x0F;
        v.note    = note;
        v.freq    = Self::note_freq(note);
        v.vel     = (velocity as f32 / 127.0).clamp(0.0, 1.0);
        v.level   = 0.0;
        v.phase1  = 0.0;
        v.phase2  = 0.5;
        v.lp      = 0.0;
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
            120 | 123 => { // all sound / notes off
                for v in self.voices.iter_mut() {
                    if (v.channel & 0x0F) as usize == ch { v.env = Env::Release; }
                }
            }
            _ => {}
        }
    }

    fn pitch_bend(&mut self, _channel: u8, _value: i16) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_produces_audio() {
        let mut s = BuiltinSynth::new();
        s.note_on(0, 69, 100); // A4
        let mut buf = vec![0.0f32; 512 * 2];
        s.render(&mut buf, 48_000);
        let peak = buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak > 0.001, "built-in synth should produce sound, peak={peak}");
    }

    #[test]
    fn note_off_releases_to_silence() {
        let mut s = BuiltinSynth::new();
        s.note_on(0, 60, 110);
        let mut buf = vec![0.0f32; 256 * 2];
        s.render(&mut buf, 48_000);
        s.note_off(0, 60);
        // Render long enough for the release to finish.
        let mut last_peak = 1.0f32;
        for _ in 0..200 {
            buf.iter_mut().for_each(|x| *x = 0.0);
            s.render(&mut buf, 48_000);
            last_peak = buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        }
        assert!(last_peak < 1e-4, "voice should release to silence, peak={last_peak}");
    }
}
