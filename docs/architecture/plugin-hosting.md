# Plugin Hosting

**Crate:** `seqterm-plugin-vst2`  
**Port:** `seqterm-ports::plugin::PluginHostPort`  
**Layer:** Infrastructure adapter

SeqTerm implements a VST2 plugin host adapter that loads external instrument and effect plugins as shared libraries at runtime. The adapter exposes a single `PluginHostPort` trait to the application layer, keeping the domain free of VST ABI specifics.

---

## Module Map

```
seqterm-plugin-vst2/src/
├── lib.rs        Vst2PluginHost (implements PluginHostPort) + Vst2Instance
└── vst2_abi.rs   Raw VST2 C ABI definitions (AEffect, opcodes, callbacks)
```

---

## Architecture

```
Application
  │
  └─ PluginHostPort (trait)
        │
        └─ Vst2PluginHost
              ├─ scan(dir)          discovers .so / .vst / .dll files
              ├─ instantiate(id, sr, bs)
              │    └─ Library::new(path)     libloading
              │    └─ VSTPluginMain(callback) → AEffect*
              │    └─ dispatch(effOpen, ...)
              │    └─ dispatch(effSetSampleRate, ...)
              │    └─ dispatch(effSetBlockSize, ...)
              └─ Vst2Instance
                    ├─ AEffect*       raw pointer into the .so
                    └─ Arc<Library>   keeps the shared library alive
```

---

## Plugin Discovery

`Vst2PluginHost::scan(dir: &Path) -> Vec<PluginDescriptor>` scans a directory recursively for files matching the platform extension (`.so` on Linux, `.dylib`/`.vst` on macOS, `.dll` on Windows). For each candidate:

1. `libloading::Library::new(path)` loads the shared object.
2. Attempts to resolve `VSTPluginMain` (fallback: `main`).
3. Calls the entry point with a dummy host callback to get the `AEffect*`.
4. Reads `AEffect.numInputs`, `numOutputs`, `numParams`, `flags`, and `uniqueID`.
5. Calls `dispatch(effGetProductString, ...)` to get the plugin name.
6. Returns a `PluginDescriptor` with the collected metadata.

Plugins that fail to load (missing symbol, crash, incompatible ABI) are skipped with a warning log.

---

## Instantiation

```rust
fn instantiate(&self, id: u32, sample_rate: u32, block_size: u32)
    -> Result<Arc<Mutex<dyn PluginInstance>>>
```

1. Loads the `.so` for the given plugin ID.
2. Calls `VSTPluginMain(host_callback)` to create a new plugin instance.
3. Dispatches: `effOpen`, `effSetSampleRate`, `effSetBlockSize`, `effMainsChanged(1)`.
4. Returns `Arc<Mutex<Vst2Instance>>`.

---

## Host Callback

VST2 plugins call back into the host through a C function pointer:

```rust
unsafe extern "C" fn host_callback(
    _effect: AEffectPtr,
    opcode: i32,
    _index: i32,
    _value: isize,
    _ptr: *mut c_void,
    _opt: c_float,
) -> isize
```

Currently responds to:

| Opcode | Response |
|--------|----------|
| `audioMasterVersion` | `2400` (VST 2.4) |
| `audioMasterCurrentId` | `0` |
| `audioMasterIdle` | `0` |
| `audioMasterGetSampleRate` | `48000` |
| `audioMasterGetBlockSize` | `512` |
| `audioMasterCanDo` | `0` (not supported) |

Other opcodes return `0`. This is sufficient for most VST2 instruments and effects that only query basic host capabilities.

---

## Processing

```rust
fn process(&self, instance: &Vst2Instance, in: &[&[f32]], out: &mut [&mut [f32]])
```

Calls the plugin's `processReplacing` function (`AEffect.processReplacing`), passing pre-allocated input and output channel buffers.

> **RT Safety Warning:** VST2 plugins are not guaranteed to be realtime-safe. `processReplacing` may allocate memory, acquire locks, or perform I/O. SeqTerm currently calls plugin processing from a non-RT context. If a known-safe plugin must run in the audio callback, it is the application's responsibility to verify RT safety.

---

## Parameter Automation

```rust
fn set_param(&self, instance, index: u32, value: f32)   // 0.0–1.0
fn get_param(&self, instance, index: u32) -> f32
```

VST2 parameter values are normalised to `[0.0, 1.0]`. Mapping to human-readable ranges is the plugin's responsibility via `getParameterDisplay`.

---

## PluginHostPort Trait

Defined in `seqterm-ports::plugin`:

```rust
pub trait PluginHostPort: Send + Sync {
    fn scan(&self, dir: &Path) -> Vec<PluginDescriptor>;
    fn instantiate(&self, id: u32, sample_rate: u32, block_size: u32)
        -> Result<Arc<Mutex<dyn PluginInstance>>>;
    fn describe(&self, id: u32) -> Option<PluginDescriptor>;
}
```

`PluginDescriptor` carries:

```rust
pub struct PluginDescriptor {
    pub id:           u32,
    pub name:         String,
    pub kind:         PluginKind,       // Instrument | Effect
    pub num_inputs:   u32,
    pub num_outputs:  u32,
    pub num_params:   u32,
    pub path:         PathBuf,
}
```

---

## Plugin Registry

`seqterm-application::PluginRegistry` is the application-layer wrapper around `PluginHostPort`. It caches scan results in memory and provides the UI with the list of available plugins for the plugin browser modal.

---

## Current Limitations

| Feature | Status |
|---------|--------|
| VST2 instruments | Supported |
| VST2 effects | Supported |
| VST3 | Planned (Phase 3) |
| CLAP | Planned (Phase 3) |
| AU (macOS) | Planned (Phase 3) |
| RT-safe plugin processing | Not enforced — plugin-dependent |
| Plugin state save/restore | Planned (Phase 2) |
| MIDI 2.0 per-note controllers | Requires physical MIDI 2.0 device |

Plugin state persistence is not yet implemented. The `.stz` project format reserves `plugins/state/{uuid}.state` for opaque plugin state blobs; the serialisation path will be wired in Phase 2.
