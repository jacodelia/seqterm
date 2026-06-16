//! # SeqTerm Ports — Hexagonal Architecture Boundary
//!
//! This crate defines all **port traits** (interfaces) used in SeqTerm's
//! hexagonal architecture. It contains NO implementations.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                    ADAPTERS (implementations)                    │
//! │  CpalAudioAdapter · MidirMidiAdapter · JsonProjectRepository     │
//! │  Sf2SynthAdapter  · AudioClipAdapter · WavExportAdapter          │
//! ├──────────────────────────────────────────────────────────────────┤
//! │                     PORTS (this crate)                           │
//! │  AudioBackendPort · MidiBackendPort  · ProjectRepository         │
//! │  ExporterPort     · PluginHostPort   · AudioSynthPort            │
//! ├──────────────────────────────────────────────────────────────────┤
//! │                  DOMAIN  (seqterm-core)                          │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Realtime Rule
//!
//! Traits in [`realtime`] MUST be implemented without allocations,
//! mutexes, or blocking calls. The audio callback is the law.

pub mod audio;
pub mod instrument;
pub mod midi;
pub mod persistence;
pub mod plugin;
pub mod realtime;
pub mod sf2_params;
pub mod export;

pub use audio::{AudioBackendPort, AudioDeviceInfo, AudioEngineConfig};
pub use instrument::{Parameter, ParameterProvider, ParameterType};
pub use sf2_params::Sf2ZoneParams;
pub use midi::{MidiBackendPort, MidiDeviceInfo, MidiInputCallback, MidiMessage};
pub use persistence::{ProjectRepository, ProjectMetadata};
pub use plugin::{PluginHostPort, PluginDescriptor, PluginKind};
pub use realtime::{AudioSource, AudioSynthPort, InstrumentBackend, PresetInfo, RealtimeEventSink};
pub use export::ExporterPort;
