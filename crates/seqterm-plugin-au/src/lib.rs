//! Audio Unit (AU) plugin hosting for SeqTerm — macOS only.
//!
//! Audio Units are Apple's native plugin format, requiring CoreAudio.
//! Full hosting is restricted to macOS builds with the `au` feature flag.
//!
//! ## Status
//!
//! Stub: scan (reads AudioComponents plist database), describe, InstrumentBackend stub.
//! Full audio processing requires macOS + `au` feature + CoreAudio framework linkage.

use std::path::PathBuf;
use seqterm_ports::realtime::{AudioSource, AudioSynthPort, InstrumentBackend, PresetInfo};

// ─── Plugin descriptor ────────────────────────────────────────────────────────

/// Metadata about a discovered Audio Unit plugin.
#[derive(Debug, Clone)]
pub struct AuPluginInfo {
    /// Component type (e.g. "aumu" = instrument, "aufx" = effect).
    pub component_type: String,
    /// Manufacturer code (4-char OSType).
    pub manufacturer: String,
    /// Component name (e.g. "Alchemy").
    pub name: String,
    /// Version integer from the component description.
    pub version: u32,
}

// ─── Scanner ──────────────────────────────────────────────────────────────────

/// Scan the system AudioComponents database for AU plugins.
///
/// On non-macOS platforms returns an empty list.
pub fn scan_system() -> Vec<AuPluginInfo> {
    #[cfg(all(target_os = "macos", feature = "au"))]
    {
        // Full implementation would use AudioComponentFindNext() from CoreAudio.
        // Stub: return empty until the au feature is properly wired.
        vec![]
    }
    #[cfg(not(all(target_os = "macos", feature = "au")))]
    {
        vec![]
    }
}

/// Default AU plugin bundle search paths.
pub fn default_search_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut paths = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join("Library/Audio/Plug-Ins/Components"));
        }
        paths.push(PathBuf::from("/Library/Audio/Plug-Ins/Components"));
        paths.push(PathBuf::from("/System/Library/Components"));
        paths
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![]
    }
}

// ─── Stub instrument ──────────────────────────────────────────────────────────

/// Placeholder AU instrument.  Produces silence on all platforms.
pub struct AuInstrument {
    name:   String,
    active: bool,
}

impl AuInstrument {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), active: true }
    }
}

impl AudioSource for AuInstrument {
    fn render(&mut self, _output: &mut [f32], _sr: u32) -> usize { 0 }
    fn is_active(&self) -> bool { self.active }
    fn stop(&mut self) { self.active = false; }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

impl AudioSynthPort for AuInstrument {
    fn note_on(&mut self, _ch: u8, _note: u8, _vel: u8) {}
    fn note_off(&mut self, _ch: u8, _note: u8) {}
    fn control_change(&mut self, _ch: u8, _cc: u8, _val: u8) {}
    fn pitch_bend(&mut self, _ch: u8, _val: i16) {}
}

impl InstrumentBackend for AuInstrument {
    fn backend_name(&self) -> &str { "Audio Unit (stub)" }
    fn select_preset(&mut self, _bank: u16, _program: u8) -> anyhow::Result<()> { Ok(()) }
    fn list_presets(&self) -> Vec<PresetInfo> {
        vec![PresetInfo { bank: 0, program: 0, name: self.name.clone() }]
    }
    fn all_notes_off(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_returns_empty_on_non_macos() {
        #[cfg(not(target_os = "macos"))]
        assert!(scan_system().is_empty());
    }

    #[test]
    fn search_paths_empty_on_non_macos() {
        #[cfg(not(target_os = "macos"))]
        assert!(default_search_paths().is_empty());
    }
}
