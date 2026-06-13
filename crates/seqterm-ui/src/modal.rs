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
    /// Create an audio clip on the arrangement timeline at a beat (Milestone C).
    AssignAudioToArrangement { track_idx: usize, start_num: i64, start_den: i64 },
    /// Assign a sample to a sampler pad (bank, pad).
    AssignSampleToPad { bank: usize, pad: usize },
    /// Pick an SF2 file to be used as the synth for an entire MIDI import.
    AssignSf2ForMidiImport,
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
            Self::AssignSf2 { .. }            => "Assign SF2 SoundFont",
            Self::AssignAudioFile { .. }      => "Assign Audio File",
            Self::AssignAudioToArrangement { .. } => "Audio Clip → Arrangement",
            Self::AssignSampleToPad { .. }    => "Assign Sample to Pad",
            Self::AssignSf2ForMidiImport      => "SF2 for MIDI Import",
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
            Self::AssignSf2 { .. }           => &["sf2", "SF2"],
            Self::AssignAudioFile { .. }     => &["wav", "flac", "mp3", "ogg"],
            Self::AssignAudioToArrangement { .. } => &["wav", "flac", "mp3", "ogg"],
            Self::AssignSampleToPad { .. }   => &["wav", "flac", "mp3", "ogg", "aiff"],
            Self::AssignSf2ForMidiImport     => &["sf2", "SF2"],
        }
    }

    pub fn mode(&self) -> FilePickerMode {
        match self {
            Self::OpenProject
            | Self::ImportMidi
            | Self::ImportKeybindings
            | Self::AssignSf2 { .. }
            | Self::AssignAudioFile { .. }
            | Self::AssignAudioToArrangement { .. }
            | Self::AssignSampleToPad { .. }
            | Self::AssignSf2ForMidiImport => FilePickerMode::Open,
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
            Self::AssignAudioToArrangement { track_idx, start_num, start_den } =>
                AppCommand::ConfirmArrangementAudioClip { track_idx, start_num, start_den, path },
            Self::AssignSampleToPad { bank, pad } => AppCommand::ConfirmSampleAssignment { bank, pad, path },
            Self::AssignSf2ForMidiImport => AppCommand::SetMidiImportSf2(path),
        }
    }
}

// ─── Sidebar (directory tree + bookmarks) ────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarItemKind {
    Header,
    Bookmark,
    Recent,
    TreeAncestor,
    TreeCurrent,
    TreeChild,
}

#[derive(Debug, Clone)]
pub struct SidebarEntry {
    pub label: String,
    pub path:  Option<PathBuf>,
    pub depth: usize,
    pub kind:  SidebarItemKind,
}

fn build_sidebar(current_dir: &Path, recent_dirs: &[PathBuf]) -> Vec<SidebarEntry> {
    let home: PathBuf = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"));
    let mut out: Vec<SidebarEntry> = Vec::new();

    // ── Places ────────────────────────────────────────────────────────────────
    out.push(SidebarEntry { label: "PLACES".into(), path: None, depth: 0, kind: SidebarItemKind::Header });
    for (label, path) in [
        ("Home",      home.clone()),
        ("Desktop",   home.join("Desktop")),
        ("Downloads", home.join("Downloads")),
        ("Documents", home.join("Documents")),
        ("Music",     home.join("Music")),
        ("Pictures",  home.join("Pictures")),
        ("Videos",    home.join("Videos")),
        ("Root",      PathBuf::from("/")),
    ] {
        if path.exists() {
            out.push(SidebarEntry { label: label.into(), path: Some(path), depth: 1, kind: SidebarItemKind::Bookmark });
        }
    }

    // ── Recent ────────────────────────────────────────────────────────────────
    let mut seen = std::collections::HashSet::new();
    let mut recent_items: Vec<&PathBuf> = Vec::new();
    for p in recent_dirs {
        if p.exists() && p.as_path() != current_dir && seen.insert(p.as_path()) {
            recent_items.push(p);
            if recent_items.len() >= 5 { break; }
        }
    }
    if !recent_items.is_empty() {
        out.push(SidebarEntry { label: "RECENT".into(), path: None, depth: 0, kind: SidebarItemKind::Header });
        for dir in recent_items {
            let label = dir.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| dir.display().to_string());
            out.push(SidebarEntry { label, path: Some(dir.clone()), depth: 1, kind: SidebarItemKind::Recent });
        }
    }

    // ── Tree: ancestry chain ──────────────────────────────────────────────────
    out.push(SidebarEntry { label: "TREE".into(), path: None, depth: 0, kind: SidebarItemKind::Header });

    let mut ancestors: Vec<PathBuf> = Vec::new();
    let mut p = current_dir.to_path_buf();
    loop {
        ancestors.push(p.clone());
        match p.parent() {
            Some(par) if par != p => p = par.to_path_buf(),
            _ => break,
        }
    }
    ancestors.reverse();

    let current_depth = ancestors.len().saturating_sub(1);
    for (depth, anc) in ancestors.iter().enumerate() {
        let label = if depth == 0 {
            "/".to_string()
        } else {
            anc.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| anc.display().to_string())
        };
        let kind = if depth == current_depth {
            SidebarItemKind::TreeCurrent
        } else {
            SidebarItemKind::TreeAncestor
        };
        out.push(SidebarEntry { label, path: Some(anc.clone()), depth, kind });
    }

    // Subdirectories of the current directory.
    let child_depth = current_depth + 1;
    if let Ok(rd) = std::fs::read_dir(current_dir) {
        let mut children: Vec<(String, PathBuf)> = rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                if !e.path().is_dir() { return None; }
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') { return None; }
                Some((name, e.path()))
            })
            .collect();
        children.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path) in children {
            out.push(SidebarEntry { label: name, path: Some(path), depth: child_depth, kind: SidebarItemKind::TreeChild });
        }
    }

    out
}

// ─── File entry ───────────────────────────────────────────────────────────────

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
    /// Recently-visited directories for quick-access.
    pub recent_dirs:    Vec<PathBuf>,
    pub show_recent:    bool,
    pub recent_cursor:  usize,
    /// Search/filter string (Open mode only — type to filter entries).
    pub search_input:   String,
    // ── Sidebar ───────────────────────────────────────────────────────────────
    pub sidebar:        Vec<SidebarEntry>,
    pub sidebar_cursor: usize,
    pub sidebar_scroll: usize,
    /// When true, keyboard focus is in the sidebar; otherwise in the file list.
    pub tree_focused:   bool,
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
            sidebar: Vec::new(),
            sidebar_cursor: 0,
            sidebar_scroll: 0,
            tree_focused: false,
        };
        s.refresh();
        s
    }

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
        self.sidebar = build_sidebar(&self.current_dir, &self.recent_dirs);
        self.sidebar_cursor = self.find_current_sidebar_idx();
        self
    }

    fn find_current_sidebar_idx(&self) -> usize {
        self.sidebar.iter()
            .position(|e| e.kind == SidebarItemKind::TreeCurrent)
            .or_else(|| self.sidebar.iter().position(|e| e.kind != SidebarItemKind::Header))
            .unwrap_or(0)
    }

    pub fn refresh(&mut self) {
        self.entries = read_dir_entries(&self.current_dir, self.target.extensions());
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
        self.scroll = 0;
        self.sidebar = build_sidebar(&self.current_dir, &self.recent_dirs);
        self.sidebar_cursor = self.find_current_sidebar_idx();
    }

    /// Navigate sidebar cursor down, skipping headers.
    pub fn sidebar_move_down(&mut self) {
        let len = self.sidebar.len();
        let mut next = self.sidebar_cursor + 1;
        while next < len && self.sidebar[next].kind == SidebarItemKind::Header { next += 1; }
        if next < len { self.sidebar_cursor = next; }
    }

    /// Navigate sidebar cursor up, skipping headers.
    pub fn sidebar_move_up(&mut self) {
        if self.sidebar_cursor == 0 { return; }
        let mut prev = self.sidebar_cursor.saturating_sub(1);
        while prev > 0 && self.sidebar[prev].kind == SidebarItemKind::Header { prev -= 1; }
        if self.sidebar[prev].kind != SidebarItemKind::Header {
            self.sidebar_cursor = prev;
        }
    }

    /// Navigate to a directory, rebuild entries and sidebar.
    pub fn navigate_to(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.current_dir = path;
            self.cursor = 0;
            self.search_input.clear();
            self.tree_focused = false;
            self.refresh();
        }
    }

    pub fn clamp_sidebar_scroll(&mut self, visible_h: usize) {
        if visible_h == 0 { return; }
        if self.sidebar_cursor < self.sidebar_scroll {
            self.sidebar_scroll = self.sidebar_cursor;
        } else if self.sidebar_cursor >= self.sidebar_scroll + visible_h {
            self.sidebar_scroll = self.sidebar_cursor + 1 - visible_h;
        }
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

/// Which tab of the AUDIO SETTINGS modal is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioTab {
    Engine,
    PluginPaths,
    Osc,
}

impl AudioTab {
    pub const ALL: [AudioTab; 3] = [AudioTab::Engine, AudioTab::PluginPaths, AudioTab::Osc];
    pub fn label(&self) -> &'static str {
        match self {
            AudioTab::Engine      => "Engine",
            AudioTab::PluginPaths => "Plugin Paths",
            AudioTab::Osc         => "OSC",
        }
    }
    pub fn index(&self) -> usize {
        Self::ALL.iter().position(|t| t == self).unwrap_or(0)
    }
}

/// Which pane has focus inside the Plugin Paths tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginPathFocus { Formats, Dirs }

#[derive(Debug)]
pub struct AudioSettingsState {
    pub tab: AudioTab,
    /// Engine-tab row cursor.
    pub cursor: usize,
    /// Scratch buffer for the display value being edited.
    pub editing_idx: Option<usize>,
    /// Snapshot of backend + sample_rate at the time the modal was opened,
    /// used to detect whether a restart is required on save.
    pub original_backend:     String,
    pub original_sample_rate: u32,

    // ── Plugin Paths tab ──────────────────────────────────────────────────────
    /// Index into `PLUGIN_PATH_FORMATS` of the selected format.
    pub fmt_cursor: usize,
    /// Index of the selected directory within the focused format's list.
    pub dir_cursor: usize,
    pub pp_focus:   PluginPathFocus,
    /// When `Some`, an inline text editor for a new directory path is open.
    pub path_input: Option<String>,

    // ── OSC tab ───────────────────────────────────────────────────────────────
    /// OSC-tab row cursor (0=enable, 1=mode, 2=udp, 3=tcp).
    pub osc_cursor: usize,
    /// When `Some`, an inline numeric editor for a port is open.
    pub port_input: Option<String>,
    /// Snapshot of OSC enable+udp at open, to detect whether to (re)start the server.
    pub original_osc_enabled: bool,
    pub original_osc_udp:     u16,
}

impl AudioSettingsState {
    pub fn new() -> Self {
        Self {
            tab: AudioTab::Engine,
            cursor: 0,
            editing_idx: None,
            original_backend: String::new(),
            original_sample_rate: 0,
            fmt_cursor: 0,
            dir_cursor: 0,
            pp_focus: PluginPathFocus::Formats,
            path_input: None,
            osc_cursor: 0,
            port_input: None,
            original_osc_enabled: false,
            original_osc_udp: 0,
        }
    }

    pub fn with_snapshot(backend: String, sample_rate: u32) -> Self {
        Self {
            original_backend: backend,
            original_sample_rate: sample_rate,
            ..Self::new()
        }
    }

    /// Record the OSC snapshot used to detect changes on save.
    pub fn with_osc_snapshot(mut self, enabled: bool, udp: u16) -> Self {
        self.original_osc_enabled = enabled;
        self.original_osc_udp = udp;
        self
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
    pub path:        PathBuf,
    pub opts:        MidiImportOptions,
    /// 0=bars_per_pattern, 1=steps_per_beat, 2=detect_drums, 3=sf2_file.
    pub cursor:      usize,
    pub track_infos: Vec<MidiTrackInfo>,
}

impl MidiImportOptionsState {
    pub fn new(path: PathBuf) -> Self {
        Self::with_last_sf2(path, None)
    }

    pub fn with_last_sf2(path: PathBuf, last_sf2: Option<std::path::PathBuf>) -> Self {
        let track_infos = seqterm_midi_io::probe_midi(&path).unwrap_or_default();
        let opts = MidiImportOptions { sf2_path: last_sf2, ..MidiImportOptions::default() };
        Self { path, opts, cursor: 0, track_infos }
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
    /// Three-button quit dialog: Save & Exit / Exit without saving / Cancel.
    QuitConfirm,
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
    /// Source picker — choose MIDI / SF2 / AudioFile for a matrix clip.
    SourcePicker(SourcePickerState),
    /// FX / plugin picker — choose an internal FX or an external plugin
    /// (VST2 / VST3 / CLAP) to insert into a tracker slot's FX chain.
    FxPicker(FxPickerState),
    /// Pattern picker — choose any project pattern to assign to a matrix cell.
    PatternPicker(PatternPickerState),
    /// Granular live-source picker — a matrix abstraction to choose exactly one
    /// pattern (clip cell) whose audio feeds the granular engine.
    GranularSourcePicker(GranularSourcePickerState),
    /// Audio clip editor — waveform view + trim, gain, normalize, fade.
    AudioEdit(AudioEditState),
    /// Interactive tutorial overlay — step-by-step guided tour.
    Tutorial(TutorialState),
    /// Lua REPL — live interactive scripting terminal.
    LuaRepl(LuaReplState),
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
    /// Currently selected bank (0-127) — kept in sync with banks[bank_cursor].
    pub bank: u8,
    /// Currently selected preset (0-127).
    pub preset: u8,
    /// Flat list of (bank, preset, name) from the SF2 file (all banks).
    pub presets: Vec<(u8, u8, String)>,
    /// Unique sorted banks available in this SF2.
    pub banks: Vec<u8>,
    /// Index into `banks` — the currently displayed bank combobox value.
    pub bank_cursor: usize,
    /// Cursor position in the **filtered** preset list (within the selected bank).
    pub cursor: usize,
    /// Scroll offset within the filtered list.
    pub scroll: usize,
    /// Whether the bank/preset list has been loaded.
    pub loaded: bool,
    /// Audio engine slot used for preview playback (Space key).
    pub preview_slot: Option<u32>,
    /// True once the preview slot's synth has been installed and a NoteOn fired.
    pub preview_loaded: bool,
    /// Drum mode: filter presets to percussion bank (bank 128) only.
    pub drum_mode: bool,
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
            banks: Vec::new(),
            bank_cursor: 0,
            cursor: 0,
            scroll: 0,
            loaded: false,
            preview_slot: None,
            preview_loaded: false,
            drum_mode: false,
        }
    }

    /// Populate preset list from a loaded SF2.
    /// Derives the unique bank list and resets cursor/scroll.
    /// In drum mode, auto-selects bank 128 (GM percussion) if available.
    pub fn set_presets(&mut self, presets: Vec<(u8, u8, String)>) {
        let mut banks: Vec<u8> = presets.iter().map(|(b, _, _)| *b).collect();
        banks.sort();
        banks.dedup();
        self.banks = banks;
        self.presets = presets;
        self.loaded = true;
        self.cursor = 0;
        self.scroll = 0;
        if self.drum_mode {
            // Jump directly to bank 128 (percussion) or the nearest available bank.
            self.bank_cursor = self.banks.iter().position(|&b| b == 128)
                .unwrap_or(0);
        } else {
            self.bank_cursor = 0;
        }
        self.bank = self.banks.get(self.bank_cursor).copied().unwrap_or(0);
    }

    /// Bank value for the current combobox position.
    pub fn selected_bank(&self) -> u8 {
        self.banks.get(self.bank_cursor).copied().unwrap_or(0)
    }

    /// Presets visible in the list — only those in the selected bank.
    pub fn filtered_presets(&self) -> Vec<&(u8, u8, String)> {
        let bank = self.selected_bank();
        self.presets.iter().filter(|(b, _, _)| *b == bank).collect()
    }

    /// Advance the bank combobox by `delta` positions (wraps around).
    pub fn shift_bank(&mut self, delta: i32) {
        let n = self.banks.len();
        if n == 0 { return; }
        self.bank_cursor = ((self.bank_cursor as i32 + delta).rem_euclid(n as i32)) as usize;
        self.bank = self.selected_bank();
        self.cursor = 0;
        self.scroll = 0;
    }

    /// Scroll the filtered list to keep cursor visible.
    pub fn clamp_scroll(&mut self, viewport_height: usize) {
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if viewport_height > 0 && self.cursor >= self.scroll + viewport_height {
            self.scroll = self.cursor + 1 - viewport_height;
        }
    }

    /// Current selection as (bank, preset, name) from the filtered list.
    pub fn selected(&self) -> Option<(u8, u8, String)> {
        let fp = self.filtered_presets();
        fp.get(self.cursor).map(|(b, p, n)| (*b, *p, n.clone()))
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
    /// Universal-model snapshot of the same parameters (descriptor + type +
    /// range + default), used by the inspector to auto-generate controls.
    pub uni: Vec<seqterm_ports::instrument::Parameter>,
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
            uni: Vec::new(),
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
        self.uni = registry.universal_parameters(self.registry_id);
    }

    pub fn clamp_scroll(&mut self, viewport_height: usize) {
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if viewport_height > 0 && self.cursor >= self.scroll + viewport_height {
            self.scroll = self.cursor + 1 - viewport_height;
        }
    }
}

// ─── Source picker ────────────────────────────────────────────────────────────

/// The four input categories shown in the source picker sidebar.
pub const SOURCE_CATEGORIES: [&str; 4] = ["MIDI", "SF2", "AUDIO", "SYNTH"];

/// Which pane of the source picker has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFocus { Categories, List }

/// A discovered synthesizer plugin offered as a note source.
#[derive(Debug, Clone)]
pub struct SynthEntry {
    pub id: String,
    pub format: String,
    pub name: String,
}

/// State for the source picker modal (matrix clip source assignment). Modelled on
/// [`FxPickerState`]: a left category sidebar (MIDI / SF2 / AUDIO / SYNTH) plus a
/// scrollable list of entries for the selected category.
#[derive(Debug)]
pub struct SourcePickerState {
    /// Matrix position.
    pub row: usize,
    pub col: usize,
    /// Highlighted category in the sidebar (index into [`SOURCE_CATEGORIES`]).
    pub cat_cursor: usize,
    /// Which pane has focus.
    pub focus: SourceFocus,
    /// Cursor within the active category's list.
    pub cursor: usize,
    /// Vertical scroll offset of the list.
    pub scroll: usize,
    /// Available MIDI output ports (names).
    pub midi_ports: Vec<String>,
    /// Discovered synthesizer plugins (instrument plugins).
    pub synths: Vec<SynthEntry>,
    /// SYNTH filter chips: "All" + each distinct plugin format present.
    pub synth_formats: Vec<String>,
    /// Active SYNTH format filter (index into `synth_formats`; 0 = "All").
    pub synth_filter: usize,
    /// Current source of the clip (shown as "Now:" label).
    pub current_source_label: String,
    /// Per-row absolute rects of the list (set each render frame for hit-testing).
    pub row_rects: Vec<ratatui::layout::Rect>,
    /// Per-row absolute rects of the sidebar categories.
    pub cat_rects: Vec<ratatui::layout::Rect>,
    /// Per-chip absolute rects of the SYNTH filter bar.
    pub filter_rects: Vec<ratatui::layout::Rect>,
}

impl SourcePickerState {
    pub fn new(
        row: usize,
        col: usize,
        midi_ports: Vec<String>,
        synths: Vec<SynthEntry>,
        current_source_label: String,
    ) -> Self {
        // Build the SYNTH filter chips: "All" then each distinct format present.
        let mut synth_formats = vec!["All".to_string()];
        for s in &synths {
            if !synth_formats.iter().any(|f| f == &s.format) {
                synth_formats.push(s.format.clone());
            }
        }
        Self {
            row, col,
            cat_cursor: 0,
            focus: SourceFocus::Categories,
            cursor: 0,
            scroll: 0,
            midi_ports,
            synths,
            synth_formats,
            synth_filter: 0,
            current_source_label,
            row_rects: Vec::new(),
            cat_rects: Vec::new(),
            filter_rects: Vec::new(),
        }
    }

    /// Indices into `synths` that match the active SYNTH format filter.
    pub fn filtered_synths(&self) -> Vec<usize> {
        let filt = self.synth_formats.get(self.synth_filter).map(|s| s.as_str());
        self.synths.iter().enumerate()
            .filter(|(_, s)| matches!(filt, None | Some("All")) || filt == Some(s.format.as_str()))
            .map(|(i, _)| i)
            .collect()
    }

    /// Resolve the synth entry under the list cursor (through the format filter).
    pub fn selected_synth(&self) -> Option<&SynthEntry> {
        self.filtered_synths().get(self.cursor).and_then(|&i| self.synths.get(i))
    }

    /// Cycle the SYNTH format filter by `delta` (wraps), resetting the list cursor.
    pub fn cycle_filter(&mut self, delta: i32) {
        let n = self.synth_formats.len() as i32;
        if n > 0 {
            self.synth_filter = (self.synth_filter as i32 + delta).rem_euclid(n) as usize;
            self.cursor = 0;
            self.scroll = 0;
        }
    }

    pub fn current_category(&self) -> &str {
        SOURCE_CATEGORIES.get(self.cat_cursor).copied().unwrap_or("MIDI")
    }

    /// Number of selectable rows in the active category's list.
    pub fn list_len(&self) -> usize {
        match self.current_category() {
            "MIDI"  => self.midi_ports.len(),
            "SYNTH" => self.filtered_synths().len(),
            // SF2 / AUDIO each show a single "Browse…" action row.
            _ => 1,
        }
    }

    /// Labels for the active category's list rows.
    pub fn list_labels(&self) -> Vec<String> {
        match self.current_category() {
            "MIDI"  => self.midi_ports.clone(),
            "SYNTH" => self.filtered_synths().iter()
                .map(|&i| { let s = &self.synths[i]; format!("[{}] {}", s.format, s.name) })
                .collect(),
            "SF2"   => vec!["Browse SF2 file…".to_string()],
            _       => vec!["Browse audio file…".to_string()],
        }
    }

    pub fn up(&mut self) {
        match self.focus {
            SourceFocus::Categories => {
                self.cat_cursor = self.cat_cursor.saturating_sub(1);
                self.cursor = 0; self.scroll = 0;
            }
            SourceFocus::List => { self.cursor = self.cursor.saturating_sub(1); }
        }
    }

    pub fn down(&mut self) {
        match self.focus {
            SourceFocus::Categories => {
                if self.cat_cursor + 1 < SOURCE_CATEGORIES.len() { self.cat_cursor += 1; }
                self.cursor = 0; self.scroll = 0;
            }
            SourceFocus::List => {
                if self.cursor + 1 < self.list_len() { self.cursor += 1; }
            }
        }
    }

    pub fn focus_categories(&mut self) { self.focus = SourceFocus::Categories; }
    pub fn focus_list(&mut self) {
        if self.list_len() > 0 { self.focus = SourceFocus::List; }
    }

    pub fn set_category(&mut self, idx: usize) {
        if idx < SOURCE_CATEGORIES.len() {
            self.cat_cursor = idx;
            self.cursor = 0;
            self.scroll = 0;
        }
    }
}

// ─── Audio clip editor ────────────────────────────────────────────────────────

/// State for the audio clip editor modal.
#[derive(Debug, Clone)]
pub struct AudioEditState {
    /// Matrix row (0-15).
    pub row: usize,
    /// Matrix column (0-7).
    pub col: usize,
    /// Audio file path.
    pub path: std::path::PathBuf,
    /// Trim start fraction (0.0–1.0).
    pub trim_start: f32,
    /// Trim end fraction (0.0–1.0).
    pub trim_end: f32,
    /// Linear gain multiplier (1.0 = unity).
    pub gain: f32,
    /// Fade-in length as fraction of clip (0.0 = no fade).
    pub fade_in: f32,
    /// Fade-out length as fraction of clip (0.0 = no fade).
    pub fade_out: f32,
    /// Active cursor field: 0=trim_start, 1=trim_end, 2=gain, 3=fade_in, 4=fade_out.
    pub cursor: usize,
    /// Whether the user has requested a peak-normalize on Apply.
    pub normalize: bool,
    /// Whether Apply has been triggered.
    pub applied: bool,
}

impl AudioEditState {
    pub fn new(row: usize, col: usize, path: std::path::PathBuf, gain: f32) -> Self {
        Self {
            row, col, path,
            trim_start: 0.0, trim_end: 1.0,
            gain: gain.max(0.01),
            fade_in: 0.0, fade_out: 0.0,
            cursor: 0, normalize: false, applied: false,
        }
    }

    pub const FIELD_COUNT: usize = 5;

    pub fn field_label(&self) -> &'static str {
        match self.cursor {
            0 => "Trim Start",
            1 => "Trim End",
            2 => "Gain",
            3 => "Fade In",
            4 => "Fade Out",
            _ => "",
        }
    }

    /// Adjust the current field by `delta` (-1 or +1 mapped to a fine step).
    pub fn adjust(&mut self, delta: f32) {
        match self.cursor {
            0 => self.trim_start = (self.trim_start + delta * 0.01).clamp(0.0, self.trim_end - 0.01),
            1 => self.trim_end   = (self.trim_end   + delta * 0.01).clamp(self.trim_start + 0.01, 1.0),
            2 => self.gain       = (self.gain        + delta * 0.05).clamp(0.0, 4.0),
            3 => self.fade_in    = (self.fade_in     + delta * 0.01).clamp(0.0, 0.5),
            4 => self.fade_out   = (self.fade_out    + delta * 0.01).clamp(0.0, 0.5),
            _ => {}
        }
    }
}

// ─── Tutorial ─────────────────────────────────────────────────────────────────

/// One step in the interactive tutorial.
#[derive(Debug, Clone)]
pub struct TutorialStep {
    /// Short title displayed in the modal header.
    pub title: &'static str,
    /// Full explanation text (may contain newlines).
    pub body:  &'static str,
    /// Optional keyboard shortcut hint to highlight.
    pub hint:  &'static str,
}

/// All tutorial steps, in order.
pub const TUTORIAL_STEPS: &[TutorialStep] = &[
    TutorialStep {
        title: "Welcome to SeqTerm",
        body:  "SeqTerm is a terminal-based modular sequencer / DAW.\n\n\
                The interface is divided into six views:\n\
                  1 = MATRIX     2 = PATTERN    3 = EDITOR\n\
                  4 = SONG       5 = MIXER      6 = CONFIG\n\n\
                Press the number keys at any time to switch views.",
        hint:  "Press → to continue",
    },
    TutorialStep {
        title: "Step 1: Create a Pattern",
        body:  "In the MATRIX view (press 1) you see an 8×8 session grid.\n\n\
                Each cell can hold a clip linked to a pattern.\n\
                Navigate with hjkl, press Enter to create/open a clip.\n\n\
                Switch to PATTERN (2) to edit the step sequence of the\n\
                selected clip with note, velocity, and gate fields.",
        hint:  "1 = Matrix  2 = Pattern",
    },
    TutorialStep {
        title: "Step 2: Assign a Sound",
        body:  "With a clip selected in the Matrix, press Ctrl+O to open\n\
                the SF2 browser and assign a SoundFont instrument.\n\n\
                Audio files (.wav/.flac/.mp3) can be assigned with Ctrl+A.\n\
                Each row in the matrix maps to one mixer channel.",
        hint:  "Ctrl+O = SF2 browser  Ctrl+A = audio file",
    },
    TutorialStep {
        title: "Step 3: Play",
        body:  "Press Space to start / stop playback.\n\
                The BPM can be changed with Ctrl+↑ / Ctrl+↓ or in CONFIG (6).\n\n\
                Patterns loop automatically; the SONG view (4) lets you\n\
                arrange clips into a full song timeline.",
        hint:  "Space = play/stop  Ctrl+↑↓ = BPM",
    },
    TutorialStep {
        title: "Step 4: Mix",
        body:  "The MIXER (5) has per-channel volume, pan, EQ, and sends.\n\n\
                Use ↑↓ to adjust volume, Enter to enter edit mode,\n\
                Tab to focus the FX sidebar where you can add effects.\n\n\
                Press G to route a channel to a group bus.\n\
                Press \\ to open the audio routing matrix.",
        hint:  "5 = Mixer  G = group bus  \\ = routing matrix",
    },
    TutorialStep {
        title: "Step 5: Export",
        body:  "Press Ctrl+E to export the full mix to a WAV file.\n\
                The SONG view's B key bounces a track to audio in-place.\n\n\
                Projects are saved as .json or .stz archives.\n\
                Ctrl+S saves, Ctrl+Shift+S saves as a new file.\n\n\
                That's the basics! Press F1 at any time for the help overlay.",
        hint:  "Ctrl+E = export  Ctrl+S = save  F1 = help",
    },
];

/// State for the tutorial modal.
#[derive(Debug, Clone)]
pub struct TutorialState {
    /// Current step index (0-based).
    pub step: usize,
}

impl TutorialState {
    pub fn new() -> Self { Self { step: 0 } }

    pub fn current(&self) -> &'static TutorialStep {
        &TUTORIAL_STEPS[self.step.min(TUTORIAL_STEPS.len() - 1)]
    }

    pub fn is_last(&self) -> bool {
        self.step + 1 >= TUTORIAL_STEPS.len()
    }

    pub fn next(&mut self) {
        if !self.is_last() { self.step += 1; }
    }

    pub fn progress(&self) -> String {
        format!("{}/{}", self.step + 1, TUTORIAL_STEPS.len())
    }
}

// ─── Lua REPL ────────────────────────────────────────────────────────────────

/// State for the interactive Lua REPL modal.
#[derive(Debug, Clone)]
pub struct LuaReplState {
    /// Current input line.
    pub input: String,
    /// Output history: (line, is_error).
    pub history: Vec<(String, bool)>,
    /// Scroll offset into the history (from bottom).
    pub scroll: usize,
}

impl LuaReplState {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            history: vec![
                ("Lua 5.4  —  SeqTerm scripting REPL".to_string(), false),
                ("Type Lua expressions and press Enter.  Esc to close.".to_string(), false),
                ("API: seqterm.status(msg), seqterm.set_bpm(bpm)".to_string(), false),
            ],
            scroll: 0,
        }
    }

    /// Append an output line.
    pub fn push_output(&mut self, line: impl Into<String>, is_error: bool) {
        self.history.push((line.into(), is_error));
        self.scroll = 0; // snap to latest
    }
}

// ─── FX / plugin picker ───────────────────────────────────────────────────────

/// One selectable entry in the FX picker: an internal DSP effect or an external
/// plugin discovered by the plugin host (VST2 / VST3 / CLAP / …).
#[derive(Debug, Clone)]
pub enum FxPickerEntry {
    /// A built-in effect processor.
    Internal(crate::app::AudioFxKind),
    /// An external plugin discovered via the plugin registry.
    Plugin {
        /// Registry plugin id (path/uid).
        id: String,
        /// Display name.
        name: String,
        /// Format tag, e.g. "VST2", "VST3", "CLAP".
        format: String,
    },
}

impl FxPickerEntry {
    /// Display label for the list row.
    pub fn label(&self) -> String {
        match self {
            FxPickerEntry::Internal(k) => format!("[FX]   {}", k.label()),
            FxPickerEntry::Plugin { name, format, .. } => format!("[{format}] {name}"),
        }
    }
}

/// Which pane of the FX picker has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxPickerFocus { Categories, List }

/// Does `entry` belong to category `cat`? `"All"` matches everything, `"FX"`
/// matches built-in effects, and any other key matches a plugin's format tag.
pub(crate) fn fx_entry_in_category(cat: &str, entry: &FxPickerEntry) -> bool {
    match (cat, entry) {
        ("All", _)                              => true,
        ("FX", FxPickerEntry::Internal(_))      => true,
        (c, FxPickerEntry::Plugin { format, .. }) => c == format,
        _                                        => false,
    }
}

/// State for the FX / plugin picker modal. Opened with Enter or a double-click
/// on a tracker FX-chain slot; inserts the chosen processor at `insert_idx`.
///
/// A left sidebar filters entries by category (All / FX / VST2 / LV2 / …); the
/// right pane is the scrollable list of entries in the selected category.
#[derive(Debug)]
pub struct FxPickerState {
    /// Audio engine slot whose FX chain is being edited.
    pub slot_id: u32,
    /// Insert position within the slot's FX chain.
    pub insert_idx: usize,
    /// All selectable entries (internal FX first, then discovered plugins).
    pub entries: Vec<FxPickerEntry>,
    /// Sidebar category keys (e.g. "All", "FX", "VST2", "LV2", …).
    pub categories: Vec<String>,
    /// Highlighted category in the sidebar.
    pub cat_cursor: usize,
    /// Which pane currently has focus.
    pub focus: FxPickerFocus,
    /// Highlighted entry **within the filtered list**.
    pub cursor: usize,
    /// Vertical scroll offset of the filtered list.
    pub scroll: usize,
    /// Per-row absolute rects for the list, set each render frame for hit-testing.
    pub row_rects: Vec<ratatui::layout::Rect>,
    /// Per-row absolute rects for the sidebar categories.
    pub cat_rects: Vec<ratatui::layout::Rect>,
}

impl FxPickerState {
    pub fn new(slot_id: u32, insert_idx: usize, entries: Vec<FxPickerEntry>) -> Self {
        // Build the category list from what's actually present, in a stable order:
        // "All" first, then "FX" if any built-in effect, then each distinct plugin
        // format tag in order of first appearance.
        let mut categories = vec!["All".to_string()];
        if entries.iter().any(|e| matches!(e, FxPickerEntry::Internal(_))) {
            categories.push("FX".to_string());
        }
        for e in &entries {
            if let FxPickerEntry::Plugin { format, .. } = e
                && !categories.iter().any(|c| c == format)
            {
                categories.push(format.clone());
            }
        }
        Self {
            slot_id, insert_idx, entries, categories,
            cat_cursor: 0,
            focus: FxPickerFocus::Categories,
            cursor: 0,
            scroll: 0,
            row_rects: Vec::new(),
            cat_rects: Vec::new(),
        }
    }

    /// The currently selected category key.
    pub fn current_category(&self) -> &str {
        self.categories.get(self.cat_cursor).map(|s| s.as_str()).unwrap_or("All")
    }

    /// Indices into `entries` that belong to the selected category.
    pub fn filtered(&self) -> Vec<usize> {
        let cat = self.current_category();
        self.entries.iter().enumerate()
            .filter(|(_, e)| fx_entry_in_category(cat, e))
            .map(|(i, _)| i)
            .collect()
    }

    /// Number of entries visible under the current category filter.
    pub fn visible_len(&self) -> usize { self.filtered().len() }

    pub fn up(&mut self) {
        match self.focus {
            FxPickerFocus::Categories => {
                self.cat_cursor = self.cat_cursor.saturating_sub(1);
                self.cursor = 0;
                self.scroll = 0;
            }
            FxPickerFocus::List => { self.cursor = self.cursor.saturating_sub(1); }
        }
    }

    pub fn down(&mut self) {
        match self.focus {
            FxPickerFocus::Categories => {
                if self.cat_cursor + 1 < self.categories.len() { self.cat_cursor += 1; }
                self.cursor = 0;
                self.scroll = 0;
            }
            FxPickerFocus::List => {
                if self.cursor + 1 < self.visible_len() { self.cursor += 1; }
            }
        }
    }

    /// Move focus to the categories sidebar.
    pub fn focus_categories(&mut self) { self.focus = FxPickerFocus::Categories; }
    /// Move focus to the list (only if the current category has entries).
    pub fn focus_list(&mut self) {
        if self.visible_len() > 0 { self.focus = FxPickerFocus::List; }
    }

    /// Select category by index, resetting the list cursor/scroll.
    pub fn set_category(&mut self, idx: usize) {
        if idx < self.categories.len() {
            self.cat_cursor = idx;
            self.cursor = 0;
            self.scroll = 0;
        }
    }

    /// The entry under the list cursor, resolved through the category filter.
    pub fn selected(&self) -> Option<&FxPickerEntry> {
        let filtered = self.filtered();
        filtered.get(self.cursor).and_then(|&i| self.entries.get(i))
    }
}

/// State for the pattern picker modal. Lists every project pattern; the chosen
/// one is assigned to matrix cell (`row`, `col`).
/// Where a [`PatternPickerState`] selection lands when confirmed.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternPickerTarget {
    /// Assign the pattern to the matrix cell `(row, col)` (the original behavior).
    Matrix,
    /// Create a rational arrangement clip on `track_idx` starting at the beat
    /// `start = (num, den)` (Phase 4 clip creation).
    Arrangement { track_idx: usize, start_num: i64, start_den: i64 },
}

#[derive(Debug)]
pub struct PatternPickerState {
    pub row: usize,
    pub col: usize,
    pub patterns: Vec<String>,
    pub cursor: usize,
    pub scroll: usize,
    /// Per-row absolute rects for mouse hit-testing (set each render frame).
    pub row_rects: Vec<ratatui::layout::Rect>,
    /// What the confirmed selection does.
    pub target: PatternPickerTarget,
}

impl PatternPickerState {
    pub fn new(row: usize, col: usize, patterns: Vec<String>) -> Self {
        Self {
            row, col, patterns, cursor: 0, scroll: 0,
            row_rects: Vec::new(),
            target: PatternPickerTarget::Matrix,
        }
    }

    /// Picker that creates an arrangement clip on `track_idx` at the given beat.
    pub fn for_arrangement(track_idx: usize, start: seqterm_core::RationalTime, patterns: Vec<String>) -> Self {
        Self {
            row: 0, col: 0, patterns, cursor: 0, scroll: 0,
            row_rects: Vec::new(),
            target: PatternPickerTarget::Arrangement {
                track_idx,
                start_num: start.num(),
                start_den: start.den(),
            },
        }
    }

    pub fn up(&mut self)   { self.cursor = self.cursor.saturating_sub(1); }
    pub fn down(&mut self) {
        if self.cursor + 1 < self.patterns.len() { self.cursor += 1; }
    }
    pub fn selected(&self) -> Option<&String> { self.patterns.get(self.cursor) }
}

// ─── Granular live-source picker ──────────────────────────────────────────────

/// State for the granular live-source picker: a compact abstraction of the
/// matrix grid where exactly one pattern (clip cell) can be chosen as the live
/// resampling source. Resolves the ambiguity of the per-row PATTERN bar when a
/// matrix row holds more than one pattern.
#[derive(Debug)]
pub struct GranularSourcePickerState {
    /// Matrix dimensions being shown.
    pub rows: usize,
    pub cols: usize,
    /// (row, col) -> (slot_id, short pattern label) for cells that have audio.
    pub sources: std::collections::HashMap<(usize, usize), (u32, String)>,
    /// Cursor cell in the grid.
    pub cursor: (usize, usize),
    /// Currently active live-source slot (marked ◆), if any.
    pub current: Option<u32>,
    /// Per-cell hit rects, set each render frame: ((row, col), rect).
    pub cell_rects: Vec<((usize, usize), ratatui::layout::Rect)>,
    /// Rect of the "OFF" (clear source) button.
    pub off_rect: ratatui::layout::Rect,
}

impl GranularSourcePickerState {
    pub fn new(
        rows: usize,
        cols: usize,
        sources: std::collections::HashMap<(usize, usize), (u32, String)>,
        current: Option<u32>,
    ) -> Self {
        // Start on the current source if present, else the first source cell.
        let cursor = sources.iter()
            .find(|(_, (sid, _))| Some(*sid) == current)
            .map(|(&rc, _)| rc)
            .or_else(|| sources.keys().min().copied())
            .unwrap_or((0, 0));
        Self {
            rows: rows.max(1),
            cols: cols.max(1),
            sources,
            cursor,
            current,
            cell_rects: Vec::new(),
            off_rect: ratatui::layout::Rect::default(),
        }
    }

    pub fn left(&mut self)  { if self.cursor.1 > 0 { self.cursor.1 -= 1; } }
    pub fn right(&mut self) { if self.cursor.1 + 1 < self.cols { self.cursor.1 += 1; } }
    pub fn up(&mut self)    { if self.cursor.0 > 0 { self.cursor.0 -= 1; } }
    pub fn down(&mut self)  { if self.cursor.0 + 1 < self.rows { self.cursor.0 += 1; } }

    /// Slot id at the cursor cell, if that cell has audio.
    pub fn selected_slot(&self) -> Option<u32> {
        self.sources.get(&self.cursor).map(|(sid, _)| *sid)
    }

    /// Label at the cursor cell, if any.
    pub fn selected_label(&self) -> Option<&str> {
        self.sources.get(&self.cursor).map(|(_, l)| l.as_str())
    }
}
