//! Minimal hand-written FFI for libfluidsynth 2.x.
//!
//! Only the symbols SeqTerm actually uses are declared. All functions are part
//! of the stable libfluidsynth 2.0+ public ABI, which is the version shipped by
//! current Debian/Ubuntu/Raspberry Pi OS, Fedora, Homebrew and vcpkg.
//!
//! This module is only compiled when the `fluidsynth` feature is enabled, so a
//! default (stub) build links no native symbols.
#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_double, c_float, c_int, c_uint, c_void};

// Opaque handles — never constructed in Rust, only passed back to the C side.
pub enum fluid_settings_t {}
pub enum fluid_synth_t {}
pub enum fluid_sfont_t {}
pub enum fluid_preset_t {}

/// libfluidsynth's sentinel for a failed call (e.g. sfload).
pub const FLUID_FAILED: c_int = -1;

#[link(name = "fluidsynth")]
extern "C" {
    // ── settings lifecycle ─────────────────────────────────────────────────
    pub fn new_fluid_settings() -> *mut fluid_settings_t;
    pub fn delete_fluid_settings(settings: *mut fluid_settings_t);
    pub fn fluid_settings_setstr(
        settings: *mut fluid_settings_t,
        name: *const c_char,
        str: *const c_char,
    ) -> c_int;
    pub fn fluid_settings_setnum(
        settings: *mut fluid_settings_t,
        name: *const c_char,
        val: c_double,
    ) -> c_int;
    pub fn fluid_settings_setint(
        settings: *mut fluid_settings_t,
        name: *const c_char,
        val: c_int,
    ) -> c_int;

    // ── synth lifecycle ────────────────────────────────────────────────────
    pub fn new_fluid_synth(settings: *mut fluid_settings_t) -> *mut fluid_synth_t;
    pub fn delete_fluid_synth(synth: *mut fluid_synth_t);
    pub fn fluid_synth_set_gain(synth: *mut fluid_synth_t, gain: c_float);

    // ── soundfont loading ──────────────────────────────────────────────────
    pub fn fluid_synth_sfload(
        synth: *mut fluid_synth_t,
        filename: *const c_char,
        reset_presets: c_int,
    ) -> c_int;
    pub fn fluid_synth_get_sfont_by_id(
        synth: *mut fluid_synth_t,
        id: c_int,
    ) -> *mut fluid_sfont_t;

    // ── preset selection ───────────────────────────────────────────────────
    pub fn fluid_synth_program_select(
        synth: *mut fluid_synth_t,
        chan: c_int,
        sfont_id: c_int,
        bank_num: c_int,
        preset_num: c_int,
    ) -> c_int;
    pub fn fluid_synth_bank_select(
        synth: *mut fluid_synth_t,
        chan: c_int,
        bank: c_uint,
    ) -> c_int;
    pub fn fluid_synth_program_change(
        synth: *mut fluid_synth_t,
        chan: c_int,
        program: c_int,
    ) -> c_int;

    // ── realtime MIDI ──────────────────────────────────────────────────────
    pub fn fluid_synth_noteon(synth: *mut fluid_synth_t, chan: c_int, key: c_int, vel: c_int) -> c_int;
    pub fn fluid_synth_noteoff(synth: *mut fluid_synth_t, chan: c_int, key: c_int) -> c_int;
    pub fn fluid_synth_cc(synth: *mut fluid_synth_t, chan: c_int, num: c_int, val: c_int) -> c_int;
    pub fn fluid_synth_pitch_bend(synth: *mut fluid_synth_t, chan: c_int, val: c_int) -> c_int;
    pub fn fluid_synth_all_notes_off(synth: *mut fluid_synth_t, chan: c_int) -> c_int;

    // ── audio rendering ────────────────────────────────────────────────────
    // Writes `len` frames into two (possibly strided) output buffers.
    pub fn fluid_synth_write_float(
        synth: *mut fluid_synth_t,
        len: c_int,
        lout: *mut c_void,
        loff: c_int,
        lincr: c_int,
        rout: *mut c_void,
        roff: c_int,
        rincr: c_int,
    ) -> c_int;

    // ── preset enumeration (non-RT) ────────────────────────────────────────
    pub fn fluid_sfont_iteration_start(sfont: *mut fluid_sfont_t);
    pub fn fluid_sfont_iteration_next(sfont: *mut fluid_sfont_t) -> *mut fluid_preset_t;
    pub fn fluid_preset_get_name(preset: *mut fluid_preset_t) -> *const c_char;
    pub fn fluid_preset_get_banknum(preset: *mut fluid_preset_t) -> c_int;
    pub fn fluid_preset_get_num(preset: *mut fluid_preset_t) -> c_int;
}
