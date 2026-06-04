//! # SeqTerm SDK
//!
//! Stable public API for SeqTerm — re-exports the domain types and port traits
//! that external tools, plugins, and scripts can rely on without coupling to
//! internal crate versions.
//!
//! ## Overview
//!
//! SeqTerm is a terminal-based modular sequencer/DAW with a hexagonal architecture:
//!
//! - **Domain**: `core` re-exports — [`Project`](core::Project), [`Pattern`](core::Pattern),
//!   [`Note`](core::Note), [`Channel`](core::Channel), etc.
//! - **Ports**: `ports` re-exports — trait definitions for audio, MIDI, and project I/O.
//! - **Utilities**: [`new_project`], [`project_to_json`], [`project_from_json`],
//!   [`sdk_version`].
//!
//! ## Quick Start
//!
//! Add to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! seqterm-sdk = { path = "path/to/seqterm/crates/seqterm-sdk" }
//! ```
//!
//! Create and serialize a project:
//!
//! ```rust
//! use seqterm_sdk::prelude::*;
//!
//! let mut proj = seqterm_sdk::new_project("My Project");
//! proj.bpm = 140.0;
//!
//! let json = seqterm_sdk::project_to_json(&proj).unwrap();
//! assert!(json.contains("140"));
//!
//! let roundtrip = seqterm_sdk::project_from_json(&json).unwrap();
//! assert_eq!(roundtrip.bpm, proj.bpm);
//! ```
//!
//! Build a pattern with notes:
//!
//! ```rust
//! use seqterm_sdk::core::{Pattern, Note};
//!
//! let mut pat = Pattern::new("KICK01", 16);
//! // Place a kick note at step 0.
//! if let Some(step) = pat.steps.get_mut(0) {
//!     *step = Note::from_midi(36, 100).unwrap(); // MIDI 36 = kick drum
//! }
//! assert!(!pat.steps[0].is_empty());
//! ```
//!
//! ## Feature Flags
//!
//! The SDK itself has no feature flags; optional backends (FluidSynth, VST3, CLAP)
//! are enabled on the `seqterm-app` / `seqterm-audio-engine` crates.

#![deny(missing_docs)]

// ── Domain re-exports ─────────────────────────────────────────────────────────

/// Core domain types — the building blocks of every SeqTerm project.
///
/// These types are serializable via serde (JSON / MessagePack) and form the
/// stable data model of the application.
pub mod core {
    #[allow(missing_docs)] // doc comments live in seqterm-core
    pub use seqterm_core::{
        Channel, ChannelType, Clip, FxKind, FxRoute, FxRouteKind, FxSlot,
        GrainParams, GranularPreset, GranularZone, Note, NOTE_NAMES,
        Pattern, PatternSource, Project, Scene, SyncMode, TrackKind,
        GM_DRUM_MAP,
    };
}

/// Port trait definitions — abstract interfaces for audio, MIDI, and storage adapters.
///
/// Implement these traits to swap in alternative backends (e.g. FluidSynth, WebAudio)
/// without changing application or domain code.
pub mod ports {
    #[allow(missing_docs)] // doc comments live in seqterm-ports
    pub use seqterm_ports::{
        AudioBackendPort, AudioDeviceInfo, AudioEngineConfig,
        MidiBackendPort, MidiDeviceInfo, MidiMessage,
        InstrumentBackend, PresetInfo,
        AudioSource, AudioSynthPort,
        ProjectRepository, ProjectMetadata,
    };
}

/// Commonly-used items for glob import (`use seqterm_sdk::prelude::*`).
pub mod prelude {
    #[allow(missing_docs)]
    pub use crate::core::{
        Channel, ChannelType, Clip, Note, Pattern, PatternSource,
        Project, Scene, TrackKind,
    };
    #[allow(missing_docs)]
    pub use anyhow::Result;
}

// ── Utility functions ─────────────────────────────────────────────────────────

/// Serialize a `Project` to a pretty-printed JSON string.
///
/// # Errors
///
/// Returns an error if serialization fails (should never happen for valid projects).
///
/// # Example
///
/// ```rust
/// let proj = seqterm_sdk::new_project("Demo");
/// let json = seqterm_sdk::project_to_json(&proj).unwrap();
/// assert!(json.contains("\"name\""));
/// ```
pub fn project_to_json(project: &core::Project) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(project)?)
}

/// Deserialize a `Project` from a JSON string.
///
/// # Errors
///
/// Returns an error if the JSON is malformed or missing required fields.
///
/// # Example
///
/// ```rust
/// let proj = seqterm_sdk::new_project("Demo");
/// let json = seqterm_sdk::project_to_json(&proj).unwrap();
/// let loaded = seqterm_sdk::project_from_json(&json).unwrap();
/// assert_eq!(loaded.name, "Demo");
/// ```
pub fn project_from_json(json: &str) -> anyhow::Result<core::Project> {
    Ok(serde_json::from_str(json)?)
}

/// Create a blank [`Project`](core::Project) with the given name, default BPM (120),
/// and an empty 8×8 session matrix.
///
/// # Example
///
/// ```rust
/// let proj = seqterm_sdk::new_project("My Track");
/// assert_eq!(proj.name, "My Track");
/// assert!(proj.bpm > 0.0);
/// ```
pub fn new_project(name: impl Into<String>) -> core::Project {
    core::Project::blank(name)
}

/// Returns the SeqTerm SDK version string (from `Cargo.toml`).
///
/// # Example
///
/// ```rust
/// let ver = seqterm_sdk::sdk_version();
/// assert!(!ver.is_empty());
/// ```
pub fn sdk_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
