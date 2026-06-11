pub mod automation;
pub mod channel;
pub mod granular;
pub mod modulation;
pub mod mpe;
pub mod note;
pub mod pad;
pub mod pattern;
pub mod preset;
pub mod project;
pub mod sf2_instrument;

pub use channel::{Channel, ChannelType, FxKind, FxRoute, FxRouteKind, FxSlot, GM_DRUM_MAP};
pub use granular::{
    AdsrEnvelope, AmplitudeParams, AudioEditOp, EditorFilter, EditorMarker, FilterKind,
    FrequencyParams, GrainDirection, GrainEnvelope, GrainParams, GranularMod, GranularPreset,
    GranularZone, LayersParams, LfoShape, LfoSlot, MarkerKind, MAX_LAYERS, ModTarget, MOD_SLOTS,
    PadEditorPreset, SamplerLayer, SampleLoopMode, SampleParams, ScanMode,
};
pub use automation::{AutomationCurve, AutomationEngine, AutomationMode, AutomationPoint};
pub use modulation::{
    MacroControl, MacroTarget, ModulationCurve, ModulationRoute, ModulationSource,
    ModulationSystem, Polarity, SourceValues, MACRO_COUNT,
};
pub use mpe::{MpeChannelMap, MpeZone, MpeZoneKind};
pub use preset::{Preset, PresetMetadata, PresetParam, SnapshotAB};
pub use sf2_instrument::{
    Sf2FilterType, Sf2Instrument, Sf2LfoWaveform, Sf2LoopMode, Sf2Zone,
};
pub use note::{Note, NOTE_NAMES};
pub use pad::{ChokeGroup, MuteGroup, PadBank, PadSlot, SamplerConfig, TriggerMode};
pub use pattern::{musical_groupings, Clip, Pattern, PatternSource};
pub use project::{AudioBus, ChainEntry, FxSpec, MidiPort, OscRoute, Project, RoutingEdge, RoutingGraph, RoutingNode, RoutingSnapshot, Scene, SyncMode, TrackKind};
