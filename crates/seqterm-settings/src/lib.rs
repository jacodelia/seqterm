use std::{fs, path::{Path, PathBuf}};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ─── MIDI Learn ───────────────────────────────────────────────────────────────

/// What parameter a MIDI CC is mapped to during MIDI Learn.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum MidiLearnTarget {
    ChannelVolume(usize),
    ChannelPan(usize),
    ChannelSendA(usize),
    ChannelSendB(usize),
    Bpm,
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
}

impl MidiLearnBinding {
    pub fn new(target: MidiLearnTarget, midi_ch: u8, cc: u8) -> Self {
        Self { target, midi_ch, cc }
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
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            audio: AudioSettings::default(),
            project_versioning: false,
            keybindings: default_keybindings(),
            midi_learn_bindings: Vec::new(),
            last_sf2_path: None,
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
