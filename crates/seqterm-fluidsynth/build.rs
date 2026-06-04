//! Cross-platform link configuration for libfluidsynth.
//!
//! Only emits link directives when the `fluidsynth` feature is active, so a
//! default build (stub mode) needs no native library at all.
//!
//! Resolution order:
//!   1. `FLUIDSYNTH_LIB_DIR` env var — explicit search path (Windows / vcpkg /
//!      custom installs). Also honoured on Unix for non-standard prefixes.
//!   2. `pkg-config` — the normal path on Linux, macOS (Homebrew) and Raspberry
//!      Pi OS, where `libfluidsynth-dev` ships a `fluidsynth.pc`.
//!   3. Bare `-lfluidsynth` fallback so the linker can still find a library that
//!      is on the default search path.

fn main() {
    println!("cargo:rerun-if-env-changed=FLUIDSYNTH_LIB_DIR");
    println!("cargo:rerun-if-env-changed=FLUIDSYNTH_LIB_NAME");

    // Nothing to link unless the feature is on (stub build produces silence).
    if std::env::var_os("CARGO_FEATURE_FLUIDSYNTH").is_none() {
        return;
    }

    // The library base name. On Windows with vcpkg this is often "fluidsynth";
    // some MSYS2 builds name it "libfluidsynth". Override with FLUIDSYNTH_LIB_NAME.
    let lib_name = std::env::var("FLUIDSYNTH_LIB_NAME").unwrap_or_else(|_| "fluidsynth".to_string());

    // (1) Explicit directory override — highest priority, every platform.
    if let Ok(dir) = std::env::var("FLUIDSYNTH_LIB_DIR") {
        println!("cargo:rustc-link-search=native={dir}");
        println!("cargo:rustc-link-lib=dylib={lib_name}");
        return;
    }

    // (2) pkg-config (Linux / macOS / Raspberry Pi). It prints the link
    //     directives itself on success.
    match pkg_config::Config::new()
        .atleast_version("2.0.0")
        .probe("fluidsynth")
    {
        Ok(_) => return,
        Err(e) => {
            println!(
                "cargo:warning=pkg-config could not locate fluidsynth ({e}); \
                 falling back to a bare -l{lib_name}. Set FLUIDSYNTH_LIB_DIR to \
                 point at the library directory if linking fails."
            );
        }
    }

    // (3) Last-resort bare link against whatever is on the default search path.
    println!("cargo:rustc-link-lib=dylib={lib_name}");
}
