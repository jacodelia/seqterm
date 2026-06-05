//! Minimal LV2 C ABI — just the pieces needed to host a plugin without the
//! official LV2 SDK headers. Mirrors `lv2.h`, `urid.h`, `atom.h`, `midi.h`.
//!
//! References: <https://lv2plug.in/ns/>

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_void};

// ─── Core (lv2core) ─────────────────────────────────────────────────────────

/// Opaque plugin instance handle (`LV2_Handle`).
pub type LV2_Handle = *mut c_void;

/// A host feature passed to `instantiate` (`LV2_Feature`).
#[repr(C)]
pub struct LV2_Feature {
    /// Feature URI (NUL-terminated).
    pub uri: *const c_char,
    /// Feature-specific data (e.g. a `*const LV2_URID_Map`).
    pub data: *mut c_void,
}

/// The plugin descriptor returned by `lv2_descriptor(index)` (`LV2_Descriptor`).
#[repr(C)]
pub struct LV2_Descriptor {
    /// Plugin URI (NUL-terminated) — matches the subject in the bundle TTL.
    pub uri: *const c_char,
    pub instantiate: Option<
        unsafe extern "C" fn(
            descriptor: *const LV2_Descriptor,
            sample_rate: f64,
            bundle_path: *const c_char,
            features: *const *const LV2_Feature,
        ) -> LV2_Handle,
    >,
    pub connect_port:
        Option<unsafe extern "C" fn(instance: LV2_Handle, port: u32, data_location: *mut c_void)>,
    pub activate: Option<unsafe extern "C" fn(instance: LV2_Handle)>,
    pub run: Option<unsafe extern "C" fn(instance: LV2_Handle, sample_count: u32)>,
    pub deactivate: Option<unsafe extern "C" fn(instance: LV2_Handle)>,
    pub cleanup: Option<unsafe extern "C" fn(instance: LV2_Handle)>,
    pub extension_data: Option<unsafe extern "C" fn(uri: *const c_char) -> *const c_void>,
}

/// Signature of the bundle entry point: `const LV2_Descriptor* lv2_descriptor(uint32_t index)`.
pub type Lv2DescriptorFn = unsafe extern "C" fn(index: u32) -> *const LV2_Descriptor;

/// The symbol name the entry point is exported under.
pub const LV2_DESCRIPTOR_SYM: &[u8] = b"lv2_descriptor";

// ─── URID extension (ext/urid) ──────────────────────────────────────────────

pub const LV2_URID_MAP_URI: &str = "http://lv2plug.in/ns/ext/urid#map";
pub const LV2_URID_UNMAP_URI: &str = "http://lv2plug.in/ns/ext/urid#unmap";

pub type LV2_URID = u32;
pub type LV2_URID_Map_Handle = *mut c_void;
pub type LV2_URID_Unmap_Handle = *mut c_void;

#[repr(C)]
pub struct LV2_URID_Map {
    pub handle: LV2_URID_Map_Handle,
    /// Map a URI string to an integer URID (never 0 on success).
    pub map: Option<unsafe extern "C" fn(handle: LV2_URID_Map_Handle, uri: *const c_char) -> LV2_URID>,
}

#[repr(C)]
pub struct LV2_URID_Unmap {
    pub handle: LV2_URID_Unmap_Handle,
    pub unmap:
        Option<unsafe extern "C" fn(handle: LV2_URID_Unmap_Handle, urid: LV2_URID) -> *const c_char>,
}

// ─── Atom + MIDI (ext/atom, ext/midi) ───────────────────────────────────────

pub const LV2_ATOM_SEQUENCE_URI: &str = "http://lv2plug.in/ns/ext/atom#Sequence";
pub const LV2_ATOM_CHUNK_URI: &str = "http://lv2plug.in/ns/ext/atom#Chunk";
pub const LV2_MIDI_EVENT_URI: &str = "http://lv2plug.in/ns/ext/midi#MidiEvent";

/// `LV2_Atom` — header common to every atom (`{ size, type }`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LV2_Atom {
    pub size: u32,
    pub type_: u32,
}

/// `LV2_Atom_Sequence_Body` — `{ unit, pad }` following the sequence atom header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LV2_Atom_Sequence_Body {
    pub unit: u32,
    pub pad: u32,
}

/// `LV2_Atom_Sequence` — header for an atom port carrying timed events.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LV2_Atom_Sequence {
    pub atom: LV2_Atom,
    pub body: LV2_Atom_Sequence_Body,
}

/// `LV2_Atom_Event` — `{ int64 frames; LV2_Atom body; <body bytes…> }`.
/// We use the audio-frame timestamp variant (not beats).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LV2_Atom_Event {
    pub frames: i64,
    pub body: LV2_Atom,
    // followed by `body.size` bytes of event payload (raw MIDI for MidiEvent)
}

/// Round `size` up to the next 8-byte boundary (atoms are 64-bit aligned).
#[inline]
pub fn pad8(size: usize) -> usize {
    (size + 7) & !7
}
