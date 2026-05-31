use std::path::PathBuf;
use seqterm_midi_io::MidiImportOptions;
use seqterm_persistence::MidiLearnTarget;

/// State-change events broadcast from the application layer to interested subscribers.
#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    ProjectLoaded  { path: PathBuf },
    ProjectSaved   { path: PathBuf },
    ProjectDirty,
    ProjectClean,
    BpmChanged(f64),
    PlaybackStarted,
    PlaybackStopped,
    RecordingStarted,
    RecordingStopped,
    PatternCreated { key: String },
    PatternDeleted { key: String },
    PatternRenamed { old: String, new: String },
    MidiImportComplete { pattern_count: usize },
    MidiExportComplete,
    Error { message: String },
    /// Timed status bar message. `duration_ms: None` = persistent.
    StatusMessage  { text: String, duration_ms: Option<u32> },
}

/// All user-initiated actions that can originate from menus, keyboard, or MIDI.
#[derive(Debug, Clone, PartialEq)]
pub enum AppCommand {
    // ── File ──────────────────────────────────────────────────────────────
    NewProject,
    NewProjectConfirmed,
    NewProjectWithBpm(u32),
    OpenProject,
    OpenProjectPath(PathBuf),
    SaveProject,
    SaveProjectAs,
    SaveProjectToPath(PathBuf),
    ImportMidi,
    ImportMidiFromPath(PathBuf),
    ImportMidiShowOptions(PathBuf),
    ImportMidiWithOptions(PathBuf, MidiImportOptions),
    /// Set the SF2 path on the currently-open MIDI import options modal.
    SetMidiImportSf2(PathBuf),
    ExportMidi,
    ExportMidiToPath(PathBuf),
    ExportMidiActiveOnly,
    ExportMidiActiveOnlyToPath(PathBuf),
    ExportMuseScore,
    ExportMuseScoreToPath(PathBuf),
    ExportAudio,
    ExportAudioToPath(PathBuf),
    RecentProject(usize),
    RecentMidiImport(usize),
    Exit,
    ExitConfirmed,
    SaveAndExit,

    // ── Edit ──────────────────────────────────────────────────────────────
    Undo,
    Redo,
    ShowRoutingConfig,
    ShowAudioSettings,
    ShowMidiSettings,
    ShowKeybindings,

    // ── About / Help ──────────────────────────────────────────────────────
    ShowAbout,
    ShowHelp(HelpTopic),
    ShowCommandPalette,

    // ── Keybindings ───────────────────────────────────────────────────────
    ExportKeybindings,
    ExportKeybindingsToPath(PathBuf),
    ImportKeybindings,
    ImportKeybindingsFromPath(PathBuf),

    // ── MIDI Learn ────────────────────────────────────────────────────────
    MidiLearn(MidiLearnTarget),
    CancelMidiLearn,

    // ── Audio sources ─────────────────────────────────────────────────────
    /// Open SF2 file picker for a matrix clip (row, col).
    AssignSf2ToClip { row: usize, col: usize },
    /// Open audio file picker for a matrix clip.
    AssignAudioFileToClip { row: usize, col: usize },
    /// SF2 file chosen — open the preset browser for it.
    OpenSf2Browser { row: usize, col: usize, path: PathBuf },
    /// Re-open the SF2 preset browser for a clip that already has an SF2 source.
    /// Skips file picking — opens directly with the clip's existing SF2 path.
    ReopenSf2Browser { row: usize, col: usize },
    /// Confirm SF2 assignment after file + preset selection.
    ConfirmSf2Assignment { row: usize, col: usize, path: PathBuf, bank: u8, preset: u8 },
    /// Confirm audio file assignment.
    ConfirmAudioFileAssignment { row: usize, col: usize, path: PathBuf },
    /// Clear source back to MIDI.
    ClearClipSource { row: usize, col: usize },
    /// Assign MIDI output port to a clip (sets source to Midi + midi_out).
    AssignMidiPort { row: usize, col: usize, port: String },
    /// Open the source picker modal for a matrix cell.
    OpenSourcePicker { row: usize, col: usize },

    /// Move (swap or displace) a clip from `from` to `to` in the matrix.
    MoveClip { from_row: usize, from_col: usize, to_row: usize, to_col: usize },

    // ── Modal control ─────────────────────────────────────────────────────
    CloseModal,

    // ── OSC server ────────────────────────────────────────────────────────
    /// Start (or restart) the OSC UDP listener on the given port.
    StartOscServer(u16),
    /// Stop the OSC UDP listener.
    StopOscServer,

    // ── Realtime capture ─────────────────────────────────────────────────
    /// Toggle live audio capture to WAV (start if off, stop if on).
    ToggleCapture,
    /// Toggle MIDI clock sync — when on, incoming 0xF8 pulses drive BPM.
    ToggleMidiClockSync,

    // ── Pattern chain ─────────────────────────────────────────────────────
    /// Toggle song-mode chain following (pattern chain advances each N bars).
    ToggleChainMode,
    /// Append a chain entry: (scene_idx, bars).
    AddChainEntry { scene_idx: usize, bars: u32 },
    /// Remove the chain entry at position `pos`.
    RemoveChainEntry { pos: usize },
    /// Seek the chain to `pos` immediately.
    SeekChain { pos: usize },

    // ── Plugin system ─────────────────────────────────────────────────────
    /// Open the parameter browser overlay for the given plugin registry ID.
    OpenPluginParams { registry_id: u64 },
    /// Scan a directory for plugins and register them.
    ScanPlugins { dir: std::path::PathBuf },
    /// Instantiate a discovered plugin.
    LoadPlugin { plugin_id: String },
    /// Destroy a plugin instance.
    UnloadPlugin { registry_id: u64 },

    // ── Sampler / SP-404 style pad system ─────────────────────────────────
    /// Trigger a pad by (bank, pad index 0-15), with velocity 0-127.
    TriggerPad { bank: usize, pad: usize, velocity: u8 },
    /// Stop a playing pad (gate-off or choke).
    StopPad { bank: usize, pad: usize },
    /// Switch to a different pad bank (0-15).
    SelectPadBank(usize),
    /// Open the file picker to assign a sample to a pad.
    AssignSampleToPad { bank: usize, pad: usize },
    /// Confirm sample assignment after file selection.
    ConfirmSampleAssignment { bank: usize, pad: usize, path: std::path::PathBuf },
    /// Clear the sample from a pad.
    ClearPad { bank: usize, pad: usize },
    /// Capture the skip-back buffer contents into a new pad slot.
    CaptureSkipBackToPad { bank: usize, pad: usize },
    /// Bounce the current pattern mix to a new pad (render → sample).
    BouncePatternToPad { pattern_key: String, bank: usize, pad: usize },

    // ── Granular engine ───────────────────────────────────────────────────
    /// Open the granular engine view for a given pad/clip.
    OpenGranularView { bank: usize, pad: usize },
    /// Freeze the granular engine's source buffer.
    GranularFreeze { bank: usize, pad: usize },
    /// Unfreeze (return to live source scanning).
    GranularUnfreeze { bank: usize, pad: usize },
    /// Set a granular parameter by name and normalised value (0.0–1.0).
    SetGranularParam { bank: usize, pad: usize, param: String, value: f32 },

    // ── Granular scene snapshots ──────────────────────────────────────────
    /// Save current granular params+zone to a named scene slot (0-7).
    SaveGranularScene { slot: usize, name: String },
    /// Recall a previously saved granular scene and apply it to the current pad.
    RecallGranularScene { slot: usize },
    /// Delete a granular scene slot.
    DeleteGranularScene { slot: usize },
    /// Randomise granular params (spray, jitter, pitch, size, density, envelope, spread).
    RandomiseGranularPreset,
    /// Morph current granular state → scene `to_slot` over `beats` beats.
    MorphGranularScene { to_slot: usize, beats: u32 },
    /// Record the current granular audio output to a new pad (live resampling).
    CaptureGranularToPad { bank: usize, pad: usize },
    /// Route a mixer audio slot's output as live input to the current granular pad.
    /// `source_slot_id = None` disconnects live input and restores normal mode.
    SetGranularLiveSource { bank: usize, pad: usize, source_slot_id: Option<u32> },
    /// Update one LFO slot in the modulation matrix for the current granular pad.
    /// `shape_idx`: 0=Sine, 1=Tri, 2=Sqr, 3=S&H. `target_idx`: 0=Spray…6=Jitter.
    SetGranularModSlot {
        slot_idx:   usize,
        enabled:    bool,
        shape_idx:  u8,
        rate_hz:    f32,
        depth:      f32,
        target_idx: u8,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum HelpTopic {
    KeyboardShortcuts,
    WorkflowGuide,
    MidiImport,
    Routing,
    PatternEditor,
    Troubleshooting,
    LatencyOptimization,
}

impl HelpTopic {
    pub fn label(&self) -> &'static str {
        match self {
            Self::KeyboardShortcuts   => "Keyboard Shortcuts",
            Self::WorkflowGuide       => "Workflow Guide",
            Self::MidiImport          => "MIDI Import Guide",
            Self::Routing             => "Routing Guide",
            Self::PatternEditor       => "Pattern Editor Guide",
            Self::Troubleshooting     => "Troubleshooting",
            Self::LatencyOptimization => "Latency Optimization",
        }
    }

    pub fn all() -> &'static [HelpTopic] {
        &[
            HelpTopic::KeyboardShortcuts,
            HelpTopic::WorkflowGuide,
            HelpTopic::MidiImport,
            HelpTopic::Routing,
            HelpTopic::PatternEditor,
            HelpTopic::Troubleshooting,
            HelpTopic::LatencyOptimization,
        ]
    }
}
