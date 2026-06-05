//! Filesystem discovery for plugin/instrument formats that SeqTerm does not yet
//! host natively: LADSPA, DSSI, LV2, SFZ, SF2/SF3 and JSFX.
//!
//! These formats share one need — letting the user **select** something installed
//! on the system — without (yet) a full real-time host. This crate provides a
//! single generic [`FileScanHost`] that implements [`PluginHostPort`] by walking
//! the filesystem and recognising each format by its on-disk convention (a file
//! extension, or a bundle directory). Instantiation returns a silent
//! [`ScanInstrument`] stub, exactly like the VST3/CLAP scaffolds — real audio
//! processing per format is future work.
//!
//! One `FileScanHost` is registered per format in the application's
//! `PluginRegistry`, so discovered items appear in the FX picker tagged with
//! their format (`[LV2]`, `[SFZ]`, …).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use seqterm_ports::plugin::{PluginDescriptor, PluginHostPort, PluginKind};
use seqterm_ports::realtime::{AudioSource, AudioSynthPort, InstrumentBackend, PresetInfo};

// ─── Recognition rule ───────────────────────────────────────────────────────

/// How a format is recognised on disk.
#[derive(Debug, Clone)]
enum ScanRule {
    /// A bundle is a *directory* whose name ends in this extension (e.g. LV2 → "lv2").
    BundleDir(&'static str),
    /// A plugin is a *file* whose extension matches one of these (case-insensitive).
    Files(&'static [&'static str]),
}

/// The platform-native dynamic-library extension used by LADSPA/DSSI.
const fn dynlib_ext() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &["dll"]
    }
    #[cfg(target_os = "macos")]
    {
        &["dylib", "so"]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &["so"]
    }
}

fn rule_for(kind: &PluginKind) -> ScanRule {
    match kind {
        PluginKind::Lv2 => ScanRule::BundleDir("lv2"),
        PluginKind::Sfz => ScanRule::Files(&["sfz"]),
        PluginKind::Sf2 => ScanRule::Files(&["sf2", "sf3"]),
        PluginKind::Jsfx => ScanRule::Files(&["jsfx"]),
        PluginKind::Ladspa | PluginKind::Dssi => ScanRule::Files(dynlib_ext()),
        // Other kinds are owned by their dedicated host crates; nothing to scan here.
        _ => ScanRule::Files(&[]),
    }
}

fn ext_matches(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| exts.iter().any(|w| e.eq_ignore_ascii_case(w)))
        .unwrap_or(false)
}

/// Maximum directory depth searched below a search root. Plugins live near the
/// top of their search dirs; bounding the walk stops a user-configured path that
/// happens to sit above a huge tree (e.g. a source repo) from freezing the scan.
const MAX_SCAN_DEPTH: usize = 6;

/// Directory names never worth descending into — version control, build output,
/// and dependency caches that can contain tens of thousands of files (and stray
/// `.so` artifacts) but never installed plugins.
fn is_pruned_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".svn" | ".hg"
            | "target" | "build" | "node_modules"
            | ".cargo" | ".rustup" | ".cache" | "__pycache__"
    )
}

/// Walk `dir` recursively collecting paths that match `rule`.
fn scan_directory(dir: &Path, rule: &ScanRule) -> Vec<PathBuf> {
    let mut out = Vec::new();
    scan_recursive(dir, rule, 0, &mut out);
    out
}

fn scan_recursive(dir: &Path, rule: &ScanRule, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        // Use the entry's own type so symlinked dirs are not followed (avoids
        // cycles / crawling into linked trees).
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        let recurse_into = |path: &Path, out: &mut Vec<PathBuf>| {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !is_pruned_dir(name) {
                scan_recursive(path, rule, depth + 1, out);
            }
        };
        match rule {
            ScanRule::BundleDir(ext) => {
                if is_dir {
                    if ext_matches(&path, &[ext]) {
                        out.push(path);
                    } else {
                        recurse_into(&path, out);
                    }
                }
            }
            ScanRule::Files(exts) => {
                if is_file {
                    if ext_matches(&path, exts) {
                        out.push(path);
                    }
                } else if is_dir {
                    recurse_into(&path, out);
                }
            }
        }
    }
}

// ─── Default search paths (Carla-style) ─────────────────────────────────────

/// Platform-default search directories for a scannable `kind`, mirroring the
/// conventions used by Carla. Returns an empty list for kinds this crate does
/// not own.
pub fn default_search_paths(kind: &PluginKind) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut p = Vec::new();
    macro_rules! home_join {
        ($sub:expr) => {
            if let Some(h) = &home {
                p.push(h.join($sub));
            }
        };
    }

    match kind {
        PluginKind::Lv2 => {
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                home_join!(".lv2");
                p.push(PathBuf::from("/usr/lib/lv2"));
                p.push(PathBuf::from("/usr/local/lib/lv2"));
            }
            #[cfg(target_os = "macos")]
            {
                home_join!("Library/Audio/Plug-Ins/LV2");
                p.push(PathBuf::from("/Library/Audio/Plug-Ins/LV2"));
            }
            #[cfg(target_os = "windows")]
            {
                if let Ok(a) = std::env::var("APPDATA") {
                    p.push(PathBuf::from(a).join("LV2"));
                }
                if let Ok(c) = std::env::var("COMMONPROGRAMFILES") {
                    p.push(PathBuf::from(c).join("LV2"));
                }
            }
        }
        PluginKind::Ladspa => {
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                home_join!(".ladspa");
                p.push(PathBuf::from("/usr/lib/ladspa"));
                p.push(PathBuf::from("/usr/local/lib/ladspa"));
            }
            #[cfg(target_os = "macos")]
            {
                home_join!("Library/Audio/Plug-Ins/LADSPA");
                p.push(PathBuf::from("/Library/Audio/Plug-Ins/LADSPA"));
            }
            #[cfg(target_os = "windows")]
            {
                if let Ok(c) = std::env::var("COMMONPROGRAMFILES") {
                    p.push(PathBuf::from(c).join("LADSPA"));
                }
            }
        }
        PluginKind::Dssi => {
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                home_join!(".dssi");
                p.push(PathBuf::from("/usr/lib/dssi"));
                p.push(PathBuf::from("/usr/local/lib/dssi"));
            }
            #[cfg(target_os = "macos")]
            {
                home_join!("Library/Audio/Plug-Ins/DSSI");
                p.push(PathBuf::from("/Library/Audio/Plug-Ins/DSSI"));
            }
            #[cfg(target_os = "windows")]
            {
                if let Ok(c) = std::env::var("COMMONPROGRAMFILES") {
                    p.push(PathBuf::from(c).join("DSSI"));
                }
            }
        }
        PluginKind::Sf2 => {
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                home_join!(".sounds/sf2");
                p.push(PathBuf::from("/usr/share/sounds/sf2"));
                p.push(PathBuf::from("/usr/share/soundfonts"));
            }
            #[cfg(target_os = "macos")]
            {
                home_join!("Library/Audio/Sounds/Banks");
            }
            #[cfg(target_os = "windows")]
            {
                if let Ok(c) = std::env::var("COMMONPROGRAMFILES") {
                    p.push(PathBuf::from(c).join("SF2"));
                }
            }
        }
        PluginKind::Sfz => {
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                home_join!(".sfz");
                p.push(PathBuf::from("/usr/share/sounds/sfz"));
            }
            #[cfg(target_os = "macos")]
            {
                home_join!("Library/Audio/Sounds/SFZ");
            }
            #[cfg(target_os = "windows")]
            {
                if let Ok(c) = std::env::var("COMMONPROGRAMFILES") {
                    p.push(PathBuf::from(c).join("SFZ"));
                }
            }
        }
        PluginKind::Jsfx => {
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                home_join!(".config/REAPER/Effects");
            }
            #[cfg(target_os = "macos")]
            {
                home_join!("Library/Application Support/REAPER/Effects");
            }
            #[cfg(target_os = "windows")]
            {
                if let Ok(a) = std::env::var("APPDATA") {
                    p.push(PathBuf::from(a).join("REAPER\\Effects"));
                }
            }
        }
        _ => {}
    }
    p
}

// ─── Stub instrument (silent passthrough) ───────────────────────────────────

/// Placeholder instrument produced on instantiation. Produces silence but
/// satisfies the `InstrumentBackend` trait so the registry can manage it.
pub struct ScanInstrument {
    path: PathBuf,
    kind: PluginKind,
    active: bool,
}

impl ScanInstrument {
    pub fn new(path: PathBuf, kind: PluginKind) -> Self {
        Self {
            path,
            kind,
            active: true,
        }
    }
}

impl AudioSource for ScanInstrument {
    fn render(&mut self, _output: &mut [f32], _sr: u32) -> usize {
        0
    }
    fn is_active(&self) -> bool {
        self.active
    }
    fn stop(&mut self) {
        self.active = false;
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl AudioSynthPort for ScanInstrument {
    fn note_on(&mut self, _ch: u8, _note: u8, _vel: u8) {}
    fn note_off(&mut self, _ch: u8, _note: u8) {}
    fn control_change(&mut self, _ch: u8, _cc: u8, _val: u8) {}
    fn pitch_bend(&mut self, _ch: u8, _val: i16) {}
}

impl InstrumentBackend for ScanInstrument {
    fn backend_name(&self) -> &str {
        self.kind.label()
    }
    fn select_preset(&mut self, _bank: u16, _program: u8) -> anyhow::Result<()> {
        Ok(())
    }
    fn list_presets(&self) -> Vec<PresetInfo> {
        vec![PresetInfo {
            bank: 0,
            program: 0,
            name: self
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.kind.label().to_string()),
        }]
    }
    fn all_notes_off(&mut self) {}
}

// ─── Generic host adapter ────────────────────────────────────────────────────

/// `PluginHostPort` adapter that discovers a single `kind` by filesystem
/// convention. Scanning is functional; processing is a documented silent stub.
pub struct FileScanHost {
    kind: PluginKind,
    plugins: Vec<PluginDescriptor>,
    instances: HashMap<u64, ScanInstrument>,
    next_id: u64,
}

impl FileScanHost {
    /// Create an empty host for `kind`. Call [`PluginHostPort::scan`] to populate.
    pub fn new(kind: PluginKind) -> Self {
        Self {
            kind,
            plugins: Vec::new(),
            instances: HashMap::new(),
            next_id: 0,
        }
    }

    /// Scan every platform-default location for this host's format.
    pub fn scan_default_paths(&mut self) -> Vec<PluginDescriptor> {
        let mut all = Vec::new();
        for dir in default_search_paths(&self.kind.clone()) {
            if let Ok(found) = self.scan(&dir) {
                all.extend(found);
            }
        }
        all
    }

    fn descriptor(&self, path: &Path) -> PluginDescriptor {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".into());
        // Effects vs instruments: LADSPA/JSFX are effects; the rest are instruments.
        let (is_effect, is_instrument) = match self.kind {
            PluginKind::Ladspa | PluginKind::Jsfx => (true, false),
            PluginKind::Dssi => (false, true),
            PluginKind::Lv2 => (true, true),
            _ => (false, true), // SFZ, SF2
        };
        PluginDescriptor {
            id: path.to_string_lossy().into_owned(),
            name,
            vendor: String::new(),
            version: String::new(),
            kind: self.kind.clone(),
            path: path.to_path_buf(),
            is_instrument,
            is_effect,
        }
    }
}

impl PluginHostPort for FileScanHost {
    fn scan(&mut self, dir: &Path) -> anyhow::Result<Vec<PluginDescriptor>> {
        let rule = rule_for(&self.kind);
        let found: Vec<PluginDescriptor> = scan_directory(dir, &rule)
            .iter()
            .map(|p| self.descriptor(p))
            .collect();
        for d in &found {
            if !self.plugins.iter().any(|p| p.id == d.id) {
                self.plugins.push(d.clone());
            }
        }
        Ok(found)
    }

    fn list_plugins(&self) -> &[PluginDescriptor] {
        &self.plugins
    }

    fn instantiate(&mut self, plugin_id: &str, _sr: u32, _block: u32) -> anyhow::Result<u64> {
        let desc = self
            .plugins
            .iter()
            .find(|p| p.id == plugin_id)
            .ok_or_else(|| {
                anyhow::anyhow!("{} plugin not found: {plugin_id}", self.kind.label())
            })?;
        self.next_id += 1;
        let id = self.next_id;
        self.instances.insert(
            id,
            ScanInstrument::new(desc.path.clone(), self.kind.clone()),
        );
        Ok(id)
    }

    fn destroy(&mut self, instance_id: u64) {
        self.instances.remove(&instance_id);
    }

    fn process(
        &mut self,
        instance_id: u64,
        _input: &[f32],
        output: &mut [f32],
    ) -> anyhow::Result<()> {
        if let Some(inst) = self.instances.get_mut(&instance_id) {
            inst.render(output, 48_000);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_nonexistent_dir_returns_empty() {
        let mut host = FileScanHost::new(PluginKind::Sfz);
        assert!(host
            .scan(Path::new("/nonexistent/sfz/path"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn discovers_files_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("piano.sfz"), b"<region>").unwrap();
        fs::write(dir.path().join("ignore.txt"), b"nope").unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        fs::write(dir.path().join("nested/strings.sfz"), b"<region>").unwrap();

        let mut host = FileScanHost::new(PluginKind::Sfz);
        let found = host.scan(dir.path()).unwrap();
        assert_eq!(found.len(), 2, "should find both .sfz files recursively");
        assert!(found.iter().all(|d| d.kind == PluginKind::Sfz));
    }

    #[test]
    fn discovers_lv2_bundle_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Amp.lv2");
        fs::create_dir(&bundle).unwrap();
        fs::write(bundle.join("manifest.ttl"), b"").unwrap();

        let mut host = FileScanHost::new(PluginKind::Lv2);
        let found = host.scan(dir.path()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Amp");
    }

    #[test]
    fn instantiate_and_destroy_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("kit.sf2");
        fs::write(&f, b"RIFF").unwrap();
        let mut host = FileScanHost::new(PluginKind::Sf2);
        host.scan(dir.path()).unwrap();
        let id = host.instantiate(&f.to_string_lossy(), 48_000, 512).unwrap();
        let mut buf = [0.0f32; 8];
        host.process(id, &[], &mut buf).unwrap();
        host.destroy(id);
        assert!(host.instances.is_empty());
    }
}
