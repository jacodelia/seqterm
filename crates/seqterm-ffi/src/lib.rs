//! # SeqTerm C FFI
//!
//! C-compatible interface for embedding SeqTerm in non-Rust hosts
//! (Max/MSP, SuperCollider, custom hardware firmware, etc.).
//!
//! ## Usage (C)
//!
//! ```c
//! #include "seqterm.h"
//!
//! seqterm_project_t *proj = seqterm_project_new("My Project");
//! seqterm_project_set_bpm(proj, 140.0);
//! char *json = seqterm_project_to_json(proj);
//! // ... use json ...
//! seqterm_string_free(json);
//! seqterm_project_free(proj);
//! ```
//!
//! ## Safety
//!
//! All pointer arguments must be non-null and valid for the lifetime of the call.
//! String results are heap-allocated; free them with `seqterm_string_free`.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int};

/// Opaque project handle. Obtain via `seqterm_project_new` or `seqterm_project_open`.
pub struct SeqTermProject {
    inner: seqterm_core::Project,
}

/// Open a project from a JSON or `.seqterm` file.
/// Returns null on failure. Free with `seqterm_project_free`.
#[no_mangle]
pub extern "C" fn seqterm_project_open(path: *const c_char) -> *mut SeqTermProject {
    if path.is_null() { return std::ptr::null_mut(); }
    let path_str = unsafe { CStr::from_ptr(path) }.to_str().unwrap_or("");
    match seqterm_sdk::project_from_json(&std::fs::read_to_string(path_str).unwrap_or_default()) {
        Ok(proj) => Box::into_raw(Box::new(SeqTermProject { inner: proj })),
        Err(_)   => std::ptr::null_mut(),
    }
}

/// Create a blank project with the given name.
/// Returns null on allocation failure. Free with `seqterm_project_free`.
#[no_mangle]
pub extern "C" fn seqterm_project_new(name: *const c_char) -> *mut SeqTermProject {
    let name_str = if name.is_null() { "New Project".to_string() }
                   else { unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned() };
    let proj = seqterm_sdk::new_project(name_str);
    Box::into_raw(Box::new(SeqTermProject { inner: proj }))
}

/// Save a project to a JSON file. Returns 0 on success, -1 on failure.
#[no_mangle]
pub extern "C" fn seqterm_project_save(
    project: *const SeqTermProject,
    path:    *const c_char,
) -> c_int {
    if project.is_null() || path.is_null() { return -1; }
    let proj = unsafe { &(*project).inner };
    let path_str = unsafe { CStr::from_ptr(path) }.to_str().unwrap_or("");
    match seqterm_sdk::project_to_json(proj) {
        Ok(json) => {
            std::fs::write(path_str, json).map(|_| 0).unwrap_or(-1)
        }
        Err(_) => -1,
    }
}

/// Serialize a project to a JSON string.
/// The caller must free the returned string with `seqterm_string_free`.
/// Returns null on failure.
#[no_mangle]
pub extern "C" fn seqterm_project_to_json(project: *const SeqTermProject) -> *mut c_char {
    if project.is_null() { return std::ptr::null_mut(); }
    let proj = unsafe { &(*project).inner };
    seqterm_sdk::project_to_json(proj)
        .ok()
        .and_then(|s| CString::new(s).ok())
        .map(|cs| cs.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Get the project name. The returned pointer is valid until the project is freed.
/// Returns null if project is null.
#[no_mangle]
pub extern "C" fn seqterm_project_name(project: *const SeqTermProject) -> *const c_char {
    if project.is_null() { return std::ptr::null(); }
    // Safety: we return a pointer into the project's String; caller must not free it.
    let proj = unsafe { &(*project).inner };
    // Leak a CString for the lifetime of the project — caller treats it as read-only.
    CString::new(proj.name.as_str()).unwrap_or_default().into_raw()
}

/// Get the project BPM.
#[no_mangle]
pub extern "C" fn seqterm_project_get_bpm(project: *const SeqTermProject) -> c_double {
    if project.is_null() { return 0.0; }
    unsafe { (*project).inner.bpm }
}

/// Set the project BPM.
#[no_mangle]
pub extern "C" fn seqterm_project_set_bpm(project: *mut SeqTermProject, bpm: c_double) {
    if project.is_null() { return; }
    unsafe { (*project).inner.bpm = bpm; }
}

/// Return the number of channels (mixer strips) in the project.
#[no_mangle]
pub extern "C" fn seqterm_project_channel_count(project: *const SeqTermProject) -> c_int {
    if project.is_null() { return 0; }
    unsafe { (*project).inner.channels.len() as c_int }
}

/// Return the SDK version string. The returned string is static (no need to free).
#[no_mangle]
pub extern "C" fn seqterm_sdk_version() -> *const c_char {
    static VERSION: std::sync::OnceLock<CString> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        CString::new(seqterm_sdk::sdk_version()).unwrap_or_default()
    }).as_ptr()
}

/// Free a project obtained from `seqterm_project_new` or `seqterm_project_open`.
/// Passing null is a no-op.
#[no_mangle]
pub extern "C" fn seqterm_project_free(project: *mut SeqTermProject) {
    if !project.is_null() {
        unsafe { drop(Box::from_raw(project)); }
    }
}

/// Free a string returned by a SeqTerm function (e.g. `seqterm_project_to_json`).
/// Passing null is a no-op.
#[no_mangle]
pub extern "C" fn seqterm_string_free(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn project_new_and_free() {
        let name = CString::new("Test").unwrap();
        let p = seqterm_project_new(name.as_ptr());
        assert!(!p.is_null());
        seqterm_project_free(p);
    }

    #[test]
    fn project_bpm_roundtrip() {
        let name = CString::new("BPM Test").unwrap();
        let p = seqterm_project_new(name.as_ptr());
        seqterm_project_set_bpm(p, 150.0);
        let bpm = seqterm_project_get_bpm(p);
        assert!((bpm - 150.0).abs() < 0.01);
        seqterm_project_free(p);
    }

    #[test]
    fn project_to_json_is_valid() {
        let name = CString::new("JSON Test").unwrap();
        let p = seqterm_project_new(name.as_ptr());
        let json_ptr = seqterm_project_to_json(p);
        assert!(!json_ptr.is_null());
        let json = unsafe { CStr::from_ptr(json_ptr) }.to_string_lossy();
        assert!(json.contains("\"name\""));
        seqterm_string_free(json_ptr);
        seqterm_project_free(p);
    }

    #[test]
    fn free_null_is_safe() {
        seqterm_project_free(std::ptr::null_mut());
        seqterm_string_free(std::ptr::null_mut());
    }
}
