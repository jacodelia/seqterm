//! Plugin host port — VST3/CLAP/AU hosting interface.

use std::path::PathBuf;
use anyhow::Result;

/// The plugin format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginKind {
    Vst2,
    Vst3,
    Clap,
    Au,
    /// LADSPA shared-library effect (`.so`/`.dll`/`.dylib`).
    Ladspa,
    /// DSSI instrument plugin (`.so`/`.dll`/`.dylib`).
    Dssi,
    /// LV2 plugin bundle (`*.lv2/` directory containing `manifest.ttl`).
    Lv2,
    /// SFZ sampler instrument definition (`.sfz`).
    Sfz,
    /// SF2/SF3 SoundFont (`.sf2`/`.sf3`).
    Sf2,
    /// JSFX (REAPER JS) effect script (`.jsfx`).
    Jsfx,
    Internal,
}

impl PluginKind {
    /// Short uppercase label used in pickers and lists (e.g. "VST3", "LV2").
    pub fn label(&self) -> &'static str {
        match self {
            Self::Vst2     => "VST2",
            Self::Vst3     => "VST3",
            Self::Clap     => "CLAP",
            Self::Au       => "AU",
            Self::Ladspa   => "LADSPA",
            Self::Dssi     => "DSSI",
            Self::Lv2      => "LV2",
            Self::Sfz      => "SFZ",
            Self::Sf2      => "SF2",
            Self::Jsfx     => "JSFX",
            Self::Internal => "FX",
        }
    }
}

/// A MIDI-style event delivered to an instrument plugin instance.
/// Channel is 0-based (0–15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginMidi {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8 },
    Cc { channel: u8, cc: u8, value: u8 },
    PitchBend { channel: u8, value: i16 },
}

/// Metadata describing a discovered plugin.
#[derive(Debug, Clone)]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub kind: PluginKind,
    pub path: PathBuf,
    pub is_instrument: bool,
    pub is_effect: bool,
}

/// Port: plugin host — scan, instantiate, and communicate with plugins.
/// Implemented by ClapPluginHost, Vst3PluginHost, etc.
pub trait PluginHostPort: Send + Sync {
    /// Scan a directory for plugins.
    fn scan(&mut self, dir: &std::path::Path) -> Result<Vec<PluginDescriptor>>;

    /// List all known plugins from the last scan.
    fn list_plugins(&self) -> &[PluginDescriptor];

    /// Instantiate a plugin by ID. Returns an opaque instance handle.
    fn instantiate(&mut self, plugin_id: &str, sample_rate: u32, block_size: u32) -> Result<u64>;

    /// Destroy a plugin instance.
    fn destroy(&mut self, instance_id: u64);

    /// Process one audio block through a plugin instance.
    fn process(&mut self, instance_id: u64, input: &[f32], output: &mut [f32]) -> Result<()>;

    /// Deliver MIDI-style events to an instrument instance before the next
    /// [`process`](Self::process). Default: no-op (effects ignore MIDI).
    fn send_midi(&mut self, _instance_id: u64, _events: &[PluginMidi]) {}

    /// Build a standalone, realtime-installable instrument source for a plugin,
    /// to be placed directly in a mixer slot and driven by note/CC events.
    /// Default: `None` (host doesn't support installable instrument sources).
    /// Hosts whose instruments can run on the audio thread (e.g. LV2) override.
    fn create_audio_source(
        &self,
        _plugin_id: &str,
        _sample_rate: u32,
        _block_size: u32,
    ) -> Option<Box<dyn crate::realtime::AudioSource>> {
        None
    }

    // ── Parameter access (optional — default: 0 params) ──────────────────────

    /// Return the number of automatable parameters for an instance.
    fn param_count(&self, _instance_id: u64) -> u32 { 0 }

    /// Get the current value of a parameter (normalised 0.0–1.0).
    fn get_param(&self, _instance_id: u64, _param_id: u32) -> f32 { 0.0 }

    /// Set a parameter value (normalised 0.0–1.0).
    fn set_param(&mut self, _instance_id: u64, _param_id: u32, _value: f32) {}

    /// Human-readable parameter name (e.g. "Attack").
    fn param_name(&self, _instance_id: u64, param_id: u32) -> String { format!("P{param_id}") }

    /// Unit label for a parameter (e.g. "ms", "%").
    fn param_label(&self, _instance_id: u64, _param_id: u32) -> String { String::new() }

    /// Formatted display string for the current value (e.g. "127.3").
    fn param_display(&self, _instance_id: u64, param_id: u32) -> String {
        format!("{:.3}", self.get_param(_instance_id, param_id))
    }

    // ── State persistence (effGetChunk / effSetChunk) ─────────────────────────

    /// Get the full plugin state as opaque bytes.
    /// Returns `None` if the plugin does not support state serialization.
    fn get_state(&self, _instance_id: u64) -> Option<Vec<u8>> { None }

    /// Restore plugin state from opaque bytes previously obtained via `get_state`.
    /// Returns true if the plugin accepted the state.
    fn set_state(&mut self, _instance_id: u64, _data: &[u8]) -> bool { false }
}
