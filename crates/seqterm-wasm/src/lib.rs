//! # SeqTerm WebAssembly Bindings
//!
//! Exposes SeqTerm's core project model and SDK to the browser via
//! `wasm-bindgen`.  The audio engine and MIDI I/O are replaced by
//! Web Audio API and Web MIDI API calls from JavaScript.
//!
//! ## Building
//!
//! ```sh
//! cargo install wasm-pack
//! wasm-pack build crates/seqterm-wasm --target web --out-dir pkg
//! ```
//!
//! ## JavaScript API
//!
//! ```js
//! import init, { WasmProject } from './pkg/seqterm_wasm.js';
//! await init();
//!
//! const proj = WasmProject.new_project("My Project", 128);
//! proj.set_bpm(140.0);
//! const json = proj.to_json();
//! console.log(json);
//! ```

use wasm_bindgen::prelude::*;

// ─── WasmProject ─────────────────────────────────────────────────────────────

/// WebAssembly-friendly wrapper around a SeqTerm project.
#[wasm_bindgen]
pub struct WasmProject {
    inner: seqterm_core::Project,
}

#[wasm_bindgen]
impl WasmProject {
    /// Create a new blank project.
    #[wasm_bindgen(constructor)]
    pub fn new_project(name: &str, bpm: f64) -> WasmProject {
        let mut proj = seqterm_sdk::new_project(name.to_string());
        proj.bpm = bpm;
        WasmProject { inner: proj }
    }

    /// Load a project from a JSON string.
    pub fn from_json(json: &str) -> Result<WasmProject, JsValue> {
        seqterm_sdk::project_from_json(json)
            .map(|inner| WasmProject { inner })
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Serialize the project to a JSON string.
    pub fn to_json(&self) -> Result<String, JsValue> {
        seqterm_sdk::project_to_json(&self.inner)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get the project name.
    pub fn name(&self) -> String { self.inner.name.clone() }

    /// Get the project BPM.
    pub fn bpm(&self) -> f64 { self.inner.bpm }

    /// Set the project BPM.
    pub fn set_bpm(&mut self, bpm: f64) {
        self.inner.bpm = bpm.clamp(20.0, 300.0);
    }

    /// Get the number of mixer channels.
    pub fn channel_count(&self) -> usize { self.inner.channels.len() }

    /// Get the SDK version string.
    pub fn sdk_version() -> String { seqterm_sdk::sdk_version().to_string() }
}

// ─── Utilities ────────────────────────────────────────────────────────────────

/// Set up the browser panic hook for better error messages in the console.
#[wasm_bindgen(start)]
pub fn main() {
    #[cfg(debug_assertions)]
    console_error_panic_hook();
}

fn console_error_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        js_sys::eval(&format!("console.error({:?})", msg)).ok();
    }));
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_roundtrip() {
        let proj = WasmProject::new_project("Test", 120.0);
        let json = proj.to_json().unwrap();
        let back = WasmProject::from_json(&json).unwrap();
        assert_eq!(back.name(), "Test");
        assert!((back.bpm() - 120.0).abs() < 0.01);
    }
}
