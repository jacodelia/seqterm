use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use seqterm_core::Project;
use tracing::{debug, info, warn};

pub use seqterm_settings::{
    AppSettings, AudioSettings, KeyBinding, MidiLearnBinding, MidiLearnTarget,
    OscPortMode, OscSettings, PluginPaths, VizSettings, PLUGIN_PATH_FORMATS,
    default_keybindings, export_keybindings, import_keybindings,
    load_settings, resolve_midi_targets, save_settings,
};

// ─── Binary (MessagePack) project format ─────────────────────────────────────

/// Save a project in MessagePack binary format (`.seqterm`).
/// Uses the same atomic write strategy as `save_project`.
pub fn save_project_msgpack(project: &Project, path: &Path) -> Result<()> {
    let mut proj = project.clone();
    make_paths_relative(&mut proj, path);
    let bytes = rmp_serde::to_vec_named(&proj)
        .context("failed to serialize project to MessagePack")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create project directory")?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &bytes).context("failed to write temp file")?;
    fs::rename(&tmp, path).context("failed to rename temp → final")?;
    info!("Project (binary) saved to {}", path.display());
    Ok(())
}

/// Load a project from a MessagePack binary file, running schema migrations first.
pub fn load_project_msgpack(path: &Path) -> Result<Project> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read binary project file: {}", path.display()))?;
    // Deserialize to a serde_json::Value so we can reuse the same migration path.
    let mut value: serde_json::Value = rmp_serde::from_slice(&bytes)
        .with_context(|| format!("failed to parse binary project: {}", path.display()))?;
    let from_version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if from_version < Project::CURRENT_VERSION {
        value = migrate(value, from_version);
    }
    let mut project: Project = serde_json::from_value(value)
        .with_context(|| format!("failed to deserialize binary project: {}", path.display()))?;
    project.migrate_legacy_arrangement();
    make_paths_absolute(&mut project, path);
    info!("Project (binary) loaded (schema v{}) from {}", project.version, path.display());
    Ok(project)
}

/// Detect project format from file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectFormat {
    Json,
    MessagePack,
}

impl ProjectFormat {
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("seqterm") => Self::MessagePack,
            _ => Self::Json,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::MessagePack => "seqterm",
        }
    }
}

/// Save using the format inferred from the file extension.
pub fn save_project_auto(project: &Project, path: &Path) -> Result<()> {
    match ProjectFormat::from_path(path) {
        ProjectFormat::MessagePack => save_project_msgpack(project, path),
        ProjectFormat::Json => save_project(project, path),
    }
}

/// Load using the format inferred from the file extension.
pub fn load_project_auto(path: &Path) -> Result<Project> {
    match ProjectFormat::from_path(path) {
        ProjectFormat::MessagePack => load_project_msgpack(path),
        ProjectFormat::Json => load_project(path),
    }
}

// ─── Project save / load ──────────────────────────────────────────────────────

// ─── Relative-path helpers ────────────────────────────────────────────────────

/// Convert all PatternSource paths in a project to relative form so the
/// project file is portable across machines / directories.
///
/// `project_path` is the destination file (its parent is the base dir).
/// Paths that are already relative or that cannot be made relative are left unchanged.
fn make_paths_relative(project: &mut Project, project_path: &Path) {
    let base = match project_path.parent() {
        Some(p) => p,
        None => return,
    };
    for slots in project.matrix.values_mut() {
        for clip_opt in slots.iter_mut().flatten() {
            match &mut clip_opt.source {
                seqterm_core::PatternSource::Sf2 { path, .. } => {
                    if path.is_absolute() {
                        if let Ok(rel) = path.strip_prefix(base) {
                            *path = rel.to_path_buf();
                        }
                    }
                }
                seqterm_core::PatternSource::AudioFile { path, .. } => {
                    if path.is_absolute() {
                        if let Ok(rel) = path.strip_prefix(base) {
                            *path = rel.to_path_buf();
                        }
                    }
                }
                seqterm_core::PatternSource::Midi
                | seqterm_core::PatternSource::Plugin { .. } => {}
            }
        }
    }
}

/// Inverse of `make_paths_relative`: join relative PatternSource paths with the
/// project directory to produce absolute (loadable) paths.
fn make_paths_absolute(project: &mut Project, project_path: &Path) {
    let base = match project_path.parent() {
        Some(p) => p,
        None => return,
    };
    for slots in project.matrix.values_mut() {
        for clip_opt in slots.iter_mut().flatten() {
            match &mut clip_opt.source {
                seqterm_core::PatternSource::Sf2 { path, .. } => {
                    if path.is_relative() {
                        *path = base.join(&path);
                    }
                }
                seqterm_core::PatternSource::AudioFile { path, .. } => {
                    if path.is_relative() {
                        *path = base.join(&path);
                    }
                }
                seqterm_core::PatternSource::Midi
                | seqterm_core::PatternSource::Plugin { .. } => {}
            }
        }
    }
}

/// Atomic write: serialise to `.tmp`, then `rename` to destination.
pub fn save_project(project: &Project, path: &Path) -> Result<()> {
    // Work on a clone so we can relativize paths without mutating the live project.
    let mut proj = project.clone();
    make_paths_relative(&mut proj, path);
    let json = serde_json::to_string_pretty(&proj)
        .context("failed to serialize project")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create project directory")?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &json).context("failed to write temp file")?;
    fs::rename(&tmp, path).context("failed to rename temp → final")?;
    info!("Project saved to {}", path.display());
    Ok(())
}

/// Deserialize a project from a JSON file, running schema migrations first.
pub fn load_project(path: &Path) -> Result<Project> {
    let json = fs::read_to_string(path)
        .with_context(|| format!("failed to read project file: {}", path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&json)
        .with_context(|| format!("failed to parse project JSON: {}", path.display()))?;
    let from_version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if from_version < Project::CURRENT_VERSION {
        value = migrate(value, from_version);
    }
    let mut project: Project = serde_json::from_value(value)
        .with_context(|| format!("failed to deserialize project: {}", path.display()))?;
    project.migrate_legacy_arrangement();
    make_paths_absolute(&mut project, path);
    info!("Project loaded (schema v{}) from {}", project.version, path.display());
    Ok(project)
}

/// Apply forward migrations from `from_version` to `Project::CURRENT_VERSION`.
fn migrate(mut value: serde_json::Value, from_version: u32) -> serde_json::Value {
    // v0 → v1: no structural changes; just stamp the version field.
    if from_version < 1 {
        value["version"] = serde_json::json!(1);
        debug!("Migrated project schema 0 → 1");
    }
    // v1 → v2: Phase 2 rational time. `Pattern.resolution` is new but fills via
    // `#[serde(default)]` to the legacy `1/16` grid, so timing is unchanged and
    // no per-pattern transform is needed — just stamp the version.
    if from_version < 2 {
        value["version"] = serde_json::json!(2);
        debug!("Migrated project schema 1 → 2 (rational time; default 1/16 resolution)");
    }
    // v2 → v3: Phase 4 arrangement. `Project.arrangement` fills via `#[serde(default)]`
    // (empty); the rational clips are populated post-deserialize by
    // `Project::migrate_legacy_arrangement` from the legacy bar-block `tracks`.
    if from_version < 3 {
        value["version"] = serde_json::json!(3);
        debug!("Migrated project schema 2 → 3 (arrangement; populated from legacy tracks on load)");
    }
    // v3 → v4: Phase 6 canonical-note layer. `Pattern.events` fills via
    // `#[serde(default)]` (empty); no per-pattern transform — just stamp.
    if from_version < 4 {
        value["version"] = serde_json::json!(4);
        debug!("Migrated project schema 3 → 4 (exact rational-note layer; default empty)");
    }
    value
}

/// Return the next versioned snapshot path for a project file.
///
/// Given `projects/demo.json`, returns `projects/demo_v001.json` if that doesn't
/// exist yet, then `projects/demo_v002.json`, and so on up to `_v999`.
pub fn next_versioned_path(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy();
    let ext  = path.extension().and_then(|e| e.to_str()).unwrap_or("json");
    let dir  = path.parent().unwrap_or(Path::new("."));
    for n in 1u32..=999 {
        // Strip any existing `_vNNN` suffix so we always number from the base name.
        let base = if let Some(pos) = stem.rfind("_v") {
            let tail = &stem[pos + 2..];
            if tail.chars().all(|c| c.is_ascii_digit()) && tail.len() == 3 {
                stem[..pos].to_string()
            } else {
                stem.to_string()
            }
        } else {
            stem.to_string()
        };
        let candidate = dir.join(format!("{base}_v{n:03}.{ext}"));
        if !candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Load a project from a path, falling back to the default project on error.
pub fn load_or_default(path: &Path) -> Project {
    match load_project(path) {
        Ok(p) => p,
        Err(e) => {
            warn!("Could not load project from {}: {e}. Using default.", path.display());
            Project::default()
        }
    }
}

/// Save a project as an STZ archive (the only on-disk project format the app writes).
pub fn save_project_stz(project: &Project, path: &Path) -> Result<()> {
    let container = seqterm_stz::from_core(project);
    seqterm_stz::save(&container, path).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Load a project from an STZ archive (uses the embedded lossless core project).
pub fn load_project_stz(path: &Path) -> Result<Project> {
    let container = seqterm_stz::load(path).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(seqterm_stz::load_core(&container))
}

/// Startup loader: load `stz_path` if present; otherwise migrate a legacy `.json`
/// project (read once, re-saved as STZ on the next save); otherwise a default project.
pub fn load_or_migrate(stz_path: &Path, legacy_json: &Path) -> Project {
    if stz_path.exists() {
        match load_project_stz(stz_path) {
            Ok(p) => return p,
            Err(e) => warn!("Could not load {}: {e}. Trying legacy/default.", stz_path.display()),
        }
    }
    if legacy_json.exists() {
        info!("Migrating legacy project {} → STZ on next save", legacy_json.display());
        return load_or_default(legacy_json);
    }
    Project::default()
}

// ─── Recent files ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct RecentFiles {
    projects: Vec<PathBuf>,
    midi_imports: Vec<PathBuf>,
}

fn config_dir() -> PathBuf {
    dirs_home().join(".config").join("seqterm")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn recent_path() -> PathBuf {
    config_dir().join("recent.json")
}

pub fn load_recent_projects() -> Vec<PathBuf> {
    load_recent().projects.into_iter().filter(|p| p.exists()).collect()
}

pub fn load_recent_midi_imports() -> Vec<PathBuf> {
    load_recent().midi_imports.into_iter().filter(|p| p.exists()).collect()
}

pub fn push_recent_project(path: &Path) {
    let mut r = load_recent();
    r.projects.retain(|p| p != path);
    r.projects.insert(0, path.to_path_buf());
    r.projects.truncate(10);
    save_recent(&r);
}

pub fn push_recent_midi_import(path: &Path) {
    let mut r = load_recent();
    r.midi_imports.retain(|p| p != path);
    r.midi_imports.insert(0, path.to_path_buf());
    r.midi_imports.truncate(10);
    save_recent(&r);
}

fn load_recent() -> RecentFiles {
    let p = recent_path();
    if !p.exists() { return RecentFiles::default(); }
    fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_recent(r: &RecentFiles) {
    let p = recent_path();
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(r) {
        let _ = fs::write(&p, json);
    }
}

// ─── Autosave ─────────────────────────────────────────────────────────────────

pub struct Autosave {
    stop_tx: flume::Sender<()>,
}

impl Autosave {
    pub fn start(project: Arc<Mutex<Project>>, path: PathBuf, interval: Duration) -> Self {
        let (stop_tx, stop_rx) = flume::bounded(1);
        thread::Builder::new()
            .name("seqterm-autosave".to_string())
            .spawn(move || loop {
                match stop_rx.recv_timeout(interval) {
                    Ok(_) | Err(flume::RecvTimeoutError::Disconnected) => break,
                    Err(flume::RecvTimeoutError::Timeout) => {}
                }
                let proj = project.lock().clone();
                // Autosave as an STZ archive (never a loose .json) — the recovery
                // file matches the main project format.
                let autosave_path = path.with_extension("autosave.stz");
                let container = seqterm_stz::from_core(&proj);
                if let Err(e) = seqterm_stz::save(&container, &autosave_path) {
                    warn!("Autosave failed: {e}");
                } else {
                    debug!("Autosave written to {}", autosave_path.display());
                    // Drop any loose history sidecar left by older versions — history
                    // now lives inside the .stz archive itself.
                    let sidecar = autosave_path.with_file_name(format!(
                        "{}.history.json",
                        autosave_path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
                    ));
                    let _ = fs::remove_file(&sidecar);
                }
            })
            .expect("failed to spawn autosave thread");
        Self { stop_tx }
    }

    pub fn stop(&self) { let _ = self.stop_tx.send(()); }
}

impl Drop for Autosave {
    fn drop(&mut self) { self.stop(); }
}

// ─── Hex-arch adapters ────────────────────────────────────────────────────────

/// Reads partial JSON to extract only the fields needed for ProjectMetadata.
#[derive(Deserialize)]
struct ProjectHeader {
    #[serde(default)]
    name: String,
    #[serde(default)]
    bpm: f64,
    #[serde(default)]
    version: u32,
}

fn read_metadata_from_path(path: &Path) -> Result<seqterm_ports::ProjectMetadata> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read: {}", path.display()))?;
    let header: ProjectHeader = match path.extension().and_then(|e| e.to_str()) {
        Some("seqterm") => {
            let val: serde_json::Value = rmp_serde::from_slice(&bytes)
                .with_context(|| format!("failed to parse binary header: {}", path.display()))?;
            serde_json::from_value(val)?
        }
        _ => serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse JSON header: {}", path.display()))?,
    };
    let modified_at = fs::metadata(path).ok().and_then(|m| m.modified().ok());
    Ok(seqterm_ports::ProjectMetadata {
        name: if header.name.is_empty() {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed")
                .to_string()
        } else {
            header.name
        },
        path: path.to_path_buf(),
        bpm: header.bpm,
        version: header.version,
        modified_at,
    })
}

/// Adapter: format-aware repository (auto-detects `.json` vs `.seqterm`).
pub struct AutoProjectRepository;

impl seqterm_ports::ProjectRepository for AutoProjectRepository {
    fn load(&self, path: &Path) -> Result<Project> {
        load_project_auto(path)
    }

    fn save(&self, project: &Project, path: &Path) -> Result<()> {
        save_project_auto(project, path)
    }

    fn read_metadata(&self, path: &Path) -> Result<seqterm_ports::ProjectMetadata> {
        read_metadata_from_path(path)
    }

    fn list(&self, dir: &Path) -> Result<Vec<seqterm_ports::ProjectMetadata>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(dir).with_context(|| format!("failed to read dir: {}", dir.display()))? {
            let entry = entry?;
            let p = entry.path();
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "json" || ext == "seqterm" {
                match read_metadata_from_path(&p) {
                    Ok(m) => out.push(m),
                    Err(e) => warn!("Skipping {}: {e}", p.display()),
                }
            }
        }
        out.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
        Ok(out)
    }

    fn backup(&self, path: &Path) -> Result<PathBuf> {
        let dst = next_versioned_path(path)
            .with_context(|| format!("could not find backup slot for {}", path.display()))?;
        fs::copy(path, &dst)
            .with_context(|| format!("failed to copy {} → {}", path.display(), dst.display()))?;
        info!("Backup written to {}", dst.display());
        Ok(dst)
    }
}

/// Adapter: JSON-only repository.
pub struct JsonProjectRepository;

impl seqterm_ports::ProjectRepository for JsonProjectRepository {
    fn load(&self, path: &Path) -> Result<Project> { load_project(path) }
    fn save(&self, project: &Project, path: &Path) -> Result<()> { save_project(project, path) }
    fn read_metadata(&self, path: &Path) -> Result<seqterm_ports::ProjectMetadata> { read_metadata_from_path(path) }
    fn list(&self, dir: &Path) -> Result<Vec<seqterm_ports::ProjectMetadata>> {
        AutoProjectRepository.list(dir).map(|v| v.into_iter().filter(|m| {
            m.path.extension().and_then(|e| e.to_str()) == Some("json")
        }).collect())
    }
    fn backup(&self, path: &Path) -> Result<PathBuf> { AutoProjectRepository.backup(path) }
}

/// Adapter: binary (MessagePack `.seqterm`) repository.
pub struct BinaryProjectRepository;

impl seqterm_ports::ProjectRepository for BinaryProjectRepository {
    fn load(&self, path: &Path) -> Result<Project> { load_project_msgpack(path) }
    fn save(&self, project: &Project, path: &Path) -> Result<()> { save_project_msgpack(project, path) }
    fn read_metadata(&self, path: &Path) -> Result<seqterm_ports::ProjectMetadata> { read_metadata_from_path(path) }
    fn list(&self, dir: &Path) -> Result<Vec<seqterm_ports::ProjectMetadata>> {
        AutoProjectRepository.list(dir).map(|v| v.into_iter().filter(|m| {
            m.path.extension().and_then(|e| e.to_str()) == Some("seqterm")
        }).collect())
    }
    fn backup(&self, path: &Path) -> Result<PathBuf> { AutoProjectRepository.backup(path) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seqterm_core::{Pattern, Project};
    use tempfile::tempdir;

    fn sample_project() -> Project {
        let mut proj = Project::default();
        proj.name = "TestProject".into();
        proj.bpm = 128.0;
        let mut pat = Pattern::new("BASS1", 16);
        pat.set_step(0, seqterm_core::Note::from_midi(36, 100).unwrap());
        proj.patterns.insert("BASS1".into(), pat);
        proj
    }

    #[test]
    fn json_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("project.json");
        let proj = sample_project();
        save_project(&proj, &path).unwrap();
        let loaded = load_project(&path).unwrap();
        assert_eq!(loaded.name, proj.name);
        assert!((loaded.bpm - proj.bpm).abs() < 1e-9);
        assert!(loaded.patterns.contains_key("BASS1"));
    }

    #[test]
    fn msgpack_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("project.seqterm");
        let proj = sample_project();
        save_project_msgpack(&proj, &path).unwrap();
        let loaded = load_project_msgpack(&path).unwrap();
        assert_eq!(loaded.name, proj.name);
        assert!((loaded.bpm - proj.bpm).abs() < 1e-9);
        assert!(loaded.patterns.contains_key("BASS1"));
    }

    #[test]
    fn auto_detect_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("auto.json");
        let proj = sample_project();
        save_project_auto(&proj, &path).unwrap();
        let loaded = load_project_auto(&path).unwrap();
        assert_eq!(loaded.name, proj.name);
    }

    #[test]
    fn auto_detect_msgpack() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("auto.seqterm");
        let proj = sample_project();
        save_project_auto(&proj, &path).unwrap();
        let loaded = load_project_auto(&path).unwrap();
        assert_eq!(loaded.name, proj.name);
    }

    #[test]
    fn atomic_write_does_not_leave_tmp_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("project.json");
        let proj = sample_project();
        save_project(&proj, &path).unwrap();
        let tmp = dir.path().join("project.json.tmp");
        assert!(!tmp.exists(), ".tmp file should not exist after successful save");
        assert!(path.exists());
    }

    #[test]
    fn load_nonexistent_returns_error() {
        let result = load_project(Path::new("/nonexistent/path/x.json"));
        assert!(result.is_err());
    }

    #[test]
    fn note_data_survives_json_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("notes.json");
        let mut proj = sample_project();
        proj.patterns.get_mut("BASS1").unwrap()
            .set_step(4, seqterm_core::Note::from_midi(48, 80).unwrap());
        save_project(&proj, &path).unwrap();
        let loaded = load_project(&path).unwrap();
        let pat = &loaded.patterns["BASS1"];
        assert_eq!(pat.steps[0].to_midi(), Some(36));
        assert_eq!(pat.steps[4].to_midi(), Some(48));
    }

    #[test]
    fn bpm_precision_survives_msgpack() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bpm.seqterm");
        let mut proj = sample_project();
        proj.bpm = 133.333;
        save_project_msgpack(&proj, &path).unwrap();
        let loaded = load_project_msgpack(&path).unwrap();
        assert!((loaded.bpm - 133.333).abs() < 1e-6);
    }

    #[test]
    fn legacy_v1_project_migrates_losslessly_to_rational() {
        use seqterm_core::{Note, RationalTime, Resolution};
        // Build a real project, then strip the v2-only `resolution` field and the
        // `version` stamp from its JSON to simulate an on-disk v1 project file.
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy.json");
        let mut proj = sample_project();
        let mut n = Note::from_midi(48, 100).unwrap();
        n.gate = 50; // half a step
        n.micro = 25; // +25% of a step
        proj.patterns.get_mut("BASS1").unwrap().steps = vec![n];
        proj.patterns.get_mut("BASS1").unwrap().length = 16;

        let mut value = serde_json::to_value(&proj).unwrap();
        value.as_object_mut().unwrap().remove("version");
        for pat in value["patterns"].as_object_mut().unwrap().values_mut() {
            pat.as_object_mut().unwrap().remove("resolution");
        }
        std::fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();

        let loaded = load_project(&path).unwrap();
        // Version is stamped up to current.
        assert_eq!(loaded.version, Project::CURRENT_VERSION);
        let pat = &loaded.patterns["BASS1"];
        // Missing `resolution` defaults to the legacy 1/16 grid.
        assert_eq!(pat.resolution, Resolution::Whole(16));
        // The legacy step (gate 50, micro +25) folds into exact rational timing.
        let events = pat.to_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].start, RationalTime::new(1, 16)); // +25% of a 1/4-beat step
        assert_eq!(events[0].duration, RationalTime::new(1, 8)); // 50% of 1/4 beat
    }

    #[test]
    fn legacy_arranger_tracks_migrate_to_rational_arrangement() {
        use seqterm_core::project::Track;
        let dir = tempdir().unwrap();
        let path = dir.path().join("arr.json");
        let mut proj = sample_project();
        let mut t = Track::new("Lead");
        t.blocks = vec![(0, 2, "BASS1".into()), (4, 1, "BASS1".into())];
        proj.tracks = vec![t];
        // Simulate an older file: clear the arrangement and strip version.
        proj.arrangement = Default::default();
        let mut value = serde_json::to_value(&proj).unwrap();
        value.as_object_mut().unwrap().remove("version");
        value.as_object_mut().unwrap().remove("arrangement");
        std::fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();

        let loaded = load_project(&path).unwrap();
        assert_eq!(loaded.version, Project::CURRENT_VERSION);
        // Legacy tracks are preserved AND mirrored into the rational arrangement.
        assert_eq!(loaded.tracks.len(), 1);
        assert!(!loaded.arrangement.is_empty());
        let lead = loaded.arrangement.tracks.iter()
            .find(|t| t.name == "Lead").expect("Lead track migrated");
        let clips = &lead.lanes[0].clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].start, seqterm_core::RationalTime::ZERO);
        assert_eq!(clips[0].length, seqterm_core::RationalTime::whole(8)); // 2 bars × 4
        assert_eq!(clips[1].start, seqterm_core::RationalTime::whole(16)); // bar 4 × 4
    }

    #[test]
    fn rational_resolution_survives_json_roundtrip() {
        use seqterm_core::Resolution;
        let dir = tempdir().unwrap();
        let path = dir.path().join("res.json");
        let mut proj = sample_project();
        proj.patterns.get_mut("BASS1").unwrap().resolution = Resolution::Whole(12);
        save_project(&proj, &path).unwrap();
        let loaded = load_project(&path).unwrap();
        assert_eq!(loaded.patterns["BASS1"].resolution, Resolution::Whole(12));
    }

    /// A fully-populated SONG (arrangement) survives a JSON save/load: tracks +
    /// routing, clips, markers, regions, sections, cycle, per-track automation,
    /// and a pattern's exact rational-note layer. Guards the song format (v4).
    #[test]
    fn full_song_arrangement_survives_roundtrip() {
        use seqterm_core::{
            ArrangementClip, ArrangementTrack, AutomationCurve, ClipKind, Note, RationalTime,
            TrackKind,
        };
        let dir = tempdir().unwrap();
        let path = dir.path().join("song.json");

        let mut proj = sample_project();
        // A pattern carrying an exact 7:9-style tuplet event (off the step grid).
        proj.patterns.get_mut("BASS1").unwrap()
            .add_event(RationalTime::new(2, 7), RationalTime::new(1, 7), Note::from_midi(60, 100).unwrap());

        let arr = &mut proj.arrangement;
        let mut track = ArrangementTrack::new("Lead", TrackKind::Midi);
        track.source_row = Some("A".into());
        track.primary_lane_mut().clips.push(ArrangementClip::new(
            0, "clipA",
            ClipKind::Pattern { pattern_key: "BASS1".into() },
            RationalTime::ZERO, RationalTime::whole(8),
        ));
        arr.tracks.push(track);
        arr.next_clip_id = 1;
        arr.set_automation_point(0, "volume", RationalTime::ZERO, 0.25, AutomationCurve::Linear);
        arr.set_automation_point(0, "volume", RationalTime::whole(8), 1.0, AutomationCurve::Linear);
        arr.add_marker(RationalTime::whole(4), "Verse");
        arr.add_region(RationalTime::ZERO, RationalTime::whole(8), "Intro");
        arr.add_section(RationalTime::ZERO, RationalTime::whole(8), "A");
        arr.cycle = Some((RationalTime::ZERO, RationalTime::whole(8)));

        save_project(&proj, &path).unwrap();
        let loaded = load_project(&path).unwrap();
        assert_eq!(loaded.version, Project::CURRENT_VERSION);

        let a = &loaded.arrangement;
        assert_eq!(a.tracks.len(), 1);
        let t = &a.tracks[0];
        assert_eq!(t.source_row.as_deref(), Some("A"));
        assert_eq!(t.lanes[0].clips[0].length, RationalTime::whole(8));
        assert_eq!(t.lanes[0].clips[0].kind.pattern_key(), Some("BASS1"));
        // Automation interpolates exactly at the midpoint.
        assert_eq!(a.automation_value(0, "volume", RationalTime::whole(4)), Some(0.625));
        assert_eq!(a.markers.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), vec!["Verse"]);
        assert_eq!(a.regions.len(), 1);
        assert_eq!(a.sections.len(), 1);
        assert_eq!(a.cycle, Some((RationalTime::ZERO, RationalTime::whole(8))));
        // The pattern's exact rational note survives and is routable for playback.
        assert!(loaded.patterns["BASS1"].events.iter().any(|e| e.start == RationalTime::new(2, 7)));
        // The whole arrangement is byte-for-byte equal after the round-trip.
        assert_eq!(loaded.arrangement, proj.arrangement);
    }
}
