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
    container
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
