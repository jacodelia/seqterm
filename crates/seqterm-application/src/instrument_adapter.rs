//! Adapter exposing a hosted plugin instance through the universal
//! [`ParameterProvider`] API, so the UI / modulation / automation engines can
//! drive any plugin format without format-specific code.
//!
//! Hosts report parameters in **normalised 0–1** form via [`PluginHostPort`],
//! so the universal parameters are modelled as `Float` over `[0, 1]` with the
//! host's name / unit / formatted display carried through.

use seqterm_ports::instrument::{Parameter, ParameterProvider, ParameterType};

use crate::plugin_registry::PluginRegistry;

/// Borrowing view of one plugin instance's parameters as a [`ParameterProvider`].
///
/// Created via [`PluginRegistry::parameters`]. The borrow keeps the registry
/// pinned for the adapter's lifetime; reads use `&self`, writes use `&mut self`.
pub struct PluginParameters<'a> {
    registry: &'a mut PluginRegistry,
    registry_id: u64,
}

impl<'a> PluginParameters<'a> {
    pub(crate) fn new(registry: &'a mut PluginRegistry, registry_id: u64) -> Self {
        Self { registry, registry_id }
    }
}

impl ParameterProvider for PluginParameters<'_> {
    fn parameter_count(&self) -> usize {
        self.registry.param_count(self.registry_id) as usize
    }

    fn parameter(&self, index: usize) -> Option<Parameter> {
        let count = self.registry.param_count(self.registry_id) as usize;
        if index >= count { return None; }
        let pid = index as u32;
        let norm = self.registry.get_param(self.registry_id, pid) as f64;
        let name = self.registry.param_name(self.registry_id, pid);
        let unit = self.registry.param_label(self.registry_id, pid);
        // Hosts report normalised 0–1 values; model as Float over [0, 1].
        // The host's richer formatted display is available via `host_display`.
        Some(Parameter {
            id: index.to_string(),
            name,
            kind: ParameterType::Float,
            value: norm,
            minimum: 0.0,
            maximum: 1.0,
            default: norm,
            unit,
            automatable: true,
            modulatable: true,
            read_only: false,
            enum_values: Vec::new(),
        })
    }

    fn set_parameter(&mut self, index: usize, value: f64) {
        let count = self.registry.param_count(self.registry_id) as usize;
        if index >= count { return; }
        // Universal value is already native == normalised 0–1 for plugins.
        let norm = value.clamp(0.0, 1.0) as f32;
        self.registry.set_param(self.registry_id, index as u32, norm);
    }
}

impl PluginParameters<'_> {
    /// The host's own formatted display string for a parameter (e.g. "127.3 Hz"),
    /// which can be richer than the universal `Parameter::display()`.
    pub fn host_display(&self, index: usize) -> String {
        self.registry.param_display(self.registry_id, index as u32)
    }
}

impl PluginRegistry {
    /// Borrow an instance's parameters as a universal [`ParameterProvider`].
    pub fn parameters(&mut self, registry_id: u64) -> PluginParameters<'_> {
        PluginParameters::new(self, registry_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seqterm_ports::plugin::{PluginDescriptor, PluginHostPort, PluginKind};
    use anyhow::Result;

    /// A fake host with 2 params to exercise the adapter without real plugins.
    struct FakeHost {
        plugins: Vec<PluginDescriptor>,
        values: Vec<f32>,
    }
    impl FakeHost {
        fn new() -> Self {
            Self {
                plugins: vec![PluginDescriptor {
                    id: "fake:synth".into(), name: "Fake".into(), vendor: "T".into(),
                    version: "1".into(), kind: PluginKind::Lv2,
                    path: std::path::PathBuf::from("/x"), is_instrument: true, is_effect: false,
                }],
                values: vec![0.25, 0.5],
            }
        }
    }
    impl PluginHostPort for FakeHost {
        fn scan(&mut self, _dir: &std::path::Path) -> Result<Vec<PluginDescriptor>> { Ok(self.plugins.clone()) }
        fn list_plugins(&self) -> &[PluginDescriptor] { &self.plugins }
        fn instantiate(&mut self, _id: &str, _sr: u32, _bs: u32) -> Result<u64> { Ok(1) }
        fn destroy(&mut self, _id: u64) {}
        fn process(&mut self, _id: u64, _i: &[f32], _o: &mut [f32]) -> Result<()> { Ok(()) }
        fn param_count(&self, _id: u64) -> u32 { 2 }
        fn get_param(&self, _id: u64, p: u32) -> f32 { self.values[p as usize] }
        fn set_param(&mut self, _id: u64, p: u32, v: f32) { self.values[p as usize] = v; }
        fn param_name(&self, _id: u64, p: u32) -> String { format!("Param{p}") }
    }

    #[test]
    fn adapter_exposes_and_writes_params() {
        let mut reg = PluginRegistry::new();
        reg.register_adapter(Box::new(FakeHost::new()));
        let rid = reg.instantiate("fake:synth", 48_000, 512).unwrap();

        let params = reg.parameters(rid);
        assert_eq!(params.parameter_count(), 2);
        let p0 = params.parameter(0).unwrap();
        assert_eq!(p0.name, "Param0");
        assert_eq!(p0.kind, ParameterType::Float);
        assert!((p0.value - 0.25).abs() < 1e-6);
        drop(params);

        let mut params = reg.parameters(rid);
        params.set_parameter_normalized(0, 0.8);
        assert!((params.parameter(0).unwrap().value - 0.8).abs() < 1e-6);
    }
}
