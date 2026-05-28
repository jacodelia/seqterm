//! Application-level commands — all user-initiated actions flow through here.
//!
//! Distinct from seqterm-command (which contains UI-level AppCommand).
//! This layer is transport-agnostic: commands can arrive from TUI, GUI, CLI, or API.

use std::path::PathBuf;

/// All actions a user (or script/API) can perform.
#[derive(Debug, Clone)]
pub enum AppCmd {
    // ── Transport ──────────────────────────────────────────────────────────
    Play,
    Stop,
    Record,
    SetBpm(f64),
    TapTempo,

    // ── Project ────────────────────────────────────────────────────────────
    NewProject { bpm: f64 },
    LoadProject(PathBuf),
    SaveProject,
    SaveProjectAs(PathBuf),

    // ── Matrix ─────────────────────────────────────────────────────────────
    SetMatrixSize { rows: usize, cols: usize },
    EnableClip { row: usize, col: usize },
    DisableClip { row: usize, col: usize },
    ToggleClip { row: usize, col: usize },
    AssignPatternToClip { row: usize, col: usize, pattern_key: String },

    // ── Audio sources ──────────────────────────────────────────────────────
    AssignSf2ToClip { row: usize, col: usize, path: PathBuf, bank: u8, preset: u8 },
    AssignAudioFileToClip { row: usize, col: usize, path: PathBuf },
    ClearClipSource { row: usize, col: usize },

    // ── Audio engine ───────────────────────────────────────────────────────
    StartAudioEngine,
    StopAudioEngine,
    SetAudioBufferSize(u32),
    SetAudioSampleRate(u32),

    // ── MIDI ───────────────────────────────────────────────────────────────
    RefreshMidiPorts,
    EnableMidiInput { port_name: String },
    DisableMidiInput { port_name: String },
    EnableMidiOutput { port_name: String },
    DisableMidiOutput { port_name: String },

    // ── Mixer ──────────────────────────────────────────────────────────────
    SetChannelVolume { channel_idx: usize, volume_db: f32 },
    SetChannelPan { channel_idx: usize, pan: i8 },
    MuteChannel { channel_idx: usize },
    UnmuteChannel { channel_idx: usize },
    SoloChannel { channel_idx: usize },

    // ── Undo/redo ──────────────────────────────────────────────────────────
    Undo,
    Redo,

    // ── Plugin system ──────────────────────────────────────────────────────
    ScanPlugins { dir: PathBuf },
    LoadPlugin { plugin_id: String, sample_rate: u32, block_size: u32 },
    UnloadPlugin { registry_id: u64 },
    AssignPluginToMixerSlot { registry_id: u64, slot: usize },
    SuspendPlugin { registry_id: u64 },
    ResumePlugin { registry_id: u64 },
}
