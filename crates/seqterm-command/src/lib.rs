use std::path::PathBuf;
use seqterm_midi_io::MidiImportOptions;
use seqterm_persistence::MidiLearnTarget;

/// How a Matrix paste combines clipboard content with the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteMode {
    /// Overwrite the destination cells entirely.
    Replace,
    /// Union notes into existing destination patterns without deleting data.
    Merge,
    /// Insert clipboard steps, shifting existing destination steps to make room.
    Insert,
}

impl PasteMode {
    pub fn label(self) -> &'static str {
        match self { Self::Replace => "replace", Self::Merge => "merge", Self::Insert => "insert" }
    }
}

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
    /// Open-project after the unsaved-changes prompt: skips the dirty re-check and
    /// shows the file picker directly (avoids re-prompting in a loop).
    OpenProjectConfirmed,
    OpenProjectPath(PathBuf),
    SaveProject,
    SaveProjectAs,
    SaveProjectToPath(PathBuf),
    /// Save to `path` skipping the "overwrite existing file?" prompt.
    SaveProjectToPathConfirmed(PathBuf),
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
    ShowSettings,
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
    /// Arm MIDI learn for whatever parameter is focused in the current view
    /// (mixer channel/FX, master FX, EDITOR param, …). The next CC binds it.
    MidiLearnFocused,
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
    /// Assign a synthesizer plugin as the clip's note source.
    AssignPluginSource { row: usize, col: usize, id: String, format: String, name: String },
    /// Open the source picker modal for a matrix cell.
    OpenSourcePicker { row: usize, col: usize },

    /// Move (swap or displace) a clip from `from` to `to` in the matrix.
    MoveClip { from_row: usize, from_col: usize, to_row: usize, to_col: usize },

    // ── Matrix clipboard (copy/cut/paste between patterns) ────────────────
    /// Copy the current Matrix selection (or cursor cell) to the internal clipboard.
    MatrixCopy,
    /// Copy then clear the current Matrix selection.
    MatrixCut,
    /// Paste the clipboard at the cursor with the given merge semantics.
    MatrixPaste(PasteMode),

    // ── Modal control ─────────────────────────────────────────────────────
    CloseModal,

    // ── OSC server ────────────────────────────────────────────────────────
    /// Start (or restart) the OSC UDP listener on the given port.
    StartOscServer(u16),
    /// Stop the OSC UDP listener.
    StopOscServer,

    // ── Lua scripting ─────────────────────────────────────────────────────
    /// Show a timed status bar message (dispatched by Lua `seqterm.status()`).
    StatusMessage { text: String, duration_ms: Option<u32> },
    /// Set the project BPM directly (dispatched by Lua `seqterm.set_bpm()`).
    SetBpm(f64),
    /// Open the Lua REPL modal (F12).
    OpenLuaRepl,

    // ── Realtime capture ─────────────────────────────────────────────────
    /// Toggle live audio capture to WAV (start if off, stop if on).
    ToggleCapture,
    /// Toggle live audio input monitoring (microphone → master mix).
    ToggleInputMonitor,
    /// Set the live input monitor gain (0.0 = mute, 1.0 = unity, 2.0 = +6 dB).
    SetInputMonitorGain(f32),
    /// Toggle recording the live audio input to a WAV file (requires monitor active).
    ToggleInputRecord,
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

    // ── Quantization ─────────────────────────────────────────────────────
    /// Quantize timing of all notes in a pattern.
    /// `strength` 0-100, `grid_divs` 1-32 (1=step, 2=half-step, 4=quarter-step…).
    QuantizePattern {
        pattern_key: String,
        strength: u8,
        grid_divs: usize,
        swing_aware: bool,
    },
    /// Humanize timing: add random micro-offsets.
    HumanizePattern { pattern_key: String, amount: u8 },

    // ── Rational editing (Phase 3) ───────────────────────────────────────────
    /// Cycle the shared edit resolution along the resolution ladder.
    /// `dir > 0` = finer, `dir < 0` = coarser.
    CycleEditResolution { dir: i32 },
    /// Toggle the active triplet (3:2) on the edit grid.
    ToggleEditTuplet,
    /// Cycle the snap mode (grid → fine → off).
    CycleSnapMode,
    /// Toggle free-time mode (bypass all snapping).
    ToggleFreeTime,
    /// Change a pattern's own resolution, preserving exact note positions/durations.
    /// `den` is the new `1/den`-of-a-whole-note grid.
    ChangePatternResolution { pattern_key: String, den: u32 },
    /// Quantize a pattern's note starts to a resolution/tuplet grid.
    /// `tuplet` is `Some((num, den))` for tuplet grids, `None` for straight.
    QuantizeToResolution {
        pattern_key: String,
        den: u32,
        tuplet: Option<(u32, u32)>,
        strength: u8,
    },
    /// Step a pattern's step-grid resolution finer (`dir>0`) / coarser along the
    /// resolution ladder, losslessly re-gridding step notes (Phase 6 zoom). The
    /// exact rational `events` layer is preserved. `dir` is +1 / -1.
    CyclePatternResolution { pattern_key: String, dir: i32 },
    /// Add an exact rational note to a pattern's canonical `events` layer at beat
    /// `start_num/den` for `dur_num/den` beats, pitch `midi`, velocity `vel`
    /// (Phase 6 — arbitrary subdivisions/tuplets, e.g. 7:9). Undoable.
    AddRationalNote {
        pattern_key: String,
        start_num: i64, start_den: i64,
        dur_num: i64, dur_den: i64,
        midi: u8, vel: u8,
    },
    /// Remove the exact rational note nearest beat `start_num/den` with pitch
    /// `midi` from a pattern's `events` layer (Phase 6). Undoable.
    RemoveRationalNote { pattern_key: String, start_num: i64, start_den: i64, midi: u8 },
    /// Set the active edit tuplet to an arbitrary `num:den` ratio (e.g. 7:9), or
    /// clear it when `num == den` (Phase 6 — complex/irregular figures).
    SetEditTuplet { num: i64, den: i64 },
    /// Insert an irregular rhythmic figure: `count` exact rational notes spaced by
    /// `cell_num/den` beats from `start_num/den`, all pitch `midi` (Phase 6). One
    /// undo step. Used to drop a whole tuplet group (any ratio) onto the grid.
    InsertTupletFigure {
        pattern_key: String,
        start_num: i64, start_den: i64,
        cell_num: i64, cell_den: i64,
        count: u32,
        midi: u8, vel: u8,
    },
    /// Resize a note's END (set its duration) to `num/den` beats.
    ResizeNoteEnd { pattern_key: String, step: usize, num: i64, den: i64 },
    /// Resize a note's START (move its onset, end fixed) to `num/den` beats.
    ResizeNoteStart { pattern_key: String, step: usize, num: i64, den: i64 },

    // ── Arrangement editor (Phase 4) ─────────────────────────────────────────
    /// Append a new arrangement track of the given kind label ("MIDI"/"AUDIO"/…).
    ArrangementAddTrack { name: String, kind: String },
    /// Add a pattern clip on `track_idx` at `start_num/den` beats spanning `len_num/den`.
    ArrangementAddClip {
        track_idx: usize,
        pattern_key: String,
        start_num: i64, start_den: i64,
        len_num: i64, len_den: i64,
    },
    /// Move clip `clip_id` by `delta_num/den` beats (clamped to ≥ 0).
    ArrangementMoveClip { clip_id: u64, delta_num: i64, delta_den: i64 },
    /// Split clip `clip_id` at absolute beat `at_num/den`.
    ArrangementSplitClip { clip_id: u64, at_num: i64, at_den: i64 },
    /// Duplicate clip `clip_id` immediately after itself.
    ArrangementDuplicateClip { clip_id: u64 },
    /// Delete clip `clip_id`.
    ArrangementDeleteClip { clip_id: u64 },
    /// Trim clip `clip_id`'s start (`edge=false`) or end (`edge=true`) to `at_num/den` beats.
    ArrangementTrimClip { clip_id: u64, edge_end: bool, at_num: i64, at_den: i64 },
    /// Create an audio clip on `track_idx` at beat `start_num/den` from `path`
    /// (Milestone C). Length is derived from the file duration at the project BPM.
    ConfirmArrangementAudioClip { track_idx: usize, start_num: i64, start_den: i64, path: PathBuf },
    /// Set/replace an automation breakpoint on `track_idx`'s `dest` lane at beat
    /// `at_num/den`, normalised `value` in `[0,1]` (Milestone F).
    ArrangementSetAutomationPoint {
        track_idx: usize,
        dest: String,
        at_num: i64, at_den: i64,
        value: f64,
    },
    /// Remove the automation breakpoint nearest beat `at_num/den` on `track_idx`'s
    /// `dest` lane (Milestone F).
    ArrangementRemoveAutomationPoint {
        track_idx: usize,
        dest: String,
        at_num: i64, at_den: i64,
    },
    /// Add (or rename in place) a timeline marker at beat `at_num/den` (Phase 5).
    ArrangementAddMarker { at_num: i64, at_den: i64, name: String },
    /// Remove the timeline marker nearest beat `at_num/den` (Phase 5).
    ArrangementRemoveMarker { at_num: i64, at_den: i64 },
    /// Add a region spanning `[start, end)` beats (Phase 5, Fase 8).
    ArrangementAddRegion {
        start_num: i64, start_den: i64,
        end_num: i64, end_den: i64,
        name: String,
    },
    /// Remove the region containing beat `at_num/den` (Phase 5, Fase 8).
    ArrangementRemoveRegion { at_num: i64, at_den: i64 },
    /// Toggle the cycle (loop) span over `[start, end)` beats (Phase 5, Fase 8).
    ArrangementToggleCycle {
        start_num: i64, start_den: i64,
        end_num: i64, end_den: i64,
    },
    /// Move the track at `track_idx` up/down one slot (Phase 5, Fase 9).
    ArrangementMoveTrack { track_idx: usize, up: bool },
    /// Delete the track at `track_idx` with its clips (Phase 5, Fase 9).
    ArrangementRemoveTrack { track_idx: usize },
    /// Cycle the track's kind (MIDI→Audio→Drum→Group→Bus→Auto) (Phase 5, Fase 9).
    ArrangementCycleTrackKind { track_idx: usize },
    /// Add a section spanning `[start, end)` beats (Phase 5, Fase 10).
    ArrangementAddSection {
        start_num: i64, start_den: i64,
        end_num: i64, end_den: i64,
        name: String,
    },
    /// Remove the section containing beat `at_num/den` (Phase 5, Fase 10).
    ArrangementRemoveSection { at_num: i64, at_den: i64 },
    /// Shift the section containing `at_num/den` (and its clips) by `delta` beats
    /// (Phase 5, Fase 10).
    ArrangementShiftSection { at_num: i64, at_den: i64, delta_num: i64, delta_den: i64 },
    /// Duplicate the section containing beat `at_num/den` (Phase 5, Fase 10).
    ArrangementDuplicateSection { at_num: i64, at_den: i64 },

    // ── Tutorial ─────────────────────────────────────────────────────────
    /// Start the interactive tutorial (shows step-by-step overlay).
    StartTutorial,
    /// Advance to the next tutorial step.
    TutorialNext,
    /// Close the tutorial.
    TutorialClose,

    // ── Audio Editing ─────────────────────────────────────────────────────
    /// Open the audio clip editor for the given matrix clip.
    OpenAudioEdit { row: usize, col: usize },
    /// Open the SF2 zone editor (own-sampler) in the EDITOR view for a clip whose
    /// source is SF2. No-op for non-SF2 clips.
    OpenSf2Edit { row: usize, col: usize },
    /// Apply audio edits (trim, gain, normalize, fade) from the AudioEdit modal.
    ApplyAudioEdit { row: usize, col: usize, trim_start: f32, trim_end: f32, gain: f32, normalize: bool },

    // ── Clip Stretching ───────────────────────────────────────────────────
    /// Time-stretch an audio clip in the matrix to match the project BPM.
    /// Operates offline in a background thread; replaces the clip source on completion.
    StretchClipToBpm { row: usize, col: usize },

    // ── Bounce / Freeze ───────────────────────────────────────────────────
    /// Bounce a matrix row in-place: render stem → WAV, reassign all clips in that
    /// row to `AudioFile` source. The original MIDI/SF2 data is preserved in patterns.
    BounceInPlace { row: usize },
    /// Bounce a single clip in-place (one column in a row).
    BounceClipInPlace { row: usize, col: usize },
    /// Freeze a track: render stem → AudioFile, bypass live MIDI/SF2 processing.
    /// Stores the original sources in `Clip::freeze_source` for later unfreeze.
    FreezeTrack { row: usize },
    /// Unfreeze a track: restore original sources from `Clip::freeze_source`.
    UnfreezeTrack { row: usize },

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
