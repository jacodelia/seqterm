//! Silent stub engine — compiled when no FluidSynth engine feature is enabled.
//!
//! Constructs successfully but produces silence, so the rest of the audio engine
//! can request a "FluidSynth" backend and transparently fall back to oxisynth
//! without any conditional compilation of its own.

use std::path::PathBuf;
use seqterm_ports::realtime::PresetInfo;

/// This engine produces no sound.
pub const REAL: bool = false;

pub struct Engine {
    #[allow(dead_code)]
    sf2_path: PathBuf,
}

impl Engine {
    pub fn new(
        sf2_path: PathBuf,
        channels: &[(u8, u8, u8)],
        sample_rate: u32,
    ) -> anyhow::Result<Self> {
        let _ = (channels, sample_rate);
        tracing::warn!(
            "No FluidSynth engine compiled in — silent stub for {}",
            sf2_path.display()
        );
        Ok(Self { sf2_path })
    }

    pub fn render_into(&mut self, l: &mut [f32], r: &mut [f32]) {
        for s in l.iter_mut() { *s = 0.0; }
        for s in r.iter_mut() { *s = 0.0; }
    }

    pub fn note_on(&mut self, _channel: u8, _note: u8, _velocity: u8) {}
    pub fn note_off(&mut self, _channel: u8, _note: u8) {}
    pub fn control_change(&mut self, _channel: u8, _cc: u8, _value: u8) {}
    pub fn pitch_bend(&mut self, _channel: u8, _value: i16) {}
    pub fn all_notes_off(&mut self) {}
    pub fn select_preset(&mut self, _bank: u16, _program: u8) {}
    pub fn list_presets(&self) -> Vec<PresetInfo> { Vec::new() }
}
