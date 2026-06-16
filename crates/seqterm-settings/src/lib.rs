use std::{fs, path::{Path, PathBuf}};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ─── MIDI Learn ───────────────────────────────────────────────────────────────

/// What parameter a MIDI CC is mapped to during MIDI Learn.
///
/// Universal: covers mixer channel strips, transport, the master-bus FX rack and
/// the EDITOR (granular/SF2) parameter cursor. A single CC may be bound to
/// several of these in different views — see [`MidiLearnBinding::view`].
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum MidiLearnTarget {
    ChannelVolume(usize),
    ChannelPan(usize),
    ChannelSendA(usize),
    ChannelSendB(usize),
    Bpm,
    /// A parameter on the master-bus FX rack: `(entry index, param index)`.
    MasterFxParam { entry: usize, param: usize },
    /// A parameter on a per-slot FX insert for the focused slot: `(entry, param)`.
    SlotFxParam { entry: usize, param: usize },
    /// A parameter in the EDITOR view, indexed by the editor parameter cursor.
    EditorParam(usize),
    Custom(String),
}

impl MidiLearnTarget {
    pub fn label(&self) -> String {
        match self {
            Self::ChannelVolume(i) => format!("CH{:02} Volume", i + 1),
            Self::ChannelPan(i)    => format!("CH{:02} Pan",    i + 1),
            Self::ChannelSendA(i)  => format!("CH{:02} Send A", i + 1),
            Self::ChannelSendB(i)  => format!("CH{:02} Send B", i + 1),
            Self::Bpm              => "BPM".to_string(),
            Self::MasterFxParam { entry, param } => format!("Master FX{} P{}", entry + 1, param + 1),
            Self::SlotFxParam { entry, param }   => format!("Slot FX{} P{}", entry + 1, param + 1),
            Self::EditorParam(i)   => format!("EDITOR P{}", i + 1),
            Self::Custom(s)        => s.clone(),
        }
    }
}

/// A saved MIDI CC → parameter mapping produced by MIDI Learn.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MidiLearnBinding {
    pub target: MidiLearnTarget,
    /// MIDI channel (0-based, 0-15).
    pub midi_ch: u8,
    /// CC number (0-127).
    pub cc: u8,
    /// View this binding belongs to, as a [`ViewKind`] index (see seqterm-ui).
    /// `None` = global (applies in any view). When an incoming CC matches both a
    /// binding for the current view and a global one, the current view wins — a
    /// knob can thus be reused per-window with the focused view taking priority.
    #[serde(default)]
    pub view: Option<u8>,
}

impl MidiLearnBinding {
    /// A global binding (active in any view).
    pub fn new(target: MidiLearnTarget, midi_ch: u8, cc: u8) -> Self {
        Self { target, midi_ch, cc, view: None }
    }

    /// A binding scoped to a specific view (by `ViewKind` index).
    pub fn with_view(target: MidiLearnTarget, midi_ch: u8, cc: u8, view: u8) -> Self {
        Self { target, midi_ch, cc, view: Some(view) }
    }
}

/// Resolve which targets an incoming CC drives, with **view priority**: among
/// the bindings matching `(midi_ch, cc)`, those scoped to `current_view` win; if
/// none match the current view, the global (view-less) bindings apply. Bindings
/// scoped to a *different* view stay dormant. This lets a single knob drive
/// different parameters in different windows.
pub fn resolve_midi_targets(
    bindings: &[MidiLearnBinding],
    midi_ch: u8,
    cc: u8,
    current_view: u8,
) -> Vec<&MidiLearnTarget> {
    let matches = bindings.iter().filter(|b| b.cc == cc && b.midi_ch == midi_ch);
    let view_specific: Vec<&MidiLearnTarget> = matches
        .clone()
        .filter(|b| b.view == Some(current_view))
        .map(|b| &b.target)
        .collect();
    if !view_specific.is_empty() {
        view_specific
    } else {
        matches.filter(|b| b.view.is_none()).map(|b| &b.target).collect()
    }
}

#[cfg(test)]
mod midi_learn_tests {
    use super::*;

    #[test]
    fn view_binding_shadows_global() {
        let bindings = vec![
            MidiLearnBinding::new(MidiLearnTarget::Bpm, 0, 7),
            MidiLearnBinding::with_view(MidiLearnTarget::EditorParam(2), 0, 7, 5),
        ];
        // In view 5, the editor binding wins (global shadowed).
        let t = resolve_midi_targets(&bindings, 0, 7, 5);
        assert_eq!(t, vec![&MidiLearnTarget::EditorParam(2)]);
        // In view 3 (no view-specific match), the global binding applies.
        let t = resolve_midi_targets(&bindings, 0, 7, 3);
        assert_eq!(t, vec![&MidiLearnTarget::Bpm]);
    }

    #[test]
    fn other_view_binding_is_dormant() {
        // Only a binding for view 4 exists; while in view 2 nothing fires.
        let bindings = vec![
            MidiLearnBinding::with_view(MidiLearnTarget::ChannelVolume(0), 0, 10, 4),
        ];
        assert!(resolve_midi_targets(&bindings, 0, 10, 2).is_empty());
        assert_eq!(resolve_midi_targets(&bindings, 0, 10, 4),
                   vec![&MidiLearnTarget::ChannelVolume(0)]);
    }

    #[test]
    fn legacy_binding_without_view_defaults_global() {
        // Deserializing a pre-`view` binding yields view = None (global).
        let json = r#"{"target":"Bpm","midi_ch":0,"cc":1}"#;
        let b: MidiLearnBinding = serde_json::from_str(json).unwrap();
        assert_eq!(b.view, None);
        assert_eq!(resolve_midi_targets(std::slice::from_ref(&b), 0, 1, 2),
                   vec![&MidiLearnTarget::Bpm]);
    }
}

// ─── Keybindings ─────────────────────────────────────────────────────────────

/// A single configurable key binding.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    pub action:    String,
    pub key:       String,
    pub modifiers: String,
    #[serde(default)]
    pub group:     String,
}

impl KeyBinding {
    pub fn new(action: &str, key: &str, modifiers: &str, group: &str) -> Self {
        Self {
            action:    action.to_string(),
            key:       key.to_string(),
            modifiers: modifiers.to_string(),
            group:     group.to_string(),
        }
    }

    /// Display form: "Ctrl+N", "F1", "Alt+H", etc.
    pub fn display(&self) -> String {
        if self.modifiers.is_empty() {
            self.key.clone()
        } else {
            let mods = self.modifiers.split('+')
                .map(|m| {
                    let mut s = m.to_string();
                    if let Some(c) = s.get_mut(0..1) { c.make_ascii_uppercase(); }
                    s
                })
                .collect::<Vec<_>>()
                .join("+");
            format!("{mods}+{}", self.key)
        }
    }
}

pub fn default_keybindings() -> Vec<KeyBinding> {
    vec![
        // ── File ─────────────────────────────────────────────────────────────
        KeyBinding::new("NewProject",        "n",   "ctrl",       "File"),
        KeyBinding::new("OpenProject",       "o",   "ctrl",       "File"),
        KeyBinding::new("SaveProject",       "s",   "ctrl",       "File"),
        KeyBinding::new("SaveProjectAs",     "s",   "ctrl+shift", "File"),
        KeyBinding::new("ImportMidi",        "i",   "ctrl",       "File"),
        KeyBinding::new("ExportMidi",        "e",   "ctrl",       "File"),
        KeyBinding::new("Exit",              "q",   "ctrl",       "File"),
        // ── Edit ─────────────────────────────────────────────────────────────
        KeyBinding::new("Undo",              "z",   "ctrl",       "Edit"),
        KeyBinding::new("Redo",              "y",   "ctrl",       "Edit"),
        KeyBinding::new("ShowRoutingConfig", "6",   "",           "Edit"),
        // ── Views ─────────────────────────────────────────────────────────────
        KeyBinding::new("ShowCommandPalette","p",   "ctrl",       "View"),
        KeyBinding::new("ShowKeybindings",   "F1",  "",           "View"),
        KeyBinding::new("ShowAbout",         "F12", "",           "View"),
        // ── Transport ─────────────────────────────────────────────────────────
        KeyBinding::new("PlayStop",          " ",   "",           "Transport"),
        KeyBinding::new("Stop",              "s",   "",           "Transport"),
        KeyBinding::new("Record",            "r",   "",           "Transport"),
        // ── Matrix ────────────────────────────────────────────────────────────
        KeyBinding::new("MatrixView",        "1",   "",           "Matrix"),
        KeyBinding::new("TrackerView",       "2",   "",           "Matrix"),
        KeyBinding::new("ArrangerView",      "3",   "",           "Matrix"),
        KeyBinding::new("MixerView",         "4",   "",           "Matrix"),
        KeyBinding::new("ConfigView",        "5",   "",           "Matrix"),
    ]
}

// ─── Audio settings ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AudioSettings {
    pub backend: String,
    pub device: String,
    pub sample_rate: u32,
    pub buffer_size: u32,
    #[serde(default)]
    pub alsa_hw_device: String,
    #[serde(default)]
    pub jack_server_name: String,
    #[serde(default)]
    pub pipewire_quantum: u32,
    #[serde(default)]
    pub wasapi_exclusive: bool,
    /// SF2 sample engine: "fluidsynth" (embedded FluidLite, default — on in the
    /// default build, zero external deps) or "oxisynth" (pure-Rust fallback).
    #[serde(default = "default_sf2_backend")]
    pub sf2_backend: String,
}

fn default_sf2_backend() -> String {
    // Prefer the embedded FluidLite engine when the build includes it (it is on
    // by default); SoundFontSynth gracefully falls back to oxisynth otherwise.
    "fluidsynth".to_string()
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            backend: "AUTO".to_string(),
            device: "default".to_string(),
            sample_rate: 48000,
            buffer_size: 256,
            alsa_hw_device: String::new(),
            jack_server_name: String::new(),
            pipewire_quantum: 0,
            wasapi_exclusive: false,
            sf2_backend: default_sf2_backend(),
        }
    }
}

// ─── Plugin search paths (Carla-style) ─────────────────────────────────────────

/// Per-format plugin search directories, mirroring Carla's layout. Each format
/// keeps its own list of directories; the FX picker scans the union of these to
/// discover installable plugins. Defaults are the platform-conventional system
/// locations; users add/remove directories in AUDIO SETTINGS → Plugin Paths.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PluginPaths {
    #[serde(default)] pub ladspa: Vec<PathBuf>,
    #[serde(default)] pub dssi:   Vec<PathBuf>,
    #[serde(default)] pub lv2:    Vec<PathBuf>,
    #[serde(default)] pub vst2:   Vec<PathBuf>,
    #[serde(default)] pub vst3:   Vec<PathBuf>,
    #[serde(default)] pub clap:   Vec<PathBuf>,
    #[serde(default)] pub sf2:    Vec<PathBuf>,
    #[serde(default)] pub sfz:    Vec<PathBuf>,
    #[serde(default)] pub jsfx:   Vec<PathBuf>,
}

/// The nine plugin-path formats, in display order, with a stable string key.
pub const PLUGIN_PATH_FORMATS: [&str; 9] =
    ["LADSPA", "DSSI", "LV2", "VST2", "VST3", "CLAP", "SF2", "SFZ", "JSFX"];

impl PluginPaths {
    /// Borrow the directory list for a format key (see [`PLUGIN_PATH_FORMATS`]).
    pub fn list(&self, format: &str) -> &Vec<PathBuf> {
        match format {
            "LADSPA" => &self.ladspa,
            "DSSI"   => &self.dssi,
            "LV2"    => &self.lv2,
            "VST2"   => &self.vst2,
            "VST3"   => &self.vst3,
            "CLAP"   => &self.clap,
            "SF2"    => &self.sf2,
            "SFZ"    => &self.sfz,
            _        => &self.jsfx,
        }
    }

    /// Mutably borrow the directory list for a format key.
    pub fn list_mut(&mut self, format: &str) -> &mut Vec<PathBuf> {
        match format {
            "LADSPA" => &mut self.ladspa,
            "DSSI"   => &mut self.dssi,
            "LV2"    => &mut self.lv2,
            "VST2"   => &mut self.vst2,
            "VST3"   => &mut self.vst3,
            "CLAP"   => &mut self.clap,
            "SF2"    => &mut self.sf2,
            "SFZ"    => &mut self.sfz,
            _        => &mut self.jsfx,
        }
    }

    /// Every configured directory across all formats (deduplicated, for scanning).
    pub fn all_dirs(&self) -> Vec<PathBuf> {
        let mut all: Vec<PathBuf> = Vec::new();
        for f in PLUGIN_PATH_FORMATS {
            for d in self.list(f) {
                if !all.contains(d) { all.push(d.clone()); }
            }
        }
        all
    }
}

fn home() -> Option<PathBuf> { std::env::var_os("HOME").map(PathBuf::from) }

/// Platform-default directories for a format key, Carla-style.
fn default_paths_for(format: &str) -> Vec<PathBuf> {
    let h = home();
    let mut p = Vec::new();
    macro_rules! hj { ($s:expr) => { if let Some(hp) = &h { p.push(hp.join($s)); } }; }

    #[cfg(all(unix, not(target_os = "macos")))]
    match format {
        "LADSPA" => { hj!(".ladspa"); p.push("/usr/lib/ladspa".into()); p.push("/usr/local/lib/ladspa".into()); }
        "DSSI"   => { hj!(".dssi");   p.push("/usr/lib/dssi".into());   p.push("/usr/local/lib/dssi".into()); }
        "LV2"    => { hj!(".lv2");    p.push("/usr/lib/lv2".into());    p.push("/usr/local/lib/lv2".into()); }
        "VST2"   => { hj!(".vst");    p.push("/usr/lib/vst".into());    p.push("/usr/local/lib/vst".into()); }
        "VST3"   => { hj!(".vst3");   p.push("/usr/lib/vst3".into());   p.push("/usr/local/lib/vst3".into()); }
        "CLAP"   => { hj!(".clap");   p.push("/usr/lib/clap".into());   p.push("/usr/local/lib/clap".into()); }
        "SF2"    => { hj!(".sounds/sf2"); p.push("/usr/share/sounds/sf2".into()); p.push("/usr/share/soundfonts".into()); }
        "SFZ"    => { hj!(".sfz");    p.push("/usr/share/sounds/sfz".into()); }
        "JSFX"   => { hj!(".config/REAPER/Effects"); }
        _ => {}
    }
    #[cfg(target_os = "macos")]
    match format {
        "LADSPA" => { hj!("Library/Audio/Plug-Ins/LADSPA"); p.push("/Library/Audio/Plug-Ins/LADSPA".into()); }
        "DSSI"   => { hj!("Library/Audio/Plug-Ins/DSSI");   p.push("/Library/Audio/Plug-Ins/DSSI".into()); }
        "LV2"    => { hj!("Library/Audio/Plug-Ins/LV2");    p.push("/Library/Audio/Plug-Ins/LV2".into()); }
        "VST2"   => { hj!("Library/Audio/Plug-Ins/VST");    p.push("/Library/Audio/Plug-Ins/VST".into()); }
        "VST3"   => { hj!("Library/Audio/Plug-Ins/VST3");   p.push("/Library/Audio/Plug-Ins/VST3".into()); }
        "CLAP"   => { hj!("Library/Audio/Plug-Ins/CLAP");   p.push("/Library/Audio/Plug-Ins/CLAP".into()); }
        "SF2"    => { hj!("Library/Audio/Sounds/Banks"); }
        "SFZ"    => { hj!("Library/Audio/Sounds/SFZ"); }
        "JSFX"   => { hj!("Library/Application Support/REAPER/Effects"); }
        _ => {}
    }
    #[cfg(target_os = "windows")]
    {
        let cpf = std::env::var("COMMONPROGRAMFILES").ok().map(PathBuf::from);
        let appdata = std::env::var("APPDATA").ok().map(PathBuf::from);
        match format {
            "VST3"   => { if let Some(c) = &cpf { p.push(c.join("VST3")); } }
            "CLAP"   => { if let Some(c) = &cpf { p.push(c.join("CLAP")); } }
            "LV2"    => { if let Some(a) = &appdata { p.push(a.join("LV2")); } if let Some(c) = &cpf { p.push(c.join("LV2")); } }
            "JSFX"   => { if let Some(a) = &appdata { p.push(a.join("REAPER\\Effects")); } }
            other    => { if let Some(c) = &cpf { p.push(c.join(other)); } }
        }
    }
    p
}

impl Default for PluginPaths {
    fn default() -> Self {
        Self {
            ladspa: default_paths_for("LADSPA"),
            dssi:   default_paths_for("DSSI"),
            lv2:    default_paths_for("LV2"),
            vst2:   default_paths_for("VST2"),
            vst3:   default_paths_for("VST3"),
            clap:   default_paths_for("CLAP"),
            sf2:    default_paths_for("SF2"),
            sfz:    default_paths_for("SFZ"),
            jsfx:   default_paths_for("JSFX"),
        }
    }
}

// ─── OSC settings (Carla-style) ────────────────────────────────────────────────

/// Whether OSC ports are fixed or chosen randomly at startup.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OscPortMode {
    /// Bind to a random free port.
    Random,
    /// Bind to the configured `udp_port` / `tcp_port`.
    Specific,
}

/// OSC server configuration, mirroring Carla's OSC settings panel.
///
/// Note: SeqTerm's OSC server is currently UDP-only — `udp_port` is wired to the
/// live server, while `tcp_port` is persisted for parity but not yet used.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OscSettings {
    pub enabled:   bool,
    pub port_mode: OscPortMode,
    pub udp_port:  u16,
    pub tcp_port:  u16,
}

impl Default for OscSettings {
    fn default() -> Self {
        Self {
            enabled:   false,
            port_mode: OscPortMode::Specific,
            udp_port:  57120,
            tcp_port:  0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub audio: AudioSettings,
    #[serde(default)]
    pub project_versioning: bool,
    #[serde(default = "default_keybindings")]
    pub keybindings: Vec<KeyBinding>,
    #[serde(default)]
    pub midi_learn_bindings: Vec<MidiLearnBinding>,
    /// Last SF2 file used in a MIDI import — pre-filled in the import dialog.
    #[serde(default)]
    pub last_sf2_path: Option<std::path::PathBuf>,
    #[serde(default)]
    pub plugin_paths: PluginPaths,
    #[serde(default)]
    pub osc: OscSettings,
    /// Maximum retained undo steps for the session history.
    #[serde(default = "default_max_undo_steps")]
    pub max_undo_steps: usize,
    /// Favourite PATTERN-view tab (0=SOURCE, 1=MODULATION, 2=FX, 3=SETTINGS) shown
    /// first when the PATTERN view is opened. Defaults to SOURCE.
    #[serde(default)]
    pub pattern_fav_tab: usize,
}

fn default_max_undo_steps() -> usize { 1000 }

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            audio: AudioSettings::default(),
            project_versioning: false,
            keybindings: default_keybindings(),
            midi_learn_bindings: Vec::new(),
            last_sf2_path: None,
            plugin_paths: PluginPaths::default(),
            osc: OscSettings::default(),
            max_undo_steps: default_max_undo_steps(),
            pattern_fav_tab: 0,
        }
    }
}

// ─── Keybindings I/O ─────────────────────────────────────────────────────────

pub fn export_keybindings(bindings: &[KeyBinding], path: &Path) -> Result<()> {
    #[derive(Serialize)]
    struct Doc<'a> { keybindings: &'a [KeyBinding] }
    let text = toml::to_string_pretty(&Doc { keybindings: bindings })
        .context("failed to serialize keybindings to TOML")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

pub fn import_keybindings(path: &Path) -> Result<Vec<KeyBinding>> {
    #[derive(Deserialize)]
    struct Doc { keybindings: Vec<KeyBinding> }
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let doc: Doc = toml::from_str(&text)
        .with_context(|| format!("parsing keybindings TOML from {}", path.display()))?;
    Ok(doc.keybindings)
}

// ─── App settings I/O ────────────────────────────────────────────────────────

fn config_dir() -> PathBuf {
    dirs_home().join(".config").join("seqterm")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn load_settings() -> AppSettings {
    let p = settings_path();
    if !p.exists() { return AppSettings::default(); }
    fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &AppSettings) -> Result<()> {
    let p = settings_path();
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(s)?;
    fs::write(&p, json)?;
    Ok(())
}
