use seqterm_command::{AppCommand, HelpTopic};

// ─── Menu kinds ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind { File, Edit, About, Help }

impl MenuKind {
    pub const ALL: &'static [MenuKind] = &[
        MenuKind::File, MenuKind::Edit, MenuKind::About, MenuKind::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MenuKind::File  => " FILE ",
            MenuKind::Edit  => " EDIT ",
            MenuKind::About => " ABOUT ",
            MenuKind::Help  => " HELP ",
        }
    }

    pub fn index(self) -> usize {
        match self {
            MenuKind::File  => 0,
            MenuKind::Edit  => 1,
            MenuKind::About => 2,
            MenuKind::Help  => 3,
        }
    }

    pub fn items(self) -> &'static [MenuItem] {
        match self {
            MenuKind::File  => FILE_MENU,
            MenuKind::Edit  => EDIT_MENU,
            MenuKind::About => ABOUT_MENU,
            MenuKind::Help  => HELP_MENU,
        }
    }

    /// Number of selectable (non-separator) items in this menu.
    pub fn selectable_count(self) -> usize {
        self.items().iter().filter(|i| !i.separator && !i.disabled).count()
    }

    /// Convert flat cursor index (skipping separators/disabled) to item index.
    pub fn item_index_for_cursor(self, cursor: usize) -> usize {
        let mut sel = 0;
        for (i, item) in self.items().iter().enumerate() {
            if item.separator || item.disabled { continue; }
            if sel == cursor { return i; }
            sel += 1;
        }
        0
    }
}

// ─── MenuItem ─────────────────────────────────────────────────────────────────

pub struct MenuItem {
    pub label:     &'static str,
    pub shortcut:  &'static str,
    pub action:    MenuAction,
    pub separator: bool,
    pub disabled:  bool,
}

impl MenuItem {
    const fn item(label: &'static str, shortcut: &'static str, action: MenuAction) -> Self {
        Self { label, shortcut, action, separator: false, disabled: false }
    }
    const fn sep() -> Self {
        Self { label: "", shortcut: "", action: MenuAction::None, separator: true, disabled: false }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum MenuAction {
    None,
    NewProject,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    ImportMidi,
    ExportMidi,
    ExportMuseScore,
    ExportAudio,
    Exit,
    Undo,
    Redo,
    RoutingConfig,
    AudioSettings,
    MidiSettings,
    Keybindings,
    ShowAbout,
    HelpKeyboard,
    HelpWorkflow,
    HelpMidiImport,
    HelpRouting,
    HelpPatternEditor,
    HelpTroubleshooting,
    HelpLatency,
}

impl MenuAction {
    pub fn to_command(self) -> Option<AppCommand> {
        Some(match self {
            Self::None          => return None,
            Self::NewProject    => AppCommand::NewProject,
            Self::OpenProject   => AppCommand::OpenProject,
            Self::SaveProject   => AppCommand::SaveProject,
            Self::SaveProjectAs => AppCommand::SaveProjectAs,
            Self::ImportMidi    => AppCommand::ImportMidi,
            Self::ExportMidi        => AppCommand::ExportMidi,
            Self::ExportMuseScore   => AppCommand::ExportMuseScore,
            Self::ExportAudio       => AppCommand::ExportAudio,
            Self::Exit          => AppCommand::Exit,
            Self::Undo          => AppCommand::Undo,
            Self::Redo          => AppCommand::Redo,
            Self::RoutingConfig => AppCommand::ShowRoutingConfig,
            Self::AudioSettings => AppCommand::ShowAudioSettings,
            Self::MidiSettings  => AppCommand::ShowMidiSettings,
            Self::Keybindings   => AppCommand::ShowKeybindings,
            Self::ShowAbout     => AppCommand::ShowAbout,
            Self::HelpKeyboard       => AppCommand::ShowHelp(HelpTopic::KeyboardShortcuts),
            Self::HelpWorkflow       => AppCommand::ShowHelp(HelpTopic::WorkflowGuide),
            Self::HelpMidiImport     => AppCommand::ShowHelp(HelpTopic::MidiImport),
            Self::HelpRouting        => AppCommand::ShowHelp(HelpTopic::Routing),
            Self::HelpPatternEditor  => AppCommand::ShowHelp(HelpTopic::PatternEditor),
            Self::HelpTroubleshooting => AppCommand::ShowHelp(HelpTopic::Troubleshooting),
            Self::HelpLatency        => AppCommand::ShowHelp(HelpTopic::LatencyOptimization),
        })
    }
}

// ─── Static menu definitions ──────────────────────────────────────────────────

static FILE_MENU: &[MenuItem] = &[
    MenuItem::item("New Project",       "Ctrl+N", MenuAction::NewProject),
    MenuItem::item("Open Project…",     "Ctrl+O", MenuAction::OpenProject),
    MenuItem::item("Save",              "Ctrl+S", MenuAction::SaveProject),
    MenuItem::item("Save As…",          "Ctrl+Shift+S", MenuAction::SaveProjectAs),
    MenuItem::sep(),
    MenuItem::item("Import MIDI…",      "Ctrl+I", MenuAction::ImportMidi),
    MenuItem::sep(),
    MenuItem::item("Export MIDI…",      "Ctrl+E", MenuAction::ExportMidi),
    MenuItem::item("Export MusicXML…",  "",       MenuAction::ExportMuseScore),
    MenuItem::item("Export Audio…",     "",       MenuAction::ExportAudio),
    MenuItem::sep(),
    MenuItem::item("Exit",              "Ctrl+Q", MenuAction::Exit),
];

static EDIT_MENU: &[MenuItem] = &[
    MenuItem::item("Undo",              "Ctrl+Z", MenuAction::Undo),
    MenuItem::item("Redo",              "Ctrl+Y", MenuAction::Redo),
    MenuItem::sep(),
    MenuItem::item("Routing Config",    "",       MenuAction::RoutingConfig),
    MenuItem::sep(),
    MenuItem::item("Audio Settings",    "",       MenuAction::AudioSettings),
    MenuItem::item("MIDI Settings",     "",       MenuAction::MidiSettings),
    MenuItem::item("Keybindings",       "",       MenuAction::Keybindings),
];

static ABOUT_MENU: &[MenuItem] = &[
    MenuItem::item("About SeqTerm",     "F12",    MenuAction::ShowAbout),
];

static HELP_MENU: &[MenuItem] = &[
    MenuItem::item("Keyboard Shortcuts",  "F1",  MenuAction::HelpKeyboard),
    MenuItem::item("Workflow Guide",      "",    MenuAction::HelpWorkflow),
    MenuItem::item("MIDI Import Guide",   "",    MenuAction::HelpMidiImport),
    MenuItem::item("Routing Guide",       "",    MenuAction::HelpRouting),
    MenuItem::item("Pattern Editor Guide","",    MenuAction::HelpPatternEditor),
    MenuItem::item("Troubleshooting",     "",    MenuAction::HelpTroubleshooting),
    MenuItem::item("Latency Optimization","",    MenuAction::HelpLatency),
];
