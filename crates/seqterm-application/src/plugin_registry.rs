//! Plugin registry — unified lifecycle manager for all plugin formats.
//!
//! Aggregates multiple [`PluginHostPort`] adapters (VST2, CLAP, Internal…)
//! and provides a single point of control for scan / instantiate / process / destroy.

use std::path::Path;

use anyhow::{Result, bail};
use tracing::{debug, warn};

use seqterm_ports::plugin::{PluginDescriptor, PluginHostPort, PluginKind};

/// A running plugin instance tracked by the registry.
#[derive(Debug)]
pub struct PluginInstance {
    /// Globally unique handle assigned by the registry.
    pub registry_id: u64,
    /// The opaque handle returned by the host adapter.
    pub host_id: u64,
    /// Which adapter owns this instance (index into `PluginRegistry::adapters`).
    adapter_idx: usize,
    /// Mixer slot this instance is wired to, if any.
    pub mixer_slot: Option<usize>,
    /// Descriptor cached at instantiation time.
    pub descriptor: PluginDescriptor,
    pub state: InstanceState,
}

/// Lifecycle state of a plugin instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceState {
    Active,
    Suspended,
    Destroyed,
}

/// Central registry for all plugin adapters and their instances.
pub struct PluginRegistry {
    adapters: Vec<Box<dyn PluginHostPort>>,
    instances: Vec<PluginInstance>,
    next_id: u64,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            adapters: Vec::new(),
            instances: Vec::new(),
            next_id: 1,
        }
    }

    /// Register a plugin host adapter (e.g. `Vst2PluginHost`, `InternalPluginHost`).
    pub fn register_adapter(&mut self, adapter: Box<dyn PluginHostPort>) {
        debug!("PluginRegistry: registered adapter");
        self.adapters.push(adapter);
    }

    /// Build a registry pre-populated with every plugin-host adapter enabled at
    /// compile time. Which adapters are present depends on this crate's feature
    /// flags: `vst2` (default), `vst3`, `clap-host`.
    pub fn with_default_adapters(sample_rate: u32, block_size: u32) -> Self {
        let mut reg = Self::new();
        let _ = (sample_rate, block_size); // unused when no plugin feature is on
        #[cfg(feature = "vst2")]
        reg.register_adapter(Box::new(
            seqterm_plugin_vst2::Vst2PluginHost::new(sample_rate, block_size),
        ));
        #[cfg(feature = "vst3")]
        reg.register_adapter(Box::new(seqterm_plugin_vst3::Vst3Host::new()));
        #[cfg(feature = "clap-host")]
        reg.register_adapter(Box::new(seqterm_plugin_clap::ClapHost::new()));
        // Filesystem-discovery adapters (always available — pure Rust, no SDK).
        for kind in [
            PluginKind::Ladspa,
            PluginKind::Dssi,
            PluginKind::Sfz,
            PluginKind::Sf2,
            PluginKind::Jsfx,
        ] {
            reg.register_adapter(Box::new(seqterm_plugin_scan::FileScanHost::new(kind)));
        }
        // Real LV2 host (TTL parse + libloading — actually processes audio).
        reg.register_adapter(Box::new(seqterm_plugin_lv2::Lv2PluginHost::new()));
        reg
    }

    /// Scan every adapter's platform-default plugin locations plus any
    /// `extra_dirs`, populating each adapter's plugin list. Returns the total
    /// number of plugins discovered.
    pub fn scan_default_locations(&mut self, extra_dirs: &[std::path::PathBuf]) -> usize {
        let mut total = 0;
        #[cfg(feature = "vst3")]
        for dir in seqterm_plugin_vst3::default_search_paths() {
            total += self.scan(&dir).len();
        }
        #[cfg(feature = "clap-host")]
        for dir in seqterm_plugin_clap::default_search_paths() {
            total += self.scan(&dir).len();
        }
        // Platform-default locations for the filesystem-discovery formats.
        for kind in [
            PluginKind::Ladspa,
            PluginKind::Dssi,
            PluginKind::Sfz,
            PluginKind::Sf2,
            PluginKind::Jsfx,
        ] {
            for dir in seqterm_plugin_scan::default_search_paths(&kind) {
                total += self.scan(&dir).len();
            }
        }
        // Real LV2 host default locations.
        for dir in seqterm_plugin_lv2::default_search_paths() {
            total += self.scan(&dir).len();
        }
        for dir in extra_dirs {
            total += self.scan(dir).len();
        }
        total
    }

    /// Scan `dir` with every registered adapter; returns all newly discovered plugins.
    pub fn scan(&mut self, dir: &Path) -> Vec<PluginDescriptor> {
        let mut all = Vec::new();
        for adapter in &mut self.adapters {
            match adapter.scan(dir) {
                Ok(found) => {
                    debug!("Scan found {} plugin(s)", found.len());
                    all.extend(found);
                }
                Err(e) => {
                    warn!("Adapter scan error for {}: {e}", dir.display());
                }
            }
        }
        all
    }

    /// Merged list of all plugins known to all adapters.
    pub fn list_plugins(&self) -> Vec<&PluginDescriptor> {
        self.adapters.iter()
            .flat_map(|a| a.list_plugins())
            .collect()
    }

    /// Find a descriptor by plugin ID across all adapters.
    pub fn find_plugin(&self, plugin_id: &str) -> Option<&PluginDescriptor> {
        self.adapters.iter()
            .flat_map(|a| a.list_plugins())
            .find(|d| d.id == plugin_id)
    }

    /// Instantiate a plugin. Returns a registry-level instance ID.
    /// The instance is immediately `Active`.
    pub fn instantiate(
        &mut self,
        plugin_id: &str,
        sample_rate: u32,
        block_size: u32,
    ) -> Result<u64> {
        // Find which adapter knows this plugin.
        let adapter_idx = self.adapters.iter()
            .position(|a| a.list_plugins().iter().any(|p| p.id == plugin_id))
            .ok_or_else(|| anyhow::anyhow!("No adapter knows plugin: {plugin_id}"))?;

        let descriptor = self.adapters[adapter_idx]
            .list_plugins()
            .iter()
            .find(|p| p.id == plugin_id)
            .cloned()
            .unwrap(); // safe — we just found it above

        let host_id = self.adapters[adapter_idx]
            .instantiate(plugin_id, sample_rate, block_size)?;

        let registry_id = self.next_id;
        self.next_id += 1;

        self.instances.push(PluginInstance {
            registry_id,
            host_id,
            adapter_idx,
            mixer_slot: None,
            descriptor,
            state: InstanceState::Active,
        });

        debug!("PluginRegistry: instantiated {plugin_id} → registry id {registry_id}");
        Ok(registry_id)
    }

    /// Build a standalone, realtime-installable instrument source for a plugin
    /// (currently LV2). Returns `None` if no adapter knows the plugin or the
    /// plugin isn't an installable instrument. The caller installs the returned
    /// source into a mixer slot and drives it with note/CC events.
    pub fn create_audio_source(
        &self,
        plugin_id: &str,
        sample_rate: u32,
        block_size: u32,
    ) -> Option<Box<dyn seqterm_ports::realtime::AudioSource>> {
        self.adapters
            .iter()
            .find(|a| a.list_plugins().iter().any(|p| p.id == plugin_id))
            .and_then(|a| a.create_audio_source(plugin_id, sample_rate, block_size))
    }

    /// Wire an instance to a mixer slot.
    pub fn assign_mixer_slot(&mut self, registry_id: u64, slot: usize) -> Result<()> {
        let inst = self.instances.iter_mut()
            .find(|i| i.registry_id == registry_id && i.state != InstanceState::Destroyed)
            .ok_or_else(|| anyhow::anyhow!("Instance {registry_id} not found"))?;
        inst.mixer_slot = Some(slot);
        Ok(())
    }

    /// Process one audio block through an active instance.
    pub fn process(
        &mut self,
        registry_id: u64,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<()> {
        let inst = self.instances.iter()
            .find(|i| i.registry_id == registry_id)
            .ok_or_else(|| anyhow::anyhow!("Instance {registry_id} not found"))?;

        if inst.state != InstanceState::Active {
            bail!("Instance {registry_id} is not active (state={:?})", inst.state);
        }

        let adapter_idx = inst.adapter_idx;
        let host_id     = inst.host_id;
        self.adapters[adapter_idx].process(host_id, input, output)
    }

    /// Suspend (silence) an instance without destroying it.
    pub fn suspend(&mut self, registry_id: u64) -> Result<()> {
        let inst = self.instances.iter_mut()
            .find(|i| i.registry_id == registry_id && i.state == InstanceState::Active)
            .ok_or_else(|| anyhow::anyhow!("Active instance {registry_id} not found"))?;
        inst.state = InstanceState::Suspended;
        Ok(())
    }

    /// Resume a suspended instance.
    pub fn resume(&mut self, registry_id: u64) -> Result<()> {
        let inst = self.instances.iter_mut()
            .find(|i| i.registry_id == registry_id && i.state == InstanceState::Suspended)
            .ok_or_else(|| anyhow::anyhow!("Suspended instance {registry_id} not found"))?;
        inst.state = InstanceState::Active;
        Ok(())
    }

    /// Destroy an instance and free its resources.
    pub fn destroy(&mut self, registry_id: u64) {
        if let Some(idx) = self.instances.iter().position(|i| i.registry_id == registry_id) {
            let inst = &mut self.instances[idx];
            if inst.state != InstanceState::Destroyed {
                self.adapters[inst.adapter_idx].destroy(inst.host_id);
                inst.state = InstanceState::Destroyed;
                debug!("PluginRegistry: destroyed instance {registry_id}");
            }
            self.instances.swap_remove(idx);
        }
    }

    /// All currently live instances (not destroyed).
    pub fn instances(&self) -> impl Iterator<Item = &PluginInstance> {
        self.instances.iter().filter(|i| i.state != InstanceState::Destroyed)
    }

    /// Instances of a specific format.
    pub fn instances_of_kind(&self, kind: PluginKind) -> impl Iterator<Item = &PluginInstance> {
        self.instances.iter()
            .filter(move |i| i.state != InstanceState::Destroyed && i.descriptor.kind == kind)
    }

    // ── Parameter access ──────────────────────────────────────────────────────

    /// Return the number of automatable parameters for an instance.
    pub fn param_count(&self, registry_id: u64) -> u32 {
        if let Some(inst) = self.instances.iter().find(|i| i.registry_id == registry_id) {
            return self.adapters[inst.adapter_idx].param_count(inst.host_id);
        }
        0
    }

    /// Get the current (normalised 0.0–1.0) value of a parameter.
    pub fn get_param(&self, registry_id: u64, param_id: u32) -> f32 {
        if let Some(inst) = self.instances.iter().find(|i| i.registry_id == registry_id) {
            return self.adapters[inst.adapter_idx].get_param(inst.host_id, param_id);
        }
        0.0
    }

    /// Set a parameter value (normalised 0.0–1.0).
    pub fn set_param(&mut self, registry_id: u64, param_id: u32, value: f32) {
        if let Some(inst) = self.instances.iter().find(|i| i.registry_id == registry_id) {
            let (adapter_idx, host_id) = (inst.adapter_idx, inst.host_id);
            self.adapters[adapter_idx].set_param(host_id, param_id, value);
        }
    }

    /// Human-readable parameter name.
    pub fn param_name(&self, registry_id: u64, param_id: u32) -> String {
        if let Some(inst) = self.instances.iter().find(|i| i.registry_id == registry_id) {
            return self.adapters[inst.adapter_idx].param_name(inst.host_id, param_id);
        }
        format!("P{param_id}")
    }

    /// Formatted display value for a parameter.
    pub fn param_display(&self, registry_id: u64, param_id: u32) -> String {
        if let Some(inst) = self.instances.iter().find(|i| i.registry_id == registry_id) {
            return self.adapters[inst.adapter_idx].param_display(inst.host_id, param_id);
        }
        String::new()
    }

    /// Unit label for a parameter.
    pub fn param_label(&self, registry_id: u64, param_id: u32) -> String {
        if let Some(inst) = self.instances.iter().find(|i| i.registry_id == registry_id) {
            return self.adapters[inst.adapter_idx].param_label(inst.host_id, param_id);
        }
        String::new()
    }

    // ── State persistence ─────────────────────────────────────────────────────

    /// Get the full plugin state as opaque bytes (effGetChunk).
    /// Returns `None` if the plugin does not support chunk-based state.
    pub fn get_state(&self, registry_id: u64) -> Option<Vec<u8>> {
        let inst = self.instances.iter()
            .find(|i| i.registry_id == registry_id && i.state != InstanceState::Destroyed)?;
        self.adapters[inst.adapter_idx].get_state(inst.host_id)
    }

    /// Restore plugin state from opaque bytes (effSetChunk).
    /// Returns true if the plugin acknowledged the state.
    pub fn set_state(&mut self, registry_id: u64, data: &[u8]) -> bool {
        if let Some(inst) = self.instances.iter()
            .find(|i| i.registry_id == registry_id && i.state != InstanceState::Destroyed)
        {
            let (adapter_idx, host_id) = (inst.adapter_idx, inst.host_id);
            return self.adapters[adapter_idx].set_state(host_id, data);
        }
        false
    }

    /// Collect state blobs for all active instances that support chunk state.
    /// Returns `(registry_id, plugin_id, state_bytes)` for each.
    pub fn collect_states(&self) -> Vec<(u64, String, Vec<u8>)> {
        self.instances.iter()
            .filter(|i| i.state != InstanceState::Destroyed)
            .filter_map(|i| {
                let data = self.adapters[i.adapter_idx].get_state(i.host_id)?;
                Some((i.registry_id, i.descriptor.id.clone(), data))
            })
            .collect()
    }

    /// Destroy all instances and clear adapters. Called on shutdown.
    pub fn shutdown(&mut self) {
        let ids: Vec<u64> = self.instances.iter().map(|i| i.registry_id).collect();
        for id in ids {
            self.destroy(id);
        }
        self.adapters.clear();
        debug!("PluginRegistry: shutdown complete");
    }
}

impl Default for PluginRegistry {
    fn default() -> Self { Self::new() }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::path::Path;
    use seqterm_ports::plugin::{PluginDescriptor, PluginHostPort, PluginKind};

    /// Stub adapter with pre-loaded descriptors but no real plugin loading.
    struct StubAdapter {
        plugins: Vec<PluginDescriptor>,
        next_id: u64,
    }

    impl StubAdapter {
        fn with_plugins(plugins: Vec<PluginDescriptor>) -> Box<dyn PluginHostPort> {
            Box::new(Self { plugins, next_id: 1 })
        }
    }

    impl PluginHostPort for StubAdapter {
        fn scan(&mut self, _dir: &Path) -> Result<Vec<PluginDescriptor>> {
            Ok(self.plugins.clone())
        }
        fn list_plugins(&self) -> &[PluginDescriptor] {
            &self.plugins
        }
        fn instantiate(&mut self, plugin_id: &str, _sr: u32, _bs: u32) -> Result<u64> {
            if self.plugins.iter().any(|p| p.id == plugin_id) {
                let id = self.next_id;
                self.next_id += 1;
                Ok(id)
            } else {
                anyhow::bail!("Unknown plugin: {plugin_id}");
            }
        }
        fn destroy(&mut self, _id: u64) {}
        fn process(&mut self, _id: u64, _input: &[f32], output: &mut [f32]) -> Result<()> {
            output.fill(0.0);
            Ok(())
        }
    }

    fn stub_desc(id: &str) -> PluginDescriptor {
        PluginDescriptor {
            id: id.into(),
            name: id.into(),
            vendor: "Test".into(),
            version: "1.0".into(),
            kind: PluginKind::Internal,
            path: std::path::PathBuf::from("/dev/null"),
            is_instrument: true,
            is_effect: false,
        }
    }

    #[test]
    fn empty_registry_lists_no_plugins() {
        let reg = PluginRegistry::new();
        assert!(reg.list_plugins().is_empty());
        assert_eq!(reg.instances().count(), 0);
    }

    #[test]
    fn register_adapter_lists_plugins() {
        let mut reg = PluginRegistry::new();
        reg.register_adapter(StubAdapter::with_plugins(vec![stub_desc("test:a")]));
        assert_eq!(reg.list_plugins().len(), 1);
        assert_eq!(reg.list_plugins()[0].id, "test:a");
    }

    #[test]
    fn instantiate_known_plugin_succeeds() {
        let mut reg = PluginRegistry::new();
        reg.register_adapter(StubAdapter::with_plugins(vec![stub_desc("test:synth")]));
        let id = reg.instantiate("test:synth", 48000, 256).unwrap();
        assert_eq!(reg.instances().count(), 1);
        assert_eq!(reg.instances().next().unwrap().registry_id, id);
    }

    #[test]
    fn instantiate_unknown_plugin_fails() {
        let mut reg = PluginRegistry::new();
        assert!(reg.instantiate("vst2:nonexistent", 48000, 256).is_err());
    }

    #[test]
    fn assign_mixer_slot_persists() {
        let mut reg = PluginRegistry::new();
        reg.register_adapter(StubAdapter::with_plugins(vec![stub_desc("test:fx")]));
        let id = reg.instantiate("test:fx", 48000, 256).unwrap();
        reg.assign_mixer_slot(id, 4).unwrap();
        assert_eq!(reg.instances().next().unwrap().mixer_slot, Some(4));
    }

    #[test]
    fn process_zeroes_output_via_stub() {
        let mut reg = PluginRegistry::new();
        reg.register_adapter(StubAdapter::with_plugins(vec![stub_desc("test:fx")]));
        let id = reg.instantiate("test:fx", 48000, 256).unwrap();
        let input = vec![1.0f32; 512];
        let mut output = vec![1.0f32; 512];
        reg.process(id, &input, &mut output).unwrap();
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn suspend_and_resume_lifecycle() {
        let mut reg = PluginRegistry::new();
        reg.register_adapter(StubAdapter::with_plugins(vec![stub_desc("test:s")]));
        let id = reg.instantiate("test:s", 48000, 256).unwrap();
        assert_eq!(reg.instances().next().unwrap().state, InstanceState::Active);

        reg.suspend(id).unwrap();
        assert_eq!(reg.instances().next().unwrap().state, InstanceState::Suspended);

        reg.resume(id).unwrap();
        assert_eq!(reg.instances().next().unwrap().state, InstanceState::Active);
    }

    #[test]
    fn destroy_removes_instance() {
        let mut reg = PluginRegistry::new();
        reg.register_adapter(StubAdapter::with_plugins(vec![stub_desc("test:d")]));
        let id = reg.instantiate("test:d", 48000, 256).unwrap();
        assert_eq!(reg.instances().count(), 1);
        reg.destroy(id);
        assert_eq!(reg.instances().count(), 0);
    }

    #[test]
    fn process_rejected_for_suspended_instance() {
        let mut reg = PluginRegistry::new();
        reg.register_adapter(StubAdapter::with_plugins(vec![stub_desc("test:p")]));
        let id = reg.instantiate("test:p", 48000, 256).unwrap();
        reg.suspend(id).unwrap();
        let result = reg.process(id, &[], &mut []);
        assert!(result.is_err());
    }

    #[test]
    fn shutdown_clears_everything() {
        let mut reg = PluginRegistry::new();
        reg.register_adapter(StubAdapter::with_plugins(vec![stub_desc("test:x")]));
        reg.instantiate("test:x", 48000, 256).unwrap();
        reg.shutdown();
        assert_eq!(reg.instances().count(), 0);
        assert_eq!(reg.list_plugins().len(), 0);
    }

    #[test]
    fn two_adapters_merged_list() {
        let mut reg = PluginRegistry::new();
        reg.register_adapter(StubAdapter::with_plugins(vec![stub_desc("test:a")]));
        reg.register_adapter(StubAdapter::with_plugins(vec![stub_desc("test:b")]));
        assert_eq!(reg.list_plugins().len(), 2);
    }
}
