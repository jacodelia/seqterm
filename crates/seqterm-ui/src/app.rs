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
    // ── Spatial / time ────────────────────────────────────────────────────────
    #[default]
    Delay,
    Reverb,
    GranDelay,
    // ── Dynamics ─────────────────────────────────────────────────────────────
    Compressor,
    Limiter,
    Gate,
    // ── EQ & Filter ──────────────────────────────────────────────────────────
    ParamEq,
    Filter,
    FilterBank,
    // ── Modulation ───────────────────────────────────────────────────────────
    Chorus,
    Flanger,
    Phaser,
    // ── Saturation / colour ───────────────────────────────────────────────────
    BitCrusher,
    Vinyl,
    Cassette,
    SoftClip,
    TubeSat,
    // ── Spatial ──────────────────────────────────────────────────────────────
    Widener,
    Isolator,
    // ── Utility ──────────────────────────────────────────────────────────────
    Gain,
    PhaseInvert,
    MonoMaker,
    // ── Looping / sidechaining ────────────────────────────────────────────────
    Looper,
    SidechainDuck,
    // ── New ──────────────────────────────────────────────────────────────────
    Expander,
    Pan,
}

pub const ALL_FX_KINDS: &[AudioFxKind] = &[
    AudioFxKind::Delay, AudioFxKind::Reverb, AudioFxKind::GranDelay,
    AudioFxKind::Compressor, AudioFxKind::Limiter, AudioFxKind::Gate, AudioFxKind::Expander,
    AudioFxKind::ParamEq, AudioFxKind::Filter, AudioFxKind::FilterBank,
    AudioFxKind::Chorus, AudioFxKind::Flanger, AudioFxKind::Phaser,
    AudioFxKind::BitCrusher, AudioFxKind::Vinyl, AudioFxKind::Cassette,
    AudioFxKind::SoftClip, AudioFxKind::TubeSat,
    AudioFxKind::Widener, AudioFxKind::Isolator,
    AudioFxKind::Gain, AudioFxKind::Pan, AudioFxKind::PhaseInvert, AudioFxKind::MonoMaker,
    AudioFxKind::Looper, AudioFxKind::SidechainDuck,
];

impl AudioFxKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Delay        => "DELAY",
            Self::Reverb       => "REVERB",
            Self::GranDelay    => "GRANDELAY",
            Self::Compressor   => "COMPRESSOR",
            Self::Limiter      => "LIMITER",
            Self::Gate         => "GATE",
            Self::ParamEq      => "PARAM EQ",
            Self::Filter       => "FILTER",
            Self::FilterBank   => "FILTERBANK",
            Self::Chorus       => "CHORUS",
            Self::Flanger      => "FLANGER",
            Self::Phaser       => "PHASER",
            Self::BitCrusher   => "BITCRUSH",
            Self::Vinyl        => "VINYL",
            Self::Cassette     => "CASSETTE",
            Self::SoftClip     => "SOFTCLIP",
            Self::TubeSat      => "TUBE SAT",
            Self::Widener      => "WIDENER",
            Self::Isolator     => "ISOLATOR",
            Self::Gain         => "GAIN",
            Self::PhaseInvert  => "PHASE INV",
            Self::MonoMaker    => "MONO",
            Self::Looper       => "LOOPER",
            Self::SidechainDuck => "SIDECHAIN",
            Self::Expander     => "EXPANDER",
            Self::Pan          => "PAN",
        }
    }

    pub fn next(self) -> Self {
        let idx = ALL_FX_KINDS.iter().position(|k| *k == self).unwrap_or(0);
        ALL_FX_KINDS[(idx + 1) % ALL_FX_KINDS.len()]
    }

    pub fn prev(self) -> Self {
        let idx = ALL_FX_KINDS.iter().position(|k| *k == self).unwrap_or(0);
        ALL_FX_KINDS[(idx + ALL_FX_KINDS.len() - 1) % ALL_FX_KINDS.len()]
    }
}

/// Descriptor for a single FX parameter (static metadata).
#[derive(Debug, Clone, Copy)]
pub struct FxParamDesc {
    pub name: &'static str,
    /// Default value, normalised 0.0–1.0.
    pub default: f32,
}

impl FxParamDesc {
    #[allow(dead_code)]
    const fn new(name: &'static str, default: f32) -> Self {
        Self { name, default }
    }
}

/// Static parameter table per FX kind. Up to 8 params per type.
/// Indices match AudioFxEntry.params[].
macro_rules! pd { ($n:literal, $d:literal) => { FxParamDesc { name: $n, default: $d } } }

/// Static parameter table per FX kind. Up to 8 params per type.
pub fn fx_param_descs(kind: AudioFxKind) -> &'static [FxParamDesc] {
    use AudioFxKind::*;
    // Spatial / time
    static DELAY:    &[FxParamDesc] = &[pd!("Time",0.30),pd!("Feedback",0.40),pd!("Damping",0.30),pd!("PingPong",0.00),pd!("Wet",1.00)];
    static REVERB:   &[FxParamDesc] = &[pd!("Room",0.50),pd!("Damping",0.50),pd!("Width",1.00),pd!("Wet",0.35)];
    static GRNDLY:   &[FxParamDesc] = &[pd!("Size",0.40),pd!("Density",0.50),pd!("Pitch",0.50),pd!("Feedback",0.30),pd!("Wet",0.80)];
    // Dynamics
    static COMP:     &[FxParamDesc] = &[pd!("Thresh",0.70),pd!("Ratio",0.18),pd!("Attack",0.10),pd!("Release",0.15),pd!("Makeup",0.00),pd!("Knee",0.50),pd!("Wet",1.00)];
    static LIMIT:    &[FxParamDesc] = &[pd!("Thresh",0.95),pd!("Release",0.25),pd!("Wet",1.00)];
    static GATE:     &[FxParamDesc] = &[pd!("Thresh",0.50),pd!("Attack",0.02),pd!("Hold",0.10),pd!("Release",0.20),pd!("Floor",0.00),pd!("Wet",1.00)];
    // EQ & filter
    static PARAMEQ:  &[FxParamDesc] = &[pd!("Low",0.50),pd!("LowMid",0.50),pd!("HiMid",0.50),pd!("High",0.50),pd!("LowFreq",0.30),pd!("HiFreq",0.70),pd!("MidQ",0.30),pd!("Wet",1.00)];
    static FILTER:   &[FxParamDesc] = &[pd!("Cutoff",0.70),pd!("Res",0.20),pd!("Wet",1.00)];
    static FILTERBNK:&[FxParamDesc] = &[pd!("Low",0.50),pd!("Mid",0.50),pd!("High",0.50),pd!("Wet",1.00)];
    // Modulation
    static CHORUS:   &[FxParamDesc] = &[pd!("Rate",0.20),pd!("Depth",0.30),pd!("Delay",0.30),pd!("Feedback",0.55),pd!("Wet",0.50)];
    static FLANGER:  &[FxParamDesc] = &[pd!("Rate",0.15),pd!("Depth",0.35),pd!("Delay",0.25),pd!("Feedback",0.70),pd!("Wet",0.70)];
    static PHASER:   &[FxParamDesc] = &[pd!("Rate",0.18),pd!("Depth",0.70),pd!("Center",0.40),pd!("Feedback",0.70),pd!("Wet",0.70)];
    // Saturation / colour
    static CRUSH:    &[FxParamDesc] = &[pd!("Bits",0.70),pd!("Rate",1.00),pd!("Wet",1.00)];
    static VINYL:    &[FxParamDesc] = &[pd!("Wow",0.20),pd!("Flutter",0.15),pd!("Crackle",0.10),pd!("Wet",1.00)];
    static CASSETTE: &[FxParamDesc] = &[pd!("Drive",0.40),pd!("Wet",1.00)];
    static SOFTCLIP: &[FxParamDesc] = &[pd!("Drive",0.25),pd!("Wet",1.00)];
    static TUBESAT:  &[FxParamDesc] = &[pd!("Drive",0.15),pd!("Tone",0.30),pd!("Wet",0.60)];
    // Spatial
    static WIDENER:  &[FxParamDesc] = &[pd!("Width",0.50),pd!("Wet",1.00)];
    static ISOLATOR: &[FxParamDesc] = &[pd!("Low",0.50),pd!("Mid",0.50),pd!("High",0.50),pd!("Wet",1.00)];
    // Utility
    static GAIN:     &[FxParamDesc] = &[pd!("Gain",0.50),pd!("Wet",1.00)];
    static PHASEINV: &[FxParamDesc] = &[pd!("InvertL",1.00),pd!("InvertR",0.00)];
    static MONO:     &[FxParamDesc] = &[pd!("Wet",1.00)];
    // Looping / sidechain
    static LOOPER:   &[FxParamDesc] = &[pd!("Length",0.50),pd!("Feedback",0.70),pd!("Wet",1.00)];
    static SIDECHAIN:&[FxParamDesc] = &[pd!("Amount",0.80),pd!("Release",0.30),pd!("Wet",1.00)];

    match kind {
        Delay        => DELAY,    Reverb      => REVERB,    GranDelay  => GRNDLY,
        Compressor   => COMP,     Limiter     => LIMIT,     Gate       => GATE,
        ParamEq      => PARAMEQ,  Filter      => FILTER,    FilterBank => FILTERBNK,
        Chorus       => CHORUS,   Flanger     => FLANGER,   Phaser     => PHASER,
        BitCrusher   => CRUSH,    Vinyl       => VINYL,     Cassette   => CASSETTE,
        SoftClip     => SOFTCLIP, TubeSat     => TUBESAT,
        Widener      => WIDENER,  Isolator    => ISOLATOR,
        Gain         => GAIN,     PhaseInvert => PHASEINV,  MonoMaker  => MONO,
        Looper       => LOOPER,   SidechainDuck => SIDECHAIN,
        Expander     => &[pd!("Thresh",0.50),pd!("Ratio",0.25),pd!("Attack",0.20),pd!("Release",0.30),pd!("Range",0.75)],
        Pan          => &[pd!("Pan",0.50),pd!("ConstPwr",1.00)],
    }
}

/// One entry in an audio slot's FX chain.
#[derive(Debug, Clone)]
pub struct AudioFxEntry {
    pub kind:    AudioFxKind,
    pub wet:     f32,
    pub enabled: bool,
    /// Normalised (0.0–1.0) values for each parameter in `fx_param_descs(kind)`.
    pub params:      Vec<f32>,
    /// Optional MIDI CC number bound to each parameter (None = unbound).
    pub cc_bindings: Vec<Option<u8>>,
}

impl AudioFxEntry {
    pub fn new(kind: AudioFxKind) -> Self {
        let descs = fx_param_descs(kind);
        let params:      Vec<f32>       = descs.iter().map(|d| d.default).collect();
        let cc_bindings: Vec<Option<u8>> = vec![None; descs.len()];
        // Mirror wet from the last param if it's labelled "Wet".
        let wet = descs.last().filter(|d| d.name == "Wet").map(|d| d.default).unwrap_or(1.0);
        Self { kind, wet, enabled: true, params, cc_bindings }
    }

    /// Param value scaled to the processor's native range.
    /// Currently all params are 0–1 normalised; this is the hook for future range mapping.
    pub fn param_native(&self, idx: usize) -> f32 {
        self.params.get(idx).copied().unwrap_or(0.0)
    }

    /// Keep `self.wet` in sync with the "Wet" parameter whenever it's edited.
    pub fn sync_wet(&mut self) {
        let descs = fx_param_descs(self.kind);
        if let Some(i) = descs.iter().position(|d| d.name == "Wet") {
            if let Some(v) = self.params.get(i) {
                self.wet = *v;
            }
        }
    }
}

/// Build a realtime FX processor chain from a list of `AudioFxEntry` entries.
/// Converts normalised (0–1) param values into native processor ranges.
pub fn build_fx_chain(
    entries: &[AudioFxEntry],
) -> Vec<Box<dyn seqterm_audio_engine::FxProcessor>> {
    use seqterm_audio_engine::fx::{
        Bitcrusher, Cassette, Chorus, Compressor, Expander, FilterBankFx, Flanger, Gain,
        Gate, GranularDelay, Isolator, Looper, MonoMaker, Pan as PanFx, ParametricEq, Phaser,
        PhaseInvert, Reverb, SidechainDuck, SoftClipper, StereoWidener,
        Svf, SvfMode, TubeSaturation, VinylSim,
    };
    use seqterm_audio_engine::fx::delay::DelayLine;

    entries.iter()
        .filter(|e| e.enabled)
        .map(|e| {
            let p = |i: usize| e.params.get(i).copied().unwrap_or(0.0);

            let mut proc: Box<dyn seqterm_audio_engine::FxProcessor> = match e.kind {
                AudioFxKind::Delay => {
                    let delay_ms = 10.0 + p(0) * 990.0;
                    let feedback = p(1);
                    let damping  = p(2);
                    let mut d    = DelayLine::new(delay_ms, feedback, damping);
                    d.set_ping_pong(p(3) > 0.5);
                    Box::new(d)
                }
                AudioFxKind::Reverb => {
                    let mut r = Reverb::new(48000);
                    r.set_room_size(p(0));
                    r.set_damp(p(1));
                    Box::new(r)
                }
                AudioFxKind::GranDelay => {
                    Box::new(GranularDelay::new(
                        20.0 + p(0) * 980.0,
                        p(1),
                        (p(2) - 0.5) * 24.0,
                        1.0 + p(3) * 31.0,
                    ))
                }
                AudioFxKind::Compressor => {
                    let mut c = Compressor::new();
                    c.threshold_db = -(1.0 - p(0)) * 60.0;
                    c.ratio        = 1.0 + p(1) * 19.0;
                    c.attack_ms    = 0.1 + p(2) * 99.9;
                    c.release_ms   = 10.0 + p(3) * 990.0;
                    c.makeup_db    = p(4) * 24.0;
                    c.knee_db      = p(5) * 12.0;
                    Box::new(c)
                }
                AudioFxKind::Limiter => {
                    let mut lim = Compressor::limiter();
                    lim.threshold_db = -(1.0 - p(0)) * 12.0;
                    lim.release_ms   = 1.0 + p(1) * 199.0;
                    Box::new(lim)
                }
                AudioFxKind::Gate => {
                    let mut g = Gate::new();
                    g.threshold_db = -(1.0 - p(0)) * 80.0;
                    g.attack_ms    = 0.1 + p(1) * 49.9;
                    g.hold_ms      = 1.0 + p(2) * 499.0;
                    g.release_ms   = 10.0 + p(3) * 990.0;
                    g.floor_db     = -(1.0 - p(4)) * 80.0;
                    Box::new(g)
                }
                AudioFxKind::ParamEq => {
                    use seqterm_audio_engine::fx::parametric_eq::EqBandKind;
                    let mut eq = ParametricEq::new();
                    // bands[0] = HP, [1] = LowShelf, [2] = Peak, [3] = HighShelf
                    eq.bands[1].gain_db = (p(0) - 0.5) * 36.0; // LowShelf
                    eq.bands[2].gain_db = (p(1) - 0.5) * 36.0; // LowMid Peak
                    eq.bands[3].gain_db = (p(2) - 0.5) * 36.0; // HighShelf (reused for HiMid)
                    // p3 = high gain → override band[3] kind to HighShelf
                    eq.bands[3].kind    = EqBandKind::HighShelf;
                    eq.bands[3].gain_db = (p(3) - 0.5) * 36.0;
                    eq.bands[1].freq    = 20.0 * (800.0f32 / 20.0).powf(p(4)); // LowFreq
                    eq.bands[3].freq    = 1000.0 * 20.0f32.powf(p(5));         // HiFreq
                    eq.bands[2].q       = 0.1 + p(6) * 9.9;                    // MidQ
                    Box::new(eq)
                }
                AudioFxKind::Filter => {
                    let freq = 20.0 + p(0) * 19980.0;
                    let res  = p(1) * 4.0 + 0.5;
                    Box::new(Svf::new(SvfMode::Lowpass, freq, res))
                }
                AudioFxKind::FilterBank => Box::new(FilterBankFx::new(48000)),
                AudioFxKind::Chorus => {
                    let mut c = Chorus::new();
                    c.rate     = 0.05 + p(0) * 4.95;
                    c.depth    = 0.5  + p(1) * 9.5;
                    c.delay_ms = 5.0  + p(2) * 25.0;
                    c.feedback = (p(3) - 0.5) * 1.8;
                    Box::new(c)
                }
                AudioFxKind::Flanger => {
                    let mut f = Flanger::new();
                    f.rate     = 0.05 + p(0) * 4.95;
                    f.depth    = p(1) * 7.0;
                    f.delay_ms = 0.5  + p(2) * 9.5;
                    f.feedback = (p(3) - 0.5) * 1.9;
                    Box::new(f)
                }
                AudioFxKind::Phaser => {
                    let mut ph = Phaser::new();
                    ph.rate     = 0.05 + p(0) * 4.95;
                    ph.depth    = p(1);
                    ph.center   = 200.0 + p(2) * 1800.0;
                    ph.feedback = (p(3) - 0.5) * 1.8;
                    Box::new(ph)
                }
                AudioFxKind::BitCrusher => {
                    let mut b = Bitcrusher::new();
                    b.set_bits((1.0 + p(0) * 15.0) as u8);
                    b.set_hold((1.0 + p(1) * 15.0) as u32);
                    Box::new(b)
                }
                AudioFxKind::Vinyl => {
                    let mut v = VinylSim::new();
                    v.set_wow(p(0) * 0.1);
                    v.set_flutter(p(1) * 0.05);
                    v.set_crackle(p(2));
                    Box::new(v)
                }
                AudioFxKind::Cassette => Box::new(Cassette::new()),
                AudioFxKind::SoftClip => {
                    let mut s = SoftClipper::new();
                    s.drive = 1.0 + p(0) * 9.0;
                    Box::new(s)
                }
                AudioFxKind::TubeSat => {
                    let mut t = TubeSaturation::new();
                    t.drive = 1.0 + p(0) * 19.0;
                    t.tone  = p(1);
                    Box::new(t)
                }
                AudioFxKind::Widener => {
                    let mut w = StereoWidener::new();
                    w.width = p(0) * 2.0;
                    Box::new(w)
                }
                AudioFxKind::Isolator => Box::new(Isolator::new()),
                AudioFxKind::Gain => {
                    let mut g = Gain::new();
                    g.gain_db = (p(0) - 0.5) * 48.0;
                    Box::new(g)
                }
                AudioFxKind::PhaseInvert => {
                    Box::new(PhaseInvert { invert_l: p(0) > 0.5, invert_r: p(1) > 0.5 })
                }
                AudioFxKind::MonoMaker => Box::new(MonoMaker::new()),
                AudioFxKind::Looper    => Box::new(Looper::new(48000)),
                AudioFxKind::SidechainDuck => Box::new(SidechainDuck::new()),
                AudioFxKind::Expander => {
                    let mut exp = Expander::new();
                    exp.threshold_db = -(1.0 - p(0)) * 80.0;
                    exp.ratio        = 1.0 + p(1) * 9.0;
                    exp.attack_ms    = 0.1 + p(2) * 49.9;
                    exp.release_ms   = 10.0 + p(3) * 990.0;
                    exp.range_db     = p(4) * 80.0;
                    Box::new(exp)
                }
                AudioFxKind::Pan => {
                    let mut pan = PanFx::new();
                    pan.pan            = (p(0) - 0.5) * 2.0;
                    pan.constant_power = p(1) > 0.5;
                    Box::new(pan)
                }
            };
            proc.set_mix(e.wet);
            proc
        })
        .collect()
}

/// Unified focus token — identifies which panel/widget currently holds keyboard focus.
/// Used by views to decide border colours and by the Tab key to advance focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusId {
    /// No specific widget focused — default navigation mode.
    #[default]
    None,
    // ── Matrix ────────────────────────────────────────────────────────────────
    MatrixGrid,
    MatrixPolymeter,
    MatrixRouting,
    MatrixHybrid,
    MatrixDrum,
    // ── Tracker ───────────────────────────────────────────────────────────────
    TrackerStepEditor,
    TrackerPianoRoll,
    TrackerGenerative,
    TrackerModulation,
    TrackerFxChain,
    // ── Arranger ──────────────────────────────────────────────────────────────
    ArrangerTracks,
    ArrangerAutomation,
    ArrangerChain,
    // ── Mixer ─────────────────────────────────────────────────────────────────
    MixerStrips,
    MixerFxSidebar,
    MixerRoutingMatrix,
    // ── Config ────────────────────────────────────────────────────────────────
    ConfigMidiIn,
    ConfigMidiOut,
    ConfigAudio,
    ConfigRouting,
    // ── Granular ──────────────────────────────────────────────────────────────
    GranularEditor,
    GranularMod,
}

impl FocusId {
    /// Advance to the next logical focus within the same view.
    /// Views that don't use the focus ring return self.
    pub fn next_in_view(self, view: super::app::ViewKind) -> Self {
        use FocusId::*;
        use super::app::ViewKind::*;
        match (view, self) {
            (Matrix,   MatrixGrid)       => MatrixPolymeter,
            (Matrix,   MatrixPolymeter)  => MatrixHybrid,
            (Matrix,   MatrixHybrid)     => MatrixDrum,
            (Matrix,   MatrixDrum)       => MatrixGrid,
            (Tracker,  TrackerStepEditor)  => TrackerPianoRoll,
            (Tracker,  TrackerPianoRoll)   => TrackerGenerative,
            (Tracker,  TrackerGenerative)  => TrackerModulation,
            (Tracker,  TrackerModulation)  => TrackerFxChain,
            (Tracker,  TrackerFxChain)     => TrackerStepEditor,
            (Arranger, ArrangerTracks)     => ArrangerAutomation,
            (Arranger, ArrangerAutomation) => ArrangerChain,
            (Arranger, ArrangerChain)      => ArrangerTracks,
            (Mixer,    MixerStrips)        => MixerFxSidebar,
            (Mixer,    MixerFxSidebar)     => MixerRoutingMatrix,
            (Mixer,    MixerRoutingMatrix)  => MixerStrips,
            (Config,   ConfigMidiIn)       => ConfigMidiOut,
            (Config,   ConfigMidiOut)      => ConfigAudio,
            (Config,   ConfigAudio)        => ConfigRouting,
            (Config,   ConfigRouting)      => ConfigMidiIn,
            _ => self,
        }
    }

    /// Return the default focus for a given view.
    pub fn default_for(view: super::app::ViewKind) -> Self {
        use super::app::ViewKind::*;
        match view {
            Matrix   => FocusId::MatrixGrid,
            Tracker  => FocusId::TrackerStepEditor,
            Arranger => FocusId::ArrangerTracks,
            Mixer    => FocusId::MixerStrips,
            Config   => FocusId::ConfigMidiIn,
            Granular => FocusId::GranularEditor,
        }
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
    Granular,
}

impl ViewKind {
    pub fn label(&self) -> &'static str {
        match self {
            ViewKind::Matrix   => "MATRIX",
            ViewKind::Tracker  => "PATTERN",
            ViewKind::Arranger => "SONG",
            ViewKind::Mixer    => "MIXER",
            ViewKind::Config   => "CONFIG",
            ViewKind::Granular => "EDITOR",
        }
    }

    pub fn index(&self) -> usize {
        // Bottom-bar order: MATRIX | TRACKER/P.ROLL | GRANULAR | ARRANGER | MIXER | CONFIG
        match self {
            ViewKind::Matrix   => 0,
            ViewKind::Tracker  => 1,
            ViewKind::Granular => 2,
            ViewKind::Arranger => 3,
            ViewKind::Mixer    => 4,
            ViewKind::Config   => 5,
        }
    }

    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(ViewKind::Matrix),
            1 => Some(ViewKind::Tracker),
            2 => Some(ViewKind::Granular),
            3 => Some(ViewKind::Arranger),
            4 => Some(ViewKind::Mixer),
            5 => Some(ViewKind::Config),
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

/// Arranger snap-to-grid granularity.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SnapGrid {
    Off,
    Bar,
    HalfBar,
    QuarterBar,
    Eighth,
    Sixteenth,
    #[default]
    ThirtySecond,
}

impl SnapGrid {
    pub fn label(self) -> &'static str {
        match self {
            SnapGrid::Off          => "OFF",
            SnapGrid::Bar          => "1BAR",
            SnapGrid::HalfBar      => "1/2B",
            SnapGrid::QuarterBar   => "1/4B",
            SnapGrid::Eighth       => "1/8",
            SnapGrid::Sixteenth    => "1/16",
            SnapGrid::ThirtySecond => "1/32",
        }
    }
    pub fn next(self) -> Self {
        match self {
            SnapGrid::Off          => SnapGrid::Bar,
            SnapGrid::Bar          => SnapGrid::HalfBar,
            SnapGrid::HalfBar      => SnapGrid::QuarterBar,
            SnapGrid::QuarterBar   => SnapGrid::Eighth,
            SnapGrid::Eighth       => SnapGrid::Sixteenth,
            SnapGrid::Sixteenth    => SnapGrid::ThirtySecond,
            SnapGrid::ThirtySecond => SnapGrid::Off,
        }
    }
}

/// Arranger edit tool — determines what clicking/Enter does on the track grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArrangerTool {
    #[default]
    Select,
    Draw,
    Slice,
    Paint,
    Mute,
}

impl ArrangerTool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "SELECT",
            Self::Draw   => "DRAW",
            Self::Slice  => "SLICE",
            Self::Paint  => "PAINT",
            Self::Mute   => "MUTE",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Select => Self::Draw,
            Self::Draw   => Self::Slice,
            Self::Slice  => Self::Paint,
            Self::Paint  => Self::Mute,
            Self::Mute   => Self::Select,
        }
    }
}

#[derive(Debug, Default)]
pub struct ArrangerState {
    /// Visible start bar (scroll offset).
    pub bar_offset: u32,
    /// Selected track index (matrix row index).
    pub selected_track: usize,
    /// Selected column index (for clip cursor operations).
    pub selected_col: usize,
    /// Automation cursor bar.
    pub automation_cursor: usize,
    /// 0=tracks, 1=automation, 2=song transport.
    pub section: usize,
    /// Which automation lane is focused.
    pub automation_lane: usize,
    /// Cursor within the song transport row: 0=PLAY, 1=STOP, 2=REC, 3=BPM.
    pub song_transport_cursor: usize,
    /// Current snap-to-grid setting.
    pub snap: SnapGrid,
    /// Multi-selected clips: set of (row_idx, col_idx).
    pub multi_select: std::collections::HashSet<(usize, usize)>,
    /// Clipboard for clip copy/paste: (source_col, Clip).
    pub clip_clipboard: Option<(usize, seqterm_core::Clip)>,
    /// Resize mode: when true, `[`/`]` adjust the selected clip's pattern length.
    pub resize_mode: bool,
    /// Horizontal bar width in screen chars (2–8). Default 4. Ctrl+scroll zooms.
    pub bar_width: u8,
    /// Active edit tool (Select / Draw / Slice / Paint / Mute). Default = Select.
    pub tool: ArrangerTool,
    /// Vertical track scroll offset (first visible track row index).
    /// Allows projects with many tracks (> visible height) to scroll vertically.
    pub track_scroll: usize,
}

#[derive(Debug, Default)]
pub struct MixerState {
    /// Selected channel index (into collect_mixer_entries result).
    pub selected_channel: usize,
    /// In edit mode for a parameter (strips panel).
    pub editing: bool,
    /// Active parameter: 0=VOL 1=EQ_LO 2=EQ_LM 3=EQ_HM 4=EQ_HI 5=PAN 6=FX
    pub active_param: usize,
    /// Which of the 3 FX slots is being edited (0-2).
    pub fx_slot_idx: usize,
    /// Cursor row in FX sidebar: 0=slot header, 3-10=param[0-7].
    pub fx_row: usize,
    /// Cursor column in FX sidebar: 0=CC#, 1=value (for param rows 3-10).
    pub fx_col: usize,
    /// First visible strip column (horizontal scroll offset).
    pub strip_scroll: usize,
    /// When true, show the audio routing matrix instead of channel strips.
    pub routing_matrix: bool,
    /// Cursor row in the routing matrix (channel index 0-15).
    pub routing_row: usize,
    /// Cursor column in the routing matrix (0=MSTR, 1-8=GRP1-8, 9=SendA, 10=SendB).
    pub routing_col: usize,
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

/// Per-param cursor IDs for the granular view (0-11 = GrainParams, 12-16 = GranularZone, 17-20 = ModMatrix, 21-24 = Macros).
pub const GRAN_PARAM_COUNT: usize = 25;

/// Active scene morph between two GranularPreset states.
pub struct GranularMorph {
    pub from:     seqterm_core::GranularPreset,
    pub to:       seqterm_core::GranularPreset,
    /// Progress from 0.0 (at `from`) to 1.0 (at `to`).
    pub progress: f32,
    /// Progress increment per UI frame (derived from beats and BPM).
    pub step:     f32,
}

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
    /// Unified focus ring — which widget currently holds keyboard focus.
    pub focus: FocusId,
    pub matrix_rows:      usize,
    pub matrix_cols:      usize,
    pub matrix_col_scroll: usize,  // plain usize in TabState (serialisable)
    pub bpm:              f64,
    pub audio_slots:      std::collections::HashMap<String, u32>,
}

// ─── App ─────────────────────────────────────────────────────────────────────

pub struct App {
    pub project: Arc<Mutex<Project>>,
    pub current_view: ViewKind,
    /// Unified focus ring — which widget currently holds keyboard focus.
    pub focus: FocusId,
    pub engine: PlaybackEngine,
    pub should_quit: bool,
    /// Whether transport is currently playing (mirrored from engine events).
    pub playing: bool,
    /// Whether transport is paused (position preserved, audio silent).
    pub paused: bool,
    /// Whether recording is active.
    pub recording: bool,
    /// Whether song-mode transport is playing (arranger).
    pub song_playing: bool,
    /// Current bar position in song-mode playback.
    pub song_bar: usize,
    /// Whether pattern chain mode is active (chain follows project.chain Vec).
    pub chain_mode: bool,
    /// Current position in project.chain (updated by ChainAdvanced event).
    pub chain_pos: usize,
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
    /// Currently highlighted scene slot index (0-7) in the granular view.
    pub granular_scene_slot: usize,
    /// Modulation matrix for the current granular pad (4 LFO slots).
    pub granular_mod: seqterm_core::GranularMod,
    /// Cursor row within the mod matrix panel (0 = slot 0 header, 1-3 = slots 1-3).
    pub granular_mod_cursor: usize,
    /// 4 macro knobs (0.0–1.0). Macro N maps: spray, density, pitch_st, grain_size.
    pub granular_macros: [f32; 4],
    /// Which macro is currently focused in the Granular view.
    pub granular_macro_cursor: usize,
    /// Active granular morph: (from_preset, to_preset, progress 0.0-1.0, step_per_frame).
    pub granular_morph: Option<GranularMorph>,
    /// Audio engine slot_id currently routed as live input to the granular engine (None = off).
    pub granular_live_source: Option<u32>,
    /// Retrigger sub-step scheduler: background threads send slot_ids to replay.
    pub retrigger_tx: flume::Sender<u32>,
    retrigger_rx: flume::Receiver<u32>,

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
    /// 0=step table, 1=piano roll, 2=generative engine, 3=track modulation, 4=FX chain.
    pub tracker_section: usize,
    /// FX chain panel: which of the 3 slots is focused.
    pub tracker_fx_slot: usize,
    /// FX chain panel: which parameter of the focused slot is selected.
    pub tracker_fx_param: usize,
    /// FX chain panel: Some((slot, param)) while waiting for a MIDI CC to learn.
    pub tracker_fx_midi_learn: Option<(usize, usize)>,
    /// Piano roll rendered area (cached via Cell for mouse hit-testing).
    pub piano_roll_area: std::cell::Cell<ratatui::layout::Rect>,
    pub piano_vel_area: std::cell::Cell<ratatui::layout::Rect>,
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
    /// Sidebar panel area inside the FilePicker modal (for mouse click/scroll).
    pub file_picker_sidebar_area: std::cell::Cell<ratatui::layout::Rect>,
    /// Confirm modal — rect of the "Yes" button (for mouse click).
    pub confirm_yes_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// Confirm modal — rect of the "No/Cancel" button (for mouse click).
    pub confirm_no_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// QuitConfirm modal — rects for the three buttons.
    pub quit_save_rect: std::cell::Cell<ratatui::layout::Rect>,
    pub quit_nosave_rect: std::cell::Cell<ratatui::layout::Rect>,
    pub quit_cancel_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// Shared OK/Accept button rect for configuration/search modals.
    pub modal_ok_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// Shared Cancel button rect for configuration/search modals.
    pub modal_cancel_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// MIDI Settings modal — click rects for the 4 tabs (Inputs/Outputs/Sync/Learn).
    pub midi_settings_tab_rects: std::cell::Cell<[ratatui::layout::Rect; 4]>,
    /// MIDI Settings modal — click rect for the list area.
    pub midi_settings_list_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// Audio Settings modal — click rects for the 3 tabs (Engine/Plugin Paths/OSC).
    pub audio_settings_tab_rects: std::cell::Cell<[ratatui::layout::Rect; 3]>,
    /// Audio Settings · Engine tab — click rects for the 5 editable rows.
    pub audio_engine_row_rects: std::cell::Cell<[ratatui::layout::Rect; 5]>,
    /// Audio Settings · Plugin Paths tab — click rects for the 9 format rows.
    pub audio_pp_fmt_rects: std::cell::Cell<[ratatui::layout::Rect; 9]>,
    /// Audio Settings · Plugin Paths tab — directory-list area (rows computed by y).
    pub audio_pp_dir_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// Audio Settings · Plugin Paths tab — [+ Add] [− Del] [Rescan] button rects.
    pub audio_pp_action_rects: std::cell::Cell<[ratatui::layout::Rect; 3]>,
    /// Audio Settings · OSC tab — click rects for the 4 rows.
    pub audio_osc_row_rects: std::cell::Cell<[ratatui::layout::Rect; 4]>,
    /// SF2 Browser: bank ◄ / ► arrow rects.
    pub sf2_bank_left_rect:  std::cell::Cell<ratatui::layout::Rect>,
    pub sf2_bank_right_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// Hit-test rect for the "♪ A3" audition button in the SF2 browser.
    pub sf2_a3_btn_rect:     std::cell::Cell<ratatui::layout::Rect>,
    /// SF2 Browser: preset list inner area (for row-click detection).
    pub sf2_list_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// SF2 Browser: "Change Bank/Preset" button in routing panel.
    pub sf2_reopen_btn_y: std::cell::Cell<u16>,

    // ── Matrix transport editing ──────────────────────────────────────────────
    /// 0 = grid navigation, 1 = transport section active (Tab toggles).
    pub matrix_section: usize,
    /// Which transport param is selected: 0=BPM, 1=rows, 2=cols.
    pub transport_cursor: usize,
    /// Number of visible matrix rows (A-P, max 16).
    pub matrix_rows: usize,
    /// Number of visible matrix columns (max min(16, 128/rows)).
    pub matrix_cols: usize,
    /// First visible column in the matrix (horizontal scroll offset).
    pub matrix_col_scroll: std::cell::Cell<usize>,
    /// Tap tempo: timestamps of recent taps for BPM detection.
    pub tap_times: Vec<std::time::Instant>,
    /// Whether a JACK server was detected during the last port refresh.
    pub jack_available: bool,
    /// Audio engine owned by the App — started in main.rs after App construction.
    pub audio_engine: Option<seqterm_audio_engine::AudioEngine>,
    /// Maps clip key (e.g. "A0") → audio engine slot_id for SF2 / AudioFile sources.
    pub audio_slots: std::collections::HashMap<String, u32>,
    /// Slot IDs that hold a SoundFontSynth (as opposed to an AudioClipPlayer).
    /// Used by silence_all_audio to avoid sending StopAudioClip to SF2 slots,
    /// which would deactivate the slot and break subsequent play cycles.
    pub sf2_slots: std::collections::HashSet<u32>,
    /// Per-slot linear gain (0.0–2.0, default 1.0 = 0 dB).
    pub audio_slot_volumes: std::collections::HashMap<u32, f32>,
    /// Per-(slot, MIDI channel) volume as CC7 (0–127, default 100) for SF2 slots
    /// that host several instruments on one shared synth — lets each instrument's
    /// volume move independently of the others on the same slot.
    pub audio_slot_channel_vol: std::collections::HashMap<(u32, u8), u8>,
    /// FX chain config per audio engine slot (not persisted — rebuilt on project reload).
    pub audio_slot_fx: std::collections::HashMap<u32, Vec<AudioFxEntry>>,
    /// Master bus FX chain (applied to final mix before soft-clip).
    pub master_fx: Vec<AudioFxEntry>,
    /// Master output volume (linear gain, 1.0 = 0 dB). Adjustable from the
    /// MASTER strips in the mixer; mirrored to the audio engine.
    pub master_volume: f32,
    /// Audio engine status (updated each frame from AudioEngineEvent drain).
    pub audio_engine_running: bool,
    pub audio_sample_rate: u32,
    pub audio_buffer_size: u32,
    pub audio_dsp_load: f32,
    pub audio_xrun_count: u32,
    /// Per-slot peak levels (0.0–1.0+), polled each frame from the audio engine.
    pub audio_slot_peaks: Vec<f32>,
    /// Master output peak [L, R], polled each frame from the audio engine.
    pub audio_master_peak: [f32; 2],
    /// Per-slot RMS levels (0.0–1.0), polled each frame.
    pub audio_slot_rms: Vec<f32>,
    /// Master RMS [L, R], polled each frame.
    pub audio_master_rms: [f32; 2],
    /// Clip indicators: true if a slot has ever peaked ≥ 1.0 since last reset ('c' in Mixer).
    pub audio_slot_clip: Vec<bool>,
    /// Master clip indicators [L, R].
    pub master_clip: [bool; 2],
    /// M/S stereo correlation coefficient (-1..+1), polled each frame.
    pub master_correlation: f32,
    /// LUFS readings: (momentary, short-term, integrated), polled each frame.
    pub master_lufs: (f32, f32, f32),
    /// Spectrum analyzer band magnitudes (SPECTRUM_BANDS bands), polled each frame.
    pub master_spectrum: Vec<f32>,
    /// Path to the open .stz container file (None = not using .stz format).
    pub stz_path: Option<std::path::PathBuf>,
    /// Last time we wrote an autosave snapshot to the .stz file.
    pub last_stz_autosave: std::time::Instant,
    /// Receiver from a background bounce-in-place / freeze render thread.
    pub bounce_done_rx: Option<flume::Receiver<Result<(), String>>>,
    /// Pending bounce target: (row, col_filter, output_path) — applied when render completes.
    pub bounce_pending_row: Option<(usize, Option<usize>, std::path::PathBuf)>,
    /// When Some, the bounce completion should freeze (not just bounce) this row.
    pub freeze_pending_row: Option<usize>,
    /// In-memory .stz container for the current session (loaded on open / saved on Ctrl+S).
    pub stz_container: Option<seqterm_stz::StzContainer>,
    /// Snapshot name buffer (used while naming a new snapshot).
    pub snapshot_name_editing: bool,
    pub snapshot_name_buffer: String,
    /// Live oscilloscope waveform for the matrix-selected audio slot (WAVE_LEN samples).
    pub live_waveform: Vec<f32>,
    /// Number of active SF2 voices tracked via AudioNoteOn/AudioNoteOff events.
    pub active_voices: usize,
    /// Active SF2 voices set: (slot_id, channel, note) — cleared on NoteOff.
    pub active_voice_set: std::collections::HashSet<(u32, u8, u8)>,
    /// Which tab is shown in the matrix right sidebar: 0=POLY  1=ROUTE  2=HYBRID.
    pub sidebar_tab: u8,
    /// Hit-test rects for the 2 sidebar tab labels (set every draw frame).
    pub sidebar_tab_rects: std::cell::Cell<[ratatui::layout::Rect; 2]>,
    /// Matrix ACTIONS buttons: which is selected (0=CLIP, 1=CHANGE SOURCE,
    /// 2=CHANGE BANK/PRESET, 3=EDIT) when the SOURCE panel is focused.
    pub matrix_action_cursor: usize,
    /// Hit-test rects for the 4 SOURCE action buttons (set every draw frame).
    pub matrix_action_btn_rects: std::cell::Cell<[ratatui::layout::Rect; 4]>,
    /// Hit-test rects for the SOURCE MIDI-channel stepper: [0]=◂ (down), [1]=▸ (up).
    /// Empty when the current source has no MIDI channel (e.g. AudioFile).
    pub source_chan_rects: std::cell::Cell<[ratatui::layout::Rect; 2]>,
    /// Inner rect of the Hybrid "ACTIVE PATTERNS" section (set every draw frame).
    pub hv_patterns_inner: std::cell::Cell<ratatui::layout::Rect>,
    /// Inner rect of the Hybrid "TRACKER MONITOR" section (set every draw frame).
    pub hv_monitor_inner: std::cell::Cell<ratatui::layout::Rect>,
    /// First step index visible in the tracker monitor (set every draw frame).
    pub hv_monitor_start_step: std::cell::Cell<usize>,
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
    /// Cursor position in the drum matrix panel (matrix_section == 4).
    /// (pad_row 0-15, step_col 0..pattern_len-1).
    pub drum_cursor: (usize, usize),
    /// Step scroll offset in the drum matrix (first visible step column).
    pub drum_step_scroll: usize,
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
    /// Last matrix cell clicked and when — used for double-click detection.
    /// Format: ((row, col), Instant)
    pub last_matrix_click: Option<((usize, usize), std::time::Instant)>,

    /// Last click time on the tracker FX-chain panel — used to detect a
    /// double-click that opens the FX / plugin picker.
    pub last_fx_panel_click: Option<std::time::Instant>,

    // ── Frame counter (wrapping) — used for spinner animation etc. ───────────
    pub frame_count: u64,
    /// When `Some`, `status_msg` reverts to a blank hint after this instant.
    pub status_expires: Option<std::time::Instant>,

    // ── MIDI port monitoring ──────────────────────────────────────────────────
    pub midi_port_rx: flume::Receiver<Vec<String>>,
    pub unavailable_midi_routes: HashSet<String>,
    /// Live MIDI input bus — receives messages from all enabled input ports.
    pub midi_input_bus: seqterm_midi::MidiInputBus,

    // ── MIDI clock sync ───────────────────────────────────────────────────────
    /// When true, incoming MIDI Clock pulses control the sequencer BPM.
    pub midi_clock_sync: bool,
    /// Timestamp of the last received MIDI Clock pulse (0xF8).
    pub midi_clock_last_pulse: Option<std::time::Instant>,
    /// Ring of recent inter-pulse intervals (microseconds); filled up to 24 entries.
    pub midi_clock_intervals: Vec<u64>,

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
    pub waveform_tx: flume::Sender<(PathBuf, Vec<f32>)>,

    // ── SF2 preset background scan ────────────────────────────────────────────
    pub sf2_presets_rx: Option<flume::Receiver<Vec<(u8, u8, String)>>>,

    // ── MIDI import background task ───────────────────────────────────────────
    pub midi_import_rx: Option<flume::Receiver<Result<seqterm_midi_io::ImportedMidi, String>>>,
    /// Saved MIDI import options state while SF2 file picker is open.
    pub pending_midi_import: Option<(std::path::PathBuf, seqterm_midi_io::MidiImportOptions)>,

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
    /// True while live microphone input is being mixed into the master output.
    pub input_monitor_active: bool,
    /// Gain applied to the live input monitor (0.0=mute, 1.0=unity, 2.0=+6dB).
    pub input_monitor_gain: f32,
    /// True while the live input is being recorded to a WAV file.
    pub input_recording: bool,

    // ── Overdub recording ──────────────────────────────────────────────────────
    /// The matrix row (0-based) currently being overdubbed, if any.
    pub overdub_row: Option<usize>,
    /// Target column for the overdub clip.
    pub overdub_col: usize,
    /// BPM at overdub start — used for bar-grid quantisation.
    pub overdub_bpm: f64,
    /// When Some, the next `ConfirmAudioFileAssignment` applies this playback-end
    /// fraction (bar-quantised clip trim). Set by the `InputRecordStopped` handler.
    pub overdub_quantise_end_frac: Option<f32>,

    // ── Lua scripting ─────────────────────────────────────────────────────────
    pub lua: seqterm_lua::LuaEngine,

    // ── Render cache ───────────────────────────────────────────────────────────
    /// Set to true whenever state changes that requires a redraw.
    /// The render loop skips `terminal.draw()` when false and no meter tick is due.
    pub dirty: bool,
    /// Timestamp of the last frame rendered (for meter refresh even when not dirty).
    pub last_render: std::time::Instant,

    // ── Pending commands (queued inside process_events, drained in event loop) ──
    pub pending_commands: Vec<seqterm_command::AppCommand>,

    // ── Audio export background task ──────────────────────────────────────────
    pub audio_export_rx: Option<flume::Receiver<AudioExportMsg>>,

    // ── Plugin registry ───────────────────────────────────────────────────────
    pub plugin_registry: seqterm_application::PluginRegistry,
    /// Whether the plugin registry has been scanned at least once. Scanning is
    /// slow (dlopens every candidate library), so we do it lazily on first need
    /// and only re-scan on explicit user request (AUDIO SETTINGS → Rescan).
    pub plugins_scanned: bool,
    /// In-flight background plugin scan: receives the fully-scanned registry from
    /// a worker thread so the UI never blocks on the (potentially multi-second)
    /// filesystem walk. `Some` while a scan is running; swapped in on completion.
    pub plugin_scan_rx: Option<flume::Receiver<seqterm_application::PluginRegistry>>,
    /// Synthesizer-source plugin instances, keyed by matrix clip key ("A0").
    /// Maps to the registry instance id used for parameter (knob) access.
    pub synth_instances: std::collections::HashMap<String, u64>,
    /// SOURCE tab: cursor over a synth source's parameter knobs.
    pub source_knob_cursor: usize,
    /// SOURCE tab: whether keyboard focus is on the synth knobs (vs action buttons).
    pub source_focus_knobs: bool,
    /// SOURCE tab: per-knob click/scroll rects (set each render frame, max 8 shown).
    pub source_knob_rects: std::cell::Cell<[ratatui::layout::Rect; 8]>,

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
    /// Absolute Y of the "Change Source" button row in the routing panel (set each frame).
    pub routing_source_btn_y: std::cell::Cell<u16>,
    /// Cell size (cell_w, cell_h) of the matrix grid, set each frame by draw_clip_grid.
    pub matrix_cell_size: std::cell::Cell<(usize, usize)>,
    /// Matrix cell currently under the mouse pointer, or None.
    pub hovered_matrix_cell: std::cell::Cell<Option<(usize, usize)>>,

    // ── Panel hit-test rects (set each frame during draw, used for mouse hover) ─
    /// Bounding rects of the 4 matrix subsections: [grid, transport, polymeter, routing].
    pub matrix_panel_rects:  std::cell::Cell<[ratatui::layout::Rect; 4]>,
    /// Bounding rects of the 7 tracker subsections:
    /// [step_table, piano_roll, generative, modulation, fx_chain, source, transport].
    pub tracker_panel_rects: std::cell::Cell<[ratatui::layout::Rect; 7]>,
    /// Tracker TRANSPORT subsection: true while the current pattern is playing in
    /// isolation (solo). Plus the hit-test rect for its play/stop button.
    pub pattern_solo_playing: bool,
    pub tracker_transport_btn_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// PATTERN → TRANSPORT: selected button cursor (0=play 1=stop 2=rwd 3=rec
    /// 4=quantize) and the hit-test rects for each of the 5 buttons.
    pub tracker_transport_cursor: usize,
    pub tracker_transport_btn_rects: std::cell::Cell<[ratatui::layout::Rect; 5]>,
    /// PATTERN: which panel is shown in the tabbed area below the piano roll.
    /// Display order: 0=SOURCE, 1=TRACK MODULATION, 2=FX CHAIN, 3=GENERATIVE.
    pub tracker_tab: usize,
    /// Hit-test rects for the 4 tab headers (set every draw frame).
    pub tracker_tab_rects: std::cell::Cell<[ratatui::layout::Rect; 4]>,
    /// FX CHAIN mouse hit-test rects (set every draw frame): per-parameter knob
    /// areas (wheel adjusts), the FX-slot boxes (up to 5), the +ADD box, the
    /// ON/OFF, DELETE and MOVE◀/MOVE▶ (routing-order) buttons.
    pub tracker_fx_param_rects: std::cell::Cell<[ratatui::layout::Rect; 8]>,
    pub tracker_fx_slot_rects: std::cell::Cell<[ratatui::layout::Rect; 5]>,
    pub tracker_fx_add_rect: std::cell::Cell<ratatui::layout::Rect>,
    pub tracker_fx_enable_rect: std::cell::Cell<ratatui::layout::Rect>,
    pub tracker_fx_delete_rect: std::cell::Cell<ratatui::layout::Rect>,
    pub tracker_fx_move_prev_rect: std::cell::Cell<ratatui::layout::Rect>,
    pub tracker_fx_move_next_rect: std::cell::Cell<ratatui::layout::Rect>,
    /// Saved clip-enabled states to restore when isolated play stops:
    /// (row_key, col, was_enabled).
    pub pattern_solo_saved: Vec<(String, usize, bool)>,
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
        let (retrigger_tx, retrigger_rx) = flume::unbounded::<u32>();

        let mut app = Self {
            project,
            current_view: ViewKind::Matrix,
            focus: FocusId::MatrixGrid,
            engine,
            should_quit: false,
            playing: false,
            paused:  false,
            recording: false,
            song_playing: false,
            song_bar: 0,
            chain_mode: false,
            chain_pos: 0,
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
            granular_scene_slot: 0,
            granular_mod: seqterm_core::GranularMod::default(),
            granular_mod_cursor: 0,
            granular_macros: [0.5; 4],
            granular_macro_cursor: 0,
            granular_morph: None,
            granular_live_source: None,
            retrigger_tx,
            retrigger_rx,
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
            tracker_fx_slot:       0,
            tracker_fx_param:      0,
            tracker_fx_midi_learn: None,
            piano_roll_area: std::cell::Cell::new(ratatui::layout::Rect::default()),
            piano_vel_area:  std::cell::Cell::new(ratatui::layout::Rect::default()),
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
            file_picker_sidebar_area: std::cell::Cell::new(ratatui::layout::Rect::default()),
            confirm_yes_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            confirm_no_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            quit_save_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            quit_nosave_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            quit_cancel_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            modal_ok_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            modal_cancel_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            midi_settings_tab_rects: std::cell::Cell::new(
                [ratatui::layout::Rect::default(); 4]
            ),
            midi_settings_list_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            audio_settings_tab_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 3]),
            audio_engine_row_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 5]),
            audio_pp_fmt_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 9]),
            audio_pp_dir_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            audio_pp_action_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 3]),
            audio_osc_row_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 4]),
            sf2_bank_left_rect:  std::cell::Cell::new(ratatui::layout::Rect::default()),
            sf2_bank_right_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            sf2_a3_btn_rect:     std::cell::Cell::new(ratatui::layout::Rect::default()),
            sf2_list_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            sf2_reopen_btn_y: std::cell::Cell::new(0),

            vim_mode: VimMode::Normal,
            visual_start: None,
            vim_yank_buffer: Vec::new(),
            arranger_track_name_editing: false,
            arranger_track_name_buffer: String::new(),

            last_mouse_pos: (0, 0),
            mouse_drag: false,
            note_click_start: None,
            last_matrix_click: None,
            last_fx_panel_click: None,

            matrix_section: 0,
            transport_cursor: 0,
            matrix_rows: 8,
            matrix_cols: 8,
            matrix_col_scroll: std::cell::Cell::new(0),
            tap_times: Vec::new(),
            jack_available: false,
            audio_engine: None,
            audio_slots: std::collections::HashMap::new(),
            sf2_slots:   std::collections::HashSet::new(),
            audio_slot_volumes: std::collections::HashMap::new(),
            audio_slot_channel_vol: std::collections::HashMap::new(),
            audio_slot_fx: std::collections::HashMap::new(),
            master_fx:     Vec::new(),
            master_volume: 1.0,
            audio_engine_running: false,
            audio_sample_rate: 48000,
            audio_buffer_size: 256,
            audio_dsp_load: 0.0,
            audio_xrun_count: 0,
            audio_slot_peaks: vec![0.0; seqterm_audio_engine::mixer::MAX_SLOTS],
            audio_master_peak: [0.0; 2],
            audio_slot_rms: vec![0.0; seqterm_audio_engine::mixer::MAX_SLOTS],
            audio_master_rms: [0.0; 2],
            audio_slot_clip: vec![false; seqterm_audio_engine::mixer::MAX_SLOTS],
            master_clip: [false; 2],
            master_correlation: 0.0,
            master_lufs: (-f32::INFINITY, -f32::INFINITY, -f32::INFINITY),
            master_spectrum: vec![0.0; seqterm_audio_engine::spectrum::SPECTRUM_BANDS],
            stz_path: None,
            stz_container: None,
            last_stz_autosave: std::time::Instant::now(),
            bounce_done_rx: None,
            bounce_pending_row: None,
            freeze_pending_row: None,
            snapshot_name_editing: false,
            snapshot_name_buffer: String::new(),
            live_waveform: Vec::new(),
            active_voices: 0,
            active_voice_set: std::collections::HashSet::new(),
            sidebar_tab: 0,
            sidebar_tab_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 2]),
            matrix_action_cursor: 0,
            matrix_action_btn_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 4]),
            source_chan_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 2]),
            hv_patterns_inner: std::cell::Cell::new(ratatui::layout::Rect::default()),
            hv_monitor_inner: std::cell::Cell::new(ratatui::layout::Rect::default()),
            hv_monitor_start_step: std::cell::Cell::new(0),
            last_midi_refresh: None,
            polymeter_cursor: 0,
            polymeter_pat_scroll: 0,
            polymeter_step_start: 0,
            routing_cursor: 0,
            routing_tab: 0,
            routing_source_cursor: 0,
            drum_cursor: (0, 0),
            drum_step_scroll: 0,

            frame_count: 0,
            status_expires: None,

            midi_port_rx,
            unavailable_midi_routes: HashSet::new(),
            midi_input_bus: seqterm_midi::MidiInputBus::new(),
            midi_clock_sync: false,
            midi_clock_last_pulse: None,
            midi_clock_intervals: Vec::with_capacity(24),

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
            pending_midi_import: None,
            capturing: false,
            capture_path: None,
            input_monitor_active: false,
            input_monitor_gain: 1.0,
            input_recording: false,
            overdub_row: None,
            overdub_col: 0,
            overdub_bpm: 128.0,
            overdub_quantise_end_frac: None,
            lua: seqterm_lua::LuaEngine::default(),
            dirty: true,
            last_render: std::time::Instant::now(),
            pending_commands: Vec::new(),
            audio_export_rx: None,
            settings: seqterm_persistence::load_settings(),

            // Registry pre-loaded with every plugin-host adapter compiled in
            // (VST2 by default; VST3/CLAP when their features are enabled).
            plugin_registry: seqterm_application::PluginRegistry::with_default_adapters(48_000, 512),
            plugins_scanned: false,
            plugin_scan_rx: None,
            synth_instances: std::collections::HashMap::new(),
            source_knob_cursor: 0,
            source_focus_knobs: false,
            source_knob_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 8]),
            midi_learn: None,
            audio_export_opts: AudioExportOpts::default(),

            tabs: Vec::new(),
            active_tab: 0,

            routing_state: RoutingState::default(),
            hovered_transport_btn: None,
            routing_list_item_y: std::cell::Cell::new(0),
            routing_channel_y: std::cell::Cell::new(0),
            routing_source_btn_y: std::cell::Cell::new(0),
            matrix_cell_size: std::cell::Cell::new((0, 0)),
            hovered_matrix_cell: std::cell::Cell::new(None),
            matrix_panel_rects:  std::cell::Cell::new([ratatui::layout::Rect::default(); 4]),
            tracker_panel_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 7]),
            pattern_solo_playing: false,
            tracker_transport_btn_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            tracker_transport_cursor: 0,
            tracker_transport_btn_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 5]),
            tracker_tab: 0,
            tracker_tab_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 4]),
            tracker_fx_param_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 8]),
            tracker_fx_slot_rects: std::cell::Cell::new([ratatui::layout::Rect::default(); 5]),
            tracker_fx_add_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            tracker_fx_enable_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            tracker_fx_delete_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            tracker_fx_move_prev_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            tracker_fx_move_next_rect: std::cell::Cell::new(ratatui::layout::Rect::default()),
            pattern_solo_saved: Vec::new(),
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

    /// Advance any active granular morph by one frame step.
    pub fn tick_granular_morph(&mut self) {
        let done = if let Some(morph) = &mut self.granular_morph {
            morph.progress = (morph.progress + morph.step).min(1.0);
            let t = morph.progress;
            // Linear interpolation of GrainParams scalar fields.
            let fp = &morph.from.params;
            let tp = &morph.to.params;
            let p = &mut self.granular_state.params;
            p.size_ms       = fp.size_ms       + t * (tp.size_ms       - fp.size_ms);
            p.density       = fp.density       + t * (tp.density       - fp.density);
            p.spray         = fp.spray         + t * (tp.spray         - fp.spray);
            p.overlap       = fp.overlap       + t * (tp.overlap       - fp.overlap);
            p.pitch_st      = fp.pitch_st      + t * (tp.pitch_st      - fp.pitch_st);
            p.pan           = fp.pan           + t * (tp.pan           - fp.pan);
            p.gain          = fp.gain          + t * (tp.gain          - fp.gain);
            p.jitter        = fp.jitter        + t * (tp.jitter        - fp.jitter);
            p.stereo_spread = fp.stereo_spread + t * (tp.stereo_spread - fp.stereo_spread);
            // Zone.
            let fz = &morph.from.zone;
            let tz = &morph.to.zone;
            let z = &mut self.granular_state.zone;
            z.position   = fz.position   + t * (tz.position   - fz.position);
            z.range      = fz.range      + t * (tz.range      - fz.range);
            z.scan_speed = fz.scan_speed + t * (tz.scan_speed - fz.scan_speed);
            // Snap discrete fields at t >= 0.5.
            if t >= 0.5 {
                p.direction = tp.direction;
                p.envelope  = tp.envelope;
                z.scan_mode = tz.scan_mode;
            }
            // Push live update.
            if let Some((bank, pad)) = self.granular_state.pad {
                if let Some(&slot_id) = self.sampler_slots.get(&(bank, pad)) {
                    if let Some(ae) = self.audio_engine.as_mut() {
                        ae.send(seqterm_audio_engine::AudioCommand::SetGranularParams {
                            slot_id, params: self.granular_state.params.clone(),
                        });
                        ae.send(seqterm_audio_engine::AudioCommand::SetGranularZone {
                            slot_id, zone: self.granular_state.zone.clone(),
                        });
                    }
                }
            }
            morph.progress >= 1.0
        } else { false };
        if done { self.granular_morph = None; }
    }

    /// Drain engine events and update mirrored state.
    pub fn process_events(&mut self) {
        self.frame_count = self.frame_count.wrapping_add(1);
        self.tick_granular_morph();
        // Always dirty on a new event frame (meters, transport).
        self.dirty = true;

        // Drain retrigger events from background threads.
        while let Ok(slot_id) = self.retrigger_rx.try_recv() {
            if let Some(ae) = &mut self.audio_engine {
                ae.send(seqterm_audio_engine::AudioCommand::PlayAudioClip { slot_id });
            }
        }

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
        // Throttled to every 8 frames (~130 ms at 60 fps) to avoid a
        // per-frame project mutex lock that can stall cursor navigation.
        if self.frame_count % 8 == 0 {
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
                    let (want_bank, want_preset) = (state.bank, state.preset);
                    state.set_presets(presets);
                    // Restore bank/preset cursor to the pre-selected values (ReopenSf2Browser).
                    if want_bank != 0 || want_preset != 0 {
                        if let Some(idx) = state.banks.iter().position(|&b| b == want_bank) {
                            state.bank_cursor = idx;
                            state.bank = want_bank;
                            // Find matching preset in that bank.
                            let fp: Vec<_> = state.presets.iter()
                                .filter(|(b, _, _)| *b == want_bank)
                                .collect();
                            if let Some(pi) = fp.iter().position(|(_, p, _)| *p == want_preset) {
                                state.cursor = pi;
                            }
                        }
                    }
                }
            }
        }

        // Poll background plugin scan; swap in the fully-scanned registry when
        // ready so the UI never blocks on the filesystem walk.
        if let Some(rx) = &self.plugin_scan_rx {
            if let Ok(reg) = rx.try_recv() {
                self.plugin_registry = reg;
                self.plugins_scanned = true;
                self.plugin_scan_rx = None;
                // If the project uses plugin synth sources, wire them up now that
                // the registry knows the plugins (no-op otherwise).
                let has_plugin_src = {
                    let proj = self.project.lock();
                    proj.matrix.values().flatten().flatten().any(|c| {
                        matches!(c.source, seqterm_core::PatternSource::Plugin { .. })
                    })
                };
                if has_plugin_src {
                    crate::rebuild_audio_slots(self);
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
                        // Fire Lua on_step hook.
                        let lua_cmds = self.lua.call_on_step(step as u32, self.bpm);
                        self.pending_commands.extend(lua_cmds);
                        // Fire Lua on_bar hook when step wraps to 0 within the pattern.
                        if pat_step == 0 {
                            let bar_cmds = self.lua.call_on_bar(self.current_bar as u32, self.bpm);
                            self.pending_commands.extend(bar_cmds);
                        }
                    }
                }
                EngineEvent::XRun => {
                    xrun_delta += 1;
                    self.status_msg = "! XRUN detected !".to_string();
                }
                EngineEvent::MidiCc { ch, cc, val } => {
                    // Tracker FX MIDI learn: bind CC to the selected parameter.
                    if let Some((fx_slot, fx_param)) = self.tracker_fx_midi_learn.take() {
                        let slot_id = self.tracker_current_slot_id();
                        if let Some(sid) = slot_id {
                            if let Some(chain) = self.audio_slot_fx.get_mut(&sid) {
                                if let Some(entry) = chain.get_mut(fx_slot) {
                                    if let Some(bind) = entry.cc_bindings.get_mut(fx_param) {
                                        *bind = Some(cc);
                                    }
                                }
                            }
                        }
                        self.status_msg = format!("Bound CC{cc} → FX slot {} param {}", fx_slot + 1, fx_param + 1);
                    } else if let Some(target) = self.midi_learn.take() {
                        use seqterm_persistence::MidiLearnBinding;
                        let binding = MidiLearnBinding::new(target.clone(), ch, cc);
                        self.settings.midi_learn_bindings.retain(|b| b.target != target);
                        self.settings.midi_learn_bindings.push(binding);
                        let _ = seqterm_persistence::save_settings(&self.settings);
                        self.status_msg = format!("Bound: CC{cc} (ch{}) → {}", ch + 1, target.label());
                    } else {
                        // Live CC → FX param for any slot that has CC bindings.
                        let slot_id_live = self.tracker_current_slot_id();
                        if let Some(sid) = slot_id_live {
                            let norm = val as f32 / 127.0;
                            // Iterate a copy to avoid borrow conflict.
                            let chain_snapshot: Vec<(usize, usize, Option<u8>)> =
                                self.audio_slot_fx.get(&sid)
                                    .map(|ch| ch.iter().enumerate().flat_map(|(si, e)| {
                                        e.cc_bindings.iter().enumerate()
                                            .map(move |(pi, &b)| (si, pi, b))
                                    }).collect())
                                    .unwrap_or_default();
                            let mut changed = false;
                            for (si, pi, bind) in chain_snapshot {
                                if bind == Some(cc) {
                                    if let Some(chain) = self.audio_slot_fx.get_mut(&sid) {
                                        if let Some(entry) = chain.get_mut(si) {
                                            if let Some(v) = entry.params.get_mut(pi) {
                                                *v = norm; changed = true;
                                                entry.sync_wet();
                                            }
                                        }
                                    }
                                }
                            }
                            if changed { self.rebuild_audio_fx_chain(sid); }
                        }

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
                                seqterm_persistence::MidiLearnTarget::ChannelPan(i) => {
                                    let mut proj = self.project.lock();
                                    if let Some(ch_strip) = proj.channels.get_mut(*i) {
                                        // CC 0..127 → pan -50..=50 (L50 … CENTER … R50).
                                        let v = (val as i32 * 100 / 127 - 50).clamp(-50, 50) as i8;
                                        ch_strip.pan = seqterm_core::channel::Pan::from_val(v);
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
                EngineEvent::AudioControlChange { slot_id, channel, cc, value } => {
                    if let Some(ae) = &mut self.audio_engine {
                        if cc == 0xFE {
                            // Sentinel: program change (value = program number).
                            ae.send(seqterm_audio_engine::AudioCommand::ProgramChange {
                                slot_id, channel, program: value,
                            });
                        } else {
                            ae.send(seqterm_audio_engine::AudioCommand::ControlChange {
                                slot_id, channel, cc, value,
                            });
                        }
                    }
                }
                EngineEvent::AudioNoteOn { slot_id, channel, note, velocity } => {
                    tracing::debug!("AudioNoteOn: slot={} ch={} note={} vel={}", slot_id, channel, note, velocity);
                    self.active_voice_set.insert((slot_id, channel, note));
                    self.active_voices = self.active_voice_set.len();
                    if let Some(ae) = &mut self.audio_engine {
                        ae.send(seqterm_audio_engine::AudioCommand::NoteOn {
                            slot_id, channel, note, velocity,
                        });
                    } else {
                        tracing::warn!("AudioNoteOn: audio_engine is None, note dropped!");
                    }
                }
                EngineEvent::AudioNoteOff { slot_id, channel, note } => {
                    self.active_voice_set.remove(&(slot_id, channel, note));
                    self.active_voices = self.active_voice_set.len();
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
                EngineEvent::ChainAdvanced { chain_pos, scene_idx } => {
                    self.chain_pos = chain_pos;
                    // Apply mute_mask from the activated scene.
                    let mute_mask = {
                        let proj = self.project.lock();
                        proj.scenes.get(scene_idx).map(|s| s.mute_mask).unwrap_or(0)
                    };
                    // Apply scene mute mask to channels (bit N = row N muted).
                    {
                        let mut proj = self.project.lock();
                        for (i, ch) in proj.channels.iter_mut().enumerate() {
                            ch.mute = (mute_mask >> i) & 1 == 1;
                        }
                    }
                }
                EngineEvent::AudioFxParam { slot_id, fx_idx, param_idx, value } => {
                    if let Some(ae) = &mut self.audio_engine {
                        ae.send(seqterm_audio_engine::AudioCommand::SetSlotFxParam {
                            slot_id, fx_idx, param_idx, value,
                        });
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
        let (audio_evs, dsp_load, slot_peaks, master_peak, slot_rms, master_rms, correlation, lufs, spectrum) =
            if let Some(ae) = &mut self.audio_engine {
                (ae.drain_events(), ae.dsp_load(),
                 ae.slot_peak_levels(), ae.master_peak_level(),
                 ae.slot_rms_levels(), ae.master_rms_levels(),
                 ae.master_correlation(), ae.master_lufs(),
                 ae.spectrum_bands())
            } else {
                (vec![], 0.0, vec![], 0.0, vec![], [0.0f32; 2], 0.0,
                 (-f32::INFINITY, -f32::INFINITY, -f32::INFINITY),
                 vec![0.0; seqterm_audio_engine::spectrum::SPECTRUM_BANDS])
            };
        for ev in audio_evs {
            use seqterm_audio_engine::AudioEngineEvent;
            match ev {
                AudioEngineEvent::StreamStarted { sample_rate, buffer_size } => {
                    self.audio_engine_running = true;
                    self.audio_sample_rate   = sample_rate;
                    self.audio_buffer_size   = buffer_size;
                    // Update lookahead using the actual buffer/sample-rate from JACK/CPAL.
                    self.engine.set_audio_latency(buffer_size, sample_rate);
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
                            // Audition at note A3 (57).
                            ae.send(seqterm_audio_engine::AudioCommand::NoteOn {
                                slot_id, channel: 0, note: 57, velocity: 100,
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
                AudioEngineEvent::InputStreamStarted { .. } => {
                    self.input_monitor_active = true;
                    self.set_timed_status("Input monitor ON — mic routed to master", 3);
                }
                AudioEngineEvent::InputStreamStopped => {
                    self.input_monitor_active = false;
                    self.input_recording = false;
                    self.set_timed_status("Input monitor OFF", 2);
                }
                AudioEngineEvent::InputRecordStopped { path, duration_secs } => {
                    self.input_recording = false;
                    if let Some(row) = self.overdub_row.take() {
                        let col = self.overdub_col;
                        // Bar-grid quantisation: snap end to nearest bar.
                        let secs_per_bar = 240.0 / self.overdub_bpm; // 4/4 assumed
                        let bars = (duration_secs / secs_per_bar).round().max(1.0);
                        let snapped_secs = bars * secs_per_bar;
                        if snapped_secs < duration_secs {
                            self.overdub_quantise_end_frac = Some((snapped_secs / duration_secs) as f32);
                        }
                        let row_label = (b'A' + row as u8) as char;
                        self.set_timed_status(
                            format!("Overdub → {}{} ({:.1}s → {} bars)", row_label, col + 1, duration_secs, bars as u32),
                            6,
                        );
                        self.pending_commands.push(seqterm_command::AppCommand::ConfirmAudioFileAssignment {
                            row, col, path,
                        });
                    } else {
                        let name = path.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "recording".into());
                        self.set_timed_status(
                            format!("Input recorded: {name}  ({duration_secs:.1}s)"), 8,
                        );
                    }
                }
                AudioEngineEvent::InputRecordFailed(e) => {
                    self.input_recording = false;
                    self.set_timed_status(format!("Input record failed: {e}"), 8);
                }
                AudioEngineEvent::InputDevicesListed(_) => {}
            }
        }
        if self.audio_engine.is_some() {
            self.audio_dsp_load = dsp_load;
            if !slot_peaks.is_empty() {
                for (i, &p) in slot_peaks.iter().enumerate() {
                    if p >= 1.0 {
                        if let Some(c) = self.audio_slot_clip.get_mut(i) { *c = true; }
                    }
                }
                self.audio_slot_peaks = slot_peaks;
            }
            if !slot_rms.is_empty() {
                self.audio_slot_rms = slot_rms;
            }
            self.audio_master_peak[0] = master_peak;
            self.audio_master_peak[1] = master_peak;
            if master_peak >= 1.0 {
                self.master_clip[0] = true;
                self.master_clip[1] = true;
            }
            self.audio_master_rms  = master_rms;
            self.master_correlation = correlation;
            self.master_lufs        = lufs;
            self.master_spectrum    = spectrum;
        }

        // Live oscilloscope: capture the audio slot for the currently selected matrix cell.
        if let Some(ae) = &self.audio_engine {
            let wf_slot = self.tracker_current_slot_id();
            ae.set_waveform_slot(wf_slot);
            if wf_slot.is_some() {
                let samples = ae.waveform_samples();
                if !samples.is_empty() {
                    self.live_waveform = samples;
                }
            } else {
                self.live_waveform.clear();
            }
        }

        // Sync transport-adjacent project fields in a single lock per frame.
        {
            let mut proj = self.project.lock();
            proj.bpm         = self.bpm;
            proj.current_bar = self.current_bar as u32;
            proj.xrun        += xrun_delta;
        }

        // Poll bounce-in-place / freeze render completion.
        if let Some(rx) = &self.bounce_done_rx {
            if let Ok(result) = rx.try_recv() {
                self.bounce_done_rx = None;
                let freeze_row = self.freeze_pending_row.take();
                if let Some((row, col_filter, wav_path)) = self.bounce_pending_row.take() {
                    match result {
                        Ok(()) => {
                            let row_key = ((b'A' + row as u8) as char).to_string();
                            let audio_source = seqterm_core::PatternSource::AudioFile {
                                path: wav_path.clone(),
                                looping: true,
                                original_bpm: 0.0,
                                gain: 1.0,
                            };
                            {
                                let mut proj = self.project.lock();
                                if let Some(slots) = proj.matrix.get_mut(&row_key) {
                                    for (col_idx, slot) in slots.iter_mut().enumerate() {
                                        if col_filter.map(|c| c != col_idx).unwrap_or(false) {
                                            continue;
                                        }
                                        if let Some(clip) = slot.as_mut() {
                                            if freeze_row.is_some() {
                                                // For freeze: store original source, mark frozen.
                                                clip.freeze_source = Some(Box::new(clip.source.clone()));
                                                clip.frozen = true;
                                            }
                                            clip.source = audio_source.clone();
                                        }
                                    }
                                }
                                // Mark the channel as frozen if this was a freeze op.
                                if freeze_row.is_some() {
                                    if let Some(ch) = proj.channels.iter_mut()
                                        .find(|c| c.midi_port.as_deref() == Some(row_key.as_str()))
                                    {
                                        ch.frozen = true;
                                    }
                                }
                            }
                            self.project_dirty = true;
                            let verb = if freeze_row.is_some() { "Frozen" } else { "Bounce complete" };
                            self.set_timed_status(
                                format!("{} → {}", verb, wav_path.display()), 4);
                        }
                        Err(e) => {
                            let verb = if freeze_row.is_some() { "Freeze" } else { "Bounce" };
                            self.set_timed_status(format!("{} failed: {e}", verb), 5);
                        }
                    }
                }
            }
        }

        // Autosave snapshot to .stz every 60 seconds when a .stz path is set.
        const STZ_AUTOSAVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
        if self.stz_path.is_some()
            && self.project_dirty
            && self.last_stz_autosave.elapsed() >= STZ_AUTOSAVE_INTERVAL
        {
            self.write_stz_autosave();
        }
    }

    /// Write an "autosave" snapshot to the active .stz container + file.
    pub fn write_stz_autosave(&mut self) {
        let Some(ref path) = self.stz_path.clone() else { return };

        let proj_json = match {
            let proj = self.project.lock();
            serde_json::to_vec(&*proj)
        } {
            Ok(v) => v,
            Err(_) => return,
        };

        if self.stz_container.is_none() {
            let name = { self.project.lock().name.clone() };
            self.stz_container = Some(seqterm_stz::StzContainer::new(name, self.bpm));
        }

        if let Some(container) = self.stz_container.as_mut() {
            // Save plugin states into the container.
            let states = self.plugin_registry.collect_states();
            for (_reg_id, plugin_id, data) in states {
                container.set_plugin_state(&plugin_id, data);
            }

            container.take_snapshot("autosave".to_string(), proj_json);
            if seqterm_stz::save(container, path).is_ok() {
                self.last_stz_autosave = std::time::Instant::now();
            }
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
        self.focus = FocusId::default_for(view);
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
            // While playing → Pause (preserves position).
            self.engine.pause();
            self.silence_all_audio();
            self.playing = false;
            self.paused  = true;
            self.project.lock().playing = false;
            // Finalise any active overdub on pause.
            self.stop_overdub_if_active();
            self.status_msg = "Paused".to_string();
            announce_status("Playback paused");
        } else {
            // While paused or stopped → Play (resumes from current position).
            self.engine.play();
            self.playing = true;
            self.paused  = false;
            self.project.lock().playing = true;
            // Start overdub recording for any armed channels.
            self.maybe_start_overdub();
            if self.overdub_row.is_none() {
                self.status_msg = "Playing".to_string();
                announce_status(&format!("Playback started — {:.0} BPM", self.bpm));
            }
        }
    }

    pub fn stop(&mut self) {
        // Finalise any active overdub before stopping transport.
        self.stop_overdub_if_active();
        self.engine.stop();
        self.silence_all_audio();
        self.playing = false;
        self.paused  = false;
        self.current_step = 0;
        self.project.lock().playing = false;
        self.status_msg = "Stopped".to_string();
        announce_status("Playback stopped");
    }

    /// Start overdub recording if any channel is record-armed and audio engine is running.
    fn maybe_start_overdub(&mut self) {
        let armed_row = self.project.lock()
            .channels.iter().position(|c| c.record_arm);
        if let Some(row) = armed_row {
            if !self.audio_engine_running { return; }
            let col = self.matrix_state.cursor.1;
            self.overdub_row = Some(row);
            self.overdub_col = col;
            self.overdub_bpm = self.bpm;
            if !self.input_monitor_active {
                if let Some(ae) = &mut self.audio_engine {
                    ae.start_input_monitor(self.input_monitor_gain);
                }
            }
            let base = self.project_path.as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let path = base.join(format!("seqterm_overdub_{ts}.wav"));
            if let Some(ae) = &mut self.audio_engine {
                ae.start_input_record(path);
                self.input_recording = true;
            }
            self.status_msg = format!(
                "● OVERDUB  {}{}  (Space/Stop to finish)",
                (b'A' + row as u8) as char, col + 1,
            );
            announce_status(&format!("Overdub recording — row {}", (b'A' + row as u8) as char));
        }
    }

    /// Stop any active overdub recording (called on pause/stop).
    fn stop_overdub_if_active(&mut self) {
        if self.input_recording && self.overdub_row.is_some() {
            if let Some(ae) = &mut self.audio_engine {
                ae.stop_input_record();
            }
            // overdub_row is cleared when InputRecordStopped event arrives.
        }
    }

    /// Rewind to bar 0 / step 0 and silence audio. Does not change play/pause state.
    pub fn rewind(&mut self) {
        self.engine.rewind();
        self.silence_all_audio();
        self.current_step = 0;
        self.current_bar  = 0;
        self.status_msg = if self.playing { "Rewound — playing".to_string() }
                          else           { "Rewound".to_string() };
    }

    /// Silence all audio on transport Stop.
    ///
    /// SF2 slots receive AllNotesOff only — StopAudioClip must NOT be sent
    /// because it calls clear_slot() → slot.active = false, which permanently
    /// silences the slot until the next NoteOn re-activates it (race condition).
    ///
    /// AudioClip slots receive StopAudioClip (fade-out) in addition, because
    /// looping clips would otherwise keep playing indefinitely.
    pub fn silence_all_audio(&mut self) {
        let unique_slots: std::collections::HashSet<u32> =
            self.audio_slots.values().copied().collect();
        if let Some(ae) = &mut self.audio_engine {
            for slot_id in &unique_slots {
                ae.send(seqterm_audio_engine::AudioCommand::AllNotesOff { slot_id: *slot_id });
                if !self.sf2_slots.contains(slot_id) {
                    ae.send(seqterm_audio_engine::AudioCommand::StopAudioClip { slot_id: *slot_id });
                }
            }
        }
        self.active_voice_set.clear();
        self.active_voices = 0;
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
            self.silence_all_audio();
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
            5 => {
                self.bpm = (self.bpm + delta as f64).round().clamp(20.0, 300.0);
                self.engine.set_bpm(self.bpm);
            }
            6 => {
                let max_rows = (128 / self.matrix_cols.max(1)).min(16).max(1) as i32;
                self.matrix_rows = ((self.matrix_rows as i32 + delta).clamp(1, max_rows)) as usize;
                let (r, c) = self.matrix_state.cursor;
                self.matrix_state.cursor = (r.min(self.matrix_rows - 1), c);
                self.ensure_matrix_size();
            }
            7 => {
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
        // Transient status messages vanish before the user can read them. Mirror
        // anything that looks like an error/warning to the log so it's recoverable.
        let lower = msg.to_lowercase();
        if ["fail", "error", "invalid", "not found", "unable", "could not", "no such"]
            .iter().any(|k| lower.contains(k))
        {
            tracing::warn!(target: "seqterm::status", "{msg}");
        }
        self.status_msg = msg;
        self.status_expires = Some(
            std::time::Instant::now() + std::time::Duration::from_secs(secs),
        );
    }

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
        self.matrix_rows       = 8;
        self.matrix_cols       = 8;
        self.matrix_col_scroll .set(0);
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
        self.focus        = tab.focus;
        self.matrix_rows       = tab.matrix_rows;
        self.matrix_cols       = tab.matrix_cols;
        self.matrix_col_scroll .set(tab.matrix_col_scroll);
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
        self.focus        = tab.focus;
        self.matrix_rows       = tab.matrix_rows;
        self.matrix_cols       = tab.matrix_cols;
        self.matrix_col_scroll .set(tab.matrix_col_scroll);
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
            focus:         self.focus,
            matrix_rows:       self.matrix_rows,
            matrix_cols:       self.matrix_cols,
            matrix_col_scroll: self.matrix_col_scroll.get(),
            bpm:               self.bpm,
            audio_slots:       self.audio_slots.clone(),
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
                                seqterm_persistence::MidiLearnTarget::ChannelPan(i) => {
                                    let mut proj = self.project.lock();
                                    if let Some(ch_strip) = proj.channels.get_mut(*i) {
                                        // CC 0..127 → pan -50..=50 (L50 … CENTER … R50).
                                        let v = (val as i32 * 100 / 127 - 50).clamp(-50, 50) as i8;
                                        ch_strip.pan = seqterm_core::channel::Pan::from_val(v);
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
                seqterm_midi::MidiMessage::Clock => {
                    if self.midi_clock_sync {
                        self.handle_midi_clock_pulse();
                    }
                }
                _ => {}
            }
        }
    }

    /// Process one MIDI Clock pulse (0xF8 = 24 pulses per quarter note).
    /// Maintains a 24-entry ring of inter-pulse intervals and derives BPM.
    fn handle_midi_clock_pulse(&mut self) {
        let now = std::time::Instant::now();
        if let Some(prev) = self.midi_clock_last_pulse {
            let interval_us = prev.elapsed().as_micros() as u64;
            // Sanity: accept only 20–300 BPM range (2500 µs – 125 000 µs per pulse)
            if interval_us >= 2500 && interval_us <= 125_000 {
                if self.midi_clock_intervals.len() >= 24 {
                    self.midi_clock_intervals.remove(0);
                }
                self.midi_clock_intervals.push(interval_us);

                // Update BPM only after a full quarter note (24 pulses collected).
                if self.midi_clock_intervals.len() >= 8 {
                    let avg_us: f64 = self.midi_clock_intervals.iter().map(|&v| v as f64).sum::<f64>()
                        / self.midi_clock_intervals.len() as f64;
                    // BPM = 60_000_000 µs/min / (24 pulses/beat * avg_µs/pulse)
                    let bpm = 60_000_000.0 / (24.0 * avg_us);
                    let bpm = bpm.clamp(20.0, 300.0);
                    self.bpm = bpm;
                    self.engine.set_bpm(bpm);
                    self.project.lock().bpm = bpm;
                }
            }
        }
        self.midi_clock_last_pulse = Some(now);
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
                    // Transport: ←→ navigates 8 items (0-7): PLAY STOP REWIND REC TAP BPM ROWS COLS
                    if dc != 0 {
                        self.transport_cursor =
                            (self.transport_cursor as i32 + dc).rem_euclid(8) as usize;
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
                    // ACTIONS buttons: ↑↓ selects (0=CLIP, 1=CHANGE SOURCE,
                    // 2=CHANGE BANK/PRESET); ← returns to the grid.
                    if dc < 0 {
                        self.matrix_section = 0;
                        return;
                    }
                    if dr != 0 {
                        self.matrix_action_cursor =
                            (self.matrix_action_cursor as i32 + dr).clamp(0, 3) as usize;
                    }
                } else {
                    // Grid navigation (section 0).
                    let (r, c) = self.matrix_state.cursor;
                    let new_c = (c as i32 + dc).clamp(0, self.matrix_cols as i32 - 1) as usize;
                    // → at rightmost column → enter routing panel on SOURCE tab.
                    if dc > 0 && new_c == c {
                        self.matrix_section = 3;
                        self.routing_tab = 1;          // show SOURCE tab directly
                        self.routing_source_cursor = 0; // cursor on "Change Source" button
                    } else {
                        self.matrix_state.cursor = (
                            (r as i32 + dr).clamp(0, self.matrix_rows as i32 - 1) as usize,
                            new_c,
                        );
                    }
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
                    4 => {
                        // FX chain: ←→ = slot, ↑↓ = param within slot, +/- key = adjust value.
                        if dc != 0 {
                            // Clamp to the number of effects actually present (max 5).
                            let n = self.tracker_current_slot_id()
                                .and_then(|sid| self.audio_slot_fx.get(&sid))
                                .map(|c| c.len())
                                .unwrap_or(0);
                            let hi = n.saturating_sub(1) as i32;
                            self.tracker_fx_slot =
                                (self.tracker_fx_slot as i32 + dc).clamp(0, hi.max(0)) as usize;
                            self.tracker_fx_param = 0;
                        }
                        if dr != 0 {
                            // ↑↓ navigates params within the selected slot.
                            let n_params = self.tracker_fx_param_count();
                            if n_params > 0 {
                                self.tracker_fx_param =
                                    (self.tracker_fx_param as i32 + dr)
                                        .rem_euclid(n_params as i32) as usize;
                            }
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
                        let new_track = (self.arranger_state.selected_track as i32 + dr)
                            .clamp(0, self.matrix_rows.saturating_sub(1) as i32) as usize;
                        self.arranger_state.selected_track = new_track;
                        // Auto-scroll track_scroll to keep selection visible.
                        // Estimate ~3 lines per track (name + clip + separator).
                        let visible_tracks = 5usize; // conservative estimate
                        if new_track < self.arranger_state.track_scroll {
                            self.arranger_state.track_scroll = new_track;
                        } else if new_track >= self.arranger_state.track_scroll + visible_tracks {
                            self.arranger_state.track_scroll = new_track.saturating_sub(visible_tracks - 1);
                        }
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
                    // Include audio-engine slots (SF2/audio instruments, e.g. from a
                    // MIDI import) so each one is selectable for independent volume.
                    let n_audio = crate::views::mixer::collect_audio_slot_entries(self).len();
                    let n = {
                        let proj = self.project.lock();
                        crate::views::mixer::total_mixer_count(&proj, n_audio).saturating_sub(1)
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
            // Use the same ordering as the mixer view (one entry per row A-P, first clip).
            let target = {
                let entries = crate::views::mixer::collect_audio_slot_entries(self);
                entries.get(audio_idx).map(|e| (e.slot_id, e.channel, e.is_sf2))
            };
            if let Some((slot_id, channel, is_sf2)) = target {
                if is_sf2 {
                    // SF2 slots may host several instruments on one synth — adjust THIS
                    // instrument's MIDI channel volume (CC7) so others stay put.
                    let cc7 = self.audio_slot_channel_vol.entry((slot_id, channel)).or_insert(100);
                    *cc7 = (*cc7 as i32 + delta * 5).clamp(0, 127) as u8;
                    let value = *cc7;
                    if let Some(ae) = self.audio_engine.as_mut() {
                        ae.send(seqterm_audio_engine::AudioCommand::ControlChange {
                            slot_id, channel, cc: 7, value,
                        });
                    }
                } else {
                    // Audio-file slot: dedicated slot → per-slot gain.
                    let vol = self.audio_slot_volumes.entry(slot_id).or_insert(1.0);
                    *vol = (*vol + delta as f32 * 0.05).clamp(0.0, 2.0);
                    let new_vol = *vol;
                    if let Some(ae) = self.audio_engine.as_mut() {
                        ae.send(seqterm_audio_engine::AudioCommand::SetSlotVolume {
                            slot_id, volume: new_vol,
                        });
                    }
                }
            }
            return;
        }

        // MASTER strips occupy indices n_midi (MASTER L) and n_midi+1 (MASTER R);
        // both drive the single master output volume.
        if entry_idx == n_midi || entry_idx == n_midi + 1 {
            if param == 0 && delta != 0 {
                self.master_volume = (self.master_volume + delta as f32 * 0.05).clamp(0.0, 2.0);
                let v = self.master_volume;
                if let Some(ae) = self.audio_engine.as_mut() {
                    ae.send(seqterm_audio_engine::AudioCommand::SetMasterVolume(v));
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
            // Mod matrix rows 17-20 (LFO slots 0-3).
            // ← / → cycles: shape (col 0), target (col 1), rate_hz±0.1 (col 2), depth±0.05 (col 3).
            // The sub-field cycles with each successive press; Enter toggles enabled.
            // Macro knobs 21-24 (macros 0-3). Each macro fans out to one GrainParams field.
            c @ 21..=24 => {
                let i = c - 21;
                self.granular_macros[i] = (self.granular_macros[i] + delta as f32 * 0.05).clamp(0.0, 1.0);
                let v = self.granular_macros[i];
                match i {
                    0 => self.granular_state.params.spray      = v,
                    1 => self.granular_state.params.density     = 1.0 + v * 99.0,
                    2 => self.granular_state.params.pitch_st    = (v * 2.0 - 1.0) * 24.0,
                    _ => self.granular_state.params.size_ms     = 10.0 + v * 490.0,
                }
            }
            c @ 17..=20 => {
                let i = c - 17;
                if let Some(slot) = self.granular_mod.slots.get_mut(i) {
                    if delta == 0 {
                        slot.enabled = !slot.enabled;
                    } else {
                        // Cycle through shape → target → rate → depth based on current sub-selection.
                        // We use `granular_mod_cursor` as the column selector within the slot.
                        match self.granular_mod_cursor % 4 {
                            0 => slot.shape  = if delta > 0 { slot.shape.next()  } else { slot.shape.prev()  },
                            1 => slot.target = if delta > 0 { slot.target.next() } else { slot.target.prev() },
                            2 => slot.rate_hz = (slot.rate_hz + delta as f32 * 0.1).clamp(0.01, 20.0),
                            _ => slot.depth   = (slot.depth   + delta as f32 * 0.05).clamp(0.0, 1.0),
                        }
                    }
                }
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
            ae.send(seqterm_audio_engine::AudioCommand::SetGranularMod {
                slot_id,
                mod_matrix: self.granular_mod.clone(),
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
    /// Slot id for the current tracker pattern row (None if no audio assigned).
    pub fn tracker_current_slot_id(&self) -> Option<u32> {
        let (row, col) = self.matrix_state.cursor;
        let row_key = ((b'A' + row as u8) as char).to_string();
        let clip_key = format!("{}{}", row_key, col);
        self.audio_slots.get(&clip_key).copied()
    }

    /// Number of parameters for the currently focused FX slot in the tracker FX panel.
    pub fn tracker_fx_param_count(&self) -> usize {
        let slot_id = match self.tracker_current_slot_id() { Some(id) => id, None => return 0 };
        let chain = match self.audio_slot_fx.get(&slot_id) { Some(c) => c, None => return 0 };
        let entry = match chain.get(self.tracker_fx_slot) { Some(e) => e, None => return 0 };
        fx_param_descs(entry.kind).len()
    }

    /// Adjust the value of the currently selected FX parameter by `delta` (-1.0 to 1.0 scaled).
    pub fn tracker_fx_adjust_param(&mut self, delta: f32) {
        let slot_id = match self.tracker_current_slot_id() { Some(id) => id, None => return };
        let param_idx = self.tracker_fx_param;
        let slot_idx  = self.tracker_fx_slot;
        if let Some(chain) = self.audio_slot_fx.get_mut(&slot_id) {
            if let Some(entry) = chain.get_mut(slot_idx) {
                if let Some(v) = entry.params.get_mut(param_idx) {
                    *v = (*v + delta).clamp(0.0, 1.0);
                    entry.sync_wet();
                }
                self.rebuild_audio_fx_chain(slot_id);
            }
        }
    }

    pub fn rebuild_audio_fx_chain(&mut self, slot_id: u32) {
        let entries = self.audio_slot_fx.get(&slot_id).cloned().unwrap_or_default();
        let chain = build_fx_chain(&entries);
        if let Some(ae) = self.audio_engine.as_mut() {
            ae.send(seqterm_audio_engine::AudioCommand::SetSlotFxChain { slot_id, chain });
        }
    }

    /// Rebuild the master bus FX chain from `master_fx` and send `SetMasterFxChain`.
    pub fn rebuild_master_fx_chain(&mut self) {
        let chain = build_fx_chain(&self.master_fx);
        if let Some(ae) = self.audio_engine.as_mut() {
            ae.send(seqterm_audio_engine::AudioCommand::SetMasterFxChain { chain });
        }
    }

    pub fn adjust_audio_fx_param(&mut self, slot_id: u32, entry_idx: usize, param_idx: usize, delta: f32) {
        if let Some(chain) = self.audio_slot_fx.get_mut(&slot_id) {
            if let Some(entry) = chain.get_mut(entry_idx) {
                if let Some(v) = entry.params.get_mut(param_idx) {
                    *v = (*v + delta).clamp(0.0, 1.0);
                    entry.sync_wet();
                }
            }
        }
        self.rebuild_audio_fx_chain(slot_id);
    }

    pub fn adjust_master_fx_param(&mut self, entry_idx: usize, param_idx: usize, delta: f32) {
        if let Some(entry) = self.master_fx.get_mut(entry_idx) {
            if let Some(v) = entry.params.get_mut(param_idx) {
                *v = (*v + delta).clamp(0.0, 1.0);
                entry.sync_wet();
            }
        }
        self.rebuild_master_fx_chain();
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
            // Only redirect the scheduler to this pattern when NOT playing,
            // so that opening the tracker during playback doesn't interrupt the mix.
            if !self.playing {
                self.engine.set_pattern(key.clone());
            }
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
            // Also populate track_names so the matrix row labels show the instrument name.
            let mut track_row_idx = 0usize;
            for track in imported.tracks {
                if !proj.tracks.iter().any(|t| t.name == track.name) {
                    // Map the track to its row letter (tracks with notes are assigned A, B, C...).
                    // We find the row by matching the track name to patterns referenced in the matrix.
                    let row_key_for_track = (b'A' + track_row_idx as u8) as char;
                    let rk = row_key_for_track.to_string();
                    if !track.name.is_empty() && !proj.track_names.contains_key(&rk) {
                        proj.track_names.insert(rk, track.name.clone());
                    }
                    track_row_idx += 1;
                    proj.tracks.push(track);
                }
            }
            // Merge automation lanes.
            for lane in imported.automation {
                if !proj.automation.iter().any(|l| l.target == lane.target) {
                    proj.automation.push(lane);
                }
            }
            // Merge timeline markers from the MIDI file.
            for (bar, name) in imported.markers {
                if !proj.markers.iter().any(|(b, _)| *b == bar) {
                    proj.markers.push((bar, name));
                }
            }
            proj.markers.sort_by_key(|(b, _)| *b);
        }
        // Create virtual MIDI ports for the newly imported patterns.
        let new_ports = seqterm_midi::create_pattern_ports(&new_pattern_keys);
        if !new_ports.is_empty() {
            self.engine.add_midi_ports(new_ports);
        }
        // Auto-expand matrix dimensions to show all imported content.
        // Rows: count how many rows (A-P) in the project actually have clips.
        // Cols: find the longest row (most patterns per row).
        {
            let proj = self.project.lock();
            let mut max_row = self.matrix_rows;
            let mut max_col = self.matrix_cols;
            for row in 0u8..16 {
                let key = ((b'A' + row) as char).to_string();
                if let Some(slots) = proj.matrix.get(&key) {
                    let has_clip = slots.iter().any(|s| s.is_some());
                    if has_clip {
                        max_row = max_row.max((row + 1) as usize);
                        // Count filled columns in this row.
                        let filled_cols = slots.iter().enumerate()
                            .filter(|(_, s)| s.is_some())
                            .map(|(i, _)| i + 1)
                            .max()
                            .unwrap_or(0);
                        max_col = max_col.max(filled_cols);
                    }
                }
            }
            self.matrix_rows = max_row.min(16);
            // Cols are capped at 16 for the UI; if the piece is long and bars_per_pattern
            // is small, we show the first 16 columns and the user can scroll.
            self.matrix_cols = max_col.min(16).max(self.matrix_cols);
        }
        self.ensure_matrix_size();
        // Load any SF2 / AudioFile sources from the imported clips into the audio engine.
        crate::rebuild_audio_slots(self);
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
            ViewKind::Granular => {}
        }
    }
}
