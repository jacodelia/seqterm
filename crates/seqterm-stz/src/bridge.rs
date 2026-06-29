/// Bridge: bidirectional conversion between `seqterm_core::Project` and `StzContainer`.
use seqterm_core::{Channel, Pattern, Project};
use uuid::Uuid;

use crate::domain::{
    ChainRef, StzAutomationLane, StzBus, StzContainer, StzFxSlot, StzMixerChannel, StzNote,
    StzPattern, StzPatternSource, StzTrack,
};

// ─── core::Project → StzContainer ────────────────────────────────────────────

pub fn from_core(project: &Project) -> StzContainer {
    let mut container = StzContainer::new(&project.name, project.bpm);

    // ── patterns ──────────────────────────────────────────────────────────────
    let mut pattern_uuid_map: std::collections::HashMap<String, Uuid> =
        std::collections::HashMap::new();

    for (name, pat) in &project.patterns {
        let stz_pat = core_pattern_to_stz(name, pat);
        pattern_uuid_map.insert(name.clone(), stz_pat.id);
        container.project.patterns.push(stz_pat.id);
        container.patterns.push(stz_pat);
    }

    // ── channels (mixer) ─────────────────────────────────────────────────────
    for ch in &project.channels {
        let stz_ch = core_channel_to_stz(ch);
        container.project.mixer_channels.push(stz_ch.id);
        container.mixer_channels.push(stz_ch);
    }

    // ── buses ─────────────────────────────────────────────────────────────────
    for bus in &project.buses {
        let stz_bus = StzBus {
            id: Uuid::new_v4(),
            version: 1,
            name: bus.name.clone(),
            volume_db: bus.volume,
            muted: bus.muted,
            fx_chain: Vec::new(),
        };
        container.project.buses.push(stz_bus.id);
        container.buses.push(stz_bus);
    }

    // ── arranger tracks ───────────────────────────────────────────────────────
    for core_track in &project.tracks {
        let mut stz_track = StzTrack::new(&core_track.name);
        stz_track.mute = core_track.mute;
        for (start, len, pat_label) in &core_track.blocks {
            if let Some(&pat_uuid) = pattern_uuid_map.get(pat_label) {
                stz_track.pattern_ids.push(pat_uuid);
                stz_track.blocks.push(crate::domain::TrackBlock {
                    start_bar: *start,
                    length_bars: *len,
                    pattern_id: pat_uuid,
                });
            }
        }
        container.project.tracks.push(stz_track.id);
        container.tracks.push(stz_track);
    }

    // ── automation ────────────────────────────────────────────────────────────
    for lane in &project.automation {
        let mut stz_lane = StzAutomationLane::new(&lane.name, &lane.target);
        stz_lane.enabled = lane.enabled;
        for (bar, val) in &lane.points {
            stz_lane.points.push(crate::domain::AutomationPoint {
                bar: *bar as f64,
                value: *val as f64,
                interpolation: crate::domain::InterpolationMode::Linear,
            });
        }
        container.project.automation.push(stz_lane.id);
        container.automation.push(stz_lane);
    }

    // ── song chain ────────────────────────────────────────────────────────────
    for entry in &project.chain {
        if let Some(scene) = project.scenes.get(entry.scene_idx) {
            container.project.chain.push(ChainRef {
                scene_name: scene.name.clone(),
                bars: entry.bars,
            });
        }
    }

    // ── transport / timeline bpm ──────────────────────────────────────────────
    container.transport.bpm = project.bpm;
    container.timeline.tempo_map.events.clear();
    container
        .timeline
        .tempo_map
        .events
        .push(crate::domain::TempoEvent { bar: 0, bpm: project.bpm });

    // ── hosted-plugin state blobs ────────────────────────────────────────────
    // Each clip's opaque plugin state (CLAP `state` / VST2 chunk) is stored as a
    // PluginState asset under `plugins/state/{clip_key}.state`.
    for (clip_key, blob) in &project.plugin_state {
        if !blob.is_empty() {
            container.set_plugin_state(clip_key, blob.clone());
        }
    }

    container.object_registry = container.build_object_registry();
    container.manifest.project_name = project.name.clone();

    // Pack referenced SF2 / audio-clip files INTO the archive so the project is
    // self-contained and portable across machines.
    embed_assets(&mut container, project);

    // Embed the authoritative, lossless core project so a save→load round-trip
    // restores the exact in-app state (the structured files above can't represent
    // everything: matrix clips/sources, channel FX/sends/EQ, scenes, routing…).
    container.core_project_json = serde_json::to_vec_pretty(project).ok();
    container
}

/// Read each matrix clip's SF2 / audio-file source and store its bytes in the
/// container (one entry per distinct source path). `save` writes them into the zip
/// under `assets/soundfonts/*` and `audio/samples/*`. `original_name` keeps the
/// original path so [`hydrate_assets`] can repoint sources after extraction.
fn embed_assets(container: &mut StzContainer, project: &Project) {
    use seqterm_core::PatternSource;
    use crate::domain::AssetType;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for slots in project.matrix.values() {
        for clip in slots.iter().flatten() {
            let (path, atype) = match &clip.source {
                PatternSource::Sf2 { path, .. } => (path.clone(), AssetType::Sf2),
                PatternSource::AudioFile { path, .. } => (path.clone(), AssetType::AudioSample),
                _ => continue,
            };
            let key = path.to_string_lossy().to_string();
            if key.is_empty() || !seen.insert(key.clone()) { continue; }
            let data = match std::fs::read(&path) {
                Ok(d) => d,
                Err(_) => continue, // file missing on disk — skip (nothing to pack)
            };
            let mut entry = crate::stz::make_asset_entry(&data, &key, atype);
            entry.original_name = key; // full original path, for repointing on load
            let uuid = entry.uuid;
            container.asset_registry.add(entry);
            container.asset_data.insert(uuid, data);
        }
    }
}

/// Extract packed assets to `dest_dir` and repoint the project's SF2 / audio-file
/// sources to the extracted files **when the original path no longer exists** (i.e.
/// the project was moved to another machine). Sources whose original file is still
/// present are left untouched. Returns the number of sources repointed.
pub fn hydrate_assets(
    project: &mut Project,
    container: &StzContainer,
    dest_dir: &std::path::Path,
) -> usize {
    use seqterm_core::PatternSource;
    use crate::domain::AssetType;
    // Map original source path → extracted file path.
    let mut map: std::collections::HashMap<String, std::path::PathBuf> =
        std::collections::HashMap::new();
    for entry in &container.asset_registry.assets {
        if !matches!(entry.asset_type, AssetType::Sf2 | AssetType::AudioSample) {
            continue;
        }
        let Some(data) = container.asset_data.get(&entry.uuid) else { continue };
        let ext = std::path::Path::new(&entry.path)
            .extension().and_then(|e| e.to_str()).unwrap_or("bin");
        let out = dest_dir.join(format!("{}.{}", entry.uuid, ext));
        if !out.exists() {
            let _ = std::fs::create_dir_all(dest_dir);
            if std::fs::write(&out, data).is_err() { continue; }
        }
        map.insert(entry.original_name.clone(), out);
    }
    if map.is_empty() { return 0; }

    let mut repointed = 0;
    for slots in project.matrix.values_mut() {
        for clip in slots.iter_mut().flatten() {
            let path = match &mut clip.source {
                PatternSource::Sf2 { path, .. } => path,
                PatternSource::AudioFile { path, .. } => path,
                _ => continue,
            };
            if path.exists() { continue; } // local file present — keep it
            let key = path.to_string_lossy().to_string();
            if let Some(extracted) = map.get(&key) {
                *path = extracted.clone();
                repointed += 1;
            }
        }
    }
    repointed
}

fn core_pattern_to_stz(name: &str, pat: &Pattern) -> StzPattern {
    let mut stz = StzPattern::new(name, pat.steps.len() as u32);
    // Pattern.source is on Clip, not Pattern — default to Midi.
    stz.source = StzPatternSource::Midi;
    stz.resolution_den = pat.resolution.den() as u32;
    for (step, note) in pat.steps.iter().enumerate() {
        if !note.is_empty() {
            stz.notes.push(StzNote {
                step: step as u32,
                note: note.note.clone(),
                velocity: note.velocity,
                prob: note.prob,
                gate: note.gate,
                micro: note.micro,
            });
        }
    }
    stz
}

fn core_channel_to_stz(ch: &Channel) -> StzMixerChannel {
    StzMixerChannel {
        id: Uuid::new_v4(),
        version: 1,
        name: ch.name.clone(),
        volume_db: ch.volume,
        pan: ch.pan.to_val() as f32 / 50.0,
        mute: ch.mute,
        solo: ch.solo,
        fx_chain: ch
            .fx
            .iter()
            .map(|slot| StzFxSlot {
                fx_type: slot.kind.label().to_string(),
                enabled: slot.enabled,
                params: std::collections::HashMap::new(),
            })
            .collect(),
        sends: Vec::new(),
    }
}

// ─── StzContainer → core::Project ────────────────────────────────────────────

/// Recover the exact core `Project` from a container written by this app, using the
/// embedded `project/seqterm-core.json`. Falls back to the lossy structured
/// [`to_core`] reconstruction for foreign STZ files that lack it.
pub fn load_core(container: &StzContainer) -> Project {
    if let Some(bytes) = &container.core_project_json {
        if let Ok(p) = serde_json::from_slice::<Project>(bytes) {
            return p;
        }
    }
    to_core(container)
}

pub fn to_core(container: &StzContainer) -> Project {
    let mut project = Project::blank(&container.project.name);
    project.bpm = container.project.bpm;

    // ── patterns ──────────────────────────────────────────────────────────────
    for stz_pat in &container.patterns {
        let mut pat = Pattern::new(&stz_pat.name, stz_pat.steps as usize);
        pat.resolution = seqterm_core::Resolution::Whole(stz_pat.resolution_den.max(1) as i64);
        for stz_note in &stz_pat.notes {
            if (stz_note.step as usize) < pat.steps.len() {
                pat.steps[stz_note.step as usize] = seqterm_core::Note {
                    note: stz_note.note.clone(),
                    velocity: stz_note.velocity,
                    prob: stz_note.prob,
                    gate: stz_note.gate,
                    micro: stz_note.micro,
                    ..Default::default()
                };
            }
        }
        project.patterns.insert(stz_pat.name.clone(), pat);
    }

    // ── channels ──────────────────────────────────────────────────────────────
    for stz_ch in &container.mixer_channels {
        let pan_val = (stz_ch.pan * 50.0).round() as i8;
        let ch = Channel {
            name: stz_ch.name.clone(),
            volume: stz_ch.volume_db,
            pan: seqterm_core::channel::Pan::from_val(pan_val),
            mute: stz_ch.mute,
            solo: stz_ch.solo,
            ..Default::default()
        };
        project.channels.push(ch);
    }

    // ── buses ─────────────────────────────────────────────────────────────────
    for stz_bus in &container.buses {
        project.buses.push(seqterm_core::AudioBus {
            name: stz_bus.name.clone(),
            volume: stz_bus.volume_db,
            muted: stz_bus.muted,
        });
    }

    // ── arranger tracks ───────────────────────────────────────────────────────
    for stz_track in &container.tracks {
        let mut track = seqterm_core::project::Track::new(&stz_track.name);
        track.mute = stz_track.mute;
        for block in &stz_track.blocks {
            let pat_label = container
                .patterns
                .iter()
                .find(|p| p.id == block.pattern_id)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            track.blocks.push((block.start_bar, block.length_bars, pat_label));
        }
        project.tracks.push(track);
    }

    // ── automation ────────────────────────────────────────────────────────────
    for stz_lane in &container.automation {
        let mut lane =
            seqterm_core::project::AutomationLane::new(&stz_lane.name, &stz_lane.target);
        lane.enabled = stz_lane.enabled;
        for pt in &stz_lane.points {
            lane.points.push((pt.bar as u32, pt.value as u8));
        }
        project.automation.push(lane);
    }

    // ── hosted-plugin state blobs (plugins/state/{clip_key}.state) ────────────
    for entry in &container.asset_registry.assets {
        if entry.asset_type == crate::domain::AssetType::PluginState {
            if let Some(data) = container.asset_data.get(&entry.uuid) {
                project.plugin_state.insert(entry.original_name.clone(), data.clone());
            }
        }
    }

    project
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use seqterm_core::{Note, Pattern, Project};

    fn sample_core_project() -> Project {
        let mut proj = Project::blank("BridgeTest");
        proj.bpm = 140.0;

        let mut pat = Pattern::new("KICK", 16);
        pat.set_step(0, Note::from_midi(36, 100).unwrap());
        pat.set_step(4, Note::from_midi(36, 80).unwrap());
        proj.patterns.insert("KICK".into(), pat);

        proj
    }

    #[test]
    fn core_to_stz_preserves_name_bpm() {
        let core = sample_core_project();
        let stz = from_core(&core);
        assert_eq!(stz.project.name, core.name);
        assert!((stz.project.bpm - core.bpm).abs() < 1e-9);
    }

    #[test]
    fn core_to_stz_preserves_pattern_count() {
        let core = sample_core_project();
        let stz = from_core(&core);
        assert_eq!(stz.patterns.len(), core.patterns.len());
    }

    #[test]
    fn load_core_is_lossless_for_matrix() {
        // The structured `to_core` drops the matrix; the embedded core JSON must
        // restore it exactly, so `load_core` round-trips a clip assignment.
        let mut core = sample_core_project();
        let clip = seqterm_core::Clip::new("KICK", 0, 0).with_pattern("KICK");
        if let Some(row) = core.matrix.get_mut("A") {
            row[0] = Some(clip);
        }
        let container = from_core(&core);
        // to_core loses the matrix…
        assert!(to_core(&container).matrix.get("A").unwrap()[0].is_none());
        // …but load_core restores it from project/seqterm-core.json.
        let restored = load_core(&container);
        assert!(restored.matrix.get("A").unwrap()[0].is_some());
        assert_eq!(
            restored.matrix.get("A").unwrap()[0].as_ref().unwrap().pattern_key.as_deref(),
            Some("KICK")
        );
    }

    #[test]
    fn audio_asset_packs_and_hydrates_when_original_missing() {
        use seqterm_core::{Clip, PatternSource};
        let dir = tempfile::tempdir().unwrap();
        // Original sample file the project references.
        let orig = dir.path().join("kick.wav");
        std::fs::write(&orig, b"RIFF....fake wav bytes").unwrap();

        let mut core = sample_core_project();
        let mut clip = Clip::new("KICK", 0, 0).with_pattern("KICK");
        clip.source = PatternSource::AudioFile {
            path: orig.clone(), looping: false, original_bpm: 0.0, gain: 1.0,
        };
        core.matrix.get_mut("A").unwrap()[0] = Some(clip);

        // Pack into the container (reads the file bytes).
        let container = from_core(&core);
        assert!(container.asset_registry.assets.iter().any(|a| a.original_name == orig.to_string_lossy()));

        // Simulate moving to another machine: original file gone.
        std::fs::remove_file(&orig).unwrap();
        let mut restored = load_core(&container);
        let extract_dir = dir.path().join("extracted");
        let n = hydrate_assets(&mut restored, &container, &extract_dir);
        assert_eq!(n, 1, "one source should be repointed");

        let new_path = match &restored.matrix.get("A").unwrap()[0].as_ref().unwrap().source {
            PatternSource::AudioFile { path, .. } => path.clone(),
            _ => panic!("expected AudioFile"),
        };
        assert!(new_path.exists(), "extracted asset should exist on disk");
        assert!(new_path.starts_with(&extract_dir));
        assert_eq!(std::fs::read(&new_path).unwrap(), b"RIFF....fake wav bytes");
    }

    #[test]
    fn plugin_state_round_trips_through_container() {
        let mut core = sample_core_project();
        core.plugin_state.insert("A0".into(), vec![1, 2, 3, 4]);
        core.plugin_state.insert("C5".into(), vec![9, 8, 7]);

        // from_core writes PluginState assets under plugins/state/{clip_key}.state;
        // to_core reads them back keyed by clip_key (original_name).
        let container = from_core(&core);
        let pstate: Vec<_> = container.asset_registry.assets.iter()
            .filter(|a| a.asset_type == crate::domain::AssetType::PluginState).collect();
        assert_eq!(pstate.len(), 2, "two PluginState assets written");
        assert!(pstate.iter().all(|a| a.path.starts_with("plugins/state/")));

        let restored = to_core(&container);
        assert_eq!(restored.plugin_state.get("A0"), Some(&vec![1, 2, 3, 4]));
        assert_eq!(restored.plugin_state.get("C5"), Some(&vec![9, 8, 7]));
    }

    #[test]
    fn core_to_stz_to_core_roundtrip() {
        let core_orig = sample_core_project();
        let stz = from_core(&core_orig);
        let core_back = to_core(&stz);
        assert_eq!(core_back.name, core_orig.name);
        assert!((core_back.bpm - core_orig.bpm).abs() < 1e-9);
        assert!(core_back.patterns.contains_key("KICK"));
        let pat = &core_back.patterns["KICK"];
        assert_eq!(pat.steps[0].to_midi(), Some(36));
        assert_eq!(pat.steps[4].to_midi(), Some(36));
    }

    #[test]
    fn pattern_resolution_round_trips_through_stz() {
        let mut core = sample_core_project();
        core.patterns.get_mut("KICK").unwrap().resolution =
            seqterm_core::Resolution::Whole(12);
        let stz = from_core(&core);
        let kick = stz.patterns.iter().find(|p| p.name == "KICK").unwrap();
        assert_eq!(kick.resolution_den, 12);
        let back = to_core(&stz);
        assert_eq!(
            back.patterns["KICK"].resolution,
            seqterm_core::Resolution::Whole(12)
        );
    }

    #[test]
    fn stz_pattern_note_count() {
        let core = sample_core_project();
        let stz = from_core(&core);
        let kick = stz.patterns.iter().find(|p| p.name == "KICK").unwrap();
        assert_eq!(kick.notes.len(), 2, "two active steps in KICK");
    }
}
