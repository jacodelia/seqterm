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
    Internal,
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
}
