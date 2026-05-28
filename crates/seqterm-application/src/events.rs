//! Domain events — things that happened, published to all listeners.

use std::path::PathBuf;

/// Events published by the application layer after state changes.
/// Frontends and adapters subscribe to these to update their views.
#[derive(Debug, Clone)]
pub enum DomainEvent {
    // ── Transport ──────────────────────────────────────────────────────────
    PlaybackStarted,
    PlaybackStopped,
    BpmChanged(f64),
    StepAdvanced { step: usize },
    BarAdvanced { bar: usize },

    // ── Project ────────────────────────────────────────────────────────────
    ProjectLoaded { name: String, path: Option<PathBuf> },
    ProjectSaved { path: PathBuf },
    ProjectDirty,

    // ── Matrix ─────────────────────────────────────────────────────────────
    ClipEnabled  { row: usize, col: usize },
    ClipDisabled { row: usize, col: usize },
    ClipSourceChanged { row: usize, col: usize },
    MatrixResized { rows: usize, cols: usize },

    // ── Audio engine ───────────────────────────────────────────────────────
    AudioEngineStarted { sample_rate: u32, buffer_size: u32 },
    AudioEngineStopped,
    AudioXrun,
    AudioDspLoad(f32),
    Sf2Loaded { slot_id: u32, preset_name: String },
    AudioFileLoaded { slot_id: u32, duration_secs: f64 },
    AudioLoadFailed { slot_id: u32, error: String },

    // ── MIDI ───────────────────────────────────────────────────────────────
    MidiPortsRefreshed,
    MidiCc { channel: u8, cc: u8, value: u8 },

    // ── Errors ─────────────────────────────────────────────────────────────
    Error { message: String },
    Warning { message: String },
}
