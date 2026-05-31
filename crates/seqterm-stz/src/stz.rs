use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
};

use sha2::Digest;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{
    domain::{
        AssetEntry, AssetType, Manifest, ProjectSnapshot, StzAutomationLane, StzBus,
        StzContainer, StzMixerChannel, StzPattern, StzPluginInstance, StzProject,
        StzRoutingGraph, StzTimeline, StzTrack, StzTransport, STZ_FORMAT_VERSION,
    },
    error::{StzError, StzResult},
    migration::ProjectMigrator,
    ports::ProjectStoragePort,
    registry::{AssetRegistry, ObjectRegistry},
    validation::DefaultValidator,
    ports::ProjectValidatorPort,
};

// ─── Zip helpers ──────────────────────────────────────────────────────────────

fn zip_options() -> zip::write::FileOptions {
    zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644)
}

fn read_zip_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> StzResult<Vec<u8>> {
    let mut entry = archive
        .by_name(name)
        .map_err(|e| match e {
            zip::result::ZipError::FileNotFound => {
                StzError::MissingObject(format!("zip entry not found: {name}"))
            }
            other => StzError::Zip(other),
        })?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    Ok(buf)
}

fn read_zip_entry_opt<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> StzResult<Option<Vec<u8>>> {
    match archive.by_name(name) {
        Ok(mut entry) => {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            Ok(Some(buf))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(StzError::Zip(e)),
    }
}

fn read_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> StzResult<T> {
    serde_json::from_slice(bytes).map_err(StzError::Json)
}

// ─── Save ─────────────────────────────────────────────────────────────────────

fn save_container(container: &StzContainer, path: &Path) -> StzResult<()> {
    let tmp = path.with_extension("stz.tmp");

    {
        let file = std::fs::File::create(&tmp)?;
        let mut zip = zip::ZipWriter::new(file);
        let opt = zip_options();

        // manifest.json
        let mut manifest = container.manifest.clone();
        manifest.touch();
        zip.start_file("manifest.json", opt)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;

        // project/project.json
        let mut project = container.project.clone();
        // Make sure UUID refs match current objects before writing.
        project.tracks = container.tracks.iter().map(|t| t.id).collect();
        project.patterns = container.patterns.iter().map(|p| p.id).collect();
        project.mixer_channels = container.mixer_channels.iter().map(|c| c.id).collect();
        project.buses = container.buses.iter().map(|b| b.id).collect();
        project.plugins = container.plugins.iter().map(|p| p.id).collect();
        project.automation = container.automation.iter().map(|a| a.id).collect();
        project.transport = container.transport.id;
        project.timeline = container.timeline.id;
        project.routing = container.routing.id;
        zip.start_file("project/project.json", opt)?;
        zip.write_all(&serde_json::to_vec_pretty(&project)?)?;

        // project/transport.json
        zip.start_file("project/transport.json", opt)?;
        zip.write_all(&serde_json::to_vec_pretty(&container.transport)?)?;

        // project/timeline.json
        zip.start_file("project/timeline.json", opt)?;
        zip.write_all(&serde_json::to_vec_pretty(&container.timeline)?)?;

        // objects/tracks/{uuid}.json
        for track in &container.tracks {
            let name = format!("objects/tracks/{}.json", track.id);
            zip.start_file(&name, opt)?;
            zip.write_all(&serde_json::to_vec_pretty(track)?)?;
        }

        // objects/patterns/{uuid}.json
        for pattern in &container.patterns {
            let name = format!("objects/patterns/{}.json", pattern.id);
            zip.start_file(&name, opt)?;
            zip.write_all(&serde_json::to_vec_pretty(pattern)?)?;
        }

        // objects/buses/{uuid}.json
        for bus in &container.buses {
            let name = format!("objects/buses/{}.json", bus.id);
            zip.start_file(&name, opt)?;
            zip.write_all(&serde_json::to_vec_pretty(bus)?)?;
        }

        // mixer.json  (all channels as an array for fast bulk load)
        zip.start_file("project/mixer.json", opt)?;
        zip.write_all(&serde_json::to_vec_pretty(&container.mixer_channels)?)?;

        // objects/automation/{uuid}.json
        for lane in &container.automation {
            let name = format!("objects/automation/{}.json", lane.id);
            zip.start_file(&name, opt)?;
            zip.write_all(&serde_json::to_vec_pretty(lane)?)?;
        }

        // objects/plugins/{uuid}.json
        for plugin in &container.plugins {
            let name = format!("objects/plugins/{}.json", plugin.id);
            zip.start_file(&name, opt)?;
            zip.write_all(&serde_json::to_vec_pretty(plugin)?)?;
        }

        // objects/routing/{uuid}.json
        let routing_name = format!("objects/routing/{}.json", container.routing.id);
        zip.start_file(&routing_name, opt)?;
        zip.write_all(&serde_json::to_vec_pretty(&container.routing)?)?;

        // registry/assets.json
        zip.start_file("registry/assets.json", opt)?;
        zip.write_all(&serde_json::to_vec_pretty(&container.asset_registry)?)?;

        // registry/objects.json
        let object_registry = container.build_object_registry();
        zip.start_file("registry/objects.json", opt)?;
        zip.write_all(&serde_json::to_vec_pretty(&object_registry)?)?;

        // asset data: audio/*, midi/*, plugins/state/*
        for entry in &container.asset_registry.assets {
            if let Some(data) = container.asset_data.get(&entry.uuid) {
                zip.start_file(&entry.path, opt)?;
                zip.write_all(data)?;
            }
        }

        zip.finish()?;
    }

    // Validate archive before replacing original.
    validate_archive(&tmp)?;

    std::fs::rename(&tmp, path)?;
    info!("STZ saved to {}", path.display());
    Ok(())
}

fn validate_archive(path: &Path) -> StzResult<()> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // manifest.json must exist and be parseable.
    let manifest_bytes = read_zip_entry(&mut archive, "manifest.json")?;
    let manifest: Manifest = read_json(&manifest_bytes)?;

    if manifest.format_version > STZ_FORMAT_VERSION {
        return Err(StzError::UnsupportedVersion(manifest.format_version));
    }
    if manifest.root_project.is_empty() {
        return Err(StzError::CorruptArchive("manifest root_project is empty".into()));
    }
    // project/project.json must exist.
    read_zip_entry(&mut archive, "project/project.json")?;

    debug!("STZ archive validation passed for {}", path.display());
    Ok(())
}

// ─── Load ─────────────────────────────────────────────────────────────────────

fn load_container(path: &Path) -> StzResult<StzContainer> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| StzError::CorruptArchive(e.to_string()))?;

    // ── 1. manifest ─────────────────────────────────────────────────────────
    let manifest_bytes = read_zip_entry(&mut archive, "manifest.json")?;
    let manifest: Manifest = read_json(&manifest_bytes)?;

    // ── 2. migration ────────────────────────────────────────────────────────
    if manifest.format_version < STZ_FORMAT_VERSION {
        // Collect all entries, migrate, then reload.
        let names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        let mut entry_map: HashMap<String, Vec<u8>> = HashMap::new();
        for name in names {
            let bytes = read_zip_entry(&mut archive, &name)?;
            entry_map.insert(name, bytes);
        }
        let migrator = ProjectMigrator::new();
        migrator.migrate_to_current(&mut entry_map, manifest.format_version)?;
        // Reconstruct archive from migrated map.
        return load_from_entry_map(entry_map);
    }

    // ── 3. validate manifest ─────────────────────────────────────────────────
    let validator = DefaultValidator;
    validator.validate_manifest(&manifest)?;

    // ── 4. project ───────────────────────────────────────────────────────────
    let project_bytes = read_zip_entry(&mut archive, "project/project.json")?;
    let project: StzProject = read_json(&project_bytes)?;

    // ── 5. transport + timeline ──────────────────────────────────────────────
    let transport: StzTransport =
        read_json(&read_zip_entry(&mut archive, "project/transport.json")?)?;
    let timeline: StzTimeline =
        read_json(&read_zip_entry(&mut archive, "project/timeline.json")?)?;

    // ── 6. tracks ────────────────────────────────────────────────────────────
    let mut tracks: Vec<StzTrack> = Vec::new();
    for uuid in &project.tracks {
        let name = format!("objects/tracks/{uuid}.json");
        match read_zip_entry_opt(&mut archive, &name)? {
            Some(b) => tracks.push(read_json(&b)?),
            None => warn!("STZ: track {uuid} referenced but not found in archive"),
        }
    }

    // ── 7. patterns ──────────────────────────────────────────────────────────
    let mut patterns: Vec<StzPattern> = Vec::new();
    for uuid in &project.patterns {
        let name = format!("objects/patterns/{uuid}.json");
        match read_zip_entry_opt(&mut archive, &name)? {
            Some(b) => patterns.push(read_json(&b)?),
            None => warn!("STZ: pattern {uuid} referenced but not found in archive"),
        }
    }

    // ── 8. mixer channels ────────────────────────────────────────────────────
    let mixer_channels: Vec<StzMixerChannel> = match read_zip_entry_opt(&mut archive, "project/mixer.json")? {
        Some(b) => read_json(&b)?,
        None => Vec::new(),
    };

    // ── 9. buses ─────────────────────────────────────────────────────────────
    let mut buses: Vec<StzBus> = Vec::new();
    for uuid in &project.buses {
        let name = format!("objects/buses/{uuid}.json");
        match read_zip_entry_opt(&mut archive, &name)? {
            Some(b) => buses.push(read_json(&b)?),
            None => warn!("STZ: bus {uuid} referenced but not found in archive"),
        }
    }

    // ── 10. automation ───────────────────────────────────────────────────────
    let mut automation: Vec<StzAutomationLane> = Vec::new();
    for uuid in &project.automation {
        let name = format!("objects/automation/{uuid}.json");
        match read_zip_entry_opt(&mut archive, &name)? {
            Some(b) => automation.push(read_json(&b)?),
            None => warn!("STZ: automation lane {uuid} referenced but not found in archive"),
        }
    }

    // ── 11. plugins ──────────────────────────────────────────────────────────
    let mut plugins: Vec<StzPluginInstance> = Vec::new();
    for uuid in &project.plugins {
        let name = format!("objects/plugins/{uuid}.json");
        match read_zip_entry_opt(&mut archive, &name)? {
            Some(b) => plugins.push(read_json(&b)?),
            None => warn!("STZ: plugin {uuid} referenced but not found in archive"),
        }
    }

    // ── 12. routing graph ────────────────────────────────────────────────────
    let routing_name = format!("objects/routing/{}.json", project.routing);
    let routing: StzRoutingGraph = match read_zip_entry_opt(&mut archive, &routing_name)? {
        Some(b) => read_json(&b)?,
        None => StzRoutingGraph::new(),
    };

    // ── 13. registries ───────────────────────────────────────────────────────
    let asset_registry: AssetRegistry = match read_zip_entry_opt(&mut archive, "registry/assets.json")? {
        Some(b) => read_json(&b)?,
        None => AssetRegistry::new(),
    };
    let object_registry: ObjectRegistry = match read_zip_entry_opt(&mut archive, "registry/objects.json")? {
        Some(b) => read_json(&b)?,
        None => ObjectRegistry::new(),
    };

    // ── 14. asset data (lazy: only load if already in archive) ───────────────
    let mut asset_data: HashMap<Uuid, Vec<u8>> = HashMap::new();
    for entry in &asset_registry.assets {
        if let Some(data) = read_zip_entry_opt(&mut archive, &entry.path)? {
            asset_data.insert(entry.uuid, data);
        }
    }

    let container = StzContainer {
        manifest,
        project,
        tracks,
        patterns,
        mixer_channels,
        buses,
        automation,
        plugins,
        transport,
        timeline,
        routing,
        asset_registry,
        object_registry,
        asset_data,
    };

    info!("STZ loaded from {}", path.display());
    Ok(container)
}

fn load_from_entry_map(entries: HashMap<String, Vec<u8>>) -> StzResult<StzContainer> {
    // Re-pack into a temporary in-memory ZIP so load_container can reuse its logic.
    use std::io::Cursor;

    let buf = {
        let cursor = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let opt = zip_options();
        for (name, data) in &entries {
            zip.start_file(name, opt)?;
            zip.write_all(data)?;
        }
        zip.finish()?.into_inner()
    };

    let tmp_dir = tempfile::tempdir()?;
    let tmp_path = tmp_dir.path().join("migrated.stz");
    std::fs::write(&tmp_path, &buf)?;
    load_container(&tmp_path)
}

// ─── Adapter ──────────────────────────────────────────────────────────────────

/// ZIP-based `.stz` project storage adapter.
pub struct StzProjectStorage;

impl ProjectStoragePort for StzProjectStorage {
    fn save(&self, container: &StzContainer, path: &Path) -> StzResult<()> {
        save_container(container, path)
    }

    fn load(&self, path: &Path) -> StzResult<StzContainer> {
        load_container(path)
    }

    fn read_manifest(&self, path: &Path) -> StzResult<Manifest> {
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| StzError::CorruptArchive(e.to_string()))?;
        let bytes = read_zip_entry(&mut archive, "manifest.json")?;
        read_json(&bytes)
    }

    fn list_snapshots(&self, path: &Path) -> StzResult<Vec<ProjectSnapshot>> {
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| StzError::CorruptArchive(e.to_string()))?;

        let snapshot_names: Vec<String> = archive
            .file_names()
            .filter(|n| n.starts_with("snapshots/") && n.ends_with(".json"))
            .map(|s| s.to_string())
            .collect();

        let mut snapshots = Vec::new();
        for name in snapshot_names {
            match read_zip_entry_opt(&mut archive, &name)? {
                Some(b) => match read_json::<ProjectSnapshot>(&b) {
                    Ok(s) => snapshots.push(s),
                    Err(e) => warn!("STZ: could not parse snapshot {name}: {e}"),
                },
                None => {}
            }
        }
        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(snapshots)
    }
}

// ─── Asset hash helper ────────────────────────────────────────────────────────

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(data))
}

pub fn make_asset_entry(
    data: &[u8],
    original_name: &str,
    asset_type: AssetType,
) -> AssetEntry {
    let uuid = Uuid::new_v4();
    let ext = std::path::Path::new(original_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let path = format!("{}/{}.{}", asset_type.directory(), uuid, ext);
    AssetEntry {
        uuid,
        asset_type,
        path,
        hash: sha256_hex(data),
        size_bytes: data.len() as u64,
        created_at: chrono::Utc::now(),
        original_name: original_name.to_string(),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_container() -> StzContainer {
        let mut c = StzContainer::new("Test Project", 128.0);

        let track = StzTrack::new("Kick");
        let pat = StzPattern::new("KCK01", 16);
        let ch = StzMixerChannel::new("CH1");
        let bus = StzBus::new("Bus A");

        c.project.tracks.push(track.id);
        c.project.patterns.push(pat.id);
        c.project.mixer_channels.push(ch.id);
        c.project.buses.push(bus.id);

        c.tracks.push(track);
        c.patterns.push(pat);
        c.mixer_channels.push(ch);
        c.buses.push(bus);

        c.object_registry = c.build_object_registry();
        c
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.stz");
        let orig = sample_container();
        let storage = StzProjectStorage;

        storage.save(&orig, &path).unwrap();
        assert!(path.exists());

        let loaded = storage.load(&path).unwrap();
        assert_eq!(loaded.project.name, orig.project.name);
        assert!((loaded.project.bpm - orig.project.bpm).abs() < 1e-9);
        assert_eq!(loaded.tracks.len(), 1);
        assert_eq!(loaded.patterns.len(), 1);
    }

    #[test]
    fn uuid_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("uuids.stz");
        let orig = sample_container();
        let storage = StzProjectStorage;

        storage.save(&orig, &path).unwrap();
        let loaded = storage.load(&path).unwrap();

        assert_eq!(loaded.project.id, orig.project.id);
        assert_eq!(loaded.tracks[0].id, orig.tracks[0].id);
        assert_eq!(loaded.patterns[0].id, orig.patterns[0].id);
        assert_eq!(loaded.transport.id, orig.transport.id);
    }

    #[test]
    fn manifest_is_readable_without_full_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mf.stz");
        let orig = sample_container();
        let storage = StzProjectStorage;

        storage.save(&orig, &path).unwrap();
        let mf = storage.read_manifest(&path).unwrap();

        assert_eq!(mf.project_uuid, orig.project.id);
        assert_eq!(mf.project_name, "Test Project");
        assert_eq!(mf.format, "STZ");
        assert_eq!(mf.format_version, 1);
    }

    #[test]
    fn atomic_save_leaves_no_tmp() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("atomic.stz");
        let c = sample_container();
        StzProjectStorage.save(&c, &path).unwrap();

        let tmp = dir.path().join("atomic.stz.tmp");
        assert!(!tmp.exists(), ".stz.tmp should be gone after successful save");
        assert!(path.exists());
    }

    #[test]
    fn corrupt_archive_returns_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("corrupt.stz");
        std::fs::write(&path, b"this is not a zip file").unwrap();
        let result = StzProjectStorage.load(&path);
        assert!(result.is_err());
        match result.unwrap_err() {
            StzError::CorruptArchive(_) | StzError::Zip(_) => {}
            other => panic!("expected corrupt/zip error, got {other:?}"),
        }
    }

    #[test]
    fn missing_manifest_returns_error() {
        use std::io::Write as _;
        let dir = tempdir().unwrap();
        let path = dir.path().join("nomanifest.stz");
        {
            let f = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opt = zip::write::FileOptions::default();
            zip.start_file("placeholder.txt", opt).unwrap();
            zip.write_all(b"nothing").unwrap();
            zip.finish().unwrap();
        }
        let result = StzProjectStorage.load(&path);
        assert!(result.is_err());
    }

    #[test]
    fn object_registry_consistency() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reg.stz");
        let orig = sample_container();
        StzProjectStorage.save(&orig, &path).unwrap();
        let loaded = StzProjectStorage.load(&path).unwrap();

        assert_eq!(loaded.object_registry.tracks.len(), 1);
        assert_eq!(loaded.object_registry.patterns.len(), 1);
        assert_eq!(loaded.object_registry.tracks[0], orig.tracks[0].id);
    }

    #[test]
    fn asset_hash_is_deterministic() {
        let data = b"hello world audio";
        let h1 = sha256_hex(data);
        let h2 = sha256_hex(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn routing_graph_cycle_detected() {
        let mut graph = StzRoutingGraph::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        graph.nodes.push(crate::domain::StzRoutingNode {
            id: a, kind: "channel".into(), label: "A".into(), target_id: None,
        });
        graph.nodes.push(crate::domain::StzRoutingNode {
            id: b, kind: "channel".into(), label: "B".into(), target_id: None,
        });
        graph.edges.push(crate::domain::StzRoutingEdge {
            from: a, to: b, kind: "audio".into(),
        });
        graph.edges.push(crate::domain::StzRoutingEdge {
            from: b, to: a, kind: "audio".into(),
        });
        assert!(graph.validate_acyclic().is_err());
    }

    #[test]
    fn routing_graph_acyclic_passes() {
        let mut graph = StzRoutingGraph::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c_id = Uuid::new_v4();
        for (id, label) in [(a, "A"), (b, "B"), (c_id, "C")] {
            graph.nodes.push(crate::domain::StzRoutingNode {
                id, kind: "channel".into(), label: label.into(), target_id: None,
            });
        }
        graph.edges.push(crate::domain::StzRoutingEdge {
            from: a, to: b, kind: "audio".into(),
        });
        graph.edges.push(crate::domain::StzRoutingEdge {
            from: b, to: c_id, kind: "audio".into(),
        });
        assert!(graph.validate_acyclic().is_ok());
    }

    #[test]
    fn migration_v0_to_v1() {
        use crate::migration::ProjectMigrator;
        let manifest_v0 = serde_json::json!({
            "format": "STZ",
            "format_version": 0,
            "project_uuid": Uuid::new_v4().to_string(),
            "project_name": "Old",
            "engine_version": "0.0.1",
            "created_at": chrono::Utc::now().to_rfc3339(),
            "modified_at": chrono::Utc::now().to_rfc3339(),
            "root_project": "project/project.json"
        });
        let mut entries: HashMap<String, Vec<u8>> = HashMap::new();
        entries.insert("manifest.json".to_string(), serde_json::to_vec(&manifest_v0).unwrap());

        let migrator = ProjectMigrator::new();
        migrator.migrate_to_current(&mut entries, 0).unwrap();

        let patched: serde_json::Value = serde_json::from_slice(&entries["manifest.json"]).unwrap();
        assert_eq!(patched["format_version"].as_u64().unwrap(), 1);
    }
}
