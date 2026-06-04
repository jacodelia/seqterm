//! VST3 plugin hosting for SeqTerm.
//!
//! This crate provides the `Vst3Host` type for scanning, loading, and processing
//! VST3 instrument and effect plugins.  The full VST3 binary interface is
//! conditionally compiled behind the `vst3` feature flag.
//!
//! Without that feature, the crate compiles as a no-op host that can scan
//! directories and record discovered plugin paths, but cannot instantiate them.
//! This lets the rest of the workspace depend on this crate without requiring
//! the VST3 SDK at every build.
//!
//! ## Architecture
//!
//! ```text
//! AppCommand::LoadPlugin { plugin_id }
//!        │
//!        ▼
//! PluginRegistry::load(id) ── seqterm_plugin_vst3::Vst3Host::instantiate(path)
//!        │
//!        ▼  (Box<dyn InstrumentBackend>)
//! AudioEngine::install_source(slot_id, source)
//! ```
//!
//! ## Status
//!
//! Stub implementation: scan / record / describe are implemented.
//! Process / parameter I/O requires the `vst3` feature + VST3 SDK.

use std::path::{Path, PathBuf};
use seqterm_ports::realtime::{
    AudioSource, AudioSynthPort, InstrumentBackend, PresetInfo,
};

// ─── Plugin descriptor ────────────────────────────────────────────────────────

/// Metadata about a discovered VST3 plugin bundle.
#[derive(Debug, Clone)]
pub struct Vst3PluginInfo {
    /// Absolute path to the `.vst3` bundle.
    pub path: PathBuf,
    /// Plugin name (from factory info, or derived from filename).
    pub name: String,
    /// Vendor string from factory info.
    pub vendor: String,
    /// Unique identifier (VST3 class ID as hex string).
    pub uid: String,
}

// ─── Scanner ──────────────────────────────────────────────────────────────────

/// Scan a directory tree for `.vst3` bundles and return their descriptors.
///
/// This does not load or validate the plugins — it only reads the filesystem.
pub fn scan_directory(dir: &Path) -> Vec<Vst3PluginInfo> {
    let mut found = Vec::new();
    scan_recursive(dir, &mut found);
    found
}

fn scan_recursive(dir: &Path, out: &mut Vec<Vst3PluginInfo>) {
    let rd = match std::fs::read_dir(dir) { Ok(r) => r, Err(_) => return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.extension().map(|e| e == "vst3").unwrap_or(false) {
                let name = path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Unknown".into());
                out.push(Vst3PluginInfo {
                    path: path.clone(),
                    name,
                    vendor: String::new(),
                    uid:    String::new(),
                });
            } else {
                scan_recursive(&path, out);
            }
        }
    }
}

/// Default VST3 search paths for the current platform.
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".vst3"));
        }
        paths.push(PathBuf::from("/usr/lib/vst3"));
        paths.push(PathBuf::from("/usr/local/lib/vst3"));
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join("Library/Audio/Plug-Ins/VST3"));
        }
        paths.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(p) = std::env::var("PROGRAMFILES") {
            paths.push(PathBuf::from(p).join("Common Files\\VST3"));
        }
    }
    paths
}

// ─── Stub instrument ──────────────────────────────────────────────────────────

/// Placeholder VST3 instrument returned when `vst3` feature is not enabled.
/// Produces silence but satisfies the `InstrumentBackend` trait.
pub struct Vst3Instrument {
    path: PathBuf,
    active: bool,
}

impl Vst3Instrument {
    pub fn new(path: PathBuf) -> Self { Self { path, active: true } }
}

impl AudioSource for Vst3Instrument {
    fn render(&mut self, _output: &mut [f32], _sr: u32) -> usize { 0 }
    fn is_active(&self) -> bool { self.active }
    fn stop(&mut self) { self.active = false; }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

impl AudioSynthPort for Vst3Instrument {
    fn note_on(&mut self, _ch: u8, _note: u8, _vel: u8) {}
    fn note_off(&mut self, _ch: u8, _note: u8) {}
    fn control_change(&mut self, _ch: u8, _cc: u8, _val: u8) {}
    fn pitch_bend(&mut self, _ch: u8, _val: i16) {}
}

impl InstrumentBackend for Vst3Instrument {
    fn backend_name(&self) -> &str { "VST3 (stub)" }
    fn select_preset(&mut self, _bank: u16, _program: u8) -> anyhow::Result<()> { Ok(()) }
    fn list_presets(&self) -> Vec<PresetInfo> {
        vec![PresetInfo { bank: 0, program: 0,
            name: self.path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "VST3".into()),
        }]
    }
    fn all_notes_off(&mut self) {}
}

// ─── PluginHostPort adapter ─────────────────────────────────────────────────
//
// Unifies VST3 hosting under the same `PluginHostPort` trait as VST2/CLAP so the
// application's `PluginRegistry` can drive every format identically. Scanning is
// fully functional; instantiation returns a `Vst3Instrument`. Real-time audio
// processing through a loaded `.vst3` bundle requires the `vst3` feature + the
// Steinberg VST3 SDK (COM ABI), and is otherwise a documented silent passthrough.

use std::collections::HashMap;
use seqterm_ports::plugin::{PluginDescriptor, PluginHostPort, PluginKind};

/// `PluginHostPort` adapter for VST3 bundles.
#[derive(Default)]
pub struct Vst3Host {
    plugins:   Vec<PluginDescriptor>,
    instances: HashMap<u64, Vst3Instrument>,
    next_id:   u64,
}

impl Vst3Host {
    /// Create an empty host. Call [`PluginHostPort::scan`] to populate it,
    /// or [`Vst3Host::scan_default_paths`] to sweep the platform locations.
    pub fn new() -> Self { Self::default() }

    /// Scan every platform-default VST3 location and return the merged list.
    pub fn scan_default_paths(&mut self) -> Vec<PluginDescriptor> {
        let mut all = Vec::new();
        for dir in default_search_paths() {
            if let Ok(found) = self.scan(&dir) { all.extend(found); }
        }
        all
    }
}

fn vst3_descriptor(info: &Vst3PluginInfo) -> PluginDescriptor {
    PluginDescriptor {
        id:            info.path.to_string_lossy().into_owned(),
        name:          info.name.clone(),
        vendor:        info.vendor.clone(),
        version:       String::new(),
        kind:          PluginKind::Vst3,
        path:          info.path.clone(),
        is_instrument: true,
        is_effect:     true,
    }
}

impl PluginHostPort for Vst3Host {
    fn scan(&mut self, dir: &Path) -> anyhow::Result<Vec<PluginDescriptor>> {
        let found: Vec<PluginDescriptor> =
            scan_directory(dir).iter().map(vst3_descriptor).collect();
        for d in &found {
            if !self.plugins.iter().any(|p| p.id == d.id) {
                self.plugins.push(d.clone());
            }
        }
        Ok(found)
    }

    fn list_plugins(&self) -> &[PluginDescriptor] { &self.plugins }

    fn instantiate(&mut self, plugin_id: &str, _sr: u32, _block: u32) -> anyhow::Result<u64> {
        let desc = self.plugins.iter().find(|p| p.id == plugin_id)
            .ok_or_else(|| anyhow::anyhow!("VST3 plugin not found: {plugin_id}"))?;
        self.next_id += 1;
        let id = self.next_id;
        self.instances.insert(id, Vst3Instrument::new(desc.path.clone()));
        Ok(id)
    }

    fn destroy(&mut self, instance_id: u64) {
        self.instances.remove(&instance_id);
    }

    fn process(&mut self, instance_id: u64, _input: &[f32], output: &mut [f32]) -> anyhow::Result<()> {
        // Instruments are generators: render into the (pre-cleared) output buffer.
        if let Some(inst) = self.instances.get_mut(&instance_id) {
            inst.render(output, 48_000);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_nonexistent_dir_returns_empty() {
        let v = scan_directory(Path::new("/nonexistent/vst3/path"));
        assert!(v.is_empty());
    }

    #[test]
    fn host_implements_port_and_tracks_instances() {
        let mut host = Vst3Host::new();
        assert!(host.list_plugins().is_empty());
        host.plugins.push(vst3_descriptor(&Vst3PluginInfo {
            path: PathBuf::from("/tmp/Test.vst3"),
            name: "Test".into(), vendor: String::new(), uid: String::new(),
        }));
        let id = host.instantiate("/tmp/Test.vst3", 48_000, 512).unwrap();
        let mut buf = [0.0f32; 8];
        host.process(id, &[], &mut buf).unwrap();
        host.destroy(id);
        assert!(host.instances.is_empty());
    }

    #[test]
    fn default_paths_is_not_empty() {
        assert!(!default_search_paths().is_empty());
    }
}
