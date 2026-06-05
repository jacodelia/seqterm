//! LV2 plugin host adapter.
//!
//! Implements [`PluginHostPort`] by parsing LV2 bundle TTL (Turtle/RDF) with a
//! pure-Rust parser and loading the plugin shared library via `libloading`,
//! driving the LV2 C ABI directly — no `liblilv`, no LV2 SDK.
//!
//! ```text
//! seqterm-plugin-lv2
//!   ├── Lv2PluginHost   ← implements PluginHostPort
//!   │     ├── scan(dir)        ← parses *.lv2 bundle TTL → descriptors + ports
//!   │     ├── instantiate(uri) ← dlopen + lv2_descriptor + connect_port + activate
//!   │     ├── process(in,out)  ← run()
//!   │     └── send_midi(...)   ← feeds an Atom/MIDI sequence to instrument ports
//!   └── Lv2Instance  (raw handle + the Library kept alive + port buffers)
//! ```

pub mod discovery;
pub mod lv2_abi;
pub mod ttl;

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use libloading::Library;
use parking_lot::Mutex;
use tracing::debug;

use seqterm_ports::plugin::{PluginDescriptor, PluginHostPort, PluginKind, PluginMidi};

use discovery::{Lv2PluginInfo, Port, PortKind};
use lv2_abi::*;

// ─── URID map (shared across instances of one host) ─────────────────────────

/// Backing store for the `urid:map`/`urid:unmap` host features. Assigns a stable
/// integer to each URI and keeps the C strings alive for `unmap`.
struct UridStore {
    map: HashMap<String, u32>,
    names: Vec<CString>, // names[urid - 1]
    next: u32,
}

impl UridStore {
    fn new() -> Self {
        Self { map: HashMap::new(), names: Vec::new(), next: 1 }
    }
    fn intern(&mut self, uri: &str) -> u32 {
        if let Some(&id) = self.map.get(uri) {
            return id;
        }
        let id = self.next;
        self.next += 1;
        self.map.insert(uri.to_string(), id);
        self.names.push(CString::new(uri).unwrap_or_default());
        id
    }
}

unsafe extern "C" fn urid_map_fn(handle: LV2_URID_Map_Handle, uri: *const c_char) -> LV2_URID {
    if uri.is_null() || handle.is_null() {
        return 0;
    }
    let store = unsafe { &*(handle as *const Mutex<UridStore>) };
    let s = unsafe { CStr::from_ptr(uri) }.to_string_lossy().into_owned();
    store.lock().intern(&s)
}

unsafe extern "C" fn urid_unmap_fn(handle: LV2_URID_Unmap_Handle, urid: LV2_URID) -> *const c_char {
    if handle.is_null() || urid == 0 {
        return std::ptr::null();
    }
    let store = unsafe { &*(handle as *const Mutex<UridStore>) };
    let guard = store.lock();
    match guard.names.get((urid - 1) as usize) {
        Some(cs) => cs.as_ptr(),
        None => std::ptr::null(),
    }
}

/// Pre-built host features (`urid:map`, `urid:unmap`) for one instance. The boxed
/// structs and the null-terminated pointer array must outlive the plugin.
struct Features {
    _store: Arc<Mutex<UridStore>>,
    _map: Box<LV2_URID_Map>,
    _unmap: Box<LV2_URID_Unmap>,
    _map_uri: CString,
    _unmap_uri: CString,
    _feats: Vec<LV2_Feature>,
    /// Null-terminated array of `*const LV2_Feature` passed to `instantiate`.
    ptrs: Vec<*const LV2_Feature>,
    /// The URID assigned to `midi:MidiEvent` (for building Atom sequences).
    midi_urid: u32,
}

impl Features {
    fn new(store: Arc<Mutex<UridStore>>) -> Self {
        let store_ptr = Arc::as_ptr(&store) as *mut c_void;
        let mut map = Box::new(LV2_URID_Map { handle: store_ptr, map: Some(urid_map_fn) });
        let mut unmap = Box::new(LV2_URID_Unmap { handle: store_ptr, unmap: Some(urid_unmap_fn) });
        let map_uri = CString::new(LV2_URID_MAP_URI).unwrap();
        let unmap_uri = CString::new(LV2_URID_UNMAP_URI).unwrap();
        let midi_urid = store.lock().intern(LV2_MIDI_EVENT_URI);

        let feats = vec![
            LV2_Feature {
                uri: map_uri.as_ptr(),
                data: map.as_mut() as *mut LV2_URID_Map as *mut c_void,
            },
            LV2_Feature {
                uri: unmap_uri.as_ptr(),
                data: unmap.as_mut() as *mut LV2_URID_Unmap as *mut c_void,
            },
        ];
        // Pointers into the Vec's heap buffer, which stays put when `feats`
        // (the Vec header) is moved into `Self` below.
        let ptrs: Vec<*const LV2_Feature> = feats
            .iter()
            .map(|f| f as *const LV2_Feature)
            .chain(std::iter::once(std::ptr::null()))
            .collect();

        Self {
            _store: store,
            _map: map,
            _unmap: unmap,
            _map_uri: map_uri,
            _unmap_uri: unmap_uri,
            _feats: feats,
            ptrs,
            midi_urid,
        }
    }

    /// URIs of features we provide; anything else in `requiredFeature` is fatal.
    fn supported(uri: &str) -> bool {
        uri == LV2_URID_MAP_URI || uri == LV2_URID_UNMAP_URI
    }
}

// ─── Instance ───────────────────────────────────────────────────────────────

struct Lv2Instance {
    handle: LV2_Handle,
    descriptor: *const LV2_Descriptor,
    _lib: Arc<Library>,
    features: Features,
    info: Lv2PluginInfo,
    block_size: usize,

    /// One f32 cell per port index (control + fallback for unused ports).
    control_values: Vec<f32>,
    /// Per-port audio buffer (only audio ports are non-empty), indexed by port idx.
    audio_bufs: Vec<Vec<f32>>,
    /// Per-port atom byte buffer (only atom ports are non-empty), indexed by port idx.
    atom_bufs: Vec<Vec<u8>>,

    audio_in: Vec<usize>,   // port indices
    audio_out: Vec<usize>,  // port indices
    atom_in: Option<usize>, // first MIDI atom input port index
    atom_out: Vec<usize>,   // atom output port indices (need capacity reset)
    /// Control input port indices, in display order — these are the parameters.
    params: Vec<usize>,

    /// MIDI queued via `send_midi`, drained into the atom sequence each `process`.
    pending_midi: Vec<[u8; 3]>,
    activated: bool,
}

// SAFETY: raw pointers into the loaded library are only touched while the
// owning `Lv2PluginHost` holds its `Mutex`, serialising all access.
unsafe impl Send for Lv2Instance {}
unsafe impl Sync for Lv2Instance {}

const ATOM_BUF_BYTES: usize = 8192;
/// Cap on MIDI events queued between `process` calls (RT-safe: drop when full).
const MAX_PENDING_MIDI: usize = 256;

impl Lv2Instance {
    fn connect_all(&mut self) {
        let Some(connect) = (unsafe { (*self.descriptor).connect_port }) else { return };
        let nports = self.control_values.len();
        for i in 0..nports {
            let ptr: *mut c_void = if !self.audio_bufs[i].is_empty() {
                self.audio_bufs[i].as_mut_ptr() as *mut c_void
            } else if !self.atom_bufs[i].is_empty() {
                self.atom_bufs[i].as_mut_ptr() as *mut c_void
            } else {
                (&mut self.control_values[i]) as *mut f32 as *mut c_void
            };
            unsafe { connect(self.handle, i as u32, ptr) };
        }
    }

    /// Write the queued MIDI as an `LV2_Atom_Sequence` into the MIDI input port.
    fn write_midi_sequence(&mut self) {
        let Some(idx) = self.atom_in else { return };
        let midi_urid = self.features.midi_urid;
        let buf = &mut self.atom_bufs[idx];
        if buf.len() < std::mem::size_of::<LV2_Atom_Sequence>() {
            return;
        }
        // Sequence header: atom.size will be filled after writing events.
        let mut write = std::mem::size_of::<LV2_Atom_Sequence>();
        for msg in &self.pending_midi {
            let ev_hdr = std::mem::size_of::<LV2_Atom_Event>();
            let needed = pad8(ev_hdr + 3);
            if write + needed > buf.len() {
                break;
            }
            let ev = LV2_Atom_Event {
                frames: 0,
                body: LV2_Atom { size: 3, type_: midi_urid },
            };
            // Copy event header.
            let ev_bytes = unsafe {
                std::slice::from_raw_parts(&ev as *const _ as *const u8, ev_hdr)
            };
            buf[write..write + ev_hdr].copy_from_slice(ev_bytes);
            // Copy 3 MIDI bytes after the header.
            buf[write + ev_hdr..write + ev_hdr + 3].copy_from_slice(&msg[..3]);
            write += needed;
        }
        let body_size = (write - std::mem::size_of::<LV2_Atom>()) as u32;
        let seq = LV2_Atom_Sequence {
            atom: LV2_Atom { size: body_size, type_: 0 }, // type filled by host=Sequence; 0 works for many
            body: LV2_Atom_Sequence_Body { unit: 0, pad: 0 },
        };
        let seq_bytes = unsafe {
            std::slice::from_raw_parts(&seq as *const _ as *const u8, std::mem::size_of::<LV2_Atom_Sequence>())
        };
        buf[..seq_bytes.len()].copy_from_slice(seq_bytes);
        self.pending_midi.clear();
    }

    /// Reset atom OUTPUT ports to an empty Chunk with full available capacity,
    /// as required before `run()`.
    fn reset_atom_outputs(&mut self) {
        for &idx in &self.atom_out {
            let buf = &mut self.atom_bufs[idx];
            if buf.len() < std::mem::size_of::<LV2_Atom>() {
                continue;
            }
            let cap = (buf.len() - std::mem::size_of::<LV2_Atom>()) as u32;
            let atom = LV2_Atom { size: cap, type_: 0 };
            let bytes = unsafe {
                std::slice::from_raw_parts(&atom as *const _ as *const u8, std::mem::size_of::<LV2_Atom>())
            };
            buf[..bytes.len()].copy_from_slice(bytes);
        }
    }

    fn run(&mut self, frames: usize) {
        if let Some(run) = unsafe { (*self.descriptor).run } {
            unsafe { run(self.handle, frames as u32) };
        }
    }

    /// Render up to one block as an instrument: feed queued MIDI, run, and write
    /// interleaved-stereo audio into `output`. Audio inputs (if any) are silenced.
    /// Returns frames written (`<= block_size`). RT-safe: no allocation.
    fn render_block(&mut self, output: &mut [f32]) -> usize {
        let frames = (output.len() / 2).min(self.block_size);
        if frames == 0 {
            return 0;
        }
        for &pi in &self.audio_in {
            for v in self.audio_bufs[pi].iter_mut().take(frames) {
                *v = 0.0;
            }
        }
        self.write_midi_sequence();
        self.reset_atom_outputs();
        self.run(frames);

        let n_out = self.audio_out.len();
        for f in 0..frames {
            let (l, r) = match n_out {
                0 => (0.0, 0.0),
                1 => {
                    let v = self.audio_bufs[self.audio_out[0]][f];
                    (v, v)
                }
                _ => (
                    self.audio_bufs[self.audio_out[0]][f],
                    self.audio_bufs[self.audio_out[1]][f],
                ),
            };
            output[f * 2] = l;
            output[f * 2 + 1] = r;
        }
        frames
    }

    /// Queue a raw 3-byte MIDI message for the next render (bounded; drops when full).
    fn queue_midi(&mut self, bytes: [u8; 3]) {
        if self.atom_in.is_some() && self.pending_midi.len() < MAX_PENDING_MIDI {
            self.pending_midi.push(bytes);
        }
    }
}

impl Drop for Lv2Instance {
    fn drop(&mut self) {
        unsafe {
            if self.activated
                && let Some(deactivate) = (*self.descriptor).deactivate
            {
                deactivate(self.handle);
            }
            if let Some(cleanup) = (*self.descriptor).cleanup {
                cleanup(self.handle);
            }
        }
    }
}

// ─── Host ─────────────────────────────────────────────────────────────────

/// LV2 host adapter. One instance owns all discovered descriptors and live
/// plugin instances, plus a shared URID map.
pub struct Lv2PluginHost {
    plugins: Vec<PluginDescriptor>,
    /// Full port metadata keyed by plugin URI (the descriptor `id`).
    infos: HashMap<String, Lv2PluginInfo>,
    instances: HashMap<u64, Lv2Instance>,
    libs: HashMap<PathBuf, Arc<Library>>,
    urids: Arc<Mutex<UridStore>>,
    next_id: u64,
}

impl Default for Lv2PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Lv2PluginHost {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            infos: HashMap::new(),
            instances: HashMap::new(),
            libs: HashMap::new(),
            urids: Arc::new(Mutex::new(UridStore::new())),
            next_id: 1,
        }
    }

    /// Build a standalone, RT-installable instrument source for a discovered
    /// plugin URI. The returned source owns its own library handle and URID map,
    /// so it can be moved onto the audio thread independently of this host.
    /// Errors if the plugin is unknown or has no MIDI input (not an instrument).
    pub fn create_instrument_source(
        &self,
        plugin_id: &str,
        sample_rate: u32,
        block_size: u32,
    ) -> Result<Lv2InstrumentSource> {
        let info = self
            .infos
            .get(plugin_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("LV2 plugin not found: {plugin_id}"))?;
        let lib = load_library(&info.binary_path)?;
        // Each source gets its own URID map so it is fully self-contained.
        let urids = Arc::new(Mutex::new(UridStore::new()));
        let inst = build_instance(info, lib, urids, sample_rate, block_size)?;
        if inst.atom_in.is_none() {
            bail!("LV2 plugin {plugin_id} has no MIDI input; not an instrument");
        }
        Ok(Lv2InstrumentSource { inst, active: false })
    }

    fn descriptor_for(info: &Lv2PluginInfo) -> PluginDescriptor {
        PluginDescriptor {
            id: info.uri.clone(),
            name: info.name.clone(),
            vendor: String::new(),
            version: String::new(),
            kind: PluginKind::Lv2,
            path: info.bundle_dir.clone(),
            is_instrument: info.is_instrument,
            is_effect: info.is_effect,
        }
    }
}

impl PluginHostPort for Lv2PluginHost {
    fn scan(&mut self, dir: &Path) -> Result<Vec<PluginDescriptor>> {
        let mut found = Vec::new();
        scan_bundles(dir, &mut |bundle| {
            for info in discovery::discover_bundle(bundle) {
                let desc = Self::descriptor_for(&info);
                if !self.infos.contains_key(&info.uri) {
                    self.infos.insert(info.uri.clone(), info);
                    self.plugins.push(desc.clone());
                    found.push(desc);
                }
            }
        });
        Ok(found)
    }

    fn list_plugins(&self) -> &[PluginDescriptor] {
        &self.plugins
    }

    fn instantiate(&mut self, plugin_id: &str, sample_rate: u32, block_size: u32) -> Result<u64> {
        let info = self
            .infos
            .get(plugin_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("LV2 plugin not found: {plugin_id}"))?;

        // Load (and cache) the shared library.
        let lib = match self.libs.get(&info.binary_path) {
            Some(l) => Arc::clone(l),
            None => {
                let l = load_library(&info.binary_path)?;
                self.libs.insert(info.binary_path.clone(), Arc::clone(&l));
                l
            }
        };

        let inst = build_instance(info, lib, Arc::clone(&self.urids), sample_rate, block_size)?;

        let id = self.next_id;
        self.next_id += 1;
        debug!("LV2: instantiated {} → id {id}", inst.info.uri);
        self.instances.insert(id, inst);
        Ok(id)
    }

    fn destroy(&mut self, instance_id: u64) {
        self.instances.remove(&instance_id);
    }

    fn process(&mut self, instance_id: u64, input: &[f32], output: &mut [f32]) -> Result<()> {
        let Some(inst) = self.instances.get_mut(&instance_id) else {
            return Ok(());
        };
        let frames = (output.len() / 2).min(inst.block_size);

        // Interleaved stereo input → per-port audio input buffers (mono dup).
        for (ch, &pi) in inst.audio_in.iter().enumerate() {
            let buf = &mut inst.audio_bufs[pi];
            for (f, slot) in buf.iter_mut().enumerate().take(frames) {
                let src = f * 2 + ch.min(1);
                *slot = input.get(src).copied().unwrap_or(0.0);
            }
        }

        inst.write_midi_sequence();
        inst.reset_atom_outputs();
        inst.run(frames);

        // Per-port audio output → interleaved stereo (mono → both channels).
        let n_out = inst.audio_out.len();
        for f in 0..frames {
            let (l, r) = match n_out {
                0 => (0.0, 0.0),
                1 => {
                    let v = inst.audio_bufs[inst.audio_out[0]][f];
                    (v, v)
                }
                _ => (
                    inst.audio_bufs[inst.audio_out[0]][f],
                    inst.audio_bufs[inst.audio_out[1]][f],
                ),
            };
            if f * 2 + 1 < output.len() {
                output[f * 2] = l;
                output[f * 2 + 1] = r;
            }
        }
        Ok(())
    }

    // ── Parameters (control input ports) ──────────────────────────────────────

    fn param_count(&self, instance_id: u64) -> u32 {
        self.instances
            .get(&instance_id)
            .map(|i| i.params.len() as u32)
            .unwrap_or(0)
    }

    fn get_param(&self, instance_id: u64, param_id: u32) -> f32 {
        let Some(inst) = self.instances.get(&instance_id) else { return 0.0 };
        let Some(&pi) = inst.params.get(param_id as usize) else { return 0.0 };
        let port = port_by_index(&inst.info, pi);
        let raw = inst.control_values[pi];
        match port {
            Some(p) if p.max > p.min => (raw - p.min) / (p.max - p.min),
            _ => raw,
        }
    }

    fn set_param(&mut self, instance_id: u64, param_id: u32, value: f32) {
        let Some(inst) = self.instances.get_mut(&instance_id) else { return };
        let Some(&pi) = inst.params.get(param_id as usize) else { return };
        let (min, max) = port_by_index(&inst.info, pi)
            .map(|p| (p.min, p.max))
            .unwrap_or((0.0, 1.0));
        let v = value.clamp(0.0, 1.0);
        inst.control_values[pi] = if max > min { min + v * (max - min) } else { v };
    }

    fn param_name(&self, instance_id: u64, param_id: u32) -> String {
        self.instances
            .get(&instance_id)
            .and_then(|i| i.params.get(param_id as usize).copied())
            .and_then(|pi| port_by_index(&self.instances[&instance_id].info, pi).cloned())
            .map(|p| if p.name.is_empty() { p.symbol } else { p.name })
            .unwrap_or_else(|| format!("P{param_id}"))
    }

    fn param_display(&self, instance_id: u64, param_id: u32) -> String {
        let Some(inst) = self.instances.get(&instance_id) else {
            return String::new();
        };
        let Some(&pi) = inst.params.get(param_id as usize) else {
            return String::new();
        };
        format!("{:.3}", inst.control_values[pi])
    }

    fn send_midi(&mut self, instance_id: u64, events: &[PluginMidi]) {
        let Some(inst) = self.instances.get_mut(&instance_id) else { return };
        for ev in events {
            inst.queue_midi(midi_bytes(*ev));
        }
    }

    fn create_audio_source(
        &self,
        plugin_id: &str,
        sample_rate: u32,
        block_size: u32,
    ) -> Option<Box<dyn seqterm_ports::AudioSource>> {
        match self.create_instrument_source(plugin_id, sample_rate, block_size) {
            Ok(src) => Some(Box::new(src)),
            Err(e) => {
                debug!("LV2: create_audio_source({plugin_id}) failed: {e}");
                None
            }
        }
    }
}

/// `dlopen` an LV2 binary into a reference-counted `Library`.
fn load_library(path: &Path) -> Result<Arc<Library>> {
    let l = unsafe {
        Library::new(path).with_context(|| format!("dlopen {}", path.display()))?
    };
    Ok(Arc::new(l))
}

/// Build a fully connected, activated [`Lv2Instance`] from a parsed plugin info
/// and an already-loaded library. Shared by the host's `instantiate` and the
/// standalone [`Lv2InstrumentSource`].
fn build_instance(
    info: Lv2PluginInfo,
    lib: Arc<Library>,
    urids: Arc<Mutex<UridStore>>,
    sample_rate: u32,
    block_size: u32,
) -> Result<Lv2Instance> {
    // Refuse plugins needing features we don't provide.
    for feat in &info.required_features {
        if !Features::supported(feat) {
            bail!("LV2 plugin {} requires unsupported feature: {feat}", info.uri);
        }
    }

    // Resolve the entry point and find the descriptor matching the URI.
    let descriptor = unsafe {
        let entry: libloading::Symbol<Lv2DescriptorFn> = lib
            .get(LV2_DESCRIPTOR_SYM)
            .context("missing lv2_descriptor symbol")?;
        let mut i = 0u32;
        let mut found: *const LV2_Descriptor = std::ptr::null();
        loop {
            let d = entry(i);
            if d.is_null() {
                break;
            }
            let uri = CStr::from_ptr((*d).uri).to_string_lossy();
            if uri == info.uri {
                found = d;
                break;
            }
            i += 1;
        }
        if found.is_null() {
            bail!("descriptor URI {} not exported by binary", info.uri);
        }
        found
    };

    let features = Features::new(urids);

    // Allocate per-port buffers.
    let nports = info.ports.iter().map(|p| p.index as usize + 1).max().unwrap_or(0);
    let mut control_values = vec![0.0f32; nports];
    let mut audio_bufs = vec![Vec::<f32>::new(); nports];
    let mut atom_bufs = vec![Vec::<u8>::new(); nports];
    let (mut audio_in, mut audio_out, mut atom_out, mut params) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut atom_in = None;

    for p in &info.ports {
        let i = p.index as usize;
        if i >= nports {
            continue;
        }
        match p.kind {
            PortKind::AudioInput => {
                audio_bufs[i] = vec![0.0; block_size as usize];
                audio_in.push(i);
            }
            PortKind::AudioOutput => {
                audio_bufs[i] = vec![0.0; block_size as usize];
                audio_out.push(i);
            }
            PortKind::ControlInput => {
                control_values[i] = p.default;
                params.push(i);
            }
            PortKind::ControlOutput | PortKind::Unknown => {
                control_values[i] = p.default;
            }
            PortKind::AtomInput => {
                atom_bufs[i] = vec![0u8; ATOM_BUF_BYTES];
                if p.is_midi && atom_in.is_none() {
                    atom_in = Some(i);
                }
            }
            PortKind::AtomOutput => {
                atom_bufs[i] = vec![0u8; ATOM_BUF_BYTES];
                atom_out.push(i);
            }
        }
    }

    // Instantiate the plugin.
    let handle = unsafe {
        let instantiate = (*descriptor)
            .instantiate
            .ok_or_else(|| anyhow::anyhow!("plugin has no instantiate fn"))?;
        let bundle = CString::new(format!("{}/", info.bundle_dir.to_string_lossy()))
            .unwrap_or_default();
        let h = instantiate(
            descriptor,
            sample_rate as f64,
            bundle.as_ptr(),
            features.ptrs.as_ptr(),
        );
        if h.is_null() {
            bail!("instantiate returned null for {}", info.uri);
        }
        h
    };

    let mut inst = Lv2Instance {
        handle,
        descriptor,
        _lib: lib,
        features,
        info,
        block_size: block_size as usize,
        control_values,
        audio_bufs,
        atom_bufs,
        audio_in,
        audio_out,
        atom_in,
        atom_out,
        params,
        pending_midi: Vec::with_capacity(MAX_PENDING_MIDI),
        activated: false,
    };

    inst.connect_all();
    if let Some(activate) = unsafe { (*descriptor).activate } {
        unsafe { activate(handle) };
    }
    inst.activated = true;
    Ok(inst)
}

/// Find a port by its LV2 index.
fn port_by_index(info: &Lv2PluginInfo, index: usize) -> Option<&Port> {
    info.ports.iter().find(|p| p.index as usize == index)
}

/// Encode a [`PluginMidi`] event into a 3-byte raw MIDI message.
fn midi_bytes(ev: PluginMidi) -> [u8; 3] {
    match ev {
        PluginMidi::NoteOn { channel, note, velocity } => {
            [0x90 | (channel & 0x0F), note & 0x7F, velocity & 0x7F]
        }
        PluginMidi::NoteOff { channel, note } => [0x80 | (channel & 0x0F), note & 0x7F, 0],
        PluginMidi::Cc { channel, cc, value } => {
            [0xB0 | (channel & 0x0F), cc & 0x7F, value & 0x7F]
        }
        PluginMidi::PitchBend { channel, value } => {
            let v = (value as i32 + 8192).clamp(0, 16383) as u16;
            [0xE0 | (channel & 0x0F), (v & 0x7F) as u8, ((v >> 7) & 0x7F) as u8]
        }
    }
}

/// Maximum directory depth searched for `*.lv2` bundles below a search root.
/// Bundles live at most a couple levels deep; this bounds the walk so a stray
/// symlink or deep tree can never turn a scan into a filesystem-wide crawl.
const MAX_SCAN_DEPTH: usize = 4;

/// Walk `dir` recursively, invoking `f` for every `*.lv2` bundle directory.
/// Symlinked directories are not followed and the walk is depth-bounded, so a
/// symlink cycle or a link into a large tree can never hang the scan.
fn scan_bundles(dir: &Path, f: &mut impl FnMut(&Path)) {
    scan_bundles_depth(dir, 0, f);
}

fn scan_bundles_depth(dir: &Path, depth: usize, f: &mut impl FnMut(&Path)) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        // Use the entry's own (non-following) type so symlinked dirs are skipped.
        let is_dir = entry
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or(false);
        if !is_dir {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("lv2") {
            f(&path);
        } else if !is_pruned_dir(path.file_name().and_then(|n| n.to_str()).unwrap_or("")) {
            scan_bundles_depth(&path, depth + 1, f);
        }
    }
}

/// Directory names never worth descending into when searching for `.lv2` bundles
/// — version control, build output, and dependency caches.
fn is_pruned_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".svn" | ".hg"
            | "target" | "build" | "node_modules"
            | ".cargo" | ".rustup" | ".cache" | "__pycache__"
    )
}

/// Platform-default LV2 search directories (Carla-style).
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut p = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        p.push(home.join(".lv2"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        p.push(PathBuf::from("/usr/lib/lv2"));
        p.push(PathBuf::from("/usr/local/lib/lv2"));
        p.push(PathBuf::from("/usr/lib/x86_64-linux-gnu/lv2"));
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            p.push(home.join("Library/Audio/Plug-Ins/LV2"));
        }
        p.push(PathBuf::from("/Library/Audio/Plug-Ins/LV2"));
    }
    let _ = &mut p;
    p
}

// ─── Instrument source (RT-installable) ─────────────────────────────────────

/// A live LV2 instrument wrapped as an [`AudioSource`]/[`AudioSynthPort`] so it
/// can be installed directly into a mixer slot and driven by the scheduler's
/// note/CC events — the same path SF2 uses. Fully self-contained (owns its
/// library + URID map), so it is safe to move onto the audio thread.
pub struct Lv2InstrumentSource {
    inst: Lv2Instance,
    active: bool,
}

impl Lv2InstrumentSource {
    /// The plugin URI this source was built from.
    pub fn uri(&self) -> &str {
        &self.inst.info.uri
    }
}

impl seqterm_ports::AudioSource for Lv2InstrumentSource {
    fn render(&mut self, output: &mut [f32], _sample_rate: u32) -> usize {
        // Render in block_size-sized chunks (the callback block may be larger).
        let mut done = 0usize;
        let total = output.len() / 2;
        while done < total {
            let chunk = &mut output[done * 2..];
            let n = self.inst.render_block(chunk);
            if n == 0 {
                break;
            }
            done += n;
        }
        done
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn stop(&mut self) {
        // Silence all voices; the plugin tail (if any) decays on its own.
        for ch in 0..16u8 {
            self.inst.queue_midi([0xB0 | ch, 123, 0]); // All Notes Off
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn as_synth(&mut self) -> Option<&mut dyn seqterm_ports::AudioSynthPort> {
        Some(self)
    }
}

impl seqterm_ports::AudioSynthPort for Lv2InstrumentSource {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        self.active = true;
        self.inst.queue_midi(midi_bytes(PluginMidi::NoteOn { channel, note, velocity }));
    }

    fn note_off(&mut self, channel: u8, note: u8) {
        self.inst.queue_midi(midi_bytes(PluginMidi::NoteOff { channel, note }));
    }

    fn control_change(&mut self, channel: u8, cc: u8, value: u8) {
        self.inst.queue_midi(midi_bytes(PluginMidi::Cc { channel, cc, value }));
    }

    fn pitch_bend(&mut self, channel: u8, value: i16) {
        self.inst.queue_midi(midi_bytes(PluginMidi::PitchBend { channel, value }));
    }
}
