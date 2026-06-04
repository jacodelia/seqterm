//! `stz` — command-line tool for SeqTerm `.stz` project archives.
//!
//! Usage:
//!   stz inspect  <file.stz>           Print manifest, registry, and snapshot list.
//!   stz extract  <file.stz> [outdir]  Extract all assets to a directory (default: `./<stem>/`).
//!   stz pack     <dir/> [out.stz]     Pack a directory into a .stz archive.
//!   stz migrate  <file.stz>           Apply all pending schema migrations in-place.
//!   stz validate <file.stz>           Check integrity: hashes, UUIDs, registry consistency.
//!   stz diff     <a.stz> <b.stz>      List objects added/removed/changed between two archives.
//!   stz snapshot <file.stz>           List snapshots stored in an archive.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "stz", about = "SeqTerm .stz project archive tool", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print manifest, object registry, snapshot list, and asset inventory.
    Inspect { file: PathBuf },
    /// Extract all assets from a .stz archive to a directory.
    Extract {
        file: PathBuf,
        /// Output directory (default: <stem>/).
        #[arg(default_value = "")]
        outdir: String,
    },
    /// Pack a directory into a .stz archive.
    Pack {
        dir: PathBuf,
        /// Output path (default: <dir>.stz).
        #[arg(default_value = "")]
        output: String,
    },
    /// Apply all pending schema migrations to a .stz file in-place.
    Migrate { file: PathBuf },
    /// Check integrity: asset hashes, UUID uniqueness, registry consistency.
    Validate { file: PathBuf },
    /// Show objects added/removed/changed between two .stz archives.
    Diff { a: PathBuf, b: PathBuf },
    /// List snapshots stored inside a .stz archive.
    Snapshot { file: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Inspect { file }                => cmd_inspect(&file),
        Cmd::Extract { file, outdir }        => cmd_extract(&file, &outdir),
        Cmd::Pack    { dir,  output }        => cmd_pack(&dir, &output),
        Cmd::Migrate { file }                => cmd_migrate(&file),
        Cmd::Validate{ file }                => cmd_validate(&file),
        Cmd::Diff    { a, b }               => cmd_diff(&a, &b),
        Cmd::Snapshot{ file }               => cmd_snapshot(&file),
    }
}

// ─── inspect ──────────────────────────────────────────────────────────────────

fn cmd_inspect(path: &Path) -> Result<()> {
    let c = seqterm_stz::load(path)
        .with_context(|| format!("Cannot load {}", path.display()))?;

    println!("=== MANIFEST ===");
    println!("  Name:     {}", c.manifest.project_name);
    println!("  UUID:     {}", c.manifest.project_uuid);
    println!("  Format:   v{}", c.manifest.format_version);
    println!("  Engine:   {}", c.manifest.engine_version);
    println!("  Created:  {}", c.manifest.created_at);
    println!("  Modified: {}", c.manifest.modified_at);

    println!("\n=== OBJECT REGISTRY ===");
    let reg = c.build_object_registry();
    println!("  Tracks:          {}", reg.tracks.len());
    println!("  Patterns:        {}", reg.patterns.len());
    println!("  Mixer channels:  {}", reg.mixer_channels.len());
    println!("  Buses:           {}", reg.buses.len());

    println!("\n=== ASSETS ({}) ===", c.asset_registry.assets.len());
    for a in &c.asset_registry.assets {
        println!("  [{}]  {}  ({} bytes)  {}", a.uuid, a.path, a.size_bytes, a.hash);
    }

    println!("\n=== SNAPSHOTS ({}) ===", c.list_snapshots().len());
    for s in c.list_snapshots() {
        println!("  [{}]  {}  ({})", s.id, s.name, s.created_at);
    }

    Ok(())
}

// ─── extract ──────────────────────────────────────────────────────────────────

fn cmd_extract(path: &Path, outdir: &str) -> Result<()> {
    let c = seqterm_stz::load(path)
        .with_context(|| format!("Cannot load {}", path.display()))?;

    let base = if outdir.is_empty() {
        let stem = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("stz_extract");
        PathBuf::from(stem)
    } else {
        PathBuf::from(outdir)
    };

    std::fs::create_dir_all(&base)
        .with_context(|| format!("Cannot create output dir {}", base.display()))?;

    let mut extracted = 0usize;
    for entry in &c.asset_registry.assets {
        if let Some(data) = c.asset_data.get(&entry.uuid) {
            let out_path = base.join(entry.original_name.replace(['/', '\\'], "_"));
            std::fs::write(&out_path, data)
                .with_context(|| format!("Cannot write {}", out_path.display()))?;
            println!("  extracted: {} → {}", entry.path, out_path.display());
            extracted += 1;
        }
    }

    // Also write the latest project JSON snapshot.
    if let Some(snap) = c.list_snapshots().into_iter().last() {
        if let Some(data) = c.snapshot_data(snap.id) {
            let json_path = base.join("project.json");
            std::fs::write(&json_path, data)?;
            println!("  extracted: project.json (snapshot '{}')", snap.name);
        }
    }

    println!("\nExtracted {} asset(s) to {}", extracted, base.display());
    Ok(())
}

// ─── pack ─────────────────────────────────────────────────────────────────────

fn cmd_pack(dir: &Path, output: &str) -> Result<()> {
    let out_path = if output.is_empty() {
        dir.with_extension("stz")
    } else {
        PathBuf::from(output)
    };

    // Minimal pack: look for project.json in the directory and build a container.
    let json_path = dir.join("project.json");
    anyhow::ensure!(json_path.exists(), "project.json not found in {}", dir.display());

    let json = std::fs::read(&json_path)?;
    let project: seqterm_core::Project = serde_json::from_slice(&json)
        .context("Failed to parse project.json")?;

    let mut container = seqterm_stz::from_core(&project);
    container.take_snapshot("packed".to_string(), json);

    // Walk the directory and register any audio files as assets.
    let audio_exts = ["wav", "flac", "mp3", "ogg", "aif", "aiff"];
    for entry in std::fs::read_dir(dir)?.flatten() {
        let p = entry.path();
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if audio_exts.contains(&ext.as_str()) {
            if let Ok(data) = std::fs::read(&p) {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("audio");
                let ae = seqterm_stz::make_asset_entry(&data, name, seqterm_stz::AssetType::AudioSample);
                container.asset_data.insert(ae.uuid, data);
                container.asset_registry.assets.push(ae);
                println!("  packed asset: {}", name);
            }
        }
    }

    seqterm_stz::save(&container, &out_path)
        .with_context(|| format!("Cannot save {}", out_path.display()))?;

    println!("Packed → {}", out_path.display());
    Ok(())
}

// ─── migrate ──────────────────────────────────────────────────────────────────

fn cmd_migrate(path: &Path) -> Result<()> {
    let mut c = seqterm_stz::load(path)
        .with_context(|| format!("Cannot load {}", path.display()))?;

    let before = c.manifest.format_version;
    let current = seqterm_stz::STZ_FORMAT_VERSION;

    if before >= current {
        println!("Already at latest schema version v{}. Nothing to migrate.", before);
        return Ok(());
    }

    // Bump version and re-save (seqterm-stz migrator auto-applies on load).
    c.manifest.format_version = current;
    seqterm_stz::save(&c, path)
        .with_context(|| format!("Cannot save {}", path.display()))?;

    println!("Migrated v{} → v{}: {}", before, current, path.display());
    Ok(())
}

// ─── validate ─────────────────────────────────────────────────────────────────

fn cmd_validate(path: &Path) -> Result<()> {
    let c = seqterm_stz::load(path)
        .with_context(|| format!("Cannot load {}", path.display()))?;

    let validator = seqterm_stz::DefaultValidator;
    use seqterm_stz::ProjectValidatorPort;
    let mut issues: Vec<String> = Vec::new();

    if let Err(e) = validator.validate_manifest(&c.manifest) {
        issues.push(format!("manifest: {e}"));
    }
    if let Err(e) = validator.validate_container(&c) {
        issues.push(format!("container: {e}"));
    }

    // Hash check: verify each asset's stored hash matches computed hash.
    for entry in &c.asset_registry.assets {
        if let Some(data) = c.asset_data.get(&entry.uuid) {
            let computed = seqterm_stz::sha256_hex(data);
            if !entry.hash.is_empty() && computed != entry.hash {
                issues.push(format!("asset {} hash mismatch (stored={}, actual={})",
                    entry.path, &entry.hash[..8], &computed[..8]));
            }
        }
    }

    if issues.is_empty() {
        println!("✓  {} — valid ({} assets, {} snapshots)",
            path.display(),
            c.asset_registry.assets.len(),
            c.list_snapshots().len());
        Ok(())
    } else {
        eprintln!("✗  {} — {} issue(s):", path.display(), issues.len());
        for issue in &issues {
            eprintln!("   • {issue}");
        }
        anyhow::bail!("Validation failed with {} issue(s)", issues.len())
    }
}

// ─── diff ─────────────────────────────────────────────────────────────────────

fn cmd_diff(path_a: &Path, path_b: &Path) -> Result<()> {
    let a = seqterm_stz::load(path_a)
        .with_context(|| format!("Cannot load {}", path_a.display()))?;
    let b = seqterm_stz::load(path_b)
        .with_context(|| format!("Cannot load {}", path_b.display()))?;

    let ra = a.build_object_registry();
    let rb = b.build_object_registry();

    // Compare asset UUIDs.
    let uuids_a: std::collections::HashSet<_> = a.asset_registry.assets.iter().map(|e| e.uuid).collect();
    let uuids_b: std::collections::HashSet<_> = b.asset_registry.assets.iter().map(|e| e.uuid).collect();

    let added:   Vec<_> = uuids_b.difference(&uuids_a).collect();
    let removed: Vec<_> = uuids_a.difference(&uuids_b).collect();
    let changed: Vec<_> = uuids_a.intersection(&uuids_b).filter(|&&id| {
        let ha = a.asset_registry.assets.iter().find(|e| e.uuid == id).map(|e| &e.hash);
        let hb = b.asset_registry.assets.iter().find(|e| e.uuid == id).map(|e| &e.hash);
        ha != hb
    }).collect();

    // Compare snapshot counts.
    let snaps_a = a.list_snapshots().len();
    let snaps_b = b.list_snapshots().len();

    println!("=== DIFF: {} ↔ {}", path_a.display(), path_b.display());
    println!("  Tracks:    {} → {}", ra.tracks.len(), rb.tracks.len());
    println!("  Patterns:  {} → {}", ra.patterns.len(), rb.patterns.len());
    println!("  Channels:  {} → {}", ra.mixer_channels.len(), rb.mixer_channels.len());
    println!("  Snapshots: {} → {}", snaps_a, snaps_b);
    println!("  Assets added:   {}", added.len());
    for id in &added {
        let name = b.asset_registry.assets.iter().find(|e| &e.uuid == *id).map(|e| e.path.as_str()).unwrap_or("?");
        println!("    + {id} {name}");
    }
    println!("  Assets removed: {}", removed.len());
    for id in &removed {
        let name = a.asset_registry.assets.iter().find(|e| &e.uuid == *id).map(|e| e.path.as_str()).unwrap_or("?");
        println!("    - {id} {name}");
    }
    println!("  Assets changed: {}", changed.len());
    for id in &changed {
        let name = a.asset_registry.assets.iter().find(|e| &e.uuid == *id).map(|e| e.path.as_str()).unwrap_or("?");
        println!("    ~ {id} {name}");
    }

    Ok(())
}

// ─── snapshot ─────────────────────────────────────────────────────────────────

fn cmd_snapshot(path: &Path) -> Result<()> {
    let c = seqterm_stz::load(path)
        .with_context(|| format!("Cannot load {}", path.display()))?;

    let snaps = c.list_snapshots();
    if snaps.is_empty() {
        println!("No snapshots in {}", path.display());
        return Ok(());
    }
    println!("=== SNAPSHOTS ({}) in {} ===", snaps.len(), path.display());
    for s in snaps {
        let size = c.snapshot_data(s.id).map(|d| d.len()).unwrap_or(0);
        println!("  [{}]  {:20}  {}  ({} bytes)", s.id, s.name, s.created_at, size);
    }
    Ok(())
}
