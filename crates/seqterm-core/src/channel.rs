use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn default_stereo() -> bool { true }
fn default_width() -> f32 { 1.0 }
fn default_channel_type() -> ChannelType { ChannelType::Audio }

/// Default GM drum map: 16 pads → standard GM percussion notes.
/// Pad 0=Kick, 1=Snare, 2=ClosedHH, 3=OpenHH, 4-7=Toms, 8=Clap, 9=Crash, 10-15=Misc.
pub const GM_DRUM_MAP: [u8; 16] = [
    36, // Kick (Bass Drum 1)
    38, // Snare (Acoustic Snare)
    42, // Closed Hi-Hat
    46, // Open Hi-Hat
    43, // High Floor Tom
    45, // Low Tom
    47, // Low-Mid Tom
    50, // High Tom
    39, // Hand Clap
    49, // Crash Cymbal 1
    51, // Ride Cymbal 1
    55, // Splash Cymbal
    41, // Low Floor Tom
    37, // Side Stick
    53, // Ride Bell
    56, // Cowbell
];

fn default_drum_map() -> [u8; 16] { GM_DRUM_MAP }

/// Mixer channel type — determines signal flow and routing options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum ChannelType {
    /// Standard audio track channel.
    #[default]
    Audio,
    /// Instrument channel (routed from an SF2 synth or plugin).
    Instrument,
    /// Group bus — receives sends from multiple channels.
    GroupBus,
    /// Return track — receives effects bus send.
    Return,
    /// Master output channel.
    Master,
}

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

/// FX effect type — covers all 24 audio-engine processors + utility FX.
/// Used in `FxSlot` for MIDI CC routing to external or internal processors.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum FxKind {
    #[default]
    None,
    // ── Time-based ──────────────────────────────────
    Delay,
    Reverb,
    Chorus,
    Flanger,
    Phaser,
    GranularDelay,
    Looper,
    // ── Dynamics ────────────────────────────────────
    Compressor,
    Limiter,
    Gate,
    Expander,
    SidechainDuck,
    // ── Equalisation ────────────────────────────────
    Equalizer,
    ParametricEq,
    Isolator,
    FilterBank,
    // ── Distortion / Colour ──────────────────────────
    Distortion,
    Bitcrusher,
    SoftClipper,
    Saturator,
    TubeSaturation,
    Cassette,
    VinylSim,
    // ── Utility ─────────────────────────────────────
    Gain,
    Pan,
    Widener,
    PhaseInvert,
    MonoMaker,
}

impl FxKind {
    pub fn label(&self) -> &'static str {
        match self {
            FxKind::None          => "None",
            FxKind::Delay         => "Delay",
            FxKind::Reverb        => "Reverb",
            FxKind::Chorus        => "Chorus",
            FxKind::Flanger       => "Flanger",
            FxKind::Phaser        => "Phaser",
            FxKind::GranularDelay => "Granular Delay",
            FxKind::Looper        => "Looper",
            FxKind::Compressor    => "Compressor",
            FxKind::Limiter       => "Limiter",
            FxKind::Gate          => "Gate",
            FxKind::Expander      => "Expander",
            FxKind::SidechainDuck => "Sidechain Duck",
            FxKind::Equalizer     => "Equalizer",
            FxKind::ParametricEq  => "Parametric EQ",
            FxKind::Isolator      => "Isolator",
            FxKind::FilterBank    => "Filter Bank",
            FxKind::Distortion    => "Distortion",
            FxKind::Bitcrusher    => "Bitcrusher",
            FxKind::SoftClipper   => "Soft Clipper",
            FxKind::Saturator     => "Saturator",
            FxKind::TubeSaturation=> "Tube Saturation",
            FxKind::Cassette      => "Cassette",
            FxKind::VinylSim      => "Vinyl Sim",
            FxKind::Gain          => "Gain",
            FxKind::Pan           => "Pan",
            FxKind::Widener       => "Widener",
            FxKind::PhaseInvert   => "Phase Invert",
            FxKind::MonoMaker     => "Mono Maker",
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            FxKind::None          => "───",
            FxKind::Delay         => "DLY",
            FxKind::Reverb        => "REV",
            FxKind::Chorus        => "CHR",
            FxKind::Flanger       => "FLG",
            FxKind::Phaser        => "PHS",
            FxKind::GranularDelay => "GDL",
            FxKind::Looper        => "LPR",
            FxKind::Compressor    => "CMP",
            FxKind::Limiter       => "LIM",
            FxKind::Gate          => "GAT",
            FxKind::Expander      => "EXP",
            FxKind::SidechainDuck => "SDK",
            FxKind::Equalizer     => "EQ ",
            FxKind::ParametricEq  => "PEQ",
            FxKind::Isolator      => "ISO",
            FxKind::FilterBank    => "FBK",
            FxKind::Distortion    => "DST",
            FxKind::Bitcrusher    => "BIT",
            FxKind::SoftClipper   => "SCL",
            FxKind::Saturator     => "SAT",
            FxKind::TubeSaturation=> "TBE",
            FxKind::Cassette      => "CST",
            FxKind::VinylSim      => "VNL",
            FxKind::Gain          => "GAN",
            FxKind::Pan           => "PAN",
            FxKind::Widener       => "WID",
            FxKind::PhaseInvert   => "PHI",
            FxKind::MonoMaker     => "MNO",
        }
    }

    /// Ordered list for cycling (next/prev).
    const ORDER: &'static [FxKind] = &[
        FxKind::None, FxKind::Delay, FxKind::Reverb, FxKind::Chorus, FxKind::Flanger,
        FxKind::Phaser, FxKind::GranularDelay, FxKind::Looper,
        FxKind::Compressor, FxKind::Limiter, FxKind::Gate, FxKind::Expander, FxKind::SidechainDuck,
        FxKind::Equalizer, FxKind::ParametricEq, FxKind::Isolator, FxKind::FilterBank,
        FxKind::Distortion, FxKind::Bitcrusher, FxKind::SoftClipper, FxKind::Saturator,
        FxKind::TubeSaturation, FxKind::Cassette, FxKind::VinylSim,
        FxKind::Gain, FxKind::Pan, FxKind::Widener, FxKind::PhaseInvert, FxKind::MonoMaker,
    ];

    pub fn next(&self) -> Self {
        let pos = Self::ORDER.iter().position(|k| k == self).unwrap_or(0);
        Self::ORDER[(pos + 1) % Self::ORDER.len()].clone()
    }

    pub fn prev(&self) -> Self {
        let pos = Self::ORDER.iter().position(|k| k == self).unwrap_or(0);
        Self::ORDER[(pos + Self::ORDER.len() - 1) % Self::ORDER.len()].clone()
    }

    /// 8 parameter names for MIDI CC routing labels.
    pub fn param_labels(&self) -> [&'static str; 8] {
        match self {
            FxKind::None          => ["─────────"; 8],
            FxKind::Delay         => ["Dry/Wet", "Pan", "Delay", "L/R Delay", "L/R Cross", "Feedback", "Hi Pass", "─────────"],
            FxKind::Reverb        => ["Dry/Wet", "Pan", "Room Sz", "Rev Time", "Init Dly", "LR Cross", "Lo Pass", "Hi Pass"],
            FxKind::Chorus        => ["Dry/Wet", "Pan", "Freq", "Depth", "Feedback", "Delay", "LR Cross", "Phase"],
            FxKind::Flanger       => ["Dry/Wet", "Pan", "Freq", "Depth", "Feedback", "Delay", "Phase", "─────────"],
            FxKind::Phaser        => ["Dry/Wet", "Pan", "Freq", "Depth", "Feedback", "Phase", "Stages", "LR Cross"],
            FxKind::GranularDelay => ["Dry/Wet", "Grain Sz", "Scatter", "Feedback", "Pitch", "Position", "─────────", "─────────"],
            FxKind::Looper        => ["Dry/Wet", "Speed", "Pitch", "Reverse", "Overdub", "─────────", "─────────", "─────────"],
            FxKind::Compressor    => ["Dry/Wet", "Threshold", "Ratio", "Attack", "Release", "Makeup", "Knee", "─────────"],
            FxKind::Limiter       => ["Threshold", "Release", "Makeup", "─────────", "─────────", "─────────", "─────────", "─────────"],
            FxKind::Gate          => ["Threshold", "Attack", "Hold", "Release", "Floor", "─────────", "─────────", "─────────"],
            FxKind::Expander      => ["Threshold", "Ratio", "Attack", "Release", "Range", "─────────", "─────────", "─────────"],
            FxKind::SidechainDuck => ["Depth", "Attack", "Release", "─────────", "─────────", "─────────", "─────────", "─────────"],
            FxKind::Equalizer     => ["Low", "Low-Mid", "High-Mid", "High", "LF Freq", "HF Freq", "─────────", "─────────"],
            FxKind::ParametricEq  => ["HP Freq", "LS Gain", "Peak Freq", "Peak Gain", "Peak Q", "HS Gain", "─────────", "─────────"],
            FxKind::Isolator      => ["Bass", "Mid", "Treble", "─────────", "─────────", "─────────", "─────────", "─────────"],
            FxKind::FilterBank    => ["Band 1", "Band 2", "Band 3", "Band 4", "Band 5", "Band 6", "Band 7", "Band 8"],
            FxKind::Distortion    => ["Dry/Wet", "Pan", "Drive", "Level", "Type", "Lo Pass", "Hi Pass", "Stereo"],
            FxKind::Bitcrusher    => ["Bit Depth", "Sample Rate", "Dry/Wet", "─────────", "─────────", "─────────", "─────────", "─────────"],
            FxKind::SoftClipper   => ["Drive", "Dry/Wet", "─────────", "─────────", "─────────", "─────────", "─────────", "─────────"],
            FxKind::Saturator     => ["Drive", "Tone", "Level", "Mix", "─────────", "─────────", "─────────", "─────────"],
            FxKind::TubeSaturation=> ["Drive", "HP Tone", "Mix", "─────────", "─────────", "─────────", "─────────", "─────────"],
            FxKind::Cassette      => ["Drive", "Noise", "Tone", "Mix", "─────────", "─────────", "─────────", "─────────"],
            FxKind::VinylSim      => ["Wow", "Flutter", "Crackle", "Mix", "─────────", "─────────", "─────────", "─────────"],
            FxKind::Gain          => ["Gain dB", "─────────", "─────────", "─────────", "─────────", "─────────", "─────────", "─────────"],
            FxKind::Pan           => ["Pan", "Law", "─────────", "─────────", "─────────", "─────────", "─────────", "─────────"],
            FxKind::Widener       => ["Width", "─────────", "─────────", "─────────", "─────────", "─────────", "─────────", "─────────"],
            FxKind::PhaseInvert   => ["L Invert", "R Invert", "─────────", "─────────", "─────────", "─────────", "─────────", "─────────"],
            FxKind::MonoMaker     => ["─────────"; 8],
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

    // ── Professional channel strip additions ──────────────────────────────────
    /// Channel type — determines signal flow.
    #[serde(default = "default_channel_type")]
    pub channel_type: ChannelType,
    /// Polarity inversion (180° phase flip on output).
    #[serde(default)]
    pub phase_invert: bool,
    /// Stereo width: 0.0 = mono, 1.0 = normal, 2.0 = wide (M/S processing).
    #[serde(default = "default_width")]
    pub width: f32,
    /// Force mono output (sum L+R).
    #[serde(default)]
    pub mono: bool,
    /// Record arm flag (used for live recording routing).
    #[serde(default)]
    pub record_arm: bool,
    /// Display color palette index 0-7 (same 8-color palette as arranger tracks).
    #[serde(default)]
    pub color: u8,

    // ── Drum channel ──────────────────────────────────────────────────────────
    /// When true, this channel is a drum/percussion channel (routes to MIDI ch 10).
    #[serde(default)]
    pub is_drum: bool,
    /// Bank select MSB (CC0) — combined with `sf2_bank` for GM2/XG navigation.
    #[serde(default)]
    pub bank_msb: u8,
    /// Bank select LSB (CC32).
    #[serde(default)]
    pub bank_lsb: u8,
    /// Drum pad note mapping: index = pad step (0-15), value = GM note number (0=disabled).
    #[serde(default = "default_drum_map")]
    pub drum_map: [u8; 16],
    /// Audio output routing: 0 = master mix, 1-8 = group bus 1-8.
    #[serde(default)]
    pub group_bus: u8,
    /// True when this track has been frozen (rendered to audio; live processing bypassed).
    #[serde(default)]
    pub frozen: bool,
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
            channel_type: ChannelType::Audio,
            phase_invert: false,
            width: 1.0,
            mono: false,
            record_arm: false,
            color: 0,
            is_drum: false,
            bank_msb: 0,
            bank_lsb: 0,
            drum_map: GM_DRUM_MAP,
            group_bus: 0,
            frozen: false,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_serialisation_roundtrip() {
        let mut ch = Channel::new("TestCh");
        ch.phase_invert = true;
        ch.width = 1.5;
        ch.mono = true;
        ch.channel_type = ChannelType::GroupBus;
        ch.record_arm = true;
        ch.is_drum = true;
        ch.color = 3;
        ch.drum_map[0] = 35;
        ch.drum_map[1] = 38;

        let json = serde_json::to_string(&ch).expect("serialize");
        let back: Channel = serde_json::from_str(&json).expect("deserialize");

        assert!(back.phase_invert);
        assert!((back.width - 1.5).abs() < 1e-6);
        assert!(back.mono);
        assert_eq!(back.channel_type, ChannelType::GroupBus);
        assert!(back.record_arm);
        assert!(back.is_drum);
        assert_eq!(back.color, 3);
        assert_eq!(back.drum_map[0], 35);
        assert_eq!(back.drum_map[1], 38);
    }

    #[test]
    fn drum_channel_uses_ch10_flag() {
        let ch = Channel::new("Drums");
        assert!(!ch.is_drum, "new channel should not be drum by default");

        let mut drum_ch = Channel::new("Kit");
        drum_ch.is_drum = true;
        assert_eq!(drum_ch.drum_map, GM_DRUM_MAP);
        // Pad 0 should be kick (36).
        assert_eq!(drum_ch.drum_map[0], 36);
        // Pad 1 should be snare (38).
        assert_eq!(drum_ch.drum_map[1], 38);
    }

    #[test]
    fn fxkind_cycle_covers_all_variants() {
        let start = FxKind::None;
        let mut current = start.next();
        let mut count = 1;
        while current != FxKind::None {
            current = current.next();
            count += 1;
            assert!(count < 100, "FxKind cycle did not return to None");
        }
        assert_eq!(count, FxKind::ORDER.len());
    }
}
