//! CLAP (CLever Audio Plugin) hosting for SeqTerm.
//!
//! CLAP is a modern, open plugin format with first-class support for:
//! - Thread-safe parameter automation
//! - Note ports (polyphonic expression)
//! - Tail handling and offline rendering
//!
//! ## Status
//!
//! Stub implementation: scan / describe are implemented.
//! Full audio processing requires the `clap` feature + `clack-host` crate.

use std::path::{Path, PathBuf};
use seqterm_ports::realtime::{
    AudioSource, AudioSynthPort, InstrumentBackend, PresetInfo,
};

// ─── Plugin descriptor ────────────────────────────────────────────────────────

/// Metadata about a discovered CLAP plugin.
#[derive(Debug, Clone)]
pub struct ClapPluginInfo {
    /// Absolute path to the `.clap` shared library.
    pub path: PathBuf,
    /// Plugin name from the descriptor.
    pub name: String,
    /// Vendor string from the descriptor.
    pub vendor: String,
    /// Unique plugin ID (reverse-DNS style, e.g. `com.example.my-plugin`).
    pub id: String,
    /// Plugin version string.
    pub version: String,
}

// ─── Scanner ──────────────────────────────────────────────────────────────────

/// Scan a directory tree for `.clap` plugins.
pub fn scan_directory(dir: &Path) -> Vec<ClapPluginInfo> {
    let mut found = Vec::new();
    scan_recursive(dir, &mut found);
    found
}

fn scan_recursive(dir: &Path, out: &mut Vec<ClapPluginInfo>) {
    let rd = match std::fs::read_dir(dir) { Ok(r) => r, Err(_) => return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().map(|e| e == "clap").unwrap_or(false) {
            let name = path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Unknown".into());
            out.push(ClapPluginInfo {
                path: path.clone(),
                name,
                vendor:  String::new(),
                id:      String::new(),
                version: String::new(),
            });
        } else if path.is_dir() {
            scan_recursive(&path, out);
        }
    }
}

/// Default CLAP search paths for the current platform.
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".clap"));
        }
        paths.push(PathBuf::from("/usr/lib/clap"));
        paths.push(PathBuf::from("/usr/local/lib/clap"));
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join("Library/Audio/Plug-Ins/CLAP"));
        }
        paths.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(p) = std::env::var("PROGRAMFILES") {
            paths.push(PathBuf::from(p).join("Common Files\\CLAP"));
        }
    }
    paths
}

// ─── Stub instrument ──────────────────────────────────────────────────────────

/// Placeholder CLAP instrument returned when the `clap` feature is not enabled.
pub struct ClapInstrument {
    path: PathBuf,
    active: bool,
}

impl ClapInstrument {
    pub fn new(path: PathBuf) -> Self { Self { path, active: true } }
}

impl AudioSource for ClapInstrument {
    fn render(&mut self, _output: &mut [f32], _sr: u32) -> usize { 0 }
    fn is_active(&self) -> bool { self.active }
    fn stop(&mut self) { self.active = false; }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

impl AudioSynthPort for ClapInstrument {
    fn note_on(&mut self, _ch: u8, _note: u8, _vel: u8) {}
    fn note_off(&mut self, _ch: u8, _note: u8) {}
    fn control_change(&mut self, _ch: u8, _cc: u8, _val: u8) {}
    fn pitch_bend(&mut self, _ch: u8, _val: i16) {}
}

impl InstrumentBackend for ClapInstrument {
    fn backend_name(&self) -> &str { "CLAP (stub)" }
    fn select_preset(&mut self, _bank: u16, _program: u8) -> anyhow::Result<()> { Ok(()) }
    fn list_presets(&self) -> Vec<PresetInfo> {
        vec![PresetInfo { bank: 0, program: 0,
            name: self.path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "CLAP".into()),
        }]
    }
    fn all_notes_off(&mut self) {}
}

// ─── PluginHostPort adapter ─────────────────────────────────────────────────
//
// Unifies CLAP hosting under the same `PluginHostPort` trait as VST2/VST3.
// Scanning is fully functional. Real-time processing through a loaded `.clap`
// requires the `clap` feature + `clack-host` (CLAP C ABI); otherwise the
// instrument is a documented silent passthrough.

use std::collections::HashMap;
use seqterm_ports::plugin::{PluginDescriptor, PluginHostPort, PluginKind};

/// `PluginHostPort` adapter for CLAP plugins.
#[derive(Default)]
pub struct ClapHost {
    plugins:   Vec<PluginDescriptor>,
    instances: HashMap<u64, ClapInstrument>,
    next_id:   u64,
}

impl ClapHost {
    /// Create an empty host.
    pub fn new() -> Self { Self::default() }

    /// Scan every platform-default CLAP location and return the merged list.
    pub fn scan_default_paths(&mut self) -> Vec<PluginDescriptor> {
        let mut all = Vec::new();
        for dir in default_search_paths() {
            if let Ok(found) = self.scan(&dir) { all.extend(found); }
        }
        all
    }
}

fn clap_descriptor(info: &ClapPluginInfo) -> PluginDescriptor {
    PluginDescriptor {
        id:            info.path.to_string_lossy().into_owned(),
        name:          info.name.clone(),
        vendor:        info.vendor.clone(),
        version:       info.version.clone(),
        kind:          PluginKind::Clap,
        path:          info.path.clone(),
        is_instrument: true,
        is_effect:     true,
    }
}

impl PluginHostPort for ClapHost {
    fn scan(&mut self, dir: &Path) -> anyhow::Result<Vec<PluginDescriptor>> {
        let found: Vec<PluginDescriptor> =
            scan_directory(dir).iter().map(clap_descriptor).collect();
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
            .ok_or_else(|| anyhow::anyhow!("CLAP plugin not found: {plugin_id}"))?;
        self.next_id += 1;
        let id = self.next_id;
        self.instances.insert(id, ClapInstrument::new(desc.path.clone()));
        Ok(id)
    }

    fn destroy(&mut self, instance_id: u64) { self.instances.remove(&instance_id); }

    fn process(&mut self, instance_id: u64, _input: &[f32], output: &mut [f32]) -> anyhow::Result<()> {
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
        assert!(scan_directory(Path::new("/nonexistent/clap")).is_empty());
    }

    #[test]
    fn host_implements_port_and_tracks_instances() {
        let mut host = ClapHost::new();
        host.plugins.push(clap_descriptor(&ClapPluginInfo {
            path: PathBuf::from("/tmp/Test.clap"), name: "Test".into(),
            vendor: String::new(), id: String::new(), version: String::new(),
        }));
        let id = host.instantiate("/tmp/Test.clap", 48_000, 512).unwrap();
        let mut buf = [0.0f32; 8];
        host.process(id, &[], &mut buf).unwrap();
        host.destroy(id);
        assert!(host.instances.is_empty());
    }

    #[test]
    fn default_paths_not_empty() {
        assert!(!default_search_paths().is_empty());
    }
}
