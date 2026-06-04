//! FluidSynth-family SF2 engine for SeqTerm.
//!
//! [`FluidSynthBackend`] is an alternative SF2 sample engine to the default
//! oxisynth. It renders into SeqTerm's own buffers ([`FluidSynthBackend::render_into`])
//! and flows through the normal mixer / FX chain — never as a standalone audio
//! server. Three build-time variants share one identical API:
//!
//! | Feature | Engine | External deps |
//! |---------|--------|---------------|
//! | `fluidlite` *(recommended)* | **Embedded FluidLite** — bundled C compiled into the binary | **none** (just a C compiler at build time) |
//! | `fluidsynth` | System **libfluidsynth 2.x** (full FluidSynth, dynamically linked) | libfluidsynth + GLib |
//! | *(neither)* | Silent stub | none |
//!
//! When both engine features are on, the embedded `fluidlite` engine wins. The
//! stub lets the rest of the audio engine request a "FluidSynth" backend and
//! transparently fall back to oxisynth when no real engine is compiled in.
//!
//! ### Embedded build (zero dependencies, all platforms)
//!
//! ```sh
//! cargo build -p seqterm-app --features fluidsynth   # → fluidlite, nothing to install
//! ```

use std::path::PathBuf;
use seqterm_ports::realtime::PresetInfo;

// ── Engine selection (priority: embedded fluidlite → system ffi → stub) ─────────
#[cfg(feature = "fluidlite")]
#[path = "engine_lite.rs"]
mod engine;

#[cfg(all(feature = "fluidsynth", not(feature = "fluidlite")))]
mod ffi;
#[cfg(all(feature = "fluidsynth", not(feature = "fluidlite")))]
#[path = "engine_ffi.rs"]
mod engine;

#[cfg(not(any(feature = "fluidlite", feature = "fluidsynth")))]
#[path = "engine_stub.rs"]
mod engine;

/// A FluidSynth-family SF2 synthesizer. Thin wrapper over the selected engine.
pub struct FluidSynthBackend {
    inner: engine::Engine,
}

impl FluidSynthBackend {
    /// Create a backend, load `sf2_path`, and configure the given channels.
    /// `channels` is `[(midi_channel_0based, bank, preset)]`.
    pub fn new(sf2_path: PathBuf, channels: &[(u8, u8, u8)], sample_rate: u32) -> anyhow::Result<Self> {
        Ok(Self { inner: engine::Engine::new(sf2_path, channels, sample_rate)? })
    }

    /// Render `l.len()` frames into separate left/right buffers (realtime-safe).
    pub fn render_into(&mut self, l: &mut [f32], r: &mut [f32]) { self.inner.render_into(l, r); }

    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) { self.inner.note_on(channel, note, velocity); }
    pub fn note_off(&mut self, channel: u8, note: u8) { self.inner.note_off(channel, note); }
    pub fn control_change(&mut self, channel: u8, cc: u8, value: u8) { self.inner.control_change(channel, cc, value); }
    pub fn pitch_bend(&mut self, channel: u8, value: i16) { self.inner.pitch_bend(channel, value); }
    pub fn all_notes_off(&mut self) { self.inner.all_notes_off(); }

    /// Select bank/preset on channel 0 (single-channel preset preview).
    pub fn select_preset(&mut self, bank: u16, program: u8) { self.inner.select_preset(bank, program); }

    /// Whether this build actually produces sound (a real engine is compiled in).
    pub fn is_real(&self) -> bool { engine::REAL }

    /// List every preset exposed by the loaded soundfont (may be empty; the SF2
    /// browser uses a separate file-parse path).
    pub fn list_presets(&self) -> Vec<PresetInfo> { self.inner.list_presets() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_creates_and_renders_silence() {
        // Without any engine feature, construction succeeds and renders silence.
        #[cfg(not(any(feature = "fluidlite", feature = "fluidsynth")))]
        {
            let mut b = FluidSynthBackend::new(PathBuf::from("nonexistent.sf2"), &[(0, 0, 0)], 48000)
                .expect("stub construction");
            assert!(!b.is_real());
            let mut l = [1.0f32; 64];
            let mut r = [1.0f32; 64];
            b.render_into(&mut l, &mut r);
            assert!(l.iter().all(|&s| s == 0.0));
            assert!(r.iter().all(|&s| s == 0.0));
        }
    }
}
