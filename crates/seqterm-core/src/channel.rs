use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn default_stereo() -> bool { true }

/// Pan position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Pan {
    Left(u8),
    Center,
    Right(u8),
}

impl Pan {
    pub fn label(&self) -> String {
        match self {
            Pan::Left(v) => format!("L{v:02}"),
            Pan::Center => "CENTER".to_string(),
            Pan::Right(v) => format!("R{v:02}"),
        }
    }

    /// Convert to signed integer in range -50..=50.
    pub fn to_val(&self) -> i8 {
        match self {
            Pan::Left(v)  => -((*v).min(50) as i8),
            Pan::Center   => 0,
            Pan::Right(v) => (*v).min(50) as i8,
        }
    }

    /// Construct from signed integer in range -50..=50.
    pub fn from_val(v: i8) -> Self {
        match v.cmp(&0) {
            std::cmp::Ordering::Less    => Pan::Left((-v) as u8),
            std::cmp::Ordering::Equal   => Pan::Center,
            std::cmp::Ordering::Greater => Pan::Right(v as u8),
        }
    }
}

impl Default for Pan {
    fn default() -> Self {
        Pan::Center
    }
}

/// FX effect type (matches ZynFX / Carla built-in rack defaults).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum FxKind {
    #[default]
    None,
    Delay,
    Reverb,
    Distortion,
    Chorus,
    Phaser,
    Compressor,
    Equalizer,
    Saturator,
}

impl FxKind {
    pub fn label(&self) -> &'static str {
        match self {
            FxKind::None       => "None",
            FxKind::Delay      => "Delay",
            FxKind::Reverb     => "Reverb",
            FxKind::Distortion => "Distortion",
            FxKind::Chorus     => "Chorus",
            FxKind::Phaser     => "Phaser",
            FxKind::Compressor => "Compressor",
            FxKind::Equalizer  => "Equalizer",
            FxKind::Saturator  => "Saturator",
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            FxKind::None       => "───",
            FxKind::Delay      => "DLY",
            FxKind::Reverb     => "REV",
            FxKind::Distortion => "DST",
            FxKind::Chorus     => "CHR",
            FxKind::Phaser     => "PHS",
            FxKind::Compressor => "CMP",
            FxKind::Equalizer  => "EQ ",
            FxKind::Saturator  => "SAT",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            FxKind::None       => FxKind::Delay,
            FxKind::Delay      => FxKind::Reverb,
            FxKind::Reverb     => FxKind::Distortion,
            FxKind::Distortion => FxKind::Chorus,
            FxKind::Chorus     => FxKind::Phaser,
            FxKind::Phaser     => FxKind::Compressor,
            FxKind::Compressor => FxKind::Equalizer,
            FxKind::Equalizer  => FxKind::Saturator,
            FxKind::Saturator  => FxKind::None,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            FxKind::None       => FxKind::Saturator,
            FxKind::Delay      => FxKind::None,
            FxKind::Reverb     => FxKind::Delay,
            FxKind::Distortion => FxKind::Reverb,
            FxKind::Chorus     => FxKind::Distortion,
            FxKind::Phaser     => FxKind::Chorus,
            FxKind::Compressor => FxKind::Phaser,
            FxKind::Equalizer  => FxKind::Compressor,
            FxKind::Saturator  => FxKind::Equalizer,
        }
    }

    /// 8 parameter names for this effect type (ZynFX/Carla built-in defaults).
    pub fn param_labels(&self) -> [&'static str; 8] {
        match self {
            FxKind::None       => ["─────────"; 8],
            FxKind::Delay      => ["Dry/Wet", "Pan", "Delay", "L/R Delay", "L/R Cross", "Feedback", "Hi Pass", "─────────"],
            FxKind::Reverb     => ["Dry/Wet", "Pan", "Room Sz", "Rev Time", "Init Dly", "LR Cross", "Lo Pass", "Hi Pass"],
            FxKind::Distortion => ["Dry/Wet", "Pan", "Drive", "Level", "Type", "Lo Pass", "Hi Pass", "Stereo"],
            FxKind::Chorus     => ["Dry/Wet", "Pan", "Freq", "Depth", "Feedback", "Delay", "LR Cross", "Phase"],
            FxKind::Phaser     => ["Dry/Wet", "Pan", "Freq", "Depth", "Feedback", "Phase", "Stages", "LR Cross"],
            FxKind::Compressor => ["Dry/Wet", "Threshold", "Ratio", "Attack", "Release", "Makeup", "Knee", "─────────"],
            FxKind::Equalizer  => ["Low", "Low-Mid", "High-Mid", "High", "LF Freq", "HF Freq", "─────────", "─────────"],
            FxKind::Saturator  => ["Drive", "Tone", "Level", "Mix", "─────────", "─────────", "─────────", "─────────"],
        }
    }
}

fn default_midi_ch() -> u8 { 1 }

/// An FX slot with effect type, 8 CC-mapped parameters, and MIDI routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxSlot {
    pub enabled: bool,
    pub kind: FxKind,
    /// MIDI output port for CC routing (empty = unassigned).
    #[serde(default)]
    pub midi_port: String,
    /// MIDI channel 1-16.
    #[serde(default = "default_midi_ch")]
    pub midi_channel: u8,
    /// CC numbers for each of the 8 parameters (0 = unassigned).
    #[serde(default)]
    pub cc_nums: [u8; 8],
    /// Current values 0-127 for each of the 8 parameters.
    #[serde(default)]
    pub cc_vals: [u8; 8],
}

impl Default for FxSlot {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: FxKind::None,
            midi_port: String::new(),
            midi_channel: 1,
            cc_nums: [0; 8],
            cc_vals: [64, 64, 64, 64, 0, 0, 0, 0],
        }
    }
}

/// Where an FX parameter change is sent: MIDI CC or OSC message.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum FxRouteKind {
    #[default]
    None,
    Midi,
    Osc,
}

impl FxRouteKind {
    pub fn label(&self) -> &'static str {
        match self {
            FxRouteKind::None => "NONE",
            FxRouteKind::Midi => "MIDI",
            FxRouteKind::Osc  => "OSC",
        }
    }
    pub fn next(&self) -> Self {
        match self { FxRouteKind::None => FxRouteKind::Midi, FxRouteKind::Midi => FxRouteKind::Osc, FxRouteKind::Osc => FxRouteKind::None }
    }
    pub fn prev(&self) -> Self {
        match self { FxRouteKind::None => FxRouteKind::Osc, FxRouteKind::Midi => FxRouteKind::None, FxRouteKind::Osc => FxRouteKind::Midi }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxRoute {
    pub kind: FxRouteKind,
    pub midi_port: String,
    pub midi_channel: u8,
    pub midi_cc: u8,
    pub osc_address: String,
    pub osc_port: u16,
}

impl Default for FxRoute {
    fn default() -> Self {
        Self {
            kind: FxRouteKind::None,
            midi_port: String::new(),
            midi_channel: 1,
            midi_cc: 91,
            osc_address: "/fx/1".to_string(),
            osc_port: 9000,
        }
    }
}

/// A mixer channel strip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub name: String,
    /// Volume in dBFS (typically -60.0 to +6.0).
    pub volume: f32,
    pub pan: Pan,
    pub mute: bool,
    pub solo: bool,
    /// Send level to bus A (0-127).
    pub send_a: u8,
    /// Send level to bus B (0-127).
    pub send_b: u8,
    pub fx: [FxSlot; 3],
    /// MIDI channel assignment (1-16, 0 = none).
    pub midi_channel: u8,
    /// MIDI output port name.
    pub midi_port: Option<String>,
    /// When true this channel's output is used as a sidechain source in the routing graph.
    #[serde(default)]
    pub sidechain_source: bool,
    /// True = this destination has stereo audio output → show L+R strip pair.
    /// Defaults to true (most hardware/software synths are stereo).
    #[serde(default = "default_stereo")]
    pub stereo: bool,
    /// EQ low-shelf gain in dB (-12..+12).
    #[serde(default)]
    pub eq_low: i8,
    /// EQ low-mid peak gain in dB (-12..+12).
    #[serde(default)]
    pub eq_low_mid: i8,
    /// EQ high-mid peak gain in dB (-12..+12).
    #[serde(default)]
    pub eq_high_mid: i8,
    /// EQ high-shelf gain in dB (-12..+12).
    #[serde(default)]
    pub eq_high: i8,
    /// FX send/insert amount (0-127).
    #[serde(default)]
    pub fx_amount: u8,
    /// Where fx_amount changes are routed (MIDI CC or OSC).
    #[serde(default)]
    pub fx_route: FxRoute,
    /// SF2 SoundFont file assigned to this channel (overrides per-clip SF2 when set).
    #[serde(default)]
    pub sf2_path: Option<PathBuf>,
    /// SF2 bank number (0-127).
    #[serde(default)]
    pub sf2_bank: u8,
    /// SF2 preset / program number (0-127).
    #[serde(default)]
    pub sf2_preset: u8,
    /// Cached preset name for display (not authoritative, refreshed on load).
    #[serde(default)]
    pub sf2_preset_name: String,
}

impl Channel {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            volume: -6.0,
            pan: Pan::Center,
            mute: false,
            solo: false,
            send_a: 0,
            send_b: 0,
            fx: [
                FxSlot::default(),
                FxSlot::default(),
                FxSlot::default(),
            ],
            midi_channel: 1,
            midi_port: None,
            sidechain_source: false,
            stereo: true,
            eq_low: 0,
            eq_low_mid: 0,
            eq_high_mid: 0,
            eq_high: 0,
            fx_amount: 0,
            fx_route: FxRoute::default(),
            sf2_path: None,
            sf2_bank: 0,
            sf2_preset: 0,
            sf2_preset_name: String::new(),
        }
    }

    pub fn with_fx(mut self, slot: usize, kind: FxKind, enabled: bool) -> Self {
        if slot < 3 {
            self.fx[slot] = FxSlot { enabled, kind, ..FxSlot::default() };
        }
        self
    }

    /// Return volume as a 0.0-1.0 amplitude ratio (linear).
    pub fn amplitude(&self) -> f32 {
        if self.mute {
            return 0.0;
        }
        10f32.powf(self.volume / 20.0)
    }

    /// Volume as 0-100 UI percentage for display.
    pub fn volume_pct(&self) -> u8 {
        // Map -60..+6 dB to 0..100%
        ((self.volume + 60.0) / 66.0 * 100.0).clamp(0.0, 100.0) as u8
    }
}

impl Default for Channel {
    fn default() -> Self {
        Self::new("CH")
    }
}
