use std::{collections::HashSet, path::PathBuf, sync::Arc};

use parking_lot::Mutex;
use seqterm_audio_export::AudioExportOpts;
use seqterm_core::Project;
use seqterm_engine::{EngineEvent, PlaybackEngine};
use seqterm_core::note::parse_note_name;
use seqterm_persistence::{AppSettings, MidiLearnTarget};

use seqterm_command::AppCommand;
use seqterm_history::{self as hist, History};

use crate::{
    menu::MenuKind,
    modal::Modal,
};

/// Progress messages sent from the audio export background thread.
pub enum AudioExportMsg {
    Update { fraction: f32, message: String },
    Done(String),
    Error(String),
}

/// Write `msg` to `/tmp/seqterm.announce` so that external screen reader scripts
/// can `tail -f` the file and vocalise status changes.
fn announce_status(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true).open("/tmp/seqterm.announce")
    {
        let _ = writeln!(f, "{msg}");
    }
}

/// FX processor types available for audio engine mixer slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioFxKind {
    #[default]
    Delay,
    Reverb,
    BitCrusher,
    Vinyl,
    Isolator,
    Cassette,
    Looper,
    SidechainDuck,
    Filter,
    FilterBank,
}

impl AudioFxKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Delay       => "DELAY",
            Self::Reverb      => "REVERB",
            Self::BitCrusher  => "BITCRUSH",
            Self::Vinyl       => "VINYL",
            Self::Isolator    => "ISOLATOR",
            Self::Cassette    => "CASSETTE",
            Self::Looper      => "LOOPER",
            Self::SidechainDuck => "SIDECHAIN",
            Self::Filter      => "FILTER",
            Self::FilterBank  => "FILTERBANK",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Delay       => Self::Reverb,
            Self::Reverb      => Self::BitCrusher,
            Self::BitCrusher  => Self::Vinyl,
            Self::Vinyl       => Self::Isolator,
            Self::Isolator    => Self::Cassette,
            Self::Cassette    => Self::Looper,
            Self::Looper      => Self::SidechainDuck,
            Self::SidechainDuck => Self::Filter,
            Self::Filter      => Self::FilterBank,
            Self::FilterBank  => Self::Delay,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Delay       => Self::FilterBank,
            Self::Reverb      => Self::Delay,
            Self::BitCrusher  => Self::Reverb,
            Self::Vinyl       => Self::BitCrusher,
            Self::Isolator    => Self::Vinyl,
            Self::Cassette    => Self::Isolator,
            Self::Looper      => Self::Cassette,
            Self::SidechainDuck => Self::Looper,
            Self::Filter      => Self::SidechainDuck,
            Self::FilterBank  => Self::Filter,
        }
    }
}

/// One entry in an audio slot's FX chain.
#[derive(Debug, Clone)]
pub struct AudioFxEntry {
    pub kind:    AudioFxKind,
    pub wet:     f32,
    pub enabled: bool,
}

impl AudioFxEntry {
    pub fn new(kind: AudioFxKind) -> Self {
        Self { kind, wet: 1.0, enabled: true }
    }
}

/// Which view is currently active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    Matrix,
    Tracker,
    Arranger,
    Mixer,
    Config,
    Sampler,
    Granular,
}

impl ViewKind {
    pub fn label(&self) -> &'static str {
        match self {
            ViewKind::Matrix   => "MATRIX",
            ViewKind::Tracker  => "TRACKER/P.ROLL",
            ViewKind::Arranger => "ARRANGER",
            ViewKind::Mixer    => "MIXER",
            ViewKind::Config   => "CONFIG",
            ViewKind::Sampler  => "SAMPLER",
            ViewKind::Granular => "GRANULAR",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            ViewKind::Matrix   => 0,
            ViewKind::Tracker  => 1,
            ViewKind::Arranger => 2,
            ViewKind::Mixer    => 3,
            ViewKind::Config   => 4,
            ViewKind::Sampler  => 5,
            ViewKind::Granular => 6,
        }
    }

    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(ViewKind::Matrix),
            1 => Some(ViewKind::Tracker),
            2 => Some(ViewKind::Arranger),
            3 => Some(ViewKind::Mixer),
            4 => Some(ViewKind::Config),
            5 => Some(ViewKind::Sampler),
            6 => Some(ViewKind::Granular),
            _ => None,
        }
    }
}

// ─── Vim mode ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
    Visual,
}

impl VimMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
        }
    }
}

// ─── Per-view state ───────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct MatrixState {
    /// Cursor (row, col) in the 8×8 matrix.
    pub cursor: (usize, usize),
    /// 0=grid, 1=polymeter, 2=routing
    pub section: usize,
    /// When `Some((row, col))`: a clip has been picked up and is being moved.
    /// The cursor shows the destination. `m`/Enter drops it; Esc cancels.
    pub grabbed_clip: Option<(usize, usize)>,
}

#[derive(Debug, Default)]
pub struct TrackerState {
    /// Selected pattern key.
    pub pattern_key: Option<String>,
    /// Cursor (row = step, col = column).
    pub cursor: (usize, usize),
    /// Column names for display.
    pub columns: Vec<&'static str>,
}

impl TrackerState {
    pub fn init() -> Self {
        Self {
            pattern_key: Some("BASS1".to_string()),
            cursor: (0, 0),
            columns: vec!["NOTE", "INS", "VEL", "FX1", "FX2", "CC01", "CC74", "GATE", "MICRO", "PROB"],
        }
    }
}

#[derive(Debug, Default)]
pub struct ArrangerState {
    /// Visible start bar (scroll offset).
    pub bar_offset: u32,
    /// Selected track index (matrix row index).
    pub selected_track: usize,
    /// Automation cursor bar.
    pub automation_cursor: usize,
    /// 0=tracks, 1=automation, 2=song transport.
    pub section: usize,
    /// Which automation lane is focused.
    pub automation_lane: usize,
    /// Cursor within the song transport row: 0=PLAY, 1=STOP, 2=REC, 3=BPM.
    pub song_transport_cursor: usize,
}

#[derive(Debug, Default)]
pub struct MixerState {
    /// Selected channel index (into collect_mixer_entries result).
    pub selected_channel: usize,
    /// In edit mode for a parameter (strips panel).
    pub editing: bool,
    /// Active parameter: 0=VOL 1=EQ_LO 2=EQ_LM 3=EQ_HM 4=EQ_HI 5=PAN 6=FX
    pub active_param: usize,
    /// True when keyboard focus is on the FX sidebar (always visible).
    pub fx_panel_focused: bool,
    /// Which of the 3 FX slots is being edited (0-2).
    pub fx_slot_idx: usize,
    /// Cursor row in FX sidebar: 0=slot header, 3-10=param[0-7].
    pub fx_row: usize,
    /// Cursor column in FX sidebar: 0=CC#, 1=value (for param rows 3-10).
    pub fx_col: usize,
}

#[derive(Debug, Default)]
pub struct ConfigState {
    /// Which section is focused: 0=MIDI in, 1=MIDI out, 2=OSC, 3=sync.
    pub section: usize,
    /// Cursor within the active section.
    pub cursor: usize,
    /// Editing a field.
    pub editing: bool,
}

#[derive(Debug, Default)]
pub struct SamplerState {
    /// (bank, pad) cursor on the 4×4 grid.
    pub cursor: (usize, usize),
}

/// Per-param cursor IDs for the granular view (0-11 = GrainParams, 12-16 = GranularZone).
pub const GRAN_PARAM_COUNT: usize = 17;

#[derive(Debug)]
pub struct GranularState {
    /// Which (bank, pad) is currently being edited. `None` = no pad selected.
    pub pad: Option<(usize, usize)>,
    /// Cursor: which param row is highlighted (0-16).
    pub cursor: usize,
    /// Cached grain params for the current pad (updated on pad open / local edits).
    pub params: seqterm_core::GrainParams,
    /// Cached granular zone for the current pad.
    pub zone: seqterm_core::GranularZone,
}

impl Default for GranularState {
    fn default() -> Self {
        Self {
            pad:    None,
            cursor: 0,
            params: seqterm_core::GrainParams::default(),
            zone:   seqterm_core::GranularZone::default(),
        }
    }
}

#[derive(Debug, Default)]
pub struct RoutingState {
    /// Cursor row in the node list (left panel).
    pub node_cursor: usize,
    /// Cursor column in the connection matrix (right side = target node index).
    pub col_cursor: usize,
    /// 0 = node list focused, 1 = matrix focused.
    pub section: usize,
    /// Scroll offset for the node list.
    pub scroll: usize,
}

// ─── Multi-project tabs ───────────────────────────────────────────────────────

/// Snapshot of per-project state saved when a tab is backgrounded.
pub struct ProjectTab {
    pub project:       Arc<Mutex<Project>>,
    pub project_path:  Option<PathBuf>,
    pub project_dirty: bool,
    pub history:       History,
    pub current_view:  ViewKind,
    pub matrix_rows:   usize,
    pub matrix_cols:   usize,
    pub bpm:           f64,
    pub audio_slots:   std::collections::HashMap<String, u32>,
}

// ─── App ─────────────────────────────────────────────────────────────────────

pub struct App {
    pub project: Arc<Mutex<Project>>,
    pub current_view: ViewKind,
    pub engine: PlaybackEngine,
    pub should_quit: bool,
    /// Whether transport is currently playing (mirrored from engine events).
    pub playing: bool,
    /// Whether recording is active.
    pub recording: bool,
    /// Whether song-mode transport is playing (arranger).
    pub song_playing: bool,
    /// Current bar position in song-mode playback.
    pub song_bar: usize,
    /// BPM display value.
    pub bpm: f64,
    /// Current step for UI animation.
    pub current_step: usize,
    /// Current bar.
    pub current_bar: usize,
    /// Status message shown in transport bar.
    pub status_msg: String,

    // Per-view state
    pub matrix_state: MatrixState,
    pub tracker_state: TrackerState,
    pub arranger_state: ArrangerState,
    pub mixer_state: MixerState,
    pub config_state: ConfigState,
    pub sampler_state: SamplerState,
    pub granular_state: GranularState,

    // ── Sampler pad system ────────────────────────────────────────────────────
    /// Maps (bank, pad) → audio engine slot_id for loaded sampler pads.
    pub sampler_slots: std::collections::HashMap<(usize, usize), u32>,
    /// Slot IDs waiting for AudioFileLoaded before we send PlayAudioClip.
    pub pending_plays: std::collections::HashSet<u32>,

    // ── Tracker extended state ────────────────────────────────────────────────
    /// Vertical scroll offset in tracker.
    pub tracker_scroll: usize,
    /// Whether tracker is in edit mode.
    pub tracker_editing: bool,
    /// Which column is being edited (0=NOTE, 1=INS, 2=VEL, ...).
    pub tracker_edit_field: usize,

    // ── Piano Roll state ──────────────────────────────────────────────────────
    /// First visible note row index (0 = C6 at top of roll).
    pub piano_note_scroll: usize,
    /// Horizontal step offset (first visible step column).
    pub piano_step_scroll: usize,
    /// (note_row, step) cursor.
    pub piano_cursor: (usize, usize),
    /// Draw vs select mode.
    pub piano_draw_mode: bool,
    /// Number of step columns visible in the current frame (set during draw).
    pub piano_visible_steps: std::cell::Cell<usize>,
    /// Number of note rows visible in the current frame (set during draw).
    pub piano_visible_rows: std::cell::Cell<usize>,

    // ── Tracker subsection state ──────────────────────────────────────────────
    /// 0=step table, 1=piano roll, 2=generative engine, 3=track modulation.
    pub tracker_section: usize,
    /// Piano roll rendered area (cached via Cell for mouse hit-testing).
    pub piano_roll_area: std::cell::Cell<ratatui::layout::Rect>,
    /// Generative engine cursor: 0=SWING, 1=RANDOM, 2=PROB.
    pub generative_cursor: usize,
    /// Track modulation cursor: 0=VEL, 1=CC01, 2=CC74, 3=GATE, 4=PROB.
    pub modulation_cursor: usize,
    /// Piano roll drag origin: (step, note_row).
    pub piano_drag_note: Option<(usize, usize)>,
    /// True while the mouse button is held down over the piano keys (left column).
    /// Used to preview notes while dragging across keys (glissando).
    pub piano_key_down: bool,
    /// Last note_row previewed via piano key click/drag, to avoid repeating the same note.
    pub piano_key_last_row: Option<usize>,
    /// Whether the pattern name text-entry field is active.
    pub pattern_name_editing: bool,
    /// Buffer for pattern name text entry.
    pub pattern_name_buffer: String,

    // ── Tracker geometry (set during draw, used for scroll clamping & mouse) ─
    /// Actual visible row count of the tracker table (updated each frame).
    pub tracker_view_height: std::cell::Cell<usize>,
    /// Rect of the velocity bar-chart body in the modulation panel (for mouse editing).
    pub vel_chart_area: std::cell::Cell<ratatui::layout::Rect>,
    /// Full rect of the tracker table widget (for mouse hit-testing).
    pub tracker_table_area: std::cell::Cell<ratatui::layout::Rect>,
    /// File list area inside the FilePicker modal (for mouse click navigation).
    pub file_picker_list_area: std::cell::Cell<ratatui::layout::Rect>,

    // ── Matrix transport editing ──────────────────────────────────────────────
    /// 0 = grid navigation, 1 = transport section active (Tab toggles).
    pub matrix_section: usize,
    /// Which transport param is selected: 0=BPM, 1=rows, 2=cols.
    pub transport_cursor: usize,
    /// Number of visible matrix rows (A-P, max 16).
    pub matrix_rows: usize,
    /// Number of visible matrix columns (max min(16, 128/rows)).
    pub matrix_cols: usize,
    /// Tap tempo: timestamps of recent taps for BPM detection.
    pub tap_times: Vec<std::time::Instant>,
    /// Whether a JACK server was detected during the last port refresh.
    pub jack_available: bool,
    /// Audio engine owned by the App — started in main.rs after App construction.
    pub audio_engine: Option<seqterm_audio_engine::AudioEngine>,
    /// Maps clip key (e.g. "A0") → audio engine slot_id for SF2 / AudioFile sources.
    pub audio_slots: std::collections::HashMap<String, u32>,
    /// Per-slot linear gain (0.0–2.0, default 1.0 = 0 dB).
    pub audio_slot_volumes: std::collections::HashMap<u32, f32>,
    /// FX chain config per audio engine slot (not persisted — rebuilt on project reload).
    pub audio_slot_fx: std::collections::HashMap<u32, Vec<AudioFxEntry>>,
    /// Master bus FX chain (applied to final mix before soft-clip).
    pub master_fx: Vec<AudioFxEntry>,
    /// Audio engine status (updated each frame from AudioEngineEvent drain).
    pub audio_engine_running: bool,
    pub audio_sample_rate: u32,
    pub audio_buffer_size: u32,
    pub audio_dsp_load: f32,
    pub audio_xrun_count: u32,
    /// Timestamp of the last MIDI port scan.
    pub last_midi_refresh: Option<std::time::Instant>,
    /// Which pattern row is selected in the polymeter visualizer.
    pub polymeter_cursor: usize,
    /// First pattern row visible (vertical scroll) in the polymeter visualizer.
    pub polymeter_pat_scroll: usize,
    /// First step shown in the polymeter step window (horizontal scroll).
    pub polymeter_step_start: usize,
    /// Cursor in the MIDI-output list when matrix_section == 3 (routing panel).
    /// 0 = (none / unrouted), 1..=n = proj.midi_outputs[cursor-1].
    pub routing_cursor: usize,
    /// Routing panel tab: 0 = MIDI OUT, 1 = SOURCE BROWSER.
    pub routing_tab: usize,
    /// Cursor in the source browser list (index into collected unique sources).
    pub routing_source_cursor: usize,

    // ── Vim modal editing ─────────────────────────────────────────────────────
    pub vim_mode: VimMode,
    /// Start step of a Visual-mode selection in the tracker step table.
    pub visual_start: Option<usize>,
    /// Yanked steps buffer (from `y` in Visual mode).
    pub vim_yank_buffer: Vec<seqterm_core::Note>,

    // ── Arranger track name editing ───────────────────────────────────────────
    pub arranger_track_name_editing: bool,
    pub arranger_track_name_buffer: String,

    // ── Global mouse state ────────────────────────────────────────────────────
    pub last_mouse_pos: (u16, u16),
    pub mouse_drag: bool,
    /// Timestamp of the last piano-roll left-click (for time-based gate on release).
    pub note_click_start: Option<std::time::Instant>,

    // ── Frame counter (wrapping) — used for spinner animation etc. ───────────
    pub frame_count: u64,
    /// When `Some`, `status_msg` reverts to a blank hint after this instant.
    pub status_expires: Option<std::time::Instant>,

    // ── MIDI port monitoring ──────────────────────────────────────────────────
    pub midi_port_rx: flume::Receiver<Vec<String>>,
    pub unavailable_midi_routes: HashSet<String>,
    /// Live MIDI input bus — receives messages from all enabled input ports.
    pub midi_input_bus: seqterm_midi::MidiInputBus,

    // ── File / project state ──────────────────────────────────────────────────
    pub project_path:  Option<PathBuf>,
    pub project_dirty: bool,
    pub recent_projects: Vec<PathBuf>,
    pub recent_midi_imports: Vec<PathBuf>,

    // ── Undo / redo ───────────────────────────────────────────────────────────
    pub history: History,

    // ── Menu bar ─────────────────────────────────────────────────────────────
    pub menu_open:   Option<MenuKind>,
    pub menu_cursor: usize,

    // ── Modal system ──────────────────────────────────────────────────────────
    pub active_modal: Option<Modal>,

    // ── Waveform preview cache ─────────────────────────────────────────────────
    /// Amplitude peaks for each decoded AudioFile path (bands × f32, 0.0–1.0).
    pub waveform_cache: std::collections::HashMap<PathBuf, Vec<f32>>,
    /// Paths queued for background waveform scan (not yet in cache).
    pub waveform_pending: std::collections::HashSet<PathBuf>,
    /// Receives (path, peaks) from background waveform threads.
    pub waveform_rx: flume::Receiver<(PathBuf, Vec<f32>)>,
    waveform_tx: flume::Sender<(PathBuf, Vec<f32>)>,

    // ── SF2 preset background scan ────────────────────────────────────────────
    pub sf2_presets_rx: Option<flume::Receiver<Vec<(u8, u8, String)>>>,

    // ── MIDI import background task ───────────────────────────────────────────
    pub midi_import_rx: Option<flume::Receiver<Result<seqterm_midi_io::ImportedMidi, String>>>,

    // ── OSC server ────────────────────────────────────────────────────────────
    /// Receiver for incoming OSC messages from the background UDP listener.
    pub osc_rx: Option<flume::Receiver<seqterm_midi_io::OscMsg>>,
    /// UDP port the OSC server is currently bound to (0 if not running).
    pub osc_port: u16,

    // ── Realtime capture ──────────────────────────────────────────────────────
    /// True while the audio engine is capturing live output to a WAV file.
    pub capturing: bool,
    /// Path of the current/last capture WAV file.
    pub capture_path: Option<PathBuf>,

    // ── Audio export background task ──────────────────────────────────────────
    pub audio_export_rx: Option<flume::Receiver<AudioExportMsg>>,

    // ── Plugin registry ───────────────────────────────────────────────────────
    pub plugin_registry: seqterm_application::PluginRegistry,

    // ── App settings ──────────────────────────────────────────────────────────
    pub settings: AppSettings,

    // ── MIDI Learn ────────────────────────────────────────────────────────────
    /// When Some, the next incoming MIDI CC will be bound to this target.
    pub midi_learn: Option<MidiLearnTarget>,

    // ── Audio export ──────────────────────────────────────────────────────────
    /// Options selected in the audio export dialog; persist between invocations.
    pub audio_export_opts: AudioExportOpts,

    // ── Multi-project tabs ────────────────────────────────────────────────────
    /// Inactive project tabs (does not include the currently active one).
    pub tabs: Vec<ProjectTab>,
    /// Index of the currently displayed tab (into conceptual tab list where
    /// index 0 corresponds to the active App fields, 1+ are stored in `tabs`).
    pub active_tab: usize,

    // ── Routing editor ────────────────────────────────────────────────────────
    pub routing_state: RoutingState,

    // ── Matrix transport button hover (set each frame by handle_hover) ──────────
    /// Index of the matrix transport button currently hovered (0=PLAY,1=STOP,2=REC,3=TAP,4=BPM).
    pub hovered_transport_btn: Option<u8>,
    /// Absolute Y of the first MIDI-output list item in the matrix routing panel (set each frame).
    pub routing_list_item_y: std::cell::Cell<u16>,
    /// Absolute Y of the `◄ CH N ►` row in the routing panel (set each frame).
    pub routing_channel_y: std::cell::Cell<u16>,
    /// Cell size (cell_w, cell_h) of the matrix grid, set each frame by draw_clip_grid.
    pub matrix_cell_size: std::cell::Cell<(usize, usize)>,
    /// Matrix cell currently under the mouse pointer, or None.
    pub hovered_matrix_cell: std::cell::Cell<Option<(usize, usize)>>,

    // ── Panel hit-test rects (set each frame during draw, used for mouse hover) ─
    /// Bounding rects of the 4 matrix subsections: [grid, transport, polymeter, routing].
    pub matrix_panel_rects:  std::cell::Cell<[ratatui::layout::Rect; 4]>,
    /// Bounding rects of the 4 tracker subsections: [step_table, piano_roll, generative, modulation].
    pub tracker_panel_rects: std::cell::Cell<[ratatui::layout::Rect; 4]>,
    /// Bounding rects of the 3 arranger subsections: [tracks, automation, song_transport].
    pub arranger_panel_rects: std::cell::Cell<[ratatui::layout::Rect; 3]>,
    /// Bounding rects of the 2 mixer subsections: [channels, automation].
    pub mixer_panel_rects:   std::cell::Cell<[ratatui::layout::Rect; 2]>,
    /// Bounding rects of the 4 config subsections: [midi_in, midi_out, osc, sync].
    pub config_panel_rects:  std::cell::Cell<[ratatui::layout::Rect; 4]>,
    /// Bounding rect of the audio engine panel in the Config view.
    pub config_audio_panel_rect: std::cell::Cell<ratatui::layout::Rect>,

    // ── Mouse hit-test rects updated every frame ──────────────────────────────
    /// Position of the [×] close button on the active modal (zero if no modal).
    pub modal_close_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// Bounding rect of the entire active modal window (zero if no modal).
    pub modal_area: std::cell::Cell<ratatui::layout::Rect>,
    /// Bounding rect of the transport bar row (set each frame in ui()).
    pub transport_area: std::cell::Cell<ratatui::layout::Rect>,

    // ── Routing graph geometry (set each frame by draw_routing_focused) ────────
    /// Full rect of the routing graph widget (bottom half of Config).
    pub routing_graph_area: std::cell::Cell<ratatui::layout::Rect>,
    /// Inner rect of the node-list panel (left column of routing graph).
    pub routing_node_list_inner: std::cell::Cell<ratatui::layout::Rect>,
    /// Inner rect of the connection matrix panel (center column).
    pub routing_matrix_inner: std::cell::Cell<ratatui::layout::Rect>,
    /// Column width (chars) used in the connection matrix.
    pub routing_matrix_col_w: std::cell::Cell<u16>,
    /// True while the mouse pointer is inside the routing graph area.
    pub routing_graph_hovered: std::cell::Cell<bool>,

    // ── Mixer mouse hit-test geometry (set each frame by draw_channel_strips) ──
    /// Rect of the entire channel-strips section (used for x→strip column mapping).
    pub mixer_strips_area: std::cell::Cell<ratatui::layout::Rect>,
    /// Absolute x-start for each strip column (up to 36). Count given by mixer_strip_count.
    pub mixer_strip_xs: std::cell::Cell<[u16; 36]>,
    /// Number of strip columns actually drawn.
    pub mixer_strip_count: std::cell::Cell<u16>,
    /// Absolute y positions of param rows: [mute, vol_label, fader_start, fader_end,
    /// eq_lo, eq_lm, eq_hm, eq_hi, pan, fx].
    pub mixer_param_ys: std::cell::Cell<[u16; 10]>,
}

impl App {
    pub fn new(project: Arc<Mutex<Project>>, engine: PlaybackEngine) -> Self {
        let bpm = project.lock().bpm;
        // Poll every 3 s; first update fires immediately if topology differs from nothing.
        let midi_port_rx = seqterm_midi::spawn_port_watcher(std::time::Duration::from_secs(3));
        let (waveform_tx, waveform_rx) = flume::unbounded::<(PathBuf, Vec<f32>)>();

        let mut app = Self {
            project,
            current_view: ViewKind::Matrix,
            engine,
            should_quit: false,
            playing: false,
            recording: false,
            song_playing: false,
            song_bar: 0,
            bpm,
            current_step: 0,
            current_bar: 0,
            status_msg: "Welcome to SeqTerm-rs  |  q=quit  space=play  Tab=switch view".to_string(),

            matrix_state: MatrixState::default(),
            tracker_state: TrackerState::init(),
            arranger_state: ArrangerState::default(),
            mixer_state: MixerState::default(),
            config_state: ConfigState::default(),
            sampler_state: SamplerState::default(),
            granular_state: GranularState::default(),
            sampler_slots: std::collections::HashMap::new(),
            pending_plays: std::collections::HashSet::new(),

            tracker_scroll: 0,
            tracker_editing: false,
            tracker_edit_field: 0,

            piano_note_scroll: 36,
            piano_step_scroll: 0,
            piano_cursor: (0, 0),
            piano_draw_mode: true,
            piano_visible_steps: std::cell::Cell::new(16),
            piano_visible_rows: std::cell::Cell::new(16),

            tracker_section: 0,
            piano_roll_area: std::cell::Cell::new(ratatui::layout::Rect::default()),
            generative_cursor: 0,
            modulation_cursor: 0,
            piano_drag_note: None,
            piano_key_down: false,
            piano_key_last_row: None,
            pattern_name_editing: false,
            pattern_name_buffer: String::new(),

            tracker_view_height: std::cell::Cell::new(20),
            vel_chart_area: std::cell::Cell::new(ratatui::layout::Rect::default()),
            tracker_table_area: std::cell::Cell::new(ratatui::layout::Rect::default()),
            file_picker_list_area: std::cell::Cell::new(ratatui::layout::Rect::default()),

            vim_mode: VimMode::Normal,
            visual_start: None,
            vim_yank_buffer: Vec::new(),
            arranger_track_name_editing: false,
            arranger_track_name_buffer: String::new(),

            last_mouse_pos: (0, 0),
            mouse_drag: false,
            note_click_start: None,

            matrix_section: 0,
            transport_cursor: 0,
            matrix_rows: 8,
            matrix_cols: 8,
            tap_times: Vec::new(),
            jack_available: false,
            audio_engine: None,
            audio_slots: std::collections::HashMap::new(),
            audio_slot_volumes: std::collections::HashMap::new(),
            audio_slot_fx: std::collections::HashMap::new(),
            master_fx:     Vec::new(),
            audio_engine_running: false,
            audio_sample_rate: 48000,
            audio_buffer_size: 256,
            audio_dsp_load: 0.0,
            audio_xrun_count: 0,
            last_midi_refresh: None,
            polymeter_cursor: 0,
            polymeter_pat_scroll: 0,
            polymeter_step_start: 0,
            routing_cursor: 0,
            routing_tab: 0,
            routing_source_cursor: 0,

            frame_count: 0,
            status_expires: None,

            midi_port_rx,
            unavailable_midi_routes: HashSet::new(),
            midi_input_bus: seqterm_midi::MidiInputBus::new(),

            project_path:  None,
            project_dirty: false,
            recent_projects: seqterm_persistence::load_recent_projects(),
            recent_midi_imports: seqterm_persistence::load_recent_midi_imports(),

            history: History::default(),
            menu_open:   None,
            menu_cursor: 0,
            active_modal: None,
            osc_rx: None,
            osc_port: 0,
            waveform_cache: std::collections::HashMap::new(),
            waveform_pending: std::collections::HashSet::new(),
            waveform_rx,
            waveform_tx,
            sf2_presets_rx: None,
            midi_import_rx: None,
            capturing: false,
            capture_path: None,
            audio_export_rx: None,
            settings: seqterm_persistence::load_settings(),

            plugin_registry: seqterm_application::PluginRegistry::new(),
            midi_learn: None,
            audio_export_opts: AudioExportOpts::default(),

            tabs: Vec::new(),
            active_tab: 0,

            routing_state: RoutingState::default(),
            hovered_transport_btn: None,
            routing_list_item_y: std::cell::Cell::new(0),
            routing_channel_y: std::cell::Cell::new(0),
            matrix_cell_size: std::cell::Cell::new((0, 0)),
            hovered_matrix_cell: std::cell::Cell::new(None),
            matrix_panel_rects:  std::cell::Cell::new([ratatui::layout::Rect::default(); 4]),
            tracker_panel_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 4]),
            arranger_panel_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 3]),
            mixer_panel_rects:   std::cell::Cell::new([ratatui::layout::Rect::default(); 2]),
            config_panel_rects:  std::cell::Cell::new([ratatui::layout::Rect::default(); 4]),
            config_audio_panel_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),

            modal_close_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            modal_area:        std::cell::Cell::new(ratatui::layout::Rect::default()),
            transport_area:    std::cell::Cell::new(ratatui::layout::Rect::default()),

            routing_graph_area:      std::cell::Cell::new(ratatui::layout::Rect::default()),
            routing_node_list_inner: std::cell::Cell::new(ratatui::layout::Rect::default()),
            routing_matrix_inner:    std::cell::Cell::new(ratatui::layout::Rect::default()),
            routing_matrix_col_w:    std::cell::Cell::new(0),
            routing_graph_hovered:   std::cell::Cell::new(false),

            mixer_strips_area: std::cell::Cell::new(ratatui::layout::Rect::default()),
            mixer_strip_xs:    std::cell::Cell::new([0u16; 36]),
            mixer_strip_count: std::cell::Cell::new(0),
            mixer_param_ys:    std::cell::Cell::new([0u16; 10]),
        };
        // Populate port list immediately so the routing panel is usable from frame 1.
        app.refresh_midi_ports();
        app
    }

    /// Drain engine events and update mirrored state.
    pub fn process_events(&mut self) {
        self.frame_count = self.frame_count.wrapping_add(1);

        // Expire timed status messages.
        if let Some(exp) = self.status_expires {
            if std::time::Instant::now() >= exp {
                self.status_expires = None;
                self.status_msg = String::new();
            }
        }

        // Drain background MIDI port updates; update proj.midi_outputs + unavailable set.
        // Take only the latest snapshot if multiple arrived during a single frame.
        let mut latest_ports: Option<Vec<String>> = None;
        while let Ok(ports) = self.midi_port_rx.try_recv() {
            latest_ports = Some(ports);
        }
        if let Some(ports) = latest_ports {
            self.apply_port_update(ports);
        }

        // Poll MIDI import background task.
        if let Some(rx) = &self.midi_import_rx {
            if let Ok(result) = rx.try_recv() {
                self.midi_import_rx = None;
                match result {
                    Ok(imported) => {
                        let summary = imported.summary.clone();
                        self.apply_midi_import(imported);
                        self.active_modal = Some(Modal::alert("Import Complete", summary));
                    }
                    Err(e) => {
                        self.active_modal = Some(Modal::alert("Import Failed", e));
                    }
                }
            }
        }

        // Drain completed waveform scans into cache.
        while let Ok((path, peaks)) = self.waveform_rx.try_recv() {
            self.waveform_pending.remove(&path);
            self.waveform_cache.insert(path, peaks);
        }

        // Queue waveform scans for AudioFile clips not yet in cache.
        let paths_to_scan: Vec<PathBuf> = {
            let proj = self.project.lock();
            proj.matrix.values()
                .flat_map(|slots| slots.iter().flatten())
                .filter_map(|clip| {
                    if let seqterm_core::PatternSource::AudioFile { path, .. } = &clip.source {
                        if !self.waveform_cache.contains_key(path)
                            && !self.waveform_pending.contains(path)
                        {
                            Some(path.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        };
        for path in paths_to_scan {
            self.waveform_pending.insert(path.clone());
            let tx = self.waveform_tx.clone();
            std::thread::spawn(move || {
                if let Ok(peaks) = seqterm_audio_engine::scan_waveform(&path, 64) {
                    let _ = tx.send((path, peaks));
                }
            });
        }

        // Drain incoming OSC messages and dispatch to the engine / project.
        if let Some(rx) = &self.osc_rx {
            while let Ok(msg) = rx.try_recv() {
                use seqterm_midi_io::OscMsg;
                match msg {
                    OscMsg::Play  => { self.engine.play(); self.playing = true; }
                    OscMsg::Stop  => { self.engine.stop(); self.playing = false; }
                    OscMsg::SetBpm(bpm) => {
                        let bpm = bpm.clamp(20.0, 300.0);
                        self.bpm = bpm;
                        self.engine.set_bpm(bpm);
                        self.project.lock().bpm = bpm;
                    }
                    OscMsg::SetChannelVolume { channel, gain } => {
                        let mut proj = self.project.lock();
                        if let Some(ch) = proj.channels.get_mut(channel) {
                            ch.volume = gain.clamp(0.0, 2.0);
                        }
                    }
                    OscMsg::Custom { address, .. } => {
                        tracing::debug!("OSC custom: {address}");
                    }
                }
            }
        }

        // Drain MIDI input bus and handle incoming messages.
        self.process_midi_inputs();

        // Poll SF2 preset scan background task.
        if let Some(rx) = &self.sf2_presets_rx {
            if let Ok(presets) = rx.try_recv() {
                self.sf2_presets_rx = None;
                if let Some(crate::modal::Modal::Sf2Browser(state)) = &mut self.active_modal {
                    state.set_presets(presets);
                }
            }
        }

        // Poll audio export background task.
        if self.audio_export_rx.is_some() {
            let msg = self.audio_export_rx.as_ref().unwrap().try_recv();
            match msg {
                Ok(AudioExportMsg::Update { fraction, message }) => {
                    if let Some(Modal::Progress { progress, message: msg, .. }) = &mut self.active_modal {
                        *progress = fraction;
                        *msg = message;
                    }
                }
                Ok(AudioExportMsg::Done(success)) => {
                    self.audio_export_rx = None;
                    self.active_modal = None;
                    self.set_timed_status(success, 3);
                }
                Ok(AudioExportMsg::Error(err)) => {
                    self.audio_export_rx = None;
                    self.active_modal = Some(Modal::alert("Export Failed", err));
                }
                Err(flume::TryRecvError::Empty) => {}
                Err(flume::TryRecvError::Disconnected) => {
                    self.audio_export_rx = None;
                    // Only clear Progress modal — an Alert may have replaced it already.
                    if matches!(&self.active_modal, Some(Modal::Progress { .. })) {
                        self.active_modal = None;
                    }
                }
            }
        }

        // ── Transport snapshot (lock-free triple buffer) ─────────────────────
        {
            let snap = self.engine.transport_snapshot();
            self.playing      = snap.playing;
            self.bpm          = snap.bpm;
            self.current_step = snap.current_step;
            self.current_bar  = snap.current_bar;
        }

        // ── Engine events ────────────────────────────────────────────────────
        // BpmChanged / BarAdvanced are superseded by the snapshot above.
        // Only StepAdvanced (for tracker scroll) and XRun need handling here.
        let mut xrun_delta: u32 = 0;
        for ev in self.engine.drain_events() {
            match ev {
                EngineEvent::StepAdvanced(step) => {
                    if self.playing {
                        let pat_len = {
                            let proj = self.project.lock();
                            self.tracker_state
                                .pattern_key
                                .as_ref()
                                .and_then(|k| proj.patterns.get(k))
                                .map(|p| p.length)
                                .unwrap_or(16)
                        };
                        let pat_step = step % pat_len.max(1);
                        self.tracker_state.cursor.0 = pat_step;
                        self.clamp_tracker_scroll();
                        self.clamp_piano_step_scroll(pat_step);
                    }
                }
                EngineEvent::XRun => {
                    xrun_delta += 1;
                    self.status_msg = "! XRUN detected !".to_string();
                }
                EngineEvent::MidiCc { ch, cc, val } => {
                    if let Some(target) = self.midi_learn.take() {
                        use seqterm_persistence::MidiLearnBinding;
                        let binding = MidiLearnBinding::new(target.clone(), ch, cc);
                        self.settings.midi_learn_bindings.retain(|b| b.target != target);
                        self.settings.midi_learn_bindings.push(binding);
                        let _ = seqterm_persistence::save_settings(&self.settings);
                        self.status_msg = format!("Bound: CC{cc} (ch{}) → {}", ch + 1, target.label());
                    } else {
                        // Apply live CC to any bound targets.
                        for b in &self.settings.midi_learn_bindings {
                            if b.cc != cc || b.midi_ch != ch { continue; }
                            match &b.target {
                                seqterm_persistence::MidiLearnTarget::ChannelVolume(i) => {
                                    let mut proj = self.project.lock();
                                    if let Some(ch_strip) = proj.channels.get_mut(*i) {
                                        ch_strip.volume = val as f32 / 127.0 * 66.0 - 60.0;
                                    }
                                }
                                seqterm_persistence::MidiLearnTarget::ChannelSendA(i) => {
                                    let mut proj = self.project.lock();
                                    if let Some(ch_strip) = proj.channels.get_mut(*i) {
                                        ch_strip.send_a = val;
                                    }
                                }
                                seqterm_persistence::MidiLearnTarget::ChannelSendB(i) => {
                                    let mut proj = self.project.lock();
                                    if let Some(ch_strip) = proj.channels.get_mut(*i) {
                                        ch_strip.send_b = val;
                                    }
                                }
                                seqterm_persistence::MidiLearnTarget::Bpm => {
                                    let bpm = 60.0 + val as f64 / 127.0 * 180.0;
                                    self.bpm = bpm;
                                    self.engine.set_bpm(bpm);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                EngineEvent::AudioNoteOn { slot_id, channel, note, velocity } => {
                    if let Some(ae) = &mut self.audio_engine {
                        ae.send(seqterm_audio_engine::AudioCommand::NoteOn {
                            slot_id, channel, note, velocity,
                        });
                    }
                }
                EngineEvent::AudioNoteOff { slot_id, channel, note } => {
                    if let Some(ae) = &mut self.audio_engine {
                        ae.send(seqterm_audio_engine::AudioCommand::NoteOff {
                            slot_id, channel, note,
                        });
                    }
                }
                EngineEvent::AudioClipTrigger { slot_id } => {
                    if let Some(ae) = &mut self.audio_engine {
                        ae.send(seqterm_audio_engine::AudioCommand::PlayAudioClip { slot_id });
                    }
                }
                EngineEvent::BarAdvanced(_)
                | EngineEvent::BpmChanged(_)
                | EngineEvent::NoteOn { .. }
                | EngineEvent::NoteOff { .. } => {}
            }
        }

        // ── Audio engine event drain ─────────────────────────────────────────
        // Collect events and stats before borrowing self mutably for status updates.
        let (audio_evs, dsp_load) = if let Some(ae) = &mut self.audio_engine {
            (ae.drain_events(), ae.dsp_load())
        } else {
            (vec![], 0.0)
        };
        for ev in audio_evs {
            use seqterm_audio_engine::AudioEngineEvent;
            match ev {
                AudioEngineEvent::StreamStarted { sample_rate, buffer_size } => {
                    self.audio_engine_running = true;
                    self.audio_sample_rate   = sample_rate;
                    self.audio_buffer_size   = buffer_size;
                }
                AudioEngineEvent::StreamStopped => {
                    self.audio_engine_running = false;
                }
                AudioEngineEvent::Xrun => {
                    self.audio_xrun_count = self.audio_xrun_count.saturating_add(1);
                }
                AudioEngineEvent::DspLoad(load) => {
                    self.audio_dsp_load = load;
                }
                AudioEngineEvent::Sf2Loaded { slot_id, preset_name } => {
                    // If this is the SF2 browser preview slot, fire a note.
                    let is_preview = if let Some(crate::modal::Modal::Sf2Browser(s)) = &mut self.active_modal {
                        if s.preview_slot == Some(slot_id) {
                            s.preview_loaded = true;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if is_preview {
                        if let Some(ae) = self.audio_engine.as_mut() {
                            ae.send(seqterm_audio_engine::AudioCommand::NoteOn {
                                slot_id, channel: 0, note: 60, velocity: 100,
                            });
                        }
                    } else {
                        self.set_timed_status(format!("SF2 ready: slot {slot_id} — {preset_name}"), 3);
                    }
                }
                AudioEngineEvent::AudioFileLoaded { slot_id, duration_secs, .. } => {
                    if self.pending_plays.remove(&slot_id) {
                        if let Some(ae) = self.audio_engine.as_mut() {
                            ae.send(seqterm_audio_engine::AudioCommand::PlayAudioClip { slot_id });
                        }
                    }
                    self.set_timed_status(
                        format!("Audio ready: slot {slot_id} ({duration_secs:.1}s)"), 3,
                    );
                }
                AudioEngineEvent::LoadFailed { slot_id, error } => {
                    self.set_timed_status(
                        format!("Load failed slot {slot_id}: {error}"), 5,
                    );
                }
                AudioEngineEvent::Error(e) => {
                    self.set_timed_status(format!("Audio error: {e}"), 5);
                }
                AudioEngineEvent::CaptureStarted(path) => {
                    self.capturing = true;
                    self.capture_path = Some(path.clone());
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "capture".into());
                    self.set_status(format!("● REC {name}  (Ctrl+R to stop)"));
                }
                AudioEngineEvent::CaptureStopped { path, duration_secs } => {
                    self.capturing = false;
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "capture".into());
                    self.set_timed_status(
                        format!("Capture saved: {name}  ({duration_secs:.1}s)"), 8,
                    );
                }
                AudioEngineEvent::CaptureFailed(e) => {
                    self.capturing = false;
                    self.set_timed_status(format!("Capture failed: {e}"), 8);
                }
            }
        }
        if self.audio_engine.is_some() {
            self.audio_dsp_load = dsp_load;
        }

        // Sync transport-adjacent project fields in a single lock per frame.
        {
            let mut proj = self.project.lock();
            proj.bpm         = self.bpm;
            proj.current_bar = self.current_bar as u32;
            proj.xrun        += xrun_delta;
        }
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    /// Find the matrix (row, col) that contains a clip referencing `key`.
    pub fn find_matrix_cell_for_pattern(&self, key: &str) -> Option<(usize, usize)> {
        let proj = self.project.lock();
        for (row_label, slots) in &proj.matrix {
            let row_char = row_label.chars().next()?;
            if row_char < 'A' || row_char > 'P' { continue; }
            let row = (row_char as u8 - b'A') as usize;
            for (col, slot) in slots.iter().enumerate() {
                if let Some(clip) = slot {
                    if clip.pattern_key.as_deref() == Some(key) {
                        return Some((row, col));
                    }
                }
            }
        }
        None
    }

    pub fn switch_view(&mut self, view: ViewKind) {
        if view != ViewKind::Matrix {
            self.matrix_section = 0;
        }
        if view == ViewKind::Config {
            self.refresh_midi_ports();
        }
        // When returning to Matrix, auto-position the cursor on the active pattern's cell.
        if view == ViewKind::Matrix {
            if let Some(key) = self.tracker_state.pattern_key.clone() {
                if let Some((row, col)) = self.find_matrix_cell_for_pattern(&key) {
                    // Expand matrix dimensions if needed so the cell is visible.
                    if row >= self.matrix_rows {
                        self.matrix_rows = (row + 1).min(16);
                        self.ensure_matrix_size();
                    }
                    if col >= self.matrix_cols {
                        let max_cols = (128 / self.matrix_rows.max(1)).min(16);
                        self.matrix_cols = (col + 1).min(max_cols);
                        self.ensure_matrix_size();
                    }
                    self.matrix_state.cursor = (row, col);
                } else {
                    self.status_msg = format!(
                        "Pattern '{}' has no matrix cell — use Enter on an empty slot to assign",
                        key
                    );
                }
            }
        }
        self.current_view = view;
        announce_status(&format!("View: {}", view.label()));
    }

    pub fn next_view(&mut self) {
        let next = (self.current_view.index() + 1) % 7;
        if let Some(v) = ViewKind::from_index(next) {
            self.current_view = v;
        }
    }

    // ── Transport ─────────────────────────────────────────────────────────────

    pub fn play_stop(&mut self) {
        if self.playing {
            self.engine.stop();
            self.playing = false;
            self.project.lock().playing = false;
            self.status_msg = "Stopped".to_string();
            announce_status("Playback stopped");
        } else {
            self.engine.play();
            self.playing = true;
            self.project.lock().playing = true;
            self.status_msg = "Playing".to_string();
            announce_status(&format!("Playback started — {:.0} BPM", self.bpm));
        }
    }

    pub fn stop(&mut self) {
        self.engine.stop();
        self.playing = false;
        self.current_step = 0;
        self.project.lock().playing = false;
        self.status_msg = "Stopped".to_string();
        announce_status("Playback stopped");
    }

    pub fn toggle_record(&mut self) {
        self.engine.toggle_record();
        self.recording = !self.recording;
        self.project.lock().recording = self.recording;
    }

    pub fn adjust_bpm(&mut self, delta: f64) {
        self.bpm = (self.bpm + delta).round().clamp(20.0, 300.0);
        self.engine.set_bpm(self.bpm);
    }

    // ── Song transport ────────────────────────────────────────────────────────

    pub fn song_play_stop(&mut self) {
        if self.song_playing {
            self.song_playing = false;
            self.engine.stop();
            self.playing = false;
            self.project.lock().playing = false;
            self.status_msg = "SONG: Stopped".to_string();
        } else {
            self.song_playing = true;
            self.song_bar = 0;
            self.engine.play();
            self.playing = true;
            self.project.lock().playing = true;
            self.status_msg = "SONG: Playing".to_string();
        }
    }

    pub fn song_stop(&mut self) {
        self.song_playing = false;
        self.song_bar = 0;
        self.engine.stop();
        self.playing = false;
        self.current_step = 0;
        self.project.lock().playing = false;
        self.status_msg = "SONG: Stopped".to_string();
    }

    /// Adjust the currently selected transport parameter (BPM, matrix rows, or cols).
    /// Only applies for cursor 4=BPM, 5=ROWS, 6=COLS; cursor 0-3 are trigger buttons.
    pub fn adjust_transport_param(&mut self, delta: i32) {
        match self.transport_cursor {
            4 => {
                self.bpm = (self.bpm + delta as f64).round().clamp(20.0, 300.0);
                self.engine.set_bpm(self.bpm);
            }
            5 => {
                let max_rows = (128 / self.matrix_cols.max(1)).min(16).max(1) as i32;
                self.matrix_rows = ((self.matrix_rows as i32 + delta).clamp(1, max_rows)) as usize;
                let (r, c) = self.matrix_state.cursor;
                self.matrix_state.cursor = (r.min(self.matrix_rows - 1), c);
                self.ensure_matrix_size();
            }
            6 => {
                let max_cols = (128 / self.matrix_rows.max(1)).min(16).max(1) as i32;
                self.matrix_cols = ((self.matrix_cols as i32 + delta).clamp(1, max_cols)) as usize;
                let (r, c) = self.matrix_state.cursor;
                self.matrix_state.cursor = (r, c.min(self.matrix_cols - 1));
                self.ensure_matrix_size();
            }
            _ => {}
        }
    }

    /// Record a tap and compute BPM from the average interval between recent taps.
    pub fn tap_tempo(&mut self) {
        let now = std::time::Instant::now();
        self.tap_times.retain(|t| now.duration_since(*t).as_secs_f64() < 3.0);
        self.tap_times.push(now);
        if self.tap_times.len() >= 2 {
            let n = self.tap_times.len();
            let total = now.duration_since(self.tap_times[0]).as_secs_f64();
            let avg = total / (n - 1) as f64;
            self.bpm = (60.0 / avg).round().clamp(20.0, 300.0);
            self.engine.set_bpm(self.bpm);
            self.status_msg = format!("TAP BPM → {}", self.bpm as u32);
        } else {
            self.status_msg = "TAP — tap again to set BPM".to_string();
        }
    }

    /// Set a status message that automatically clears after `secs` seconds.
    pub fn set_timed_status(&mut self, msg: impl Into<String>, secs: u64) {
        let msg = msg.into();
        announce_status(&msg);
        self.status_msg = msg;
        self.status_expires = Some(
            std::time::Instant::now() + std::time::Duration::from_secs(secs),
        );
    }

    /// Set a persistent status message and announce it for screen readers.
    // ── Tab management ────────────────────────────────────────────────────────

    /// Display name for the tab at logical index `idx` (0 = active tab).
    pub fn tab_name(&self, idx: usize) -> String {
        if idx == self.active_tab {
            self.project_path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "untitled".to_string())
        } else {
            let stored_idx = if idx < self.active_tab { idx } else { idx - 1 };
            self.tabs.get(stored_idx)
                .and_then(|t| t.project_path.as_ref())
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("untitled{}", stored_idx + 1))
        }
    }

    /// Total number of open tabs (active + inactive).
    pub fn tab_count(&self) -> usize {
        self.tabs.len() + 1
    }

    /// Open a new empty project in a new tab and switch to it.
    pub fn new_tab(&mut self) {
        // Save current state into tabs vec at position `active_tab`.
        self.push_current_to_tabs(self.active_tab);
        // Insert after current.
        let new_idx = self.active_tab + 1;
        self.active_tab = new_idx;

        // Bring up a blank project.
        let new_proj = Arc::new(Mutex::new(Project::default()));
        self.project = Arc::clone(&new_proj);
        self.project_path = None;
        self.project_dirty = false;
        self.history = History::default();
        self.current_view = ViewKind::Matrix;
        self.matrix_rows = 8;
        self.matrix_cols = 8;
        self.bpm = self.project.lock().bpm;
        self.audio_slots = std::collections::HashMap::new();

        self.engine.stop();
        self.engine.set_project(Arc::clone(&new_proj));
        self.set_status(format!("Tab {} — new project", new_idx + 1));
    }

    /// Switch to the tab at logical index `target`.
    pub fn switch_tab(&mut self, target: usize) {
        let total = self.tab_count();
        if target >= total || target == self.active_tab {
            return;
        }
        // Save current active state into the tabs vec.
        self.push_current_to_tabs(self.active_tab);

        // Load the target tab.
        let stored_idx = if target < self.active_tab { target } else { target - 1 };
        let tab = self.tabs.remove(stored_idx);
        self.active_tab = target;

        self.project = tab.project;
        self.project_path = tab.project_path;
        self.project_dirty = tab.project_dirty;
        self.history = tab.history;
        self.current_view = tab.current_view;
        self.matrix_rows = tab.matrix_rows;
        self.matrix_cols = tab.matrix_cols;
        self.bpm = tab.bpm;
        self.audio_slots = tab.audio_slots;

        self.engine.stop();
        self.engine.set_project(Arc::clone(&self.project));
        self.engine.set_audio_slots(self.audio_slots.clone());
        self.set_status(format!(
            "Tab {} — {}",
            target + 1,
            self.tab_name(target)
        ));
    }

    /// Close the current tab. Switches to the previous tab (or next if first).
    /// Does nothing if this is the only open tab.
    pub fn close_tab(&mut self) {
        if self.tab_count() <= 1 {
            self.set_status("Only one tab open — use q to quit");
            return;
        }
        // Drop the current active state (don't save it).
        let new_active = if self.active_tab > 0 { self.active_tab - 1 } else { 0 };

        // Load the tab we're switching to.
        let stored_idx = if new_active < self.active_tab { new_active } else { new_active - 1 };
        let tab = self.tabs.remove(stored_idx);

        self.active_tab = new_active;
        self.project = tab.project;
        self.project_path = tab.project_path;
        self.project_dirty = tab.project_dirty;
        self.history = tab.history;
        self.current_view = tab.current_view;
        self.matrix_rows = tab.matrix_rows;
        self.matrix_cols = tab.matrix_cols;
        self.bpm = tab.bpm;
        self.audio_slots = tab.audio_slots;

        self.engine.stop();
        self.engine.set_project(Arc::clone(&self.project));
        self.engine.set_audio_slots(self.audio_slots.clone());
        self.set_status(format!(
            "Closed tab — now on {}",
            self.tab_name(new_active)
        ));
    }

    /// Snapshot the currently active project state into `self.tabs` at `logical_idx`.
    fn push_current_to_tabs(&mut self, logical_idx: usize) {
        let tab = ProjectTab {
            project:       Arc::clone(&self.project),
            project_path:  self.project_path.clone(),
            project_dirty: self.project_dirty,
            history:       std::mem::take(&mut self.history),
            current_view:  self.current_view,
            matrix_rows:   self.matrix_rows,
            matrix_cols:   self.matrix_cols,
            bpm:           self.bpm,
            audio_slots:   self.audio_slots.clone(),
        };
        // Insert at the stored index corresponding to logical_idx.
        let stored_idx = if logical_idx <= self.tabs.len() { logical_idx } else { self.tabs.len() };
        self.tabs.insert(stored_idx, tab);
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        announce_status(&msg);
        self.status_msg = msg;
        self.status_expires = None;
    }

    /// Scan real system MIDI ports and update project + unavailability tracking.
    /// Called at startup and when the user opens Config view.
    pub fn refresh_midi_ports(&mut self) {
        let outputs = seqterm_midi::list_output_ports().unwrap_or_default();
        let inputs  = seqterm_midi::list_input_ports().unwrap_or_default();

        // Update inputs separately (not tracked by the background watcher).
        {
            use std::collections::HashMap;
            let mut proj = self.project.lock();
            let old_in: HashMap<String, (bool, u8)> = proj.midi_inputs.iter()
                .map(|p| (p.name.clone(), (p.enabled, p.channel)))
                .collect();
            proj.midi_inputs = inputs
                .into_iter()
                .map(|name| {
                    let (enabled, ch) = old_in.get(&name).copied().unwrap_or((false, 1));
                    seqterm_core::MidiPort { name, enabled, channel: ch }
                })
                .collect();
        }

        // Delegate output handling to the shared updater.
        self.apply_port_update(outputs);
        // Open/close input ports to match the newly refreshed enabled flags.
        self.sync_midi_input_bus();
    }

    /// Apply a fresh snapshot of available output port names.
    /// Updates `proj.midi_outputs`, computes `unavailable_midi_routes`, detects JACK.
    pub fn apply_port_update(&mut self, new_outputs: Vec<String>) {
        use std::collections::HashMap;

        let port_set: HashSet<_> = new_outputs.iter().cloned().collect();

        // Detect JACK/PipeWire from port names or jack_lsp.
        self.jack_available = port_set.iter().any(|n| {
            let l = n.to_lowercase();
            l.contains("jack") || l.contains("a2j") || l.contains("pipewire")
        }) || std::process::Command::new("jack_lsp")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        let mut proj = self.project.lock();

        // Merge: preserve enabled/channel state for ports we already knew.
        let old: HashMap<String, (bool, u8)> = proj.midi_outputs.iter()
            .map(|p| (p.name.clone(), (p.enabled, p.channel)))
            .collect();

        proj.midi_outputs = new_outputs.into_iter()
            .map(|name| {
                let (enabled, ch) = old.get(&name).copied().unwrap_or((false, 1));
                seqterm_core::MidiPort { name, enabled, channel: ch }
            })
            .collect();

        // Recompute which clip routes point to ports that are gone.
        let mut unavailable = HashSet::new();
        for slots in proj.matrix.values() {
            for clip in slots.iter().flatten() {
                if let Some(out) = &clip.midi_out {
                    if !port_set.contains(out.as_str()) {
                        unavailable.insert(out.clone());
                    }
                }
            }
        }
        drop(proj);

        self.unavailable_midi_routes = unavailable;
        self.last_midi_refresh = Some(std::time::Instant::now());
    }

    /// Open/close MIDI input ports on the bus to match `proj.midi_inputs[*].enabled`.
    pub fn sync_midi_input_bus(&mut self) {
        let enabled: Vec<String> = {
            let proj = self.project.lock();
            proj.midi_inputs
                .iter()
                .filter(|p| p.enabled)
                .map(|p| p.name.clone())
                .collect()
        };
        // Close ports that are open but no longer enabled.
        let open: Vec<String> = {
            let proj = self.project.lock();
            proj.midi_inputs
                .iter()
                .filter(|p| !p.enabled)
                .map(|p| p.name.clone())
                .collect()
        };
        for name in &open {
            if self.midi_input_bus.is_open(name) {
                self.midi_input_bus.close_port(name);
            }
        }
        // Open ports that are enabled but not yet open.
        for name in &enabled {
            if !self.midi_input_bus.is_open(name) {
                if let Err(e) = self.midi_input_bus.open_port(name) {
                    tracing::warn!("MIDI input open '{name}': {e}");
                }
            }
        }
    }

    /// Drain all pending MIDI input messages and dispatch them.
    ///
    /// CC messages feed the MIDI-learn system (same path as `EngineEvent::MidiCc`).
    /// NoteOn/Off are forwarded to the audio engine for the currently focused SF2/AudioFile slot.
    pub fn process_midi_inputs(&mut self) {
        // Collect messages first to avoid borrow issues.
        let mut messages = Vec::new();
        while let Some(msg) = self.midi_input_bus.try_recv() {
            messages.push(msg);
        }
        for (_port, msg) in messages {
            match msg {
                seqterm_midi::MidiMessage::CC { channel: ch, control: cc, value: val } => {
                    if let Some(target) = self.midi_learn.take() {
                        use seqterm_persistence::MidiLearnBinding;
                        let binding = MidiLearnBinding::new(target.clone(), ch, cc);
                        self.settings.midi_learn_bindings.retain(|b| b.target != target);
                        self.settings.midi_learn_bindings.push(binding);
                        let _ = seqterm_persistence::save_settings(&self.settings);
                        self.status_msg = format!("Bound: CC{cc} (ch{}) → {}", ch + 1, target.label());
                    } else {
                        for b in &self.settings.midi_learn_bindings {
                            if b.cc != cc || b.midi_ch != ch { continue; }
                            match &b.target {
                                seqterm_persistence::MidiLearnTarget::ChannelVolume(i) => {
                                    let mut proj = self.project.lock();
                                    if let Some(ch_strip) = proj.channels.get_mut(*i) {
                                        ch_strip.volume = val as f32 / 127.0 * 66.0 - 60.0;
                                    }
                                }
                                seqterm_persistence::MidiLearnTarget::ChannelSendA(i) => {
                                    let mut proj = self.project.lock();
                                    if let Some(ch_strip) = proj.channels.get_mut(*i) {
                                        ch_strip.send_a = val;
                                    }
                                }
                                seqterm_persistence::MidiLearnTarget::ChannelSendB(i) => {
                                    let mut proj = self.project.lock();
                                    if let Some(ch_strip) = proj.channels.get_mut(*i) {
                                        ch_strip.send_b = val;
                                    }
                                }
                                seqterm_persistence::MidiLearnTarget::Bpm => {
                                    let bpm = 60.0 + val as f64 / 127.0 * 180.0;
                                    self.bpm = bpm;
                                    self.engine.set_bpm(bpm);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                seqterm_midi::MidiMessage::NoteOn { channel, note, velocity } => {
                    // Forward to the audio engine slot of the focused tracker pattern.
                    if let Some(slot_id) = self.focused_tracker_slot() {
                        if let Some(ae) = &mut self.audio_engine {
                            ae.send(seqterm_audio_engine::AudioCommand::NoteOn {
                                slot_id, channel, note, velocity,
                            });
                        }
                    }
                    // Live record: insert note at current step when recording is active.
                    if self.recording {
                        self.record_midi_note(note, velocity, true);
                    }
                }
                seqterm_midi::MidiMessage::NoteOff { channel, note } => {
                    if let Some(slot_id) = self.focused_tracker_slot() {
                        if let Some(ae) = &mut self.audio_engine {
                            ae.send(seqterm_audio_engine::AudioCommand::NoteOff {
                                slot_id, channel, note,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Return the audio engine slot_id for the pattern currently open in the tracker, if any.
    fn focused_tracker_slot(&self) -> Option<u32> {
        let pat_key = self.tracker_state.pattern_key.as_ref()?;
        let proj = self.project.lock();
        for (row_key, slots) in &proj.matrix {
            for (col, slot) in slots.iter().enumerate() {
                if let Some(clip) = slot {
                    if clip.pattern_key.as_deref() == Some(pat_key) {
                        let clip_key = format!("{row_key}{col}");
                        drop(proj);
                        return self.audio_slots.get(&clip_key).copied();
                    }
                }
            }
        }
        None
    }

    /// Insert a note at the current playback step in the focused tracker pattern.
    fn record_midi_note(&mut self, note: u8, velocity: u8, is_on: bool) {
        if !is_on { return; }
        let Some(pat_key) = self.tracker_state.pattern_key.clone() else { return };
        let result = {
            let proj = self.project.lock();
            let pat = match proj.patterns.get(&pat_key) { Some(p) => p, None => return };
            let step = self.current_step % pat.length.max(1);
            let old = match pat.steps.get(step) { Some(n) => n.clone(), None => return };
            (step, old)
        };
        let (step, old_note) = result;
        let Ok(new_note) = seqterm_core::Note::from_midi(note, velocity) else { return };
        if old_note != new_note {
            let mut proj = self.project.lock();
            self.history.push(
                Box::new(seqterm_history::SetNote {
                    pattern_key: pat_key,
                    step,
                    old: old_note,
                    new: new_note,
                }),
                &mut proj,
            );
        }
    }

    /// Toggle enabled/disabled for the focused item in the Config view.
    pub fn toggle_config_item_enabled(&mut self) {
        let cursor = self.config_state.cursor;
        match self.config_state.section {
            0 => {
                let result = {
                    let mut proj = self.project.lock();
                    proj.midi_inputs.get_mut(cursor).map(|p| {
                        p.enabled = !p.enabled;
                        (p.name.clone(), p.enabled)
                    })
                };
                if let Some((name, en)) = result {
                    self.status_msg = format!(
                        "MIDI IN '{}' → {}",
                        name,
                        if en { "ENABLED" } else { "DISABLED" }
                    );
                    // Open or close the port on the input bus immediately.
                    self.sync_midi_input_bus();
                }
            }
            1 => {
                let result = {
                    let mut proj = self.project.lock();
                    proj.midi_outputs.get_mut(cursor).map(|p| {
                        p.enabled = !p.enabled;
                        (p.name.clone(), p.enabled)
                    })
                };
                if let Some((name, en)) = result {
                    self.status_msg = format!(
                        "MIDI OUT '{}' → {}",
                        name,
                        if en { "ENABLED" } else { "DISABLED" }
                    );
                }
            }
            2 => {
                let result = {
                    let mut proj = self.project.lock();
                    proj.osc_routes.get_mut(cursor).map(|r| {
                        r.enabled = !r.enabled;
                        (r.address.clone(), r.enabled)
                    })
                };
                if let Some((addr, en)) = result {
                    self.status_msg = format!(
                        "OSC '{}' → {}",
                        addr,
                        if en { "ENABLED" } else { "DISABLED" }
                    );
                }
            }
            3 => {
                // Select sync mode under cursor.
                use seqterm_core::SyncMode;
                let modes = [SyncMode::Internal, SyncMode::Usb, SyncMode::Midi, SyncMode::Clock];
                if let Some(mode) = modes.get(cursor) {
                    let label = mode.label();
                    self.project.lock().sync_mode = mode.clone();
                    self.status_msg = format!("Sync mode → {}", label);
                }
            }
            _ => {}
        }
    }

    /// Toggle the enabled state of the clip at the current matrix cursor.
    pub fn toggle_clip_enabled(&mut self) {
        let (row, col) = self.matrix_state.cursor;
        let row_key = ((b'A' + row as u8) as char).to_string();
        let result = {
            let mut proj = self.project.lock();
            proj.matrix
                .get_mut(&row_key)
                .and_then(|slots| slots.get_mut(col))
                .and_then(|slot| slot.as_mut())
                .map(|clip| {
                    clip.enabled = !clip.enabled;
                    (clip.name.clone(), clip.enabled)
                })
        };
        if let Some((name, enabled)) = result {
            self.status_msg = format!(
                "Clip {} → {}",
                name,
                if enabled { "ENABLED" } else { "DISABLED" }
            );
        }
    }

    /// Ensure the project matrix has enough rows/cols allocated for the current size.
    pub fn ensure_matrix_size(&mut self) {
        let mut proj = self.project.lock();
        for r in 0..self.matrix_rows {
            let row_key = ((b'A' + r as u8) as char).to_string();
            let slots = proj.matrix.entry(row_key).or_insert_with(Vec::new);
            if slots.len() < self.matrix_cols {
                slots.resize(self.matrix_cols, None);
            }
        }
    }

    // ── View-specific actions ──────────────────────────────────────────────────

    pub fn move_cursor(&mut self, dr: i32, dc: i32) {
        match self.current_view {
            ViewKind::Matrix => {
                if self.matrix_section == 1 {
                    // Transport: ←→ navigates 7 items (0-6), ↑↓ adjusts value for tc=4-6.
                    if dc != 0 {
                        self.transport_cursor =
                            (self.transport_cursor as i32 + dc).rem_euclid(7) as usize;
                    } else if dr != 0 {
                        self.adjust_transport_param(-dr);
                    }
                } else if self.matrix_section == 2 {
                    // Polymeter: ↑↓ selects pattern row, ←→ scrolls the step window.
                    if dr != 0 {
                        let n = self.project.lock().patterns.len();
                        self.polymeter_cursor = (self.polymeter_cursor as i32 + dr)
                            .clamp(0, n.saturating_sub(1) as i32) as usize;
                        // Keep pat_scroll so cursor stays visible (clamped at draw time).
                        self.polymeter_pat_scroll = self.polymeter_pat_scroll
                            .min(self.polymeter_cursor);
                    }
                    if dc != 0 {
                        let new_start = self.polymeter_step_start as i32 + dc * 4;
                        self.polymeter_step_start = new_start.max(0) as usize;
                    }
                } else if self.matrix_section == 3 {
                    if self.routing_tab == 1 {
                        // Source browser: ↑↓ navigates source list.
                        if dr != 0 {
                            let n = {
                                let proj = self.project.lock();
                                let mut count = 0usize;
                                let mut seen: Vec<(std::path::PathBuf, u8, u8, bool)> = Vec::new();
                                for slots in proj.matrix.values() {
                                    for opt in slots {
                                        if let Some(clip) = opt {
                                            let key = match &clip.source {
                                                seqterm_core::PatternSource::Sf2 { path, bank, preset, .. } =>
                                                    Some((path.clone(), *bank, *preset, false)),
                                                seqterm_core::PatternSource::AudioFile { path, .. } =>
                                                    Some((path.clone(), 0, 0, true)),
                                                _ => None,
                                            };
                                            if let Some(k) = key {
                                                if !seen.contains(&k) {
                                                    seen.push(k);
                                                    count += 1;
                                                }
                                            }
                                        }
                                    }
                                }
                                count
                            };
                            if n > 0 {
                                self.routing_source_cursor =
                                    (self.routing_source_cursor as i32 + dr).clamp(0, n as i32 - 1) as usize;
                            }
                        }
                    } else {
                        // MIDI tab: ↑↓ navigates MIDI output list; ←→ adjusts MIDI channel.
                        if dr != 0 {
                            let n = self.project.lock().midi_outputs.len();
                            self.routing_cursor = (self.routing_cursor as i32 + dr)
                                .clamp(0, n as i32) as usize;
                        }
                        if dc != 0 {
                            let (row, col) = self.matrix_state.cursor;
                            let row_key = ((b'A' + row as u8) as char).to_string();
                            let mut proj = self.project.lock();
                            if let Some(slots) = proj.matrix.get_mut(&row_key) {
                                if let Some(Some(clip)) = slots.get_mut(col) {
                                    clip.midi_channel = (clip.midi_channel as i32 + dc)
                                        .clamp(1, 16) as u8;
                                }
                            }
                        }
                    }
                } else {
                    let (r, c) = self.matrix_state.cursor;
                    self.matrix_state.cursor = (
                        (r as i32 + dr).clamp(0, self.matrix_rows as i32 - 1) as usize,
                        (c as i32 + dc).clamp(0, self.matrix_cols as i32 - 1) as usize,
                    );
                }
            }
            ViewKind::Tracker => {
                match self.tracker_section {
                    2 => {
                        // Generative engine: ↑↓ moves between rows, ←→ adjusts value.
                        if dr != 0 {
                            self.generative_cursor =
                                (self.generative_cursor as i32 + dr).clamp(0, 13) as usize;
                        }
                        if dc != 0 {
                            self.adjust_generative_param(dc);
                        }
                    }
                    1 => {
                        // Piano roll: ←→ moves piano cursor step, ↑↓ moves note row.
                        // The view scrolls automatically to follow the cursor.
                        let pat_len = {
                            let proj = self.project.lock();
                            self.tracker_state
                                .pattern_key
                                .as_ref()
                                .and_then(|k| proj.patterns.get(k))
                                .map(|p| p.length)
                                .unwrap_or(16)
                        };
                        if dc != 0 {
                            let new_step = (self.piano_cursor.1 as i32 + dc)
                                .clamp(0, pat_len as i32 - 1) as usize;
                            self.piano_cursor.1 = new_step;
                            self.tracker_state.cursor.0 = new_step;
                            self.clamp_piano_step_scroll(new_step);
                            self.clamp_tracker_scroll();
                        }
                        if dr != 0 {
                            // NOTE_ROWS has 88 entries (C9=row0 down to A1=row87).
                            let new_row = (self.piano_cursor.0 as i32 + dr)
                                .clamp(0, 87) as usize;
                            self.piano_cursor.0 = new_row;
                            self.clamp_piano_note_scroll(new_row);
                        }
                    }
                    3 => {
                        // Track modulation: ←→ moves between parameters, ↑↓ adjusts value.
                        if dc != 0 {
                            self.modulation_cursor =
                                (self.modulation_cursor as i32 + dc).clamp(0, 7) as usize;
                        }
                        if dr != 0 {
                            self.adjust_modulation_param(-dr);
                        }
                    }
                    _ => {
                        // Step table: navigation and edit-mode column switching.
                        if self.tracker_editing {
                            // ↑↓ adjust value of current field.
                            if dr != 0 {
                                self.adjust_tracker_field(dr);
                            }
                            // ←→ switch to adjacent column, staying in edit mode.
                            if dc != 0 {
                                let col_count = self.tracker_state.columns.len();
                                let new_col = (self.tracker_state.cursor.1 as i32 + dc)
                                    .clamp(0, col_count as i32 - 1) as usize;
                                self.tracker_state.cursor.1 = new_col;
                                self.tracker_edit_field = new_col;
                                self.status_msg = format!(
                                    "EDIT: {} | ↑↓=adjust  ←→=column  Esc=exit",
                                    self.tracker_state.columns.get(new_col).unwrap_or(&"?")
                                );
                            }
                        } else {
                            let proj = self.project.lock();
                            let pat_len = self
                                .tracker_state
                                .pattern_key
                                .as_ref()
                                .and_then(|k| proj.patterns.get(k))
                                .map(|p| p.length)
                                .unwrap_or(16) as i32;
                            let col_count = self.tracker_state.columns.len() as i32;
                            drop(proj);
                            let (r, c) = self.tracker_state.cursor;
                            self.tracker_state.cursor = (
                                (r as i32 + dr).clamp(0, pat_len - 1) as usize,
                                (c as i32 + dc).clamp(0, col_count - 1) as usize,
                            );
                            self.clamp_tracker_scroll();
                            // Keep piano cursor step in sync with tracker cursor row.
                            let new_step = self.tracker_state.cursor.0;
                            self.piano_cursor.1 = new_step;
                            self.clamp_piano_step_scroll(new_step);
                        }
                    }
                }
            }
            ViewKind::Arranger => {
                if self.arranger_state.section == 1 {
                    // Automation section: navigate lanes and cursor.
                    if dr != 0 {
                        let proj = self.project.lock();
                        let n_lanes = proj.automation.len().saturating_sub(1);
                        drop(proj);
                        self.arranger_state.automation_lane =
                            (self.arranger_state.automation_lane as i32 + dr)
                                .clamp(0, n_lanes as i32) as usize;
                    }
                    if dc != 0 {
                        let new_cur =
                            (self.arranger_state.automation_cursor as i32 + dc).max(0) as usize;
                        self.arranger_state.automation_cursor = new_cur;
                    }
                } else {
                    if dr != 0 {
                        self.arranger_state.selected_track =
                            (self.arranger_state.selected_track as i32 + dr)
                                .clamp(0, self.matrix_rows.saturating_sub(1) as i32) as usize;
                    }
                    if dc != 0 {
                        let new_offset =
                            (self.arranger_state.bar_offset as i32 + dc).max(0) as u32;
                        self.arranger_state.bar_offset = new_offset;
                    }
                }
            }
            ViewKind::Mixer => {
                if self.mixer_state.editing {
                    let idx = self.mixer_state.selected_channel;
                    if dr != 0 {
                        let param = self.mixer_state.active_param;
                        self.adjust_mixer_param(idx, param, -dr);
                    }
                    if dc != 0 {
                        self.mixer_state.active_param =
                            (self.mixer_state.active_param as i32 + dc).rem_euclid(7) as usize;
                    }
                } else if dc != 0 {
                    let n = {
                        let proj = self.project.lock();
                        crate::views::mixer::mixer_entry_count(&proj).saturating_sub(1)
                    };
                    self.mixer_state.selected_channel =
                        (self.mixer_state.selected_channel as i32 + dc)
                            .clamp(0, n as i32) as usize;
                } else if dr != 0 {
                    // Non-edit: quick volume nudge on selected entry.
                    let idx = self.mixer_state.selected_channel;
                    self.adjust_mixer_param(idx, 0, -dr);
                }
            }
            ViewKind::Config => {
                if self.config_state.section == 4 {
                    // Routing graph sub-panel inside Config view.
                    let n = self.project.lock().routing.nodes.len();
                    if n == 0 { return; }
                    if self.routing_state.section == 0 {
                        if dr != 0 {
                            self.routing_state.node_cursor =
                                (self.routing_state.node_cursor as i32 + dr)
                                    .clamp(0, n.saturating_sub(1) as i32) as usize;
                            let c = self.routing_state.node_cursor;
                            let s = &mut self.routing_state.scroll;
                            if c < *s { *s = c; }
                        }
                    } else {
                        if dr != 0 {
                            self.routing_state.node_cursor =
                                (self.routing_state.node_cursor as i32 + dr)
                                    .clamp(0, n.saturating_sub(1) as i32) as usize;
                        }
                        if dc != 0 {
                            self.routing_state.col_cursor =
                                (self.routing_state.col_cursor as i32 + dc)
                                    .clamp(0, n.saturating_sub(1) as i32) as usize;
                        }
                    }
                } else {
                    if dc != 0 {
                        self.config_state.section =
                            (self.config_state.section as i32 + dc).clamp(0, 4) as usize;
                        self.config_state.cursor = 0;
                    }
                    if dr != 0 {
                        let max = match self.config_state.section {
                            0 => self.project.lock().midi_inputs.len().saturating_sub(1),
                            1 => self.project.lock().midi_outputs.len().saturating_sub(1),
                            2 => self.project.lock().osc_routes.len().saturating_sub(1),
                            _ => 3,
                        };
                        self.config_state.cursor =
                            (self.config_state.cursor as i32 + dr).clamp(0, max as i32) as usize;
                    }
                }
            }
            ViewKind::Sampler => {
                let (r, c) = self.sampler_state.cursor;
                let new_r = (r as i32 + dr).rem_euclid(4) as usize;
                let new_c = (c as i32 + dc).rem_euclid(4) as usize;
                self.sampler_state.cursor = (new_r, new_c);
            }
            ViewKind::Granular => {
                if dr != 0 {
                    self.granular_state.cursor = (self.granular_state.cursor as i32 + dr)
                        .rem_euclid(GRAN_PARAM_COUNT as i32) as usize;
                }
                if dc != 0 {
                    self.adjust_granular_param(dc);
                }
            }
        }
    }

    pub fn toggle_mute(&mut self) {
        if self.current_view == ViewKind::Mixer {
            let idx = self.mixer_state.selected_channel;
            let dest = {
                let proj = self.project.lock();
                let entries = crate::views::mixer::collect_mixer_entries(&proj);
                entries.get(idx).map(|e| e.dest.clone())
            };
            if let Some(dest) = dest {
                let mut proj = self.project.lock();
                if !proj.channels.iter().any(|c| c.midi_port.as_deref() == Some(dest.as_str())) {
                    let mut ch = seqterm_core::Channel::new(dest.clone());
                    ch.midi_port = Some(dest.clone());
                    proj.channels.push(ch);
                }
                if let Some(ch) = proj.channels.iter_mut()
                    .find(|c| c.midi_port.as_deref() == Some(dest.as_str()))
                {
                    ch.mute = !ch.mute;
                }
            }
        }
    }

    /// Adjust a mixer parameter for the given entry index.
    /// `param`: 0=VOL, 1=EQ_LO, 2=EQ_LM, 3=EQ_HM, 4=EQ_HI, 5=PAN, 6=FX.
    pub fn adjust_mixer_param(&mut self, entry_idx: usize, param: usize, delta: i32) {
        let n_midi = {
            let proj = self.project.lock();
            crate::views::mixer::collect_mixer_entries(&proj).len()
        };

        // Audio engine slot: indices n_midi+2 onward (after MASTER L / MASTER R).
        if entry_idx >= n_midi + 2 {
            let audio_idx = entry_idx - n_midi - 2;
            let mut sorted_keys: Vec<String> = self.audio_slots.keys().cloned().collect();
            sorted_keys.sort();
            if let Some(key) = sorted_keys.get(audio_idx) {
                if let Some(&slot_id) = self.audio_slots.get(key) {
                    let vol = self.audio_slot_volumes.entry(slot_id).or_insert(1.0);
                    *vol = (*vol + delta as f32 * 0.05).clamp(0.0, 2.0);
                    let new_vol = *vol;
                    if let Some(ae) = self.audio_engine.as_mut() {
                        ae.send(seqterm_audio_engine::AudioCommand::SetSlotVolume {
                            slot_id,
                            volume: new_vol,
                        });
                    }
                }
            }
            return;
        }

        let dest = {
            let proj = self.project.lock();
            let entries = crate::views::mixer::collect_mixer_entries(&proj);
            entries.get(entry_idx).map(|e| e.dest.clone())
        };
        let Some(dest) = dest else { return };

        // Read old value for history.
        let (ch_param, old_val) = {
            let proj = self.project.lock();
            let ch = proj.channels.iter().find(|c| c.midi_port.as_deref() == Some(dest.as_str()));
            let defaults = seqterm_core::Channel::new(dest.clone());
            let ch = ch.unwrap_or(&defaults);
            match param {
                0 => (Some(hist::ChannelParam::Volume),    (ch.volume * 10.0) as i32),
                1 => (Some(hist::ChannelParam::EqLow),     ch.eq_low as i32),
                2 => (Some(hist::ChannelParam::EqLowMid),  ch.eq_low_mid as i32),
                3 => (Some(hist::ChannelParam::EqHighMid), ch.eq_high_mid as i32),
                4 => (Some(hist::ChannelParam::EqHigh),    ch.eq_high as i32),
                5 => (Some(hist::ChannelParam::Pan),       ch.pan.to_val() as i32),
                6 => (Some(hist::ChannelParam::FxAmount),  ch.fx_amount as i32),
                _ => (None, 0),
            }
        };
        let Some(ch_param) = ch_param else { return };

        let new_val = match ch_param {
            hist::ChannelParam::Volume    => ((old_val as f32 / 10.0 + delta as f32 * 0.5).clamp(-60.0, 6.0) * 10.0) as i32,
            hist::ChannelParam::Pan       => (old_val + delta).clamp(-50, 50),
            hist::ChannelParam::FxAmount  => (old_val + delta).clamp(0, 127),
            _                             => (old_val + delta).clamp(-12, 12),
        };
        if old_val == new_val { return; }

        {
            let mut proj = self.project.lock();
            if !proj.channels.iter().any(|c| c.midi_port.as_deref() == Some(dest.as_str())) {
                let mut ch = seqterm_core::Channel::new(dest.clone());
                ch.midi_port = Some(dest.clone());
                proj.channels.push(ch);
            }
            self.history.push(Box::new(hist::SetChannelParam {
                channel_port: dest,
                param: ch_param,
                old: old_val,
                new: new_val,
            }), &mut proj);
        }
        self.project_dirty = true;
    }

    /// Adjust a field in the FX slot panel.
    /// fx_row: 0=type, 1=midi_port, 2=midi_ch, 3-10=param[0-7].
    /// fx_col: 0=CC#, 1=value (for param rows).
    pub fn adjust_fx_slot_param(&mut self, delta: i32) {
        let entry_idx = self.mixer_state.selected_channel;
        let slot_idx  = self.mixer_state.fx_slot_idx;
        let row       = self.mixer_state.fx_row;
        let col       = self.mixer_state.fx_col;

        let dest = {
            let proj = self.project.lock();
            let entries = crate::views::mixer::collect_mixer_entries(&proj);
            entries.get(entry_idx).map(|e| e.dest.clone())
        };
        let Some(dest) = dest else { return };

        let available_ports = {
            let proj = self.project.lock();
            proj.midi_outputs.iter().map(|p| p.name.clone()).collect::<Vec<_>>()
        };

        let mut proj = self.project.lock();
        if !proj.channels.iter().any(|c| c.midi_port.as_deref() == Some(dest.as_str())) {
            let mut ch = seqterm_core::Channel::new(dest.clone());
            ch.midi_port = Some(dest.clone());
            proj.channels.push(ch);
        }
        if let Some(ch) = proj.channels.iter_mut()
            .find(|c| c.midi_port.as_deref() == Some(dest.as_str()))
        {
            let slot = &mut ch.fx[slot_idx];
            match row {
                0 => {
                    slot.kind = if delta > 0 { slot.kind.next() } else { slot.kind.prev() };
                    slot.enabled = slot.kind != seqterm_core::FxKind::None;
                }
                1 => {
                    if available_ports.is_empty() { return; }
                    let cur = available_ports.iter().position(|p| *p == slot.midi_port).unwrap_or(0);
                    let next = (cur as i32 + delta).rem_euclid(available_ports.len() as i32) as usize;
                    slot.midi_port = available_ports[next].clone();
                }
                2 => {
                    slot.midi_channel = (slot.midi_channel as i32 + delta).clamp(1, 16) as u8;
                }
                r @ 3..=10 => {
                    let p = r - 3;
                    if col == 0 {
                        slot.cc_nums[p] = (slot.cc_nums[p] as i32 + delta).clamp(0, 127) as u8;
                    } else {
                        slot.cc_vals[p] = (slot.cc_vals[p] as i32 + delta).clamp(0, 127) as u8;
                    }
                }
                _ => {}
            }
        }
    }

    /// Adjust the granular param at the current cursor position by `delta` steps.
    /// Also sends the updated params/zone to the audio engine if a slot is loaded.
    pub fn adjust_granular_param(&mut self, delta: i32) {
        use seqterm_core::{GrainDirection, GrainEnvelope, ScanMode};

        let cursor = self.granular_state.cursor;
        let slot_id = self.granular_state.pad
            .and_then(|key| self.sampler_slots.get(&key).copied());

        match cursor {
            0  => self.granular_state.params.size_ms =
                    (self.granular_state.params.size_ms + delta as f32 * 5.0).clamp(1.0, 500.0),
            1  => self.granular_state.params.density =
                    (self.granular_state.params.density + delta as f32).clamp(1.0, 200.0),
            2  => self.granular_state.params.spray =
                    (self.granular_state.params.spray + delta as f32 * 0.01).clamp(0.0, 1.0),
            3  => self.granular_state.params.overlap =
                    (self.granular_state.params.overlap + delta as f32 * 0.05).clamp(0.0, 1.0),
            4  => self.granular_state.params.pitch_st =
                    (self.granular_state.params.pitch_st + delta as f32).clamp(-24.0, 24.0),
            5  => {
                self.granular_state.params.direction = match delta {
                    d if d > 0 => match self.granular_state.params.direction {
                        GrainDirection::Forward  => GrainDirection::Backward,
                        GrainDirection::Backward => GrainDirection::Random,
                        GrainDirection::Random   => GrainDirection::Forward,
                    },
                    _ => match self.granular_state.params.direction {
                        GrainDirection::Forward  => GrainDirection::Random,
                        GrainDirection::Random   => GrainDirection::Backward,
                        GrainDirection::Backward => GrainDirection::Forward,
                    },
                };
            }
            6  => self.granular_state.params.pan =
                    (self.granular_state.params.pan + delta as f32 * 0.05).clamp(-1.0, 1.0),
            7  => self.granular_state.params.gain =
                    (self.granular_state.params.gain + delta as f32 * 0.05).clamp(0.0, 4.0),
            8  => self.granular_state.params.jitter =
                    (self.granular_state.params.jitter + delta as f32 * 0.01).clamp(0.0, 1.0),
            9  => self.granular_state.params.stereo_spread =
                    (self.granular_state.params.stereo_spread + delta as f32 * 0.05).clamp(0.0, 1.0),
            10 => {
                self.granular_state.params.envelope = match delta {
                    d if d > 0 => match self.granular_state.params.envelope {
                        GrainEnvelope::Hann        => GrainEnvelope::Gaussian,
                        GrainEnvelope::Gaussian    => GrainEnvelope::Triangle,
                        GrainEnvelope::Triangle    => GrainEnvelope::Exponential,
                        GrainEnvelope::Exponential => GrainEnvelope::Hann,
                    },
                    _ => match self.granular_state.params.envelope {
                        GrainEnvelope::Hann        => GrainEnvelope::Exponential,
                        GrainEnvelope::Exponential => GrainEnvelope::Triangle,
                        GrainEnvelope::Triangle    => GrainEnvelope::Gaussian,
                        GrainEnvelope::Gaussian    => GrainEnvelope::Hann,
                    },
                };
            }
            11 => self.granular_state.params.max_voices =
                    ((self.granular_state.params.max_voices as i32 + delta).clamp(1, 32)) as u8,
            12 => self.granular_state.zone.position =
                    (self.granular_state.zone.position + delta as f32 * 0.01).clamp(0.0, 1.0),
            13 => self.granular_state.zone.range =
                    (self.granular_state.zone.range + delta as f32 * 0.05).clamp(0.0, 1.0),
            14 => self.granular_state.zone.scan_speed =
                    (self.granular_state.zone.scan_speed + delta as f32 * 0.05).clamp(0.0, 2.0),
            15 => {
                self.granular_state.zone.scan_mode = match delta {
                    d if d > 0 => match self.granular_state.zone.scan_mode {
                        ScanMode::Linear     => ScanMode::RandomWalk,
                        ScanMode::RandomWalk => ScanMode::Freeze,
                        ScanMode::Freeze     => ScanMode::Linear,
                    },
                    _ => match self.granular_state.zone.scan_mode {
                        ScanMode::Linear     => ScanMode::Freeze,
                        ScanMode::Freeze     => ScanMode::RandomWalk,
                        ScanMode::RandomWalk => ScanMode::Linear,
                    },
                };
            }
            16 => {
                self.granular_state.zone.frozen = !self.granular_state.zone.frozen;
            }
            _ => {}
        }

        if let (Some(slot_id), Some(ae)) = (slot_id, self.audio_engine.as_mut()) {
            ae.send(seqterm_audio_engine::AudioCommand::SetGranularParams {
                slot_id,
                params: self.granular_state.params.clone(),
            });
            ae.send(seqterm_audio_engine::AudioCommand::SetGranularZone {
                slot_id,
                zone: self.granular_state.zone.clone(),
            });
        }
    }

    /// Return the audio engine slot_id for the currently selected mixer channel,
    /// or `None` if the selected channel is a MIDI channel or MASTER.
    pub fn selected_audio_slot_id(&self) -> Option<u32> {
        use crate::views::mixer::{collect_mixer_entries, collect_audio_slot_entries};
        let n_midi = { let proj = self.project.lock(); collect_mixer_entries(&proj).len() };
        let sel = self.mixer_state.selected_channel;
        if sel < n_midi + 2 { return None; }
        let audio_idx = sel - (n_midi + 2);
        let audio_entries = collect_audio_slot_entries(self);
        audio_entries.get(audio_idx).map(|e| e.slot_id)
    }

    /// True when the MASTER channel (L or R) is focused in the Mixer view.
    pub fn is_master_channel_selected(&self) -> bool {
        use crate::views::mixer::collect_mixer_entries;
        let n_midi = { let proj = self.project.lock(); collect_mixer_entries(&proj).len() };
        let sel = self.mixer_state.selected_channel;
        sel == n_midi || sel == n_midi + 1
    }

    /// Rebuild the audio FX chain for `slot_id` from `audio_slot_fx` and
    /// send `AudioCommand::SetSlotFxChain` to the audio engine.
    pub fn rebuild_audio_fx_chain(&mut self, slot_id: u32) {
        use seqterm_audio_engine::fx::{
            FxProcessor, DelayLine, Reverb, Bitcrusher, VinylSim,
            Isolator, Cassette, Looper, SidechainDuck, Svf, SvfMode as SvfModeAudio,
            FilterBankFx,
        };

        let entries = self.audio_slot_fx
            .get(&slot_id)
            .cloned()
            .unwrap_or_default();

        let chain: Vec<Box<dyn FxProcessor>> = entries.iter()
            .filter(|e| e.enabled)
            .map(|e| {
                let mut p: Box<dyn FxProcessor> = match e.kind {
                    AudioFxKind::Delay       => Box::new(DelayLine::new(250.0, 0.4, 0.3)),
                    AudioFxKind::Reverb      => Box::new(Reverb::new(48000)),
                    AudioFxKind::BitCrusher  => Box::new(Bitcrusher::new()),
                    AudioFxKind::Vinyl       => Box::new(VinylSim::new()),
                    AudioFxKind::Isolator    => Box::new(Isolator::new()),
                    AudioFxKind::Cassette    => Box::new(Cassette::new()),
                    AudioFxKind::Looper      => Box::new(Looper::new(48000)),
                    AudioFxKind::SidechainDuck => Box::new(SidechainDuck::new()),
                    AudioFxKind::Filter        => Box::new(Svf::new(SvfModeAudio::Lowpass, 2000.0, 0.7)),
                    AudioFxKind::FilterBank    => Box::new(FilterBankFx::new(48000)),
                };
                p.set_mix(e.wet);
                p
            })
            .collect();

        if let Some(ae) = self.audio_engine.as_mut() {
            ae.send(seqterm_audio_engine::AudioCommand::SetSlotFxChain { slot_id, chain });
        }
    }

    /// Rebuild the master bus FX chain from `master_fx` and send `SetMasterFxChain`.
    pub fn rebuild_master_fx_chain(&mut self) {
        use seqterm_audio_engine::fx::{
            FxProcessor, DelayLine, Reverb, Bitcrusher, VinylSim,
            Isolator, Cassette, Looper, SidechainDuck, Svf, SvfMode as SvfModeAudio,
            FilterBankFx,
        };

        let chain: Vec<Box<dyn FxProcessor>> = self.master_fx.iter()
            .filter(|e| e.enabled)
            .map(|e| {
                let mut p: Box<dyn FxProcessor> = match e.kind {
                    AudioFxKind::Delay       => Box::new(DelayLine::new(250.0, 0.4, 0.3)),
                    AudioFxKind::Reverb      => Box::new(Reverb::new(48000)),
                    AudioFxKind::BitCrusher  => Box::new(Bitcrusher::new()),
                    AudioFxKind::Vinyl       => Box::new(VinylSim::new()),
                    AudioFxKind::Isolator    => Box::new(Isolator::new()),
                    AudioFxKind::Cassette    => Box::new(Cassette::new()),
                    AudioFxKind::Looper      => Box::new(Looper::new(48000)),
                    AudioFxKind::SidechainDuck => Box::new(SidechainDuck::new()),
                    AudioFxKind::Filter        => Box::new(Svf::new(SvfModeAudio::Lowpass, 2000.0, 0.7)),
                    AudioFxKind::FilterBank    => Box::new(FilterBankFx::new(48000)),
                };
                p.set_mix(e.wet);
                p
            })
            .collect();

        if let Some(ae) = self.audio_engine.as_mut() {
            ae.send(seqterm_audio_engine::AudioCommand::SetMasterFxChain { chain });
        }
    }

    /// Sync routing graph nodes from the current project state (patterns + MIDI ports).
    pub fn sync_routing_nodes(&mut self) {
        use seqterm_core::RoutingNode;

        let (pat_keys, in_ports, out_ports) = {
            let proj = self.project.lock();
            let pats = proj.patterns.keys().cloned().collect::<Vec<_>>();
            let ins  = proj.midi_inputs.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
            let outs = proj.midi_outputs.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
            (pats, ins, outs)
        };
        let mut proj = self.project.lock();
        let graph = &mut proj.routing;

        let existing: std::collections::HashSet<String> =
            graph.nodes.values().map(|n| n.label()).collect();

        let mut added = 0usize;
        for key in pat_keys {
            let n = RoutingNode::PatternOut { key };
            if !existing.contains(&n.label()) { graph.add_node(n); added += 1; }
        }
        for port in in_ports {
            let n = RoutingNode::MidiIn { port };
            if !existing.contains(&n.label()) { graph.add_node(n); added += 1; }
        }
        for port in out_ports {
            let n = RoutingNode::MidiOut { port };
            if !existing.contains(&n.label()) { graph.add_node(n); added += 1; }
        }
        drop(proj);
        if added > 0 { self.project_dirty = true; }
        self.status_msg = format!("Synced routing: {added} new node(s)");
    }

    pub fn toggle_edit_mode(&mut self) {
        match self.current_view {
            ViewKind::Tracker => {
                self.tracker_editing = !self.tracker_editing;
                if self.tracker_editing {
                    self.tracker_edit_field = self.tracker_state.cursor.1;
                    self.status_msg = format!(
                        "EDIT MODE: {} | ↑↓=adjust | Esc=exit",
                        self.tracker_state.columns.get(self.tracker_edit_field).unwrap_or(&"?")
                    );
                } else {
                    self.status_msg = "Navigate mode | Enter=edit | hjkl=move".to_string();
                }
            }
            ViewKind::Mixer => {
                self.mixer_state.editing = !self.mixer_state.editing;
                if self.mixer_state.editing {
                    self.status_msg = "EDIT: ↑↓=adjust  ←→=param  m=mute  s=solo  Esc=exit".to_string();
                } else {
                    self.status_msg = "MIXER: ←→=channel  ↑↓=volume  Enter=edit  m=mute  s=solo".to_string();
                }
            }
            ViewKind::Config => {
                self.config_state.editing = !self.config_state.editing;
            }
            ViewKind::Matrix => {
                self.navigate_matrix_to_tracker();
            }
            _ => {}
        }
    }

    /// Navigate to the pattern associated with the matrix clip at cursor.
    pub fn navigate_matrix_to_tracker(&mut self) {
        let (row, col) = self.matrix_state.cursor;
        let row_key = ((b'A' + row as u8) as char).to_string();
        let pat_key = {
            let proj = self.project.lock();
            proj.matrix
                .get(&row_key)
                .and_then(|r| r.get(col))
                .and_then(|c| c.as_ref())
                .and_then(|c| c.pattern_key.clone())
        };
        if let Some(key) = pat_key {
            // Auto-repair: if the clip references a pattern that no longer exists, create it.
            {
                let mut proj = self.project.lock();
                if !proj.patterns.contains_key(&key) {
                    let pat = seqterm_core::Pattern::new(&key, 32);
                    proj.patterns.insert(key.clone(), pat);
                }
            }
            self.tracker_state.pattern_key = Some(key.clone());
            self.tracker_state.cursor = (0, 0);
            self.tracker_scroll = 0;
            self.piano_step_scroll = 0;
            self.piano_cursor = (0, 0);
            self.engine.set_pattern(key.clone());
            self.current_view = ViewKind::Tracker;
            self.status_msg = format!("Tracker: {} | hjkl=move  Enter=edit  Esc=back", key);
        } else {
            // Empty slot → create a new pattern and open it.
            self.create_pattern_at_cursor();
        }
    }

    /// Create a new empty pattern at the current matrix cursor, assign it to the slot,
    /// and immediately open the tracker so the user can edit / rename it.
    pub fn create_pattern_at_cursor(&mut self) {
        let (row, col) = self.matrix_state.cursor;
        let row_key = ((b'A' + row as u8) as char).to_string();

        // Bail if the slot is already occupied.
        {
            let proj = self.project.lock();
            let occupied = proj.matrix
                .get(&row_key)
                .and_then(|r| r.get(col))
                .and_then(|c| c.as_ref())
                .is_some();
            if occupied { return; }
        }

        // Name = matrix position (e.g. "C05"). Fall back to sequential if that key exists.
        let new_key = {
            let proj = self.project.lock();
            let row_label = (b'A' + row as u8) as char;
            let position_key = format!("{}{:02}", row_label, col + 1);
            if !proj.patterns.contains_key(&position_key) {
                position_key
            } else {
                let mut candidate = String::new();
                for n in 1u32..=99 {
                    let k = format!("{}{:02}", row_label, n);
                    if !proj.patterns.contains_key(&k) {
                        candidate = k;
                        break;
                    }
                }
                if candidate.is_empty() {
                    format!("{}{:02}X", row_label, col + 1)
                } else {
                    candidate
                }
            }
        };

        // Insert pattern and clip.
        {
            let mut proj = self.project.lock();
            let pat = seqterm_core::Pattern::new(&new_key, 32);
            proj.patterns.insert(new_key.clone(), pat);
            if let Some(slots) = proj.matrix.get_mut(&row_key) {
                if col < slots.len() {
                    let clip = seqterm_core::Clip::new(new_key.clone(), row, col)
                        .with_pattern(new_key.clone());
                    slots[col] = Some(clip);
                }
            }
        }

        // Open tracker on the new pattern.
        self.tracker_state.pattern_key = Some(new_key.clone());
        self.tracker_state.cursor = (0, 0);
        self.tracker_scroll = 0;
        self.piano_step_scroll = 0;
        self.piano_cursor = (0, 0);
        self.engine.set_pattern(new_key.clone());
        self.current_view = ViewKind::Tracker;
        self.status_msg = format!(
            "New pattern '{}' — Tab→Generative Engine to rename",
            new_key
        );
    }

    /// Remove the clip at the current matrix cursor (unassigns the slot).
    /// The underlying pattern is kept in the project so it can be re-used.
    pub fn remove_clip_at_cursor(&mut self) {
        let (row, col) = self.matrix_state.cursor;
        let row_key = ((b'A' + row as u8) as char).to_string();
        let mut proj = self.project.lock();
        if let Some(slots) = proj.matrix.get_mut(&row_key) {
            if col < slots.len() {
                if let Some(clip) = slots[col].take() {
                    drop(proj);
                    self.status_msg = format!(
                        "Clip '{}' removed from {}{} (pattern kept)",
                        clip.name, row_key, col + 1
                    );
                    return;
                }
            }
        }
        self.status_msg = "Slot already empty".to_string();
    }

    /// Scroll the tracker so the cursor row is always visible.
    /// This must be called after moving the cursor.
    /// Adjusts `tracker_scroll` so the cursor row stays within the visible area.
    pub fn clamp_tracker_scroll(&mut self) {
        let row = self.tracker_state.cursor.0;
        let vh = self.tracker_view_height.get().max(1);
        if row < self.tracker_scroll {
            self.tracker_scroll = row;
        } else if row + 1 > self.tracker_scroll + vh {
            self.tracker_scroll = (row + 1).saturating_sub(vh);
        }
    }

    /// Adjusts `piano_step_scroll` so that `step` is always visible horizontally.
    pub fn clamp_piano_step_scroll(&mut self, step: usize) {
        let vw = self.piano_visible_steps.get().max(1);
        if step < self.piano_step_scroll {
            self.piano_step_scroll = step;
        } else if step + 1 > self.piano_step_scroll + vw {
            self.piano_step_scroll = (step + 1).saturating_sub(vw);
        }
    }

    /// Adjusts `piano_note_scroll` so that `row` is always visible vertically.
    pub fn clamp_piano_note_scroll(&mut self, row: usize) {
        let vr = self.piano_visible_rows.get().max(1);
        if row < self.piano_note_scroll {
            self.piano_note_scroll = row;
        } else if row + 1 > self.piano_note_scroll + vr {
            self.piano_note_scroll = (row + 1).saturating_sub(vr);
        }
    }

    /// Adjust the current tracker field value by `delta`.
    pub fn adjust_tracker_field(&mut self, delta: i32) {
        let (row, col) = self.tracker_state.cursor;
        let col_name = self.tracker_state.columns.get(col).copied().unwrap_or("NOTE");
        let pat_key = match &self.tracker_state.pattern_key {
            Some(k) => k.clone(),
            None => return,
        };

        // Read old note (first lock).
        let old_note = {
            let proj = self.project.lock();
            match proj.patterns.get(&pat_key).and_then(|p| p.steps.get(row)) {
                Some(n) => n.clone(),
                None => return,
            }
        };

        // Compute new note from old without touching the project.
        let mut new_note = old_note.clone();
        static NOTE_CYCLE: &[&str] = &[
            "---", "C-1", "C#1", "D-1", "D#1", "E-1", "F-1", "F#1", "G-1", "G#1",
            "A-1", "A#1", "B-1", "C-2", "C#2", "D-2", "D#2", "E-2", "F-2", "F#2",
            "G-2", "G#2", "A-2", "A#2", "B-2", "C-3", "C#3", "D-3", "D#3", "E-3",
            "F-3", "F#3", "G-3", "G#3", "A-3", "A#3", "B-3", "C-4", "C#4", "D-4",
            "D#4", "E-4", "F-4", "F#4", "G-4", "G#4", "A-4", "A#4", "B-4", "C-5",
            "C#5", "D-5", "D#5", "E-5", "F-5", "F#5", "G-5", "G#5", "A-5", "A#5",
            "B-5", "C-6",
        ];
        const FX_CMDS: &[&str] = &[
            "--", "V10", "V20", "V40", "V7F",
            "D10", "D20", "D40", "S01", "S04", "S08",
            "C01", "R01", "G01",
        ];
        match col_name {
            "VEL"  => new_note.velocity  = (old_note.velocity  as i32 + delta).clamp(0, 127)  as u8,
            "CC01" => new_note.cc01      = (old_note.cc01      as i32 + delta).clamp(0, 127)  as u8,
            "CC74" => new_note.cc74      = (old_note.cc74      as i32 + delta).clamp(0, 127)  as u8,
            "GATE" => new_note.gate      = (old_note.gate      as i32 + delta * 5).clamp(0, 200) as u16,
            "MICRO"=> new_note.micro     = (old_note.micro     as i32 + delta).clamp(-99, 99) as i8,
            "PROB" => new_note.prob      = (old_note.prob      as i32 + delta).clamp(0, 100)  as u8,
            "INS"  => new_note.instrument= (old_note.instrument as i32 + delta).clamp(0, 15)  as u8,
            "NOTE" => {
                let pos = NOTE_CYCLE.iter().position(|&n| n == old_note.note.as_str()).unwrap_or(0);
                let new_pos = (pos as i32 + delta).rem_euclid(NOTE_CYCLE.len() as i32) as usize;
                new_note.note = NOTE_CYCLE[new_pos].to_string();
                if NOTE_CYCLE[new_pos] != "---" && new_note.velocity == 0 {
                    new_note.velocity = 100;
                }
            }
            "FX1" | "FX2" => {
                let cur = if col_name == "FX1" { &old_note.fx1 } else { &old_note.fx2 };
                let pos = FX_CMDS.iter().position(|&s| s == cur.as_str()).unwrap_or(0);
                let new_pos = (pos as i32 + delta).rem_euclid(FX_CMDS.len() as i32) as usize;
                if col_name == "FX1" {
                    new_note.fx1 = FX_CMDS[new_pos].to_string();
                } else {
                    new_note.fx2 = FX_CMDS[new_pos].to_string();
                }
            }
            _ => return,
        }

        if old_note == new_note { return; }

        // Apply via history so undo/redo works (second lock).
        {
            let mut proj = self.project.lock();
            self.history.push(Box::new(seqterm_history::SetNote {
                pattern_key: pat_key,
                step: row,
                old: old_note,
                new: new_note,
            }), &mut proj);
        }
        self.project_dirty = true;
    }

    /// Rename the current pattern: re-keys the HashMap and updates every reference
    /// in the project (matrix clips, scenes, arranger blocks) plus app state.
    pub fn commit_pattern_name(&mut self, new_name: &str) {
        let new_key = new_name.trim().to_uppercase();
        if new_key.is_empty() { return; }
        let old_key = match &self.tracker_state.pattern_key {
            Some(k) => k.clone(),
            None => return,
        };
        if new_key == old_key { return; }

        let mut proj = self.project.lock();

        // Refuse if the new key is already taken by a different pattern.
        if proj.patterns.contains_key(&new_key) {
            drop(proj);
            self.status_msg = format!("Name '{}' already in use", new_key);
            return;
        }

        // 1. Move the pattern to the new HashMap key.
        if let Some(mut pat) = proj.patterns.remove(&old_key) {
            pat.name = new_key.clone();
            proj.patterns.insert(new_key.clone(), pat);
        } else {
            return;
        }

        // 2. Update every matrix clip that references the old key.
        for row_clips in proj.matrix.values_mut() {
            for slot in row_clips.iter_mut().flatten() {
                if slot.name == old_key { slot.name = new_key.clone(); }
                if slot.pattern_key.as_deref() == Some(old_key.as_str()) {
                    slot.pattern_key = Some(new_key.clone());
                }
            }
        }

        // 3. Update scene active-clip references.
        for scene in proj.scenes.iter_mut() {
            for clip_ref in scene.active_clips.iter_mut().flatten() {
                if clip_ref == &old_key { *clip_ref = new_key.clone(); }
            }
        }

        // 4. Update arranger track block labels.
        for track in proj.tracks.iter_mut() {
            for block in track.blocks.iter_mut() {
                if block.2 == old_key { block.2 = new_key.clone(); }
            }
        }

        drop(proj);

        // 5. Update app state.
        if self.tracker_state.pattern_key.as_deref() == Some(old_key.as_str()) {
            self.tracker_state.pattern_key = Some(new_key.clone());
            // Inform the engine so playback uses the new key.
            self.engine.set_pattern(new_key.clone());
        }

        self.status_msg = format!("Pattern renamed: {} → {}", old_key, new_key);
    }

    /// Adjust the current pattern's LEN by `delta`.
    pub fn adjust_pattern_len(&mut self, delta: i32) {
        let key = match &self.tracker_state.pattern_key {
            Some(k) => k.clone(),
            None => return,
        };
        let (old_len, new_len) = {
            let proj = self.project.lock();
            let old = proj.patterns.get(&key).map(|p| p.length).unwrap_or(0);
            let new = ((old as i32 + delta).clamp(1, 128)) as usize;
            (old, new)
        };
        if old_len == new_len { return; }
        {
            let mut proj = self.project.lock();
            self.history.push(Box::new(seqterm_history::SetPatternLength {
                pattern_key: key,
                old: old_len,
                new: new_len,
            }), &mut proj);
        }
        self.project_dirty = true;
    }

    /// Adjust the generative engine parameter at `generative_cursor` by `delta`.
    /// gc: 0=NAME, 1=LEN, 2=TIME_N, 3=TIME_D, 4=BEAT_GROUP,
    ///     5=SWING, 6=PROB, 7=RANDOM, 8=EUCLID_FILL, 9=EUCLID_LEN,
    ///     10=PROB_LOCK, 11=MICROSHIFT, 12=EVOLUTION, 13=HUMANIZATION.
    pub fn adjust_generative_param(&mut self, delta: i32) {
        let key = match &self.tracker_state.pattern_key {
            Some(k) => k.clone(),
            None => return,
        };
        let mut proj = self.project.lock();
        if let Some(pat) = proj.patterns.get_mut(&key) {
            match self.generative_cursor {
                0 => {} // NAME: text edit via Enter
                1 => {
                    // Step by one complete measure (time_sig_num steps) per key press.
                    let num = pat.time_sig_num.max(1) as usize;
                    let cur_measures = (pat.length / num).max(1) as i32;
                    let new_measures = (cur_measures + delta).clamp(1, 128 / num as i32) as usize;
                    let new_len = (new_measures * num).clamp(1, 128);
                    pat.length = new_len;
                    pat.steps.resize(new_len, seqterm_core::Note::default());
                }
                2 => {
                    // Numerator: free range 1-128. Auto-round length to whole measures.
                    pat.time_sig_num = ((pat.time_sig_num as i32 + delta).clamp(1, 128)) as u8;
                    pat.beat_groups = vec![];
                    let num = pat.time_sig_num.max(1) as usize;
                    let measures = ((pat.length + num - 1) / num).max(1);
                    let new_len = (measures * num).min(128);
                    pat.length = new_len;
                    pat.steps.resize(new_len, seqterm_core::Note::default());
                }
                3 => {
                    // Denominator: free range 1-128.
                    pat.time_sig_den = ((pat.time_sig_den as i32 + delta).clamp(1, 128)) as u8;
                }
                4 => {
                    // Cycle through musical groupings for current time_sig_num.
                    let options = seqterm_core::musical_groupings(pat.time_sig_num);
                    let cur = options.iter()
                        .position(|g| g.as_slice() == pat.beat_groups.as_slice())
                        .unwrap_or(0);
                    let next = (cur as i32 + delta).rem_euclid(options.len() as i32) as usize;
                    pat.beat_groups = options[next].clone();
                }
                5 => pat.swing = ((pat.swing as i32 + delta).clamp(50, 80)) as u8,
                6 => pat.prob = ((pat.prob as i32 + delta).clamp(0, 100)) as u8,
                7 => pat.random = ((pat.random as i32 + delta).clamp(0, 100)) as u8,
                8 => {
                    let max = pat.euclid_len as i32;
                    pat.euclid_fill = ((pat.euclid_fill as i32 + delta).clamp(1, max)) as usize;
                }
                9 => {
                    let new_len = ((pat.euclid_len as i32 + delta).clamp(2, 128)) as usize;
                    pat.euclid_len = new_len;
                    if pat.euclid_fill > new_len { pat.euclid_fill = new_len; }
                }
                10 => {} // PROB LOCK: toggle via Enter
                11 => pat.microshift = ((pat.microshift as i32 + delta).clamp(-99, 99)) as i8,
                12 => pat.evolution = ((pat.evolution as i32 + delta).clamp(0, 3)) as u8,
                13 => pat.humanization = ((pat.humanization as i32 + delta).clamp(0, 100)) as u8,
                _ => {}
            }
        }
    }

    /// Adjust automation param for the current step based on `modulation_cursor`.
    /// Cursor 0-7 maps to: VEL, GAIN, PAN, LP, HP, LFO, SPD, AMP.
    pub fn adjust_modulation_param(&mut self, delta: i32) {
        let step = self.tracker_state.cursor.0;
        let key = match &self.tracker_state.pattern_key {
            Some(k) => k.clone(),
            None => return,
        };
        let mut proj = self.project.lock();
        if let Some(pat) = proj.patterns.get_mut(&key) {
            if step >= pat.steps.len() { return; }
            let s = &mut pat.steps[step];
            match self.modulation_cursor {
                0 => s.velocity = (s.velocity as i32 + delta).clamp(0, 127) as u8,
                1 => s.gain    = (s.gain    as i32 + delta).clamp(0, 127) as u8,
                2 => s.pan     = (s.pan     as i32 + delta).clamp(0, 127) as u8,
                3 => s.lp      = (s.lp      as i32 + delta).clamp(0, 127) as u8,
                4 => s.hp      = (s.hp      as i32 + delta).clamp(0, 127) as u8,
                5 => s.lfo     = (s.lfo     as i32 + delta).clamp(0, 127) as u8,
                6 => s.speed   = (s.speed   as i32 + delta).clamp(0, 127) as u8,
                7 => s.amp     = (s.amp     as i32 + delta).clamp(0, 127) as u8,
                _ => {}
            }
        }
    }

    /// Toggle a note voice at `(note_row, step)` in the piano roll.
    /// If the voice is already present (primary or chord), it is removed.
    /// If absent, it is added (as primary if the step is empty, otherwise as a chord voice).
    pub fn toggle_piano_note_at(&mut self, note_row: usize, step: usize) {
        let key = match &self.tracker_state.pattern_key {
            Some(k) => k.clone(),
            None => return,
        };
        let midi = (108usize).saturating_sub(note_row) as u8;
        if midi < 21 || midi > 108 { return; }
        let mut proj = self.project.lock();
        if let Some(pat) = proj.patterns.get_mut(&key) {
            if step >= pat.steps.len() { return; }
            let slot = &mut pat.steps[step];
            let primary_midi = parse_note_name(&slot.note);
            let chord_idx = slot.chord_notes.iter()
                .position(|n| parse_note_name(n) == Some(midi));

            if primary_midi == Some(midi) {
                // Remove primary, promote first chord voice to keep the step alive.
                if let Some(promoted) = slot.chord_notes.first().cloned() {
                    let promoted_vel = slot.chord_velocities.first().copied().unwrap_or(slot.velocity);
                    slot.note = promoted;
                    slot.velocity = promoted_vel;
                    slot.chord_notes.remove(0);
                    if !slot.chord_velocities.is_empty() { slot.chord_velocities.remove(0); }
                } else {
                    *slot = seqterm_core::Note::default();
                }
            } else if let Some(idx) = chord_idx {
                slot.chord_notes.remove(idx);
                if idx < slot.chord_velocities.len() { slot.chord_velocities.remove(idx); }
            } else {
                // Add the note voice.
                if slot.is_empty() {
                    if let Ok(new_note) = seqterm_core::Note::from_midi(midi, 100) {
                        *slot = new_note;
                    }
                } else if 1 + slot.chord_notes.len() < 128 {
                    if let Ok(new_note) = seqterm_core::Note::from_midi(midi, 100) {
                        slot.chord_notes.push(new_note.note);
                        slot.chord_velocities.push(slot.velocity);
                    }
                }
            }
        }
    }

    /// Place a note voice at `(note_row, step)` without ever removing an existing voice.
    /// Used for left-click DAW behavior; right-click calls `remove_piano_note_at`.
    pub fn place_piano_note_at(&mut self, note_row: usize, step: usize) {
        let key = match &self.tracker_state.pattern_key {
            Some(k) => k.clone(),
            None => return,
        };
        let midi = (108usize).saturating_sub(note_row) as u8;
        if midi < 21 || midi > 108 { return; }
        let mut proj = self.project.lock();
        if let Some(pat) = proj.patterns.get_mut(&key) {
            if step >= pat.steps.len() { return; }
            let slot = &mut pat.steps[step];
            let already_primary = parse_note_name(&slot.note) == Some(midi);
            let already_chord = slot.chord_notes.iter().any(|n| parse_note_name(n) == Some(midi));
            if already_primary || already_chord { return; }

            if slot.is_empty() {
                if let Ok(new_note) = seqterm_core::Note::from_midi(midi, 100) {
                    *slot = new_note;
                }
            } else if 1 + slot.chord_notes.len() < 128 {
                if let Ok(new_note) = seqterm_core::Note::from_midi(midi, 100) {
                    slot.chord_notes.push(new_note.note);
                    slot.chord_velocities.push(slot.velocity);
                }
            }
        }
    }

    /// Remove the note voice at `(note_row, step)`.
    /// If it was the primary note, the first chord voice is promoted to keep the step alive.
    pub fn remove_piano_note_at(&mut self, note_row: usize, step: usize) {
        let key = match &self.tracker_state.pattern_key {
            Some(k) => k.clone(),
            None => return,
        };
        let midi = (108usize).saturating_sub(note_row) as u8;
        if midi < 21 || midi > 108 { return; }
        let mut proj = self.project.lock();
        if let Some(pat) = proj.patterns.get_mut(&key) {
            if step >= pat.steps.len() { return; }
            let slot = &mut pat.steps[step];
            if parse_note_name(&slot.note) == Some(midi) {
                if let Some(promoted) = slot.chord_notes.first().cloned() {
                    let promoted_vel = slot.chord_velocities.first().copied().unwrap_or(slot.velocity);
                    slot.note = promoted;
                    slot.velocity = promoted_vel;
                    slot.chord_notes.remove(0);
                    if !slot.chord_velocities.is_empty() { slot.chord_velocities.remove(0); }
                } else {
                    *slot = seqterm_core::Note::default();
                }
            } else if let Some(idx) = slot.chord_notes.iter()
                .position(|n| parse_note_name(n) == Some(midi))
            {
                slot.chord_notes.remove(idx);
                if idx < slot.chord_velocities.len() { slot.chord_velocities.remove(idx); }
            }
        }
    }

    /// Store a custom display name for the currently selected arranger track row.
    pub fn commit_track_name(&mut self, name: &str) {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() { return; }
        let row_key = ((b'A' + self.arranger_state.selected_track as u8) as char).to_string();
        self.project.lock().track_names.insert(row_key.clone(), trimmed.clone());
        self.status_msg = format!("Track {} → \"{}\"", row_key, trimmed);
    }

    /// Merge imported MIDI patterns, clips, tracks and automation into the project.
    pub fn apply_midi_import(&mut self, imported: seqterm_midi_io::ImportedMidi) {
        self.bpm = imported.bpm;
        self.engine.set_bpm(imported.bpm);
        let new_pattern_keys: Vec<String> = imported.patterns.keys().cloned().collect();
        {
            let mut proj = self.project.lock();
            proj.bpm = imported.bpm;
            for (k, v) in imported.patterns { proj.patterns.insert(k, v); }
            for (row_key, slots) in imported.matrix {
                let entry = proj.matrix.entry(row_key).or_insert_with(Vec::new);
                for (i, slot) in slots.into_iter().enumerate() {
                    if slot.is_some() {
                        if entry.len() <= i { entry.resize(i + 1, None); }
                        entry[i] = slot;
                    }
                }
            }
            // Merge arranger tracks (append; don't overwrite existing).
            for track in imported.tracks {
                if !proj.tracks.iter().any(|t| t.name == track.name) {
                    proj.tracks.push(track);
                }
            }
            // Merge automation lanes.
            for lane in imported.automation {
                if !proj.automation.iter().any(|l| l.target == lane.target) {
                    proj.automation.push(lane);
                }
            }
        }
        // Create virtual MIDI ports for the newly imported patterns.
        let new_ports = seqterm_midi::create_pattern_ports(&new_pattern_keys);
        if !new_ports.is_empty() {
            self.engine.add_midi_ports(new_ports);
        }
        self.ensure_matrix_size();
        self.project_dirty = true;
    }

    /// Dispatch an `AppCommand` — called from menus, keyboard shortcuts, etc.
    /// The actual logic lives in `crate::dispatch_command`.
    pub fn dispatch(&mut self, cmd: AppCommand) {
        crate::dispatch_command(self, cmd);
    }

    /// Set the gate of the note at `step` (used for drag-to-extend in piano roll).
    pub fn set_piano_note_gate(&mut self, step: usize, gate: u16) {
        let key = match &self.tracker_state.pattern_key {
            Some(k) => k.clone(),
            None => return,
        };
        let mut proj = self.project.lock();
        if let Some(pat) = proj.patterns.get_mut(&key) {
            if step < pat.steps.len() && !pat.steps[step].is_empty() {
                pat.steps[step].gate = gate.clamp(10, 400);
            }
        }
    }

    /// Handle scroll-wheel for current view context.
    pub fn handle_scroll_delta(&mut self, delta: i32) {
        match self.current_view {
            ViewKind::Tracker => {
                match self.tracker_section {
                    1 => {
                        // Piano roll: scroll note rows (88-key range, row 0=C9, row 87=A1).
                        let max = 87usize;
                        let new = self.piano_note_scroll as i32 - delta;
                        self.piano_note_scroll = new.clamp(0, max as i32) as usize;
                    }
                    2 => self.adjust_generative_param(delta),
                    3 => self.adjust_modulation_param(delta),
                    _ => {
                        if self.tracker_editing {
                            self.adjust_tracker_field(delta);
                        } else {
                            let proj = self.project.lock();
                            let max = self
                                .tracker_state
                                .pattern_key
                                .as_ref()
                                .and_then(|k| proj.patterns.get(k))
                                .map(|p| p.length)
                                .unwrap_or(0);
                            drop(proj);
                            let new = self.tracker_scroll as i32 - delta;
                            self.tracker_scroll = new.clamp(0, max.saturating_sub(1) as i32) as usize;
                        }
                    }
                }
            }
            ViewKind::Mixer => {
                let idx = self.mixer_state.selected_channel;
                let param = self.mixer_state.active_param;
                self.adjust_mixer_param(idx, param, delta);
            }
            ViewKind::Matrix => {
                if delta > 0 {
                    self.adjust_bpm(1.0);
                } else {
                    self.adjust_bpm(-1.0);
                }
            }
            ViewKind::Arranger => {
                if delta > 0 {
                    self.arranger_state.bar_offset =
                        self.arranger_state.bar_offset.saturating_sub(1);
                } else {
                    self.arranger_state.bar_offset += 1;
                }
            }
            ViewKind::Config   => {}
            ViewKind::Sampler  => {}
            ViewKind::Granular => {}
        }
    }
}
