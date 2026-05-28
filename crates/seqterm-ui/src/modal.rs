use std::path::{Path, PathBuf};

use seqterm_audio_export::{AudioExportOpts, ExportMode};
use seqterm_midi_io::{MidiImportOptions, MidiTrackInfo};
use seqterm_persistence::KeyBinding;
use seqterm_command::{AppCommand, HelpTopic};

// ─── Input dialog ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct InputDialogState {
    pub title:       String,
    pub placeholder: String,
    pub value:       String,
    /// Called with the submitted string to produce the follow-up command.
    pub on_submit:   fn(String) -> AppCommand,
}

impl InputDialogState {
    pub fn new(
        title: impl Into<String>,
        placeholder: impl Into<String>,
        on_submit: fn(String) -> AppCommand,
    ) -> Self {
        Self {
            title: title.into(),
            placeholder: placeholder.into(),
            value: String::new(),
            on_submit,
        }
    }
}

// ─── File picker ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilePickerMode { Open, Save }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilePickerTarget {
    OpenProject,
    SaveProject,
    ImportMidi,
    ExportMidi,
    ExportMidiActiveOnly,
    ExportMuseScore,
    ExportAudio,
    ExportKeybindings,
    ImportKeybindings,
    /// Assign an SF2 SoundFont to a matrix clip (row, col).
    AssignSf2 { row: usize, col: usize },
    /// Assign an audio file to a matrix clip.
    AssignAudioFile { row: usize, col: usize },
    /// Assign a sample to a sampler pad (bank, pad).
    AssignSampleToPad { bank: usize, pad: usize },
}

impl FilePickerTarget {
    pub fn title(&self) -> &'static str {
        match self {
            Self::OpenProject        => "Open Project",
            Self::SaveProject        => "Save Project As",
            Self::ImportMidi         => "Import MIDI File",
            Self::ExportMidi         => "Export MIDI (Full)",
            Self::ExportMidiActiveOnly => "Export MIDI (Active Rows)",
            Self::ExportMuseScore    => "Export MusicXML / MuseScore",
            Self::ExportAudio        => "Export Audio",
            Self::ExportKeybindings  => "Export Keybindings",
            Self::ImportKeybindings  => "Import Keybindings",
            Self::AssignSf2 { .. }          => "Assign SF2 SoundFont",
            Self::AssignAudioFile { .. }     => "Assign Audio File",
            Self::AssignSampleToPad { .. }   => "Assign Sample to Pad",
        }
    }

    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::OpenProject              => &["json", "seqterm"],
            Self::SaveProject              => &["json", "seqterm"],
            Self::ImportMidi               => &["mid", "midi"],
            Self::ExportMidi               => &["mid"],
            Self::ExportMidiActiveOnly     => &["mid"],
            Self::ExportMuseScore          => &["xml", "musicxml"],
            Self::ExportAudio              => &["wav", "flac", "ogg"],
            Self::ExportKeybindings        => &["toml"],
            Self::ImportKeybindings        => &["toml"],
            Self::AssignSf2 { .. }         => &["sf2", "SF2"],
            Self::AssignAudioFile { .. }   => &["wav", "flac", "mp3", "ogg"],
            Self::AssignSampleToPad { .. } => &["wav", "flac", "mp3", "ogg", "aiff"],
        }
    }

    pub fn mode(&self) -> FilePickerMode {
        match self {
            Self::OpenProject
            | Self::ImportMidi
            | Self::ImportKeybindings
            | Self::AssignSf2 { .. }
            | Self::AssignAudioFile { .. }
            | Self::AssignSampleToPad { .. } => FilePickerMode::Open,
            _ => FilePickerMode::Save,
        }
    }

    pub fn into_confirm_command(self, path: PathBuf) -> AppCommand {
        match self {
            Self::OpenProject        => AppCommand::OpenProjectPath(path),
            Self::SaveProject        => AppCommand::SaveProjectToPath(path),
            Self::ImportMidi         => AppCommand::ImportMidiFromPath(path),
            Self::ExportMidi         => AppCommand::ExportMidiToPath(path),
            Self::ExportMidiActiveOnly => AppCommand::ExportMidiActiveOnlyToPath(path),
            Self::ExportMuseScore    => AppCommand::ExportMuseScoreToPath(path),
            Self::ExportAudio        => AppCommand::ExportAudioToPath(path),
            Self::ExportKeybindings  => AppCommand::ExportKeybindingsToPath(path),
            Self::ImportKeybindings  => AppCommand::ImportKeybindingsFromPath(path),
            Self::AssignSf2 { row, col } => AppCommand::OpenSf2Browser { row, col, path },
            Self::AssignAudioFile { row, col } => AppCommand::ConfirmAudioFileAssignment { row, col, path },
            Self::AssignSampleToPad { bank, pad } => AppCommand::ConfirmSampleAssignment { bank, pad, path },
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug)]
pub struct FilePickerState {
    pub target:         FilePickerTarget,
    pub current_dir:    PathBuf,
    pub entries:        Vec<FileEntry>,
    pub cursor:         usize,
    pub scroll:         usize,
    /// Only used in Save mode.
    pub filename_input: String,
    /// Input focus in save mode: false=list, true=filename box.
    pub input_focused:  bool,
    /// Recently-visited directories for quick-access (press `r` to toggle).
    pub recent_dirs:    Vec<PathBuf>,
    /// When true, show the recent-dirs shortcut list instead of the regular file list.
    pub show_recent:    bool,
    /// Cursor within the recent-dirs list.
    pub recent_cursor:  usize,
    /// Search/filter string (Open mode only — type to filter entries).
    pub search_input:   String,
}

impl FilePickerState {
    pub fn new(target: FilePickerTarget) -> Self {
        let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut s = Self {
            target,
            current_dir: dir,
            entries: Vec::new(),
            cursor: 0,
            scroll: 0,
            filename_input: String::new(),
            input_focused: false,
            recent_dirs: Vec::new(),
            show_recent: false,
            recent_cursor: 0,
            search_input: String::new(),
        };
        s.refresh();
        s
    }

    /// Populate the recent-dirs list from parent directories of recent file paths.
    pub fn with_recent_dirs(mut self, paths: &[std::path::PathBuf]) -> Self {
        let mut seen = std::collections::HashSet::new();
        for p in paths {
            if let Some(parent) = p.parent() {
                if parent.exists() && seen.insert(parent.to_path_buf()) {
                    self.recent_dirs.push(parent.to_path_buf());
                    if self.recent_dirs.len() >= 8 { break; }
                }
            }
        }
        self
    }

    pub fn refresh(&mut self) {
        self.entries = read_dir_entries(&self.current_dir, self.target.extensions());
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
        self.scroll = 0;
    }

    pub fn descend(&mut self) {
        let path = self.visible_entries()
            .get(self.cursor)
            .filter(|e| e.is_dir)
            .map(|e| e.path.clone());
        if let Some(p) = path {
            self.current_dir = p;
            self.cursor = 0;
            self.search_input.clear();
            self.refresh();
        }
    }

    pub fn ascend(&mut self) {
        if let Some(parent) = self.current_dir.parent().map(|p| p.to_path_buf()) {
            self.current_dir = parent;
            self.cursor = 0;
            self.refresh();
        }
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        match self.target.mode() {
            FilePickerMode::Open => self
                .entries
                .get(self.cursor)
                .filter(|e| !e.is_dir)
                .map(|e| e.path.clone()),
            FilePickerMode::Save => {
                let name = self.filename_input.trim();
                if name.is_empty() { return None; }
                let mut p = self.current_dir.join(name);
                // Ensure correct extension.
                if let Some(ext) = self.target.extensions().first() {
                    if p.extension().is_none() { p.set_extension(ext); }
                }
                Some(p)
            }
        }
    }

    /// Entries filtered by `search_input` (case-insensitive substring match).
    /// Returns all entries when search is empty.
    pub fn visible_entries(&self) -> Vec<&FileEntry> {
        if self.search_input.is_empty() {
            self.entries.iter().collect()
        } else {
            let q = self.search_input.to_lowercase();
            self.entries.iter().filter(|e| e.name.to_lowercase().contains(&q)).collect()
        }
    }

    pub fn clamp_scroll(&mut self, visible_rows: usize) {
        let len = self.visible_entries().len();
        self.cursor = self.cursor.min(len.saturating_sub(1));
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor + 1 > self.scroll + visible_rows {
            self.scroll = (self.cursor + 1).saturating_sub(visible_rows);
        }
    }

    /// Return the selected path using the filtered entry list.
    pub fn selected_visible_path(&self) -> Option<PathBuf> {
        match self.target.mode() {
            FilePickerMode::Open => self
                .visible_entries()
                .get(self.cursor)
                .filter(|e| !e.is_dir)
                .map(|e| e.path.clone()),
            FilePickerMode::Save => {
                let name = self.filename_input.trim();
                if name.is_empty() { return None; }
                let mut p = self.current_dir.join(name);
                if let Some(ext) = self.target.extensions().first() {
                    if p.extension().is_none() { p.set_extension(ext); }
                }
                Some(p)
            }
        }
    }
}

fn read_dir_entries(dir: &Path, allowed_ext: &[&str]) -> Vec<FileEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new(); };
    let mut entries: Vec<FileEntry> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') { return None; }
            let is_dir = path.is_dir();
            if !is_dir {
                let ext = path.extension()
                    .map(|x| x.to_string_lossy().to_lowercase());
                if !allowed_ext.iter().any(|a| ext.as_deref() == Some(*a)) {
                    return None;
                }
            }
            Some(FileEntry { name, path, is_dir })
        })
        .collect();
    entries.sort_unstable_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
    });
    entries
}

// ─── Help modal ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct HelpState {
    pub topic:  HelpTopic,
    pub scroll: usize,
    /// Which topic in the sidebar is highlighted.
    pub sidebar_cursor: usize,
}

impl HelpState {
    pub fn new(topic: HelpTopic) -> Self {
        let idx = HelpTopic::all().iter().position(|t| t == &topic).unwrap_or(0);
        Self { topic, scroll: 0, sidebar_cursor: idx }
    }
}

// ─── Settings states ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct AudioSettingsState {
    pub cursor: usize,
    /// Scratch buffer for the display value being edited.
    pub editing_idx: Option<usize>,
    /// Snapshot of backend + sample_rate at the time the modal was opened,
    /// used to detect whether a restart is required on save.
    pub original_backend:     String,
    pub original_sample_rate: u32,
}

impl AudioSettingsState {
    pub fn new() -> Self {
        Self {
            cursor: 0,
            editing_idx: None,
            original_backend: String::new(),
            original_sample_rate: 0,
        }
    }

    pub fn with_snapshot(backend: String, sample_rate: u32) -> Self {
        Self {
            cursor: 0,
            editing_idx: None,
            original_backend: backend,
            original_sample_rate: sample_rate,
        }
    }
}

#[derive(Debug)]
pub struct MidiSettingsState {
    pub tab:    usize, // 0=Inputs 1=Outputs 2=Sync
    pub cursor: usize,
}

impl MidiSettingsState {
    pub fn new() -> Self { Self { tab: 0, cursor: 0 } }
}

// ─── Command palette ──────────────────────────────────────────────────────────

/// A single result entry in the command palette.
#[derive(Debug, Clone)]
pub struct PaletteEntry {
    /// Display label shown in the list.
    pub label:    String,
    /// Short right-aligned hint (keyboard shortcut).
    pub shortcut: String,
    /// Action to execute on Enter.
    pub command:  AppCommand,
}

impl PaletteEntry {
    pub fn new(label: impl Into<String>, shortcut: impl Into<String>, command: AppCommand) -> Self {
        Self { label: label.into(), shortcut: shortcut.into(), command }
    }
}

/// All available palette entries — built once and filtered per keystroke.
pub fn all_palette_entries() -> Vec<PaletteEntry> {
    vec![
        PaletteEntry::new("New Project",           "Ctrl+N",       AppCommand::NewProject),
        PaletteEntry::new("Open Project…",         "Ctrl+O",       AppCommand::OpenProject),
        PaletteEntry::new("Save Project",          "Ctrl+S",       AppCommand::SaveProject),
        PaletteEntry::new("Save Project As…",      "Ctrl+Shift+S", AppCommand::SaveProjectAs),
        PaletteEntry::new("Import MIDI…",          "Ctrl+I",       AppCommand::ImportMidi),
        PaletteEntry::new("Export MIDI…",          "Ctrl+E",       AppCommand::ExportMidi),
        PaletteEntry::new("Export Audio…",         "",             AppCommand::ExportAudio),
        PaletteEntry::new("Undo",                  "Ctrl+Z",       AppCommand::Undo),
        PaletteEntry::new("Redo",                  "Ctrl+Y",       AppCommand::Redo),
        PaletteEntry::new("Routing Editor",        "6",            AppCommand::ShowRoutingConfig),
        PaletteEntry::new("Audio Settings…",       "",             AppCommand::ShowAudioSettings),
        PaletteEntry::new("MIDI Settings…",        "",             AppCommand::ShowMidiSettings),
        PaletteEntry::new("Keyboard Shortcuts",    "F1",           AppCommand::ShowKeybindings),
        PaletteEntry::new("About SeqTerm",         "F12",          AppCommand::ShowAbout),
        PaletteEntry::new("Help: Workflow Guide",  "",             AppCommand::ShowHelp(HelpTopic::WorkflowGuide)),
        PaletteEntry::new("Help: MIDI Import",     "",             AppCommand::ShowHelp(HelpTopic::MidiImport)),
        PaletteEntry::new("Help: Routing",         "",             AppCommand::ShowHelp(HelpTopic::Routing)),
        PaletteEntry::new("Help: Pattern Editor",  "",             AppCommand::ShowHelp(HelpTopic::PatternEditor)),
        PaletteEntry::new("Help: Troubleshooting", "",             AppCommand::ShowHelp(HelpTopic::Troubleshooting)),
        PaletteEntry::new("Help: Latency Tips",    "",             AppCommand::ShowHelp(HelpTopic::LatencyOptimization)),
        PaletteEntry::new("Exit",                  "q / Ctrl+Q",   AppCommand::Exit),
    ]
}

#[derive(Debug)]
pub struct CommandPaletteState {
    pub query:   String,
    pub cursor:  usize,
    /// Cached filtered results (indices into `all_palette_entries()`).
    pub results: Vec<PaletteEntry>,
}

impl CommandPaletteState {
    pub fn new() -> Self {
        let results = all_palette_entries();
        Self { query: String::new(), cursor: 0, results }
    }

    pub fn update_filter(&mut self) {
        use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};

        let q = self.query.trim();
        if q.is_empty() {
            self.results = all_palette_entries();
        } else {
            let matcher = SkimMatcherV2::default();
            let mut scored: Vec<(i64, PaletteEntry)> = all_palette_entries()
                .into_iter()
                .filter_map(|e| {
                    matcher.fuzzy_match(&e.label, q).map(|score| (score, e))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0));
            self.results = scored.into_iter().map(|(_, e)| e).collect();
        }
        self.cursor = self.cursor.min(self.results.len().saturating_sub(1));
    }

    pub fn selected(&self) -> Option<AppCommand> {
        self.results.get(self.cursor).map(|e| e.command.clone())
    }
}

// ─── MIDI import options ──────────────────────────────────────────────────────

#[derive(Debug)]
pub struct MidiImportOptionsState {
    /// The MIDI file path to be imported once options are confirmed.
    pub path:   PathBuf,
    /// Current import options (editable by the user in this dialog).
    pub opts:   MidiImportOptions,
    /// Which row is focused: 0=bars_per_pattern, 1=steps_per_beat, 2=detect_drums.
    pub cursor: usize,
    /// Track info from a quick pre-scan (name, channel, note count, drum flag).
    pub track_infos: Vec<MidiTrackInfo>,
}

impl MidiImportOptionsState {
    pub fn new(path: PathBuf) -> Self {
        let track_infos = seqterm_midi_io::probe_midi(&path).unwrap_or_default();
        Self { path, opts: MidiImportOptions::default(), cursor: 0, track_infos }
    }
}

// ─── Keybindings editor ───────────────────────────────────────────────────────

/// State for the keybindings editor modal.
#[derive(Debug)]
pub struct KeybindingsEditorState {
    /// Working copy of the bindings (from settings).
    pub bindings: Vec<KeyBinding>,
    /// Cursor row in the binding list.
    pub cursor: usize,
    /// When Some, we are waiting for the user to press a new key for this action.
    pub rebinding: Option<String>,
    /// Dirty flag: true if bindings were changed and need to be saved.
    pub dirty: bool,
}

impl KeybindingsEditorState {
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        Self { bindings, cursor: 0, rebinding: None, dirty: false }
    }

    /// Returns indices of bindings that share the same key+modifiers as another.
    pub fn conflicts(&self) -> Vec<usize> {
        let mut conflicts = Vec::new();
        for (i, a) in self.bindings.iter().enumerate() {
            for (j, b) in self.bindings.iter().enumerate() {
                if i != j && a.key == b.key && a.modifiers == b.modifiers && !a.key.is_empty() {
                    conflicts.push(i);
                    break;
                }
            }
        }
        conflicts
    }
}

// ─── Audio export options ─────────────────────────────────────────────────────

pub const EXPORT_SAMPLE_RATES: &[u32] = &[44100, 48000, 96000];
pub const EXPORT_BIT_DEPTHS:   &[u8]  = &[16, 24, 32];

#[derive(Debug)]
pub struct AudioExportOptionsState {
    /// 0 = sample rate, 1 = bit depth, 2 = mode
    pub cursor:          usize,
    pub sample_rate_idx: usize,
    pub bit_depth_idx:   usize,
    pub stems:           bool,
}

impl AudioExportOptionsState {
    pub fn new(opts: &AudioExportOpts) -> Self {
        let sample_rate_idx = EXPORT_SAMPLE_RATES
            .iter().position(|&r| r == opts.sample_rate).unwrap_or(1);
        let bit_depth_idx = EXPORT_BIT_DEPTHS
            .iter().position(|&d| d == opts.bit_depth).unwrap_or(0);
        Self {
            cursor: 0,
            sample_rate_idx,
            bit_depth_idx,
            stems: opts.mode == ExportMode::Stems,
        }
    }

    pub fn to_opts(&self) -> AudioExportOpts {
        AudioExportOpts {
            sample_rate: EXPORT_SAMPLE_RATES[self.sample_rate_idx],
            bit_depth:   EXPORT_BIT_DEPTHS[self.bit_depth_idx],
            mode: if self.stems { ExportMode::Stems } else { ExportMode::Mixdown },
        }
    }
}

// ─── Alert kind ───────────────────────────────────────────────────────────────

/// Controls the border color of an Alert modal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlertKind {
    /// Neutral info (blue accent).
    Info,
    /// Operation succeeded (green).
    Success,
    /// Operation failed or warning (red).
    Error,
}

// ─── Modal enum ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum Modal {
    Alert   { title: String, message: String, kind: AlertKind },
    Confirm { title: String, body: String, on_confirm: AppCommand },
    Progress { title: String, message: String, progress: f32, cancelable: bool },
    Input(InputDialogState),
    FilePicker(FilePickerState),
    About,
    Help(HelpState),
    AudioSettings(AudioSettingsState),
    MidiSettings(MidiSettingsState),
    CommandPalette(CommandPaletteState),
    MidiImportOptions(MidiImportOptionsState),
    KeybindingsEditor(KeybindingsEditorState),
    AudioExportOptions(AudioExportOptionsState),
    /// SF2 preset browser — shown after an SF2 file is picked.
    Sf2Browser(Sf2BrowserState),
    /// VST2 plugin parameter browser — floating overlay for live parameter editing.
    PluginParams(PluginParamBrowserState),
}

// ─── SF2 preset browser ───────────────────────────────────────────────────────

/// State for the SF2 preset browser modal.
#[derive(Debug)]
pub struct Sf2BrowserState {
    /// The SF2 file that was picked.
    pub path: std::path::PathBuf,
    /// Which matrix clip this is for.
    pub row: usize,
    pub col: usize,
    /// Currently selected bank (0-127).
    pub bank: u8,
    /// Currently selected preset (0-127).
    pub preset: u8,
    /// Flat list of (bank, preset, name) from the SF2 file.
    pub presets: Vec<(u8, u8, String)>,
    /// Cursor position in the presets list.
    pub cursor: usize,
    /// Scroll offset.
    pub scroll: usize,
    /// Whether the bank/preset list has been loaded.
    pub loaded: bool,
    /// Audio engine slot used for preview playback (Space key).
    pub preview_slot: Option<u32>,
    /// True once the preview slot's synth has been installed and a NoteOn fired.
    pub preview_loaded: bool,
}

impl Sf2BrowserState {
    pub fn new(path: std::path::PathBuf, row: usize, col: usize) -> Self {
        Self {
            path,
            row,
            col,
            bank: 0,
            preset: 0,
            presets: Vec::new(),
            cursor: 0,
            scroll: 0,
            loaded: false,
            preview_slot: None,
            preview_loaded: false,
        }
    }

    /// Populate preset list from a loaded SF2 (called on background thread result).
    pub fn set_presets(&mut self, presets: Vec<(u8, u8, String)>) {
        self.presets = presets;
        self.loaded = true;
    }

    /// Scroll the list to keep cursor visible.
    pub fn clamp_scroll(&mut self, viewport_height: usize) {
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + viewport_height {
            self.scroll = self.cursor + 1 - viewport_height;
        }
    }

    /// Current selection as (bank, preset, name).
    pub fn selected(&self) -> Option<(u8, u8, &str)> {
        self.presets.get(self.cursor).map(|(b, p, n)| (*b, *p, n.as_str()))
    }
}

impl Modal {
    pub fn alert(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Alert { title: title.into(), message: message.into(), kind: AlertKind::Info }
    }

    pub fn error(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Alert { title: title.into(), message: message.into(), kind: AlertKind::Error }
    }

    pub fn success(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Alert { title: title.into(), message: message.into(), kind: AlertKind::Success }
    }

    pub fn confirm(
        title: impl Into<String>,
        body: impl Into<String>,
        on_confirm: AppCommand,
    ) -> Self {
        Self::Confirm { title: title.into(), body: body.into(), on_confirm }
    }

    pub fn progress(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Progress {
            title:       title.into(),
            message:     message.into(),
            progress:    0.0,
            cancelable:  true,
        }
    }

    pub fn input(
        title: impl Into<String>,
        placeholder: impl Into<String>,
        on_submit: fn(String) -> AppCommand,
    ) -> Self {
        Self::Input(InputDialogState::new(title, placeholder, on_submit))
    }
}

// ─── Plugin parameter browser ─────────────────────────────────────────────────

/// A cached snapshot of one plugin parameter's metadata + value.
#[derive(Debug, Clone)]
pub struct PluginParamEntry {
    pub id:      u32,
    pub name:    String,
    pub label:   String,
    pub display: String,
    pub value:   f32,
}

/// State for the floating VST2 plugin parameter browser modal.
#[derive(Debug)]
pub struct PluginParamBrowserState {
    /// Registry ID of the plugin instance being edited.
    pub registry_id: u64,
    /// Plugin display name.
    pub plugin_name: String,
    /// Cached parameter list (refreshed on open and after each set_param).
    pub params: Vec<PluginParamEntry>,
    /// Cursor position in the parameter list.
    pub cursor: usize,
    /// Scroll offset.
    pub scroll: usize,
    /// True when the value of the selected parameter is being nudged.
    pub editing: bool,
}

impl PluginParamBrowserState {
    pub fn new(registry_id: u64, plugin_name: impl Into<String>) -> Self {
        Self {
            registry_id,
            plugin_name: plugin_name.into(),
            params: Vec::new(),
            cursor: 0,
            scroll: 0,
            editing: false,
        }
    }

    /// Reload parameter list from the registry.
    pub fn refresh(&mut self, registry: &seqterm_application::PluginRegistry) {
        let count = registry.param_count(self.registry_id);
        self.params = (0..count)
            .map(|id| PluginParamEntry {
                id,
                name:    registry.param_name(self.registry_id, id),
                label:   registry.param_label(self.registry_id, id),
                display: registry.param_display(self.registry_id, id),
                value:   registry.get_param(self.registry_id, id),
            })
            .collect();
    }

    pub fn clamp_scroll(&mut self, viewport_height: usize) {
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if viewport_height > 0 && self.cursor >= self.scroll + viewport_height {
            self.scroll = self.cursor + 1 - viewport_height;
        }
    }
}
