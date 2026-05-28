//! VST2 plugin host adapter.
//!
//! Implements [`PluginHostPort`] by dynamically loading VST2 shared libraries
//! (.so on Linux, .dylib/.vst on macOS, .dll on Windows) and communicating
//! via the VST2 binary ABI without requiring the official Steinberg SDK.
//!
//! # Architecture
//!
//! ```text
//! seqterm-plugin-vst2  (this crate)
//!   ├── Vst2PluginHost   ← implements PluginHostPort
//!   │     ├── scan(dir)  ← discovers .so/.vst/.dll files
//!   │     ├── instantiate(id, sr, bs)
//!   │     └── process(instance, in, out)
//!   └── Vst2Instance
//!         ├── AEffect*   (raw pointer into the loaded shared lib)
//!         └── Library    (keeps the .so alive for the instance lifetime)
//! ```
//!
//! # RT Safety
//!
//! `process()` calls `processReplacing` on the VST2 plugin. This is NOT
//! guaranteed to be RT-safe (the plugin may alloc). Use from a non-RT thread
//! or ensure the plugin is known-safe before calling from the audio callback.

pub mod vst2_abi;

use std::{
    os::raw::{c_float, c_int, c_void},
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use libloading::Library;
use parking_lot::Mutex;
use tracing::{debug, warn};

use seqterm_ports::plugin::{PluginDescriptor, PluginHostPort, PluginKind};
use vst2_abi::*;

// ─── Host callback ────────────────────────────────────────────────────────────

/// Global host callback invoked by VST2 plugins to communicate with the host.
/// SAFETY: VST2 requires this to be a plain function pointer (no closure).
unsafe extern "C" fn host_callback(
    _effect: AEffectPtr,
    opcode: i32,
    _index: i32,
    _value: isize,
    _ptr: *mut c_void,
    _opt: c_float,
) -> isize {
    match opcode {
        host_opcode::VERSION         => VST_VERSION as isize,
        host_opcode::CURRENT_ID      => 0,
        host_opcode::IDLE            => 0,
        host_opcode::GET_SAMPLE_RATE => 48000,
        host_opcode::GET_BLOCK_SIZE  => 512,
        host_opcode::CAN_DO          => 0,
        host_opcode::AUTOMATE        => 0,
        _ => 0,
    }
}

// ─── Instance ────────────────────────────────────────────────────────────────

struct Vst2Instance {
    /// Raw pointer to the VST2 AEffect struct (owned by the shared library).
    effect: AEffectPtr,
    /// Keeps the shared library loaded for as long as the instance lives.
    _lib: Arc<Library>,
    sample_rate: f32,
    block_size: usize,
    /// Pre-allocated input/output buffer pointers for processReplacing.
    input_ptrs:  Vec<Vec<f32>>,
    output_ptrs: Vec<Vec<f32>>,
}

// SAFETY: AEffect is behind a raw pointer. We ensure single-threaded access
// through the Mutex wrapping Vst2PluginHost.
unsafe impl Send for Vst2Instance {}
unsafe impl Sync for Vst2Instance {}

impl Vst2Instance {
    fn dispatch(&self, opcode: i32, index: i32, value: isize, ptr: *mut c_void, opt: f32) -> isize {
        // SAFETY: We call dispatcher only on a valid AEffect from the loaded library.
        unsafe {
            if let Some(dispatcher) = (*self.effect).dispatcher {
                dispatcher(self.effect, opcode, index, value, ptr, opt)
            } else {
                0
            }
        }
    }

    fn open(&self) {
        self.dispatch(opcode::OPEN, 0, 0, std::ptr::null_mut(), 0.0);
    }

    fn close(&self) {
        self.dispatch(opcode::CLOSE, 0, 0, std::ptr::null_mut(), 0.0);
    }

    fn set_sample_rate(&self) {
        self.dispatch(opcode::SET_SAMPLE_RATE, 0, 0, std::ptr::null_mut(), self.sample_rate);
    }

    fn set_block_size(&self) {
        self.dispatch(opcode::SET_BLOCK_SIZE, 0, self.block_size as isize, std::ptr::null_mut(), 0.0);
    }

    fn resume(&self) {
        self.dispatch(opcode::MAIN_RESUME, 0, 1, std::ptr::null_mut(), 0.0);
    }

    fn suspend(&self) {
        self.dispatch(opcode::MAIN_RESUME, 0, 0, std::ptr::null_mut(), 0.0);
    }

    fn num_inputs(&self)  -> usize { unsafe { (*self.effect).num_inputs  as usize } }
    fn num_outputs(&self) -> usize { unsafe { (*self.effect).num_outputs as usize } }

    /// Retrieve a NUL-terminated string via a dispatcher opcode.
    fn get_string(&self, op: i32, index: i32) -> String {
        let mut buf = [0u8; 256];
        self.dispatch(op, index, 0, buf.as_mut_ptr() as *mut c_void, 0.0);
        let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..nul]).trim().to_string()
    }

    fn process_replacing(&mut self, input: &[f32], output: &mut [f32]) {
        let n_in  = self.num_inputs();
        let n_out = self.num_outputs();
        let frames = self.block_size;

        // Copy interleaved input → de-interleaved channel buffers.
        for ch in 0..n_in {
            let buf = &mut self.input_ptrs[ch];
            for f in 0..frames {
                let src_idx = f * n_in.max(1) + ch.min(n_in.saturating_sub(1));
                buf[f] = input.get(src_idx).copied().unwrap_or(0.0);
            }
        }

        // Collect raw pointers (all within the pre-allocated Vecs).
        let mut in_ptrs:  Vec<*mut c_float> = self.input_ptrs.iter_mut()
            .map(|v| v.as_mut_ptr())
            .collect();
        let mut out_ptrs: Vec<*mut c_float> = self.output_ptrs.iter_mut()
            .map(|v| v.as_mut_ptr())
            .collect();

        // SAFETY: pointers are valid, non-overlapping, correct length.
        unsafe {
            if let Some(proc_fn) = (*self.effect).process_replacing {
                proc_fn(
                    self.effect,
                    in_ptrs.as_mut_ptr(),
                    out_ptrs.as_mut_ptr(),
                    frames as c_int,
                );
            }
        }

        // Copy de-interleaved output → interleaved output buffer.
        for f in 0..frames {
            for ch in 0..n_out {
                let dst_idx = f * n_out.max(1) + ch;
                if dst_idx < output.len() {
                    output[dst_idx] = self.output_ptrs[ch][f];
                }
            }
        }
    }
}

impl Drop for Vst2Instance {
    fn drop(&mut self) {
        self.suspend();
        self.close();
    }
}

// ─── Host ─────────────────────────────────────────────────────────────────────

/// VST2 plugin host. Implements [`PluginHostPort`].
pub struct Vst2PluginHost {
    known_plugins: Vec<PluginDescriptor>,
    instances: Vec<(u64, Mutex<Vst2Instance>)>,
    next_instance_id: u64,
    pub sample_rate: u32,
    pub block_size: u32,
}

impl Vst2PluginHost {
    pub fn new(sample_rate: u32, block_size: u32) -> Self {
        Self {
            known_plugins: Vec::new(),
            instances: Vec::new(),
            next_instance_id: 1,
            sample_rate,
            block_size,
        }
    }

    /// Load a single VST2 shared library and return a PluginDescriptor if valid.
    fn try_load_descriptor(path: &Path) -> Option<PluginDescriptor> {
        let lib = unsafe { Library::new(path) }.ok()?;

        // Try both common VST2 entry point names.
        let main_fn: libloading::Symbol<VstPluginMainFn> = unsafe {
            lib.get(b"VSTPluginMain\0")
                .or_else(|_| lib.get(b"main\0"))
                .ok()?
        };

        let effect: AEffectPtr = unsafe { main_fn(host_callback) };
        if effect.is_null() {
            return None;
        }

        // Validate magic number.
        if unsafe { (*effect).magic } != VST_MAGIC {
            return None;
        }

        // Extract plugin metadata via dispatcher calls.
        let get_str = |op: i32| -> String {
            let mut buf = [0i8; 256];
            unsafe {
                if let Some(disp) = (*effect).dispatcher {
                    disp(effect, op, 0, 0, buf.as_mut_ptr() as *mut c_void, 0.0);
                }
            }
            c_str_from_buf(&buf)
        };

        let name    = get_str(opcode::GET_PLUGIN_NAME);
        let vendor  = get_str(opcode::GET_VENDOR_STRING);
        let version = unsafe { (*effect).version }.to_string();
        let is_synth = (unsafe { (*effect).flags } & flags::IS_SYNTH) != 0;
        let id = format!("vst2:{:08x}", unsafe { (*effect).unique_id });

        // Close the effect — we opened it only for scanning.
        unsafe {
            if let Some(disp) = (*effect).dispatcher {
                disp(effect, opcode::CLOSE, 0, 0, std::ptr::null_mut(), 0.0);
            }
        }
        // The library is dropped here (closed), which is fine for scanning.
        drop(lib);

        Some(PluginDescriptor {
            id,
            name: if name.is_empty() {
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Unknown".into())
            } else { name },
            vendor,
            version,
            kind: PluginKind::Vst2,
            path: path.to_path_buf(),
            is_instrument: is_synth,
            is_effect: !is_synth,
        })
    }
}

impl PluginHostPort for Vst2PluginHost {
    fn scan(&mut self, dir: &Path) -> Result<Vec<PluginDescriptor>> {
        let extensions: &[&str] = &[
            "so",    // Linux
            "dylib", // macOS
            "vst",   // macOS bundle (handled as flat file here)
            "dll",   // Windows
        ];

        let mut found = Vec::new();

        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("Cannot read plugin directory: {}", dir.display()))?;

        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if !extensions.contains(&ext.as_str()) {
                continue;
            }

            debug!("Scanning VST2 candidate: {}", path.display());
            match Self::try_load_descriptor(&path) {
                Some(desc) => {
                    debug!("Found VST2 plugin: {} ({})", desc.name, desc.id);
                    found.push(desc);
                }
                None => {
                    warn!("Skipping non-VST2 or invalid library: {}", path.display());
                }
            }
        }

        // Merge new plugins into known_plugins (update if already present).
        for desc in &found {
            if let Some(existing) = self.known_plugins.iter_mut().find(|p| p.id == desc.id) {
                *existing = desc.clone();
            } else {
                self.known_plugins.push(desc.clone());
            }
        }

        Ok(found)
    }

    fn list_plugins(&self) -> &[PluginDescriptor] {
        &self.known_plugins
    }

    fn instantiate(&mut self, plugin_id: &str, sample_rate: u32, block_size: u32) -> Result<u64> {
        let desc = self.known_plugins.iter()
            .find(|p| p.id == plugin_id)
            .cloned()
            .with_context(|| format!("Plugin not found: {plugin_id}"))?;

        let path = desc.path.clone();

        // Load the shared library (kept alive for the instance lifetime).
        let lib = Arc::new(unsafe { Library::new(&path) }
            .with_context(|| format!("Failed to load: {}", path.display()))?);

        // Get the entry point.
        let main_fn: libloading::Symbol<VstPluginMainFn> = unsafe {
            lib.get(b"VSTPluginMain\0")
                .or_else(|_| lib.get(b"main\0"))
                .context("No VSTPluginMain entry point")?
        };

        let effect: AEffectPtr = unsafe { main_fn(host_callback) };
        if effect.is_null() {
            bail!("VSTPluginMain returned null for {plugin_id}");
        }
        if unsafe { (*effect).magic } != VST_MAGIC {
            bail!("Invalid VST2 magic number for {plugin_id}");
        }

        let sr = sample_rate as f32;
        let bs = block_size as usize;
        let n_in  = unsafe { (*effect).num_inputs  as usize };
        let n_out = unsafe { (*effect).num_outputs as usize };

        let inst = Vst2Instance {
            effect,
            _lib:        Arc::clone(&lib),
            sample_rate: sr,
            block_size:  bs,
            input_ptrs:  vec![vec![0.0f32; bs]; n_in.max(1)],
            output_ptrs: vec![vec![0.0f32; bs]; n_out.max(1)],
        };

        inst.open();
        inst.set_sample_rate();
        inst.set_block_size();
        inst.resume();

        let id = self.next_instance_id;
        self.next_instance_id += 1;
        self.instances.push((id, Mutex::new(inst)));

        debug!("Instantiated VST2 plugin {} → instance {id}", plugin_id);
        Ok(id)
    }

    fn destroy(&mut self, instance_id: u64) {
        if let Some(pos) = self.instances.iter().position(|(id, _)| *id == instance_id) {
            self.instances.swap_remove(pos);
            // Vst2Instance::drop() calls suspend + close.
        }
    }

    fn process(&mut self, instance_id: u64, input: &[f32], output: &mut [f32]) -> Result<()> {
        let inst_pair = self.instances.iter()
            .find(|(id, _)| *id == instance_id)
            .with_context(|| format!("Instance {instance_id} not found"))?;

        let mut inst = inst_pair.1.lock();
        inst.process_replacing(input, output);
        Ok(())
    }

    fn param_count(&self, instance_id: u64) -> u32 {
        self.instances.iter()
            .find(|(id, _)| *id == instance_id)
            .map(|(_, mu)| unsafe { (*mu.lock().effect).num_params as u32 })
            .unwrap_or(0)
    }

    fn get_param(&self, instance_id: u64, param_id: u32) -> f32 {
        self.instances.iter()
            .find(|(id, _)| *id == instance_id)
            .and_then(|(_, mu)| {
                let inst = mu.lock();
                unsafe {
                    (*inst.effect).get_parameter
                        .map(|f| f(inst.effect, param_id as i32))
                }
            })
            .unwrap_or(0.0)
    }

    fn set_param(&mut self, instance_id: u64, param_id: u32, value: f32) {
        if let Some((_, mu)) = self.instances.iter().find(|(id, _)| *id == instance_id) {
            let inst = mu.lock();
            unsafe {
                if let Some(f) = (*inst.effect).set_parameter {
                    f(inst.effect, param_id as i32, value);
                }
            }
        }
    }

    fn param_name(&self, instance_id: u64, param_id: u32) -> String {
        self.instances.iter()
            .find(|(id, _)| *id == instance_id)
            .map(|(_, mu)| {
                let inst = mu.lock();
                inst.get_string(opcode::GET_PARAM_NAME, param_id as i32)
            })
            .unwrap_or_default()
    }

    fn param_label(&self, instance_id: u64, param_id: u32) -> String {
        self.instances.iter()
            .find(|(id, _)| *id == instance_id)
            .map(|(_, mu)| {
                let inst = mu.lock();
                inst.get_string(opcode::GET_PARAM_LABEL, param_id as i32)
            })
            .unwrap_or_default()
    }

    fn param_display(&self, instance_id: u64, param_id: u32) -> String {
        self.instances.iter()
            .find(|(id, _)| *id == instance_id)
            .map(|(_, mu)| {
                let inst = mu.lock();
                inst.get_string(opcode::GET_PARAM_DISPLAY, param_id as i32)
            })
            .unwrap_or_default()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn host_creates_with_defaults() {
        let host = Vst2PluginHost::new(44100, 512);
        assert_eq!(host.list_plugins().len(), 0);
        assert_eq!(host.sample_rate, 44100);
        assert_eq!(host.block_size, 512);
    }

    #[test]
    fn scan_empty_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = Vst2PluginHost::new(48000, 256);
        let result = host.scan(dir.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn scan_nonexistent_dir_returns_error() {
        let mut host = Vst2PluginHost::new(48000, 256);
        let result = host.scan(Path::new("/nonexistent/path/for/vst2/test"));
        assert!(result.is_err());
    }

    #[test]
    fn destroy_nonexistent_instance_is_noop() {
        let mut host = Vst2PluginHost::new(48000, 256);
        // Should not panic.
        host.destroy(9999);
    }

    #[test]
    fn instantiate_unknown_plugin_returns_error() {
        let mut host = Vst2PluginHost::new(48000, 256);
        let result = host.instantiate("vst2:notfound", 48000, 256);
        assert!(result.is_err());
    }
}
