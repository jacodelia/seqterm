use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

pub use seqterm_routing::{RoutingEdge, RoutingGraph, RoutingNode, RoutingSnapshot};

use crate::{
    channel::{Channel, FxKind},
    note::Note,
    pad::SamplerConfig,
    pattern::{Clip, Pattern},
};

/// MIDI clock sync source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SyncMode {
    Internal,
    Usb,
    Midi,
    Clock,
}

impl SyncMode {
    pub fn label(&self) -> &'static str {
        match self {
            SyncMode::Internal => "INT",
            SyncMode::Usb => "USB",
            SyncMode::Midi => "MIDI",
            SyncMode::Clock => "CLK",
        }
    }
}

impl Default for SyncMode {
    fn default() -> Self {
        SyncMode::Internal
    }
}

/// A MIDI I/O port descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiPort {
    pub name: String,
    pub enabled: bool,
    pub channel: u8,
}

impl MidiPort {
    pub fn new(name: impl Into<String>, channel: u8) -> Self {
        Self {
            name: name.into(),
            enabled: true,
            channel,
        }
    }
}

/// An OSC route mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OscRoute {
    pub address: String,
    pub target: String,
    pub enabled: bool,
}

impl OscRoute {
    pub fn new(address: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            target: target.into(),
            enabled: true,
        }
    }
}

/// Arranger track kind — determines icon and signal-flow semantics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum TrackKind {
    #[default]
    Midi,
    Audio,
    Drum,
    Group,
    Bus,
    Auto,
}

impl TrackKind {
    pub fn short_label(self) -> &'static str {
        match self {
            TrackKind::Midi  => "MIDI",
            TrackKind::Audio => "AUDI",
            TrackKind::Drum  => "DRUM",
            TrackKind::Group => "GRP ",
            TrackKind::Bus   => "BUS ",
            TrackKind::Auto  => "AUTO",
        }
    }
    pub fn next(self) -> Self {
        match self {
            TrackKind::Midi  => TrackKind::Audio,
            TrackKind::Audio => TrackKind::Drum,
            TrackKind::Drum  => TrackKind::Group,
            TrackKind::Group => TrackKind::Bus,
            TrackKind::Bus   => TrackKind::Auto,
            TrackKind::Auto  => TrackKind::Midi,
        }
    }
}

/// An arranger track with clip placement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub name: String,
    /// Clip blocks: (start_bar, length_bars, label).
    pub blocks: Vec<(u32, u32, String)>,
    pub mute: bool,
}

impl Track {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            blocks: Vec::new(),
            mute: false,
        }
    }
}

/// An automation lane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationLane {
    pub name: String,
    pub target: String,
    /// Automation points: (bar, value 0-127).
    pub points: Vec<(u32, u8)>,
    pub enabled: bool,
}

impl AutomationLane {
    pub fn new(name: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            target: target.into(),
            points: Vec::new(),
            enabled: true,
        }
    }
}

/// A scene (snapshot of which clips are playing per row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    /// Which pattern key is active per row (8 rows).
    pub active_clips: Vec<Option<String>>,
    pub mute_mask: u8,
    pub fx_mask: u8,
}

impl Scene {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            active_clips: vec![None; 8],
            mute_mask: 0,
            fx_mask: 0,
        }
    }
}

// ─── Pattern chain ────────────────────────────────────────────────────────────

/// One entry in the song-mode pattern chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEntry {
    /// Index into `Project.scenes`.
    pub scene_idx: usize,
    /// How many bars to play this scene before advancing.
    pub bars: u32,
}

impl ChainEntry {
    pub fn new(scene_idx: usize, bars: u32) -> Self { Self { scene_idx, bars } }
}

// ─── Audio buses ─────────────────────────────────────────────────────────────

/// One of up to 8 named audio return buses (A–H).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioBus {
    /// Bus label shown in the mixer and routing editor (e.g. "Reverb", "Delay").
    pub name: String,
    /// Return volume in dBFS (-60 to +6).
    #[serde(default = "default_bus_volume")]
    pub volume: f32,
    pub muted: bool,
}

fn default_bus_volume() -> f32 { -6.0 }

impl AudioBus {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), volume: -6.0, muted: false }
    }
}

/// The top-level project / live-set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Schema version — bumped whenever the on-disk format changes incompatibly.
    /// Absence in JSON is treated as version 0 (pre-versioning).
    #[serde(default)]
    pub version: u32,
    pub name: String,
    pub bpm: f64,
    /// Session matrix: 8 rows (A-H), each with 8 optional clips.
    pub matrix: HashMap<String, Vec<Option<Clip>>>,
    /// Named patterns.
    pub patterns: HashMap<String, Pattern>,
    pub channels: Vec<Channel>,
    pub tracks: Vec<Track>,
    pub automation: Vec<AutomationLane>,
    pub scenes: Vec<Scene>,
    pub midi_inputs: Vec<MidiPort>,
    pub midi_outputs: Vec<MidiPort>,
    pub osc_routes: Vec<OscRoute>,
    pub sync_mode: SyncMode,
    /// MIDI routing graph (node-edge model).
    #[serde(default)]
    pub routing: RoutingGraph,
    /// Audio return buses (up to 8, indexed A-H). Channels send to them via `send_a`/`send_b`.
    #[serde(default)]
    pub buses: Vec<AudioBus>,
    /// Custom display names for arranger track rows (key = "A"-"P").
    #[serde(default)]
    pub track_names: HashMap<String, String>,
    /// Arranger track kind per row (key = "A"-"P").
    #[serde(default)]
    pub track_types: HashMap<String, TrackKind>,
    /// Track color palette index (0-7) per row (key = "A"-"P").
    #[serde(default)]
    pub track_colors: HashMap<String, u8>,
    /// Hidden track rows (row keys, e.g. "A", "C").
    #[serde(default)]
    pub track_hidden: HashSet<String>,
    /// Named timeline markers: (bar_number, name).
    #[serde(default)]
    pub markers: Vec<(u32, String)>,
    /// Loop region: (in_bar, out_bar). None = loop disabled.
    #[serde(default)]
    pub loop_region: Option<(u32, u32)>,
    /// Variable track row height per row (key = "A"-"P", value = lines 2-6). Default 2.
    #[serde(default)]
    pub track_heights: HashMap<String, u8>,
    /// Sampler pad configuration.
    #[serde(default)]
    pub sampler: SamplerConfig,
    /// Song-mode pattern chain (ordered list of scene+bar-count entries).
    #[serde(default)]
    pub chain: Vec<ChainEntry>,
    /// Saved granular presets (up to 8 quick-recall slots).
    #[serde(default)]
    pub granular_scenes: Vec<crate::GranularPreset>,
    pub playing: bool,
    pub recording: bool,
    pub current_bar: u32,
    pub current_step: usize,
    pub cpu: u8,
    pub xrun: u32,
}

impl Project {
    /// Current schema version written to every saved project.
    pub const CURRENT_VERSION: u32 = 1;

    /// Create a blank project.
    pub fn blank(name: impl Into<String>) -> Self {
        let mut matrix = HashMap::new();
        for row in 0..8u8 {
            let key = ((b'A' + row) as char).to_string();
            matrix.insert(key, vec![None; 8]);
        }
        Self {
            version: Self::CURRENT_VERSION,
            name: name.into(),
            bpm: 128.0,
            matrix,
            patterns: HashMap::new(),
            channels: Vec::new(),
            tracks: Vec::new(),
            automation: Vec::new(),
            scenes: Vec::new(),
            midi_inputs: Vec::new(),
            midi_outputs: Vec::new(),
            osc_routes: Vec::new(),
            sync_mode: SyncMode::Internal,
            routing: RoutingGraph::default(),
            buses: vec![
                AudioBus::new("Bus A"),
                AudioBus::new("Bus B"),
            ],
            track_names: HashMap::new(),
            track_types: HashMap::new(),
            track_colors: HashMap::new(),
            track_hidden: HashSet::new(),
            markers: Vec::new(),
            loop_region: None,
            track_heights: HashMap::new(),
            sampler: SamplerConfig::default(),
            chain: Vec::new(),
            granular_scenes: Vec::new(),
            playing: false,
            recording: false,
            current_bar: 0,
            current_step: 0,
            cpu: 0,
            xrun: 0,
        }
    }
}

impl Default for Project {
    fn default() -> Self {
        let mut project = Project::blank("Demo Live Set");
        project.bpm = 128.0;

        // --- Patterns ---
        let mut kck = Pattern::new("KCK01", 16);
        for &step in &[0, 4, 8, 12] {
            kck.steps[step] = Note {
                note: "C-1".to_string(),
                velocity: 110,
                ..Default::default()
            };
        }
        project.patterns.insert("KCK01".to_string(), kck);

        let mut snr = Pattern::new("SNR01", 16);
        for &step in &[4, 12] {
            snr.steps[step] = Note {
                note: "D-1".to_string(),
                velocity: 100,
                ..Default::default()
            };
        }
        project.patterns.insert("SNR01".to_string(), snr);

        let mut hat = Pattern::new("HAT01", 16);
        for step in 0..16 {
            hat.steps[step] = Note {
                note: "F#1".to_string(),
                velocity: if step % 2 == 0 { 90 } else { 60 },
                ..Default::default()
            };
        }
        project.patterns.insert("HAT01".to_string(), hat);

        let bass_notes = ["C-2", "---", "---", "G-2", "---", "A#2", "---", "---",
                          "C-2", "---", "D-2", "---", "F-2", "---", "---", "---"];
        let mut bass = Pattern::new("BASS1", 16);
        for (i, &n) in bass_notes.iter().enumerate() {
            bass.steps[i] = Note {
                note: n.to_string(),
                velocity: 95,
                ..Default::default()
            };
        }
        project.patterns.insert("BASS1".to_string(), bass);

        let seq_notes = ["E-3", "G-3", "A-3", "C-4", "E-3", "D-3", "B-2", "A-2",
                         "E-3", "G-3", "A-3", "C-4", "G-3", "E-3", "D-3", "C-3"];
        let mut seq = Pattern::new("SEQ01", 16);
        for (i, &n) in seq_notes.iter().enumerate() {
            seq.steps[i] = Note {
                note: n.to_string(),
                velocity: 85,
                ..Default::default()
            };
        }
        project.patterns.insert("SEQ01".to_string(), seq);

        let arp_notes = ["C-4", "E-4", "G-4", "B-4", "C-5", "B-4", "G-4", "E-4",
                         "C-4", "E-4", "G-4", "B-4", "A-4", "G-4", "F-4", "E-4"];
        let mut arp = Pattern::new("ARP01", 16);
        for (i, &n) in arp_notes.iter().enumerate() {
            arp.steps[i] = Note {
                note: n.to_string(),
                velocity: 80,
                gate: 50,
                ..Default::default()
            };
        }
        project.patterns.insert("ARP01".to_string(), arp);

        let mut pad = Pattern::new("PAD01", 32);
        pad.steps[0] = Note { note: "C-3".to_string(), velocity: 70, gate: 200, ..Default::default() };
        pad.steps[8] = Note { note: "E-3".to_string(), velocity: 65, gate: 200, ..Default::default() };
        pad.steps[16] = Note { note: "G-3".to_string(), velocity: 68, gate: 200, ..Default::default() };
        pad.steps[24] = Note { note: "A-3".to_string(), velocity: 65, gate: 200, ..Default::default() };
        project.patterns.insert("PAD01".to_string(), pad);

        // --- Matrix clips ---
        let row_patterns = [
            ("A", "KCK01"),
            ("B", "SNR01"),
            ("C", "HAT01"),
            ("D", "BASS1"),
            ("E", "SEQ01"),
            ("F", "ARP01"),
            ("G", "PAD01"),
        ];
        for (row_key, pat_key) in &row_patterns {
            let clips = project.matrix.get_mut(*row_key).unwrap();
            clips[0] = Some(Clip::new(pat_key.to_string(), 0, 0).with_pattern(*pat_key));
        }

        // --- Channels ---
        project.channels = vec![
            Channel::new("KICK").with_fx(0, FxKind::Compressor, true),
            Channel::new("SNARE").with_fx(0, FxKind::Compressor, true).with_fx(1, FxKind::Equalizer, true),
            Channel::new("HATS"),
            Channel::new("BASS").with_fx(0, FxKind::Equalizer, true).with_fx(1, FxKind::Saturator, false),
            Channel::new("SYNTH").with_fx(0, FxKind::Equalizer, true),
            Channel::new("PAD"),
        ];

        // Adjust volumes
        project.channels[0].volume = -3.0;
        project.channels[1].volume = -6.0;
        project.channels[2].volume = -9.0;
        project.channels[3].volume = -6.0;
        project.channels[4].volume = -8.0;
        project.channels[5].volume = -12.0;

        // --- Tracks ---
        let mut intro = Track::new("INTRO");
        intro.blocks = vec![(0, 4, "KCK01".to_string()), (0, 4, "HAT01".to_string())];

        let mut build = Track::new("BUILD");
        build.blocks = vec![(4, 8, "BASS1".to_string()), (4, 8, "SEQ01".to_string())];

        let mut drop = Track::new("DROP");
        drop.blocks = vec![(12, 8, "KCK01".to_string()), (12, 8, "BASS1".to_string()), (12, 8, "ARP01".to_string())];

        let mut break_t = Track::new("BREAK");
        break_t.blocks = vec![(20, 4, "PAD01".to_string()), (20, 4, "SEQ01".to_string())];

        let mut outro = Track::new("OUTRO");
        outro.blocks = vec![(24, 8, "KCK01".to_string()), (24, 8, "BASS1".to_string())];

        project.tracks = vec![intro, build, drop, break_t, outro];

        // --- Automation ---
        let mut filter = AutomationLane::new("FILTER", "channel.4.cc74");
        filter.points = vec![(0, 20), (4, 40), (8, 80), (12, 127), (20, 60), (28, 20)];

        let mut reverb = AutomationLane::new("REVERB", "channel.5.send_a");
        reverb.points = vec![(0, 0), (8, 40), (16, 80), (24, 40), (32, 0)];

        let mut tempo = AutomationLane::new("TEMPO", "project.bpm");
        tempo.points = vec![(0, 64), (16, 80), (32, 64)];

        project.automation = vec![filter, reverb, tempo];

        // --- Scenes ---
        let mut intro_scene = Scene::new("INTRO");
        intro_scene.active_clips[0] = Some("KCK01".to_string());
        intro_scene.active_clips[2] = Some("HAT01".to_string());

        let mut build_scene = Scene::new("BUILD");
        build_scene.active_clips[0] = Some("KCK01".to_string());
        build_scene.active_clips[1] = Some("SNR01".to_string());
        build_scene.active_clips[3] = Some("BASS1".to_string());

        let mut drop_scene = Scene::new("DROP");
        for i in 0..6 {
            drop_scene.active_clips[i] = Some(["KCK01", "SNR01", "HAT01", "BASS1", "SEQ01", "ARP01"][i].to_string());
        }

        let mut break_scene = Scene::new("BREAK");
        break_scene.active_clips[5] = Some("ARP01".to_string());
        break_scene.active_clips[6] = Some("PAD01".to_string());

        let mut outro_scene = Scene::new("OUTRO");
        outro_scene.active_clips[0] = Some("KCK01".to_string());
        outro_scene.active_clips[3] = Some("BASS1".to_string());

        project.scenes = vec![intro_scene, build_scene, drop_scene, break_scene, outro_scene];

        // --- MIDI I/O ---
        project.midi_inputs = vec![
            MidiPort::new("USB MIDI Interface", 1),
            MidiPort::new("Arturia KeyStep", 2),
            MidiPort::new("KORG nanoKONTROL2", 3),
            MidiPort::new("IAC Driver Bus 1", 1),
        ];

        project.midi_outputs = vec![
            MidiPort::new("USB MIDI Interface", 1),
            MidiPort::new("Elektron Digitakt", 10),
            MidiPort::new("Roland TR-8S", 10),
            MidiPort::new("IAC Driver Bus 1", 1),
            MidiPort::new("Ableton Live (loopback)", 1),
        ];

        project.osc_routes = vec![
            OscRoute::new("/seq/bpm", "project.bpm"),
            OscRoute::new("/seq/play", "engine.play"),
            OscRoute::new("/seq/stop", "engine.stop"),
            OscRoute::new("/mixer/vol/*", "channel.volume"),
        ];

        project
    }
}
