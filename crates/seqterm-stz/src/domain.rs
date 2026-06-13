use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::registry::{AssetRegistry, ObjectRegistry};

pub const STZ_FORMAT: &str = "STZ";
pub const STZ_FORMAT_VERSION: u32 = 2;
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

// ─── Manifest ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub project_uuid: Uuid,
    pub project_name: String,
    pub engine_version: String,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub root_project: String,
}

impl Manifest {
    pub fn new(project_uuid: Uuid, project_name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            format: STZ_FORMAT.to_string(),
            format_version: STZ_FORMAT_VERSION,
            project_uuid,
            project_name: project_name.into(),
            engine_version: ENGINE_VERSION.to_string(),
            created_at: now,
            modified_at: now,
            root_project: "project/project.json".to_string(),
        }
    }

    pub fn touch(&mut self) {
        self.modified_at = Utc::now();
    }
}

// ─── Project ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzProject {
    pub id: Uuid,
    pub version: u32,
    pub name: String,
    pub bpm: f64,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub tracks: Vec<Uuid>,
    pub patterns: Vec<Uuid>,
    pub mixer_channels: Vec<Uuid>,
    pub buses: Vec<Uuid>,
    pub plugins: Vec<Uuid>,
    pub automation: Vec<Uuid>,
    pub transport: Uuid,
    pub timeline: Uuid,
    pub routing: Uuid,
    #[serde(default)]
    pub chain: Vec<ChainRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainRef {
    pub scene_name: String,
    pub bars: u32,
}

// ─── Track ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzTrack {
    pub id: Uuid,
    pub version: u32,
    pub name: String,
    pub mute: bool,
    pub solo: bool,
    pub pattern_ids: Vec<Uuid>,
    pub blocks: Vec<TrackBlock>,
    pub mixer_channel_id: Option<Uuid>,
}

impl StzTrack {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            name: name.into(),
            mute: false,
            solo: false,
            pattern_ids: Vec::new(),
            blocks: Vec::new(),
            mixer_channel_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackBlock {
    pub start_bar: u32,
    pub length_bars: u32,
    pub pattern_id: Uuid,
}

// ─── Pattern ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzPattern {
    pub id: Uuid,
    pub version: u32,
    pub name: String,
    pub steps: u32,
    pub notes: Vec<StzNote>,
    pub source: StzPatternSource,
    /// Grid resolution as `1/resolution_den` of a whole note (Phase 2 rational
    /// time). Defaults to `16` (a 1/16 grid) so older `.stz` files round-trip
    /// with unchanged timing.
    #[serde(default = "default_resolution_den")]
    pub resolution_den: u32,
}

fn default_resolution_den() -> u32 {
    16
}

impl StzPattern {
    pub fn new(name: impl Into<String>, steps: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            name: name.into(),
            steps,
            notes: Vec::new(),
            source: StzPatternSource::Midi,
            resolution_den: default_resolution_den(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzNote {
    pub step: u32,
    pub note: String,
    pub velocity: u8,
    /// Trigger probability 0-100.
    pub prob: u8,
    /// Gate time 0-400 (100 = 100% of step length).
    pub gate: u16,
    /// Microtiming offset -99..+99.
    pub micro: i8,
}

impl StzNote {
    pub fn new(step: u32, note: impl Into<String>, velocity: u8) -> Self {
        Self {
            step,
            note: note.into(),
            velocity,
            prob: 100,
            gate: 100,
            micro: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StzPatternSource {
    #[default]
    Midi,
    Sf2 { path: String, bank: u32, preset: u8 },
    AudioFile { path: String },
}

// ─── MIDI clip ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzMidiClip {
    pub id: Uuid,
    pub version: u32,
    pub name: String,
    pub asset_id: Option<Uuid>,
    pub notes: Vec<MidiNote>,
    pub length_bars: f64,
    pub loop_enabled: bool,
}

impl StzMidiClip {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            name: name.into(),
            asset_id: None,
            notes: Vec::new(),
            length_bars: 1.0,
            loop_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiNote {
    pub pitch: u8,
    pub velocity: u8,
    pub start_ticks: u32,
    pub duration_ticks: u32,
    pub channel: u8,
}

// ─── Audio clip ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzAudioClip {
    pub id: Uuid,
    pub version: u32,
    pub name: String,
    pub asset_id: Uuid,
    pub trim_start: f64,
    pub trim_end: f64,
    pub pitch_semitones: f32,
    pub reverse: bool,
    pub normalize: bool,
    pub loop_enabled: bool,
    pub loop_start: f64,
    pub loop_end: f64,
}

impl StzAudioClip {
    pub fn new(name: impl Into<String>, asset_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            name: name.into(),
            asset_id,
            trim_start: 0.0,
            trim_end: 1.0,
            pitch_semitones: 0.0,
            reverse: false,
            normalize: false,
            loop_enabled: false,
            loop_start: 0.0,
            loop_end: 1.0,
        }
    }
}

// ─── Mixer channel ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzMixerChannel {
    pub id: Uuid,
    pub version: u32,
    pub name: String,
    pub volume_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub fx_chain: Vec<StzFxSlot>,
    pub sends: Vec<StzSend>,
}

impl StzMixerChannel {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            name: name.into(),
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            fx_chain: Vec::new(),
            sends: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzFxSlot {
    pub fx_type: String,
    pub enabled: bool,
    pub params: HashMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzSend {
    pub bus_id: Uuid,
    pub level_db: f32,
    pub enabled: bool,
}

// ─── Bus ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzBus {
    pub id: Uuid,
    pub version: u32,
    pub name: String,
    pub volume_db: f32,
    pub muted: bool,
    pub fx_chain: Vec<StzFxSlot>,
}

impl StzBus {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            name: name.into(),
            volume_db: -6.0,
            muted: false,
            fx_chain: Vec::new(),
        }
    }
}

// ─── Automation ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzAutomationLane {
    pub id: Uuid,
    pub version: u32,
    pub name: String,
    pub target: String,
    pub target_id: Option<Uuid>,
    pub points: Vec<AutomationPoint>,
    pub enabled: bool,
}

impl StzAutomationLane {
    pub fn new(name: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            name: name.into(),
            target: target.into(),
            target_id: None,
            points: Vec::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationPoint {
    pub bar: f64,
    pub value: f64,
    pub interpolation: InterpolationMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum InterpolationMode {
    #[default]
    Linear,
    Step,
    Cubic,
}

// ─── Plugin ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzPluginInstance {
    pub id: Uuid,
    pub version: u32,
    pub name: String,
    pub plugin_id: String,
    pub format: PluginFormat,
    pub state_path: Option<String>,
    pub params: HashMap<String, f64>,
    pub enabled: bool,
}

impl StzPluginInstance {
    pub fn new(
        name: impl Into<String>,
        plugin_id: impl Into<String>,
        format: PluginFormat,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            name: name.into(),
            plugin_id: plugin_id.into(),
            format,
            state_path: None,
            params: HashMap::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginFormat {
    Internal,
    Vst2,
    Vst3,
    Clap,
    Au,
}

// ─── Transport ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzTransport {
    pub id: Uuid,
    pub version: u32,
    pub bpm: f64,
    pub time_sig_num: u8,
    pub time_sig_den: u8,
    pub loop_enabled: bool,
    pub loop_start_bar: u32,
    pub loop_end_bar: u32,
    pub sync_mode: StzSyncMode,
}

impl StzTransport {
    pub fn new(bpm: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            bpm,
            time_sig_num: 4,
            time_sig_den: 4,
            loop_enabled: false,
            loop_start_bar: 0,
            loop_end_bar: 8,
            sync_mode: StzSyncMode::Internal,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum StzSyncMode {
    #[default]
    Internal,
    MidiClock,
    Jack,
}

// ─── Timeline ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzTimeline {
    pub id: Uuid,
    pub version: u32,
    pub length_bars: u32,
    pub tempo_map: StzTempoMap,
}

impl StzTimeline {
    pub fn new(bpm: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            length_bars: 128,
            tempo_map: StzTempoMap::new(bpm),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzTempoMap {
    pub events: Vec<TempoEvent>,
    pub time_signatures: Vec<TimeSignatureEvent>,
}

impl StzTempoMap {
    pub fn new(bpm: f64) -> Self {
        Self {
            events: vec![TempoEvent { bar: 0, bpm }],
            time_signatures: vec![TimeSignatureEvent { bar: 0, numerator: 4, denominator: 4 }],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoEvent {
    pub bar: u32,
    pub bpm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSignatureEvent {
    pub bar: u32,
    pub numerator: u8,
    pub denominator: u8,
}

// ─── Routing graph ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzRoutingGraph {
    pub id: Uuid,
    pub version: u32,
    pub nodes: Vec<StzRoutingNode>,
    pub edges: Vec<StzRoutingEdge>,
}

impl StzRoutingGraph {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Detect cycles using DFS; returns Err if a cycle is found.
    pub fn validate_acyclic(&self) -> crate::error::StzResult<()> {
        use std::collections::HashSet;

        fn dfs(
            node: Uuid,
            edges: &[StzRoutingEdge],
            visited: &mut HashSet<Uuid>,
            stack: &mut HashSet<Uuid>,
        ) -> bool {
            visited.insert(node);
            stack.insert(node);
            for edge in edges.iter().filter(|e| e.from == node) {
                if !visited.contains(&edge.to) {
                    if dfs(edge.to, edges, visited, stack) {
                        return true;
                    }
                } else if stack.contains(&edge.to) {
                    return true;
                }
            }
            stack.remove(&node);
            false
        }

        let mut visited = std::collections::HashSet::new();
        let mut stack = std::collections::HashSet::new();
        for node in &self.nodes {
            if !visited.contains(&node.id) && dfs(node.id, &self.edges, &mut visited, &mut stack) {
                return Err(crate::error::StzError::InvalidRoutingGraph(
                    "cycle detected".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzRoutingNode {
    pub id: Uuid,
    pub kind: String,
    pub label: String,
    pub target_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StzRoutingEdge {
    pub from: Uuid,
    pub to: Uuid,
    pub kind: String,
}

// ─── Assets ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetType {
    AudioSample,
    AudioRecording,
    AudioStem,
    AudioRendered,
    MidiImported,
    MidiGenerated,
    MidiExported,
    PluginState,
    /// Lua script stored as `scripts/{name}.lua`.
    Script,
}

impl AssetType {
    pub fn directory(&self) -> &'static str {
        match self {
            Self::AudioSample => "audio/samples",
            Self::AudioRecording => "audio/recordings",
            Self::AudioStem => "audio/stems",
            Self::AudioRendered => "audio/rendered",
            Self::MidiImported => "midi/imported",
            Self::MidiGenerated => "midi/generated",
            Self::MidiExported => "midi/exported",
            Self::PluginState => "plugins/state",
            Self::Script => "scripts",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetEntry {
    pub uuid: Uuid,
    #[serde(rename = "type")]
    pub asset_type: AssetType,
    pub path: String,
    pub hash: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    pub original_name: String,
}

// ─── Snapshot ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSnapshot {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub description: String,
    pub manifest: Manifest,
}

impl ProjectSnapshot {
    pub fn new(name: impl Into<String>, manifest: Manifest) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            created_at: Utc::now(),
            description: String::new(),
            manifest,
        }
    }
}

// ─── Container ────────────────────────────────────────────────────────────────

/// Complete in-memory representation of a `.stz` archive.
#[derive(Debug, Clone)]
pub struct StzContainer {
    pub manifest: Manifest,
    pub project: StzProject,
    pub tracks: Vec<StzTrack>,
    pub patterns: Vec<StzPattern>,
    pub mixer_channels: Vec<StzMixerChannel>,
    pub buses: Vec<StzBus>,
    pub automation: Vec<StzAutomationLane>,
    pub plugins: Vec<StzPluginInstance>,
    pub transport: StzTransport,
    pub timeline: StzTimeline,
    pub routing: StzRoutingGraph,
    pub asset_registry: AssetRegistry,
    pub object_registry: ObjectRegistry,
    /// Raw asset bytes loaded into memory (UUID → bytes).
    #[allow(dead_code)]
    pub asset_data: HashMap<Uuid, Vec<u8>>,
    /// Snapshots stored in this container: (metadata, serialized project JSON bytes).
    pub snapshots: Vec<(ProjectSnapshot, Vec<u8>)>,
    /// UUIDs of objects that have changed since the last save.
    /// Used by incremental save to avoid rewriting unchanged objects.
    pub dirty_objects: std::collections::HashSet<uuid::Uuid>,
}

impl StzContainer {
    pub fn new(name: impl Into<String>, bpm: f64) -> Self {
        let name = name.into();
        let transport = StzTransport::new(bpm);
        let timeline = StzTimeline::new(bpm);
        let routing = StzRoutingGraph::new();
        let now = Utc::now();
        let project = StzProject {
            id: Uuid::new_v4(),
            version: 1,
            name: name.clone(),
            bpm,
            created_at: now,
            modified_at: now,
            tracks: Vec::new(),
            patterns: Vec::new(),
            mixer_channels: Vec::new(),
            buses: Vec::new(),
            plugins: Vec::new(),
            automation: Vec::new(),
            transport: transport.id,
            timeline: timeline.id,
            routing: routing.id,
            chain: Vec::new(),
        };
        let manifest = Manifest::new(project.id, &name);
        Self {
            manifest,
            project,
            tracks: Vec::new(),
            patterns: Vec::new(),
            mixer_channels: Vec::new(),
            buses: Vec::new(),
            automation: Vec::new(),
            plugins: Vec::new(),
            transport,
            timeline,
            routing,
            asset_registry: AssetRegistry::new(),
            object_registry: ObjectRegistry::new(),
            asset_data: HashMap::new(),
            snapshots: Vec::new(),
            dirty_objects: std::collections::HashSet::new(),
        }
    }

    /// Mark an object as dirty so it gets rewritten on the next incremental save.
    pub fn mark_dirty(&mut self, id: uuid::Uuid) {
        self.dirty_objects.insert(id);
    }

    /// Clear the dirty set after a successful save.
    pub fn clear_dirty(&mut self) {
        self.dirty_objects.clear();
    }

    /// Returns true if there are any unsaved changes.
    pub fn is_dirty(&self) -> bool {
        !self.dirty_objects.is_empty()
    }

    /// Take a snapshot of the given project, appending it to this container's snapshot list.
    /// Call `save()` afterward to persist to disk.
    pub fn take_snapshot(&mut self, name: impl Into<String>, project_json: Vec<u8>) -> &ProjectSnapshot {
        let manifest = self.manifest.clone();
        let snap = ProjectSnapshot::new(name, manifest);
        self.snapshots.push((snap, project_json));
        &self.snapshots.last().unwrap().0
    }

    /// Find a snapshot by ID and return the stored project JSON bytes, if present.
    pub fn snapshot_data(&self, id: uuid::Uuid) -> Option<&[u8]> {
        self.snapshots.iter()
            .find(|(s, _)| s.id == id)
            .map(|(_, data)| data.as_slice())
    }

    /// List snapshot metadata (most-recent first).
    pub fn list_snapshots(&self) -> Vec<&ProjectSnapshot> {
        let mut snaps: Vec<&ProjectSnapshot> = self.snapshots.iter().map(|(s, _)| s).collect();
        snaps.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        snaps
    }

    /// Remove a snapshot by ID. Returns true if found and removed.
    pub fn remove_snapshot(&mut self, id: uuid::Uuid) -> bool {
        let before = self.snapshots.len();
        self.snapshots.retain(|(s, _)| s.id != id);
        self.snapshots.len() < before
    }

    /// Store opaque plugin state bytes under `plugins/state/{plugin_id}.state`.
    /// Overwrites any previously stored state for this plugin.
    pub fn set_plugin_state(&mut self, plugin_id: &str, data: Vec<u8>) {
        // Store in asset_data under a synthetic UUID derived from the plugin_id.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        plugin_id.hash(&mut h);
        let uuid = uuid::Uuid::from_u128(h.finish() as u128);
        self.asset_data.insert(uuid, data);
        // Track in asset_registry so it gets saved.
        if !self.asset_registry.assets.iter().any(|a| a.uuid == uuid) {
            self.asset_registry.assets.push(crate::domain::AssetEntry {
                uuid,
                path: format!("plugins/state/{}.state", plugin_id.replace('/', "_")),
                asset_type: AssetType::PluginState,
                hash: String::new(),
                size_bytes: 0,
                created_at: chrono::Utc::now(),
                original_name: plugin_id.to_string(),
            });
        }
    }

    /// Retrieve plugin state bytes for the given plugin_id, if stored.
    pub fn get_plugin_state(&self, plugin_id: &str) -> Option<&[u8]> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        plugin_id.hash(&mut h);
        let uuid = uuid::Uuid::from_u128(h.finish() as u128);
        self.asset_data.get(&uuid).map(|v| v.as_slice())
    }

    // ── Lua script storage ────────────────────────────────────────────────────

    fn script_uuid(name: &str) -> Uuid {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        "script:".hash(&mut h);
        name.hash(&mut h);
        uuid::Uuid::from_u128(h.finish() as u128)
    }

    /// Store a Lua script under `scripts/{name}.lua`.
    /// Name must not contain path separators; `.lua` extension is added automatically.
    pub fn set_script(&mut self, name: &str, source: &str) {
        let safe_name = name.replace(['/', '\\', '\0'], "_");
        let uuid = Self::script_uuid(&safe_name);
        let data = source.as_bytes().to_vec();
        self.asset_data.insert(uuid, data);
        if let Some(entry) = self.asset_registry.assets.iter_mut().find(|a| a.uuid == uuid) {
            entry.size_bytes = source.len() as u64;
            entry.created_at = chrono::Utc::now();
        } else {
            self.asset_registry.assets.push(crate::domain::AssetEntry {
                uuid,
                path: format!("scripts/{safe_name}.lua"),
                asset_type: AssetType::Script,
                hash: String::new(),
                size_bytes: source.len() as u64,
                created_at: chrono::Utc::now(),
                original_name: safe_name,
            });
        }
    }

    /// Retrieve a stored Lua script by name (without `.lua` extension).
    pub fn get_script(&self, name: &str) -> Option<String> {
        let uuid = Self::script_uuid(name);
        self.asset_data.get(&uuid)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .map(|s| s.to_owned())
    }

    /// Delete a stored Lua script by name. Returns true if it existed.
    pub fn delete_script(&mut self, name: &str) -> bool {
        let uuid = Self::script_uuid(name);
        let removed = self.asset_data.remove(&uuid).is_some();
        self.asset_registry.assets.retain(|a| a.uuid != uuid);
        removed
    }

    /// List all stored Lua script names (without `.lua` extension).
    pub fn list_scripts(&self) -> Vec<String> {
        self.asset_registry.assets.iter()
            .filter(|a| matches!(a.asset_type, AssetType::Script))
            .map(|a| a.original_name.clone())
            .collect()
    }

    /// Rebuild the object registry from the current in-memory state.
    pub fn build_object_registry(&self) -> ObjectRegistry {
        ObjectRegistry {
            tracks: self.tracks.iter().map(|t| t.id).collect(),
            patterns: self.patterns.iter().map(|p| p.id).collect(),
            clips: Vec::new(),
            mixer_channels: self.mixer_channels.iter().map(|c| c.id).collect(),
            buses: self.buses.iter().map(|b| b.id).collect(),
            automation: self.automation.iter().map(|a| a.id).collect(),
            plugins: self.plugins.iter().map(|p| p.id).collect(),
            routing_graphs: vec![self.routing.id],
        }
    }

    /// Recompute project UUID references from current objects.
    pub fn sync_project_refs(&mut self) {
        self.project.tracks = self.tracks.iter().map(|t| t.id).collect();
        self.project.patterns = self.patterns.iter().map(|p| p.id).collect();
        self.project.mixer_channels = self.mixer_channels.iter().map(|c| c.id).collect();
        self.project.buses = self.buses.iter().map(|b| b.id).collect();
        self.project.plugins = self.plugins.iter().map(|p| p.id).collect();
        self.project.automation = self.automation.iter().map(|a| a.id).collect();
        self.project.transport = self.transport.id;
        self.project.timeline = self.timeline.id;
        self.project.routing = self.routing.id;
    }
}
