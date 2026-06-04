//! # SeqTerm Plugin Sandbox
//!
//! Runs VST2/VST3/CLAP plugins in a separate OS process to isolate crashes
//! and memory corruption from the main audio engine.  Audio data and MIDI
//! events are exchanged via a shared-memory ring buffer; control messages
//! (parameter changes, preset load) use a Unix domain socket (or named pipe
//! on Windows).
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────┐      ┌──────────────────────────────┐
//! │  SeqTerm (host process)     │      │  Plugin sandbox process       │
//! │                             │      │                               │
//! │  SandboxedPlugin            │      │  seqterm-sandbox-host binary  │
//! │  ├─ ctrl_socket (Unix)  ◀──▶│◀────▶│  ├─ ctrl_socket              │
//! │  └─ shm: ShmAudioBridge ◀──▶│◀────▶│  └─ shm: ShmAudioBridge     │
//! │                             │      │  └─ plugin.so (VST2/VST3)    │
//! └─────────────────────────────┘      └──────────────────────────────┘
//! ```
//!
//! ## Shared-Memory Audio Bridge
//!
//! The bridge uses two lock-free ring buffers (one per direction) mapped into
//! both processes.  Each buffer holds stereo f32 frames at the configured
//! block size.  A spin-lock-free flag signals when data is ready.
//!
//! ## Status
//!
//! Scaffold and protocol definitions.  Full shm + process spawning requires
//! the `sandbox` feature and platform-specific code.

use std::path::PathBuf;
use uuid::Uuid;
use seqterm_ports::realtime::{AudioSource, AudioSynthPort, InstrumentBackend, PresetInfo};

// ─── Protocol messages ────────────────────────────────────────────────────────

/// Control messages sent from host → sandbox process.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMsg {
    /// Load a plugin from `path` with the given ID.
    LoadPlugin { plugin_id: String, path: PathBuf },
    /// Send a MIDI NoteOn.
    NoteOn { channel: u8, note: u8, velocity: u8 },
    /// Send a MIDI NoteOff.
    NoteOff { channel: u8, note: u8 },
    /// Send a MIDI CC.
    Cc { channel: u8, cc: u8, value: u8 },
    /// Change a parameter value.
    SetParam { index: u32, value: f32 },
    /// Request all parameter values.
    GetParams,
    /// Request plugin state (VST2 chunk or VST3 state).
    GetState,
    /// Restore plugin state.
    SetState { data: Vec<u8> },
    /// Terminate the sandbox process.
    Shutdown,
}

/// Control messages sent from sandbox → host process.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SandboxMsg {
    /// Plugin loaded successfully.
    PluginLoaded { name: String, vendor: String, num_params: u32 },
    /// Plugin load failed.
    LoadError { error: String },
    /// Parameter value report (response to GetParams).
    ParamValues { values: Vec<(u32, f32, String)> },
    /// Plugin state blob (response to GetState).
    State { data: Vec<u8> },
    /// Sandbox crashed or plugin panicked.
    Crashed { reason: String },
    /// Sandbox is ready.
    Ready,
}

// ─── Shared-memory layout ────────────────────────────────────────────────────

/// Header placed at offset 0 in the shared memory region.
/// Total size = sizeof(ShmHeader) + 2 * ring_cap * sizeof(f32).
#[repr(C)]
pub struct ShmHeader {
    /// Host write pointer (frames, wrapping at ring_cap).
    pub host_write:    std::sync::atomic::AtomicU64,
    /// Sandbox read pointer.
    pub sandbox_read:  std::sync::atomic::AtomicU64,
    /// Sandbox write pointer.
    pub sandbox_write: std::sync::atomic::AtomicU64,
    /// Host read pointer.
    pub host_read:     std::sync::atomic::AtomicU64,
    /// Block size in frames.
    pub block_frames:  u32,
    /// Sample rate.
    pub sample_rate:   u32,
}

// ─── Sandbox descriptor ───────────────────────────────────────────────────────

/// Unique identifier for a sandboxed plugin instance.
#[derive(Debug, Clone)]
pub struct SandboxId(Uuid);

impl SandboxId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }

    /// The underlying UUID for this sandbox instance.
    pub fn as_uuid(&self) -> &Uuid { &self.0 }
}

impl Default for SandboxId {
    fn default() -> Self { Self::new() }
}

// ─── Sandboxed plugin (stub) ──────────────────────────────────────────────────

/// A plugin instance running in a sandboxed child process.
///
/// Without the `sandbox` feature this is a no-op stub.
pub struct SandboxedPlugin {
    id:     SandboxId,
    name:   String,
    active: bool,
}

impl SandboxedPlugin {
    /// Spawn a child process to host `plugin_path` and return a handle to it.
    pub fn spawn(plugin_path: PathBuf) -> anyhow::Result<Self> {
        let name = plugin_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Plugin".into());

        #[cfg(feature = "sandbox")]
        {
            // Full implementation: fork or spawn seqterm-sandbox-host binary,
            // create shared memory region, connect Unix socket, send LoadPlugin msg.
            anyhow::bail!("sandbox feature not yet fully implemented");
        }
        #[cfg(not(feature = "sandbox"))]
        {
            tracing::warn!(
                "Plugin sandbox feature not enabled; '{}' will produce silence.",
                name
            );
            Ok(Self { id: SandboxId::new(), name, active: true })
        }
    }

    pub fn sandbox_id(&self) -> &SandboxId { &self.id }
}

// ── AudioSource ───────────────────────────────────────────────────────────────

impl AudioSource for SandboxedPlugin {
    fn render(&mut self, _output: &mut [f32], _sr: u32) -> usize { 0 }
    fn is_active(&self) -> bool { self.active }
    fn stop(&mut self) { self.active = false; }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

impl AudioSynthPort for SandboxedPlugin {
    fn note_on(&mut self, _ch: u8, _note: u8, _vel: u8) {}
    fn note_off(&mut self, _ch: u8, _note: u8) {}
    fn control_change(&mut self, _ch: u8, _cc: u8, _val: u8) {}
    fn pitch_bend(&mut self, _ch: u8, _val: i16) {}
}

impl InstrumentBackend for SandboxedPlugin {
    fn backend_name(&self) -> &str { "Sandboxed Plugin (stub)" }
    fn select_preset(&mut self, _bank: u16, _program: u8) -> anyhow::Result<()> { Ok(()) }
    fn list_presets(&self) -> Vec<PresetInfo> {
        vec![PresetInfo { bank: 0, program: 0, name: self.name.clone() }]
    }
    fn all_notes_off(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_id_unique() {
        let a = SandboxId::new();
        let b = SandboxId::new();
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn stub_spawn_succeeds_without_file() {
        #[cfg(not(feature = "sandbox"))]
        {
            let s = SandboxedPlugin::spawn(PathBuf::from("fake.vst3"));
            assert!(s.is_ok());
        }
    }
}
