# Cross-Compilation Strategy

SeqTerm supports 6 target platforms from a Linux x86\_64 host (CI/CD) using either native toolchains or the `cross` Docker-based approach.

---

## Targets

| Target triple | Platform | Release artifact | Strategy |
|---|---|---|---|
| `x86_64-unknown-linux-gnu` | Linux x86\_64 | `.deb` `.rpm` `.tar.gz` | Native |
| `aarch64-unknown-linux-gnu` | Linux ARM64 (Pi 5, etc.) | `.deb` | `cross` |
| `armv7-unknown-linux-gnueabihf` | Linux ARMv7 (Pi 4 32-bit) | `.deb` | `cross` |
| `x86_64-apple-darwin` + `aarch64-apple-darwin` | macOS Universal | `.dmg` | macOS runner |
| `x86_64-pc-windows-msvc` | Windows x86\_64 | `.msi` / `.zip` | Windows runner |
| `aarch64-pc-windows-msvc` | Windows ARM64 | `.zip` | Windows runner |

---

## Toolchain Setup

### Native cross-compilation (without Docker)

For ARM64:
```bash
# Debian/Ubuntu
sudo apt install gcc-aarch64-linux-gnu libasound2-dev:arm64

# .cargo/config.toml
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"

# Build
PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-linux-gnu \
PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig \
cargo build --release --target aarch64-unknown-linux-gnu -p seqterm-app
```

For ARMv7:
```bash
sudo apt install gcc-arm-linux-gnueabihf libasound2-dev:armhf

[target.armv7-unknown-linux-gnueabihf]
linker = "arm-linux-gnueabihf-gcc"

PKG_CONFIG_SYSROOT_DIR=/usr/arm-linux-gnueabihf \
PKG_CONFIG_PATH=/usr/lib/arm-linux-gnueabihf/pkgconfig \
cargo build --release --target armv7-unknown-linux-gnueabihf -p seqterm-app
```

### Using `cross` (Docker-based, handles sysroot automatically)

```bash
cargo install cross --git https://github.com/cross-rs/cross --locked

cross build --release --target aarch64-unknown-linux-gnu  -p seqterm-app
cross build --release --target armv7-unknown-linux-gnueabihf -p seqterm-app
```

`cross` is the recommended approach in CI because it handles the ALSA sysroot, pkg-config, and linker configuration automatically via Docker containers.

---

## Cargo Configuration

### `.cargo/config.toml` (in repo root for local builds)

```toml
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"

[target.armv7-unknown-linux-gnueabihf]
linker = "arm-linux-gnueabihf-gcc"

[target.aarch64-pc-windows-msvc]
# Windows ARM64 — only available on Windows runners; no extra linker needed.
```

### Workspace-level build profiles

```toml
# Cargo.toml (workspace)
[profile.release]
opt-level = 3
lto = "thin"    # faster than full LTO for most targets

[profile.release-lto]
inherits = "release"
lto = "fat"     # maximum optimization for distribution

[profile.rpi]
inherits = "release"
opt-level = "z"       # optimize for size (smaller binary for Pi)
strip = true          # strip debug symbols
codegen-units = 1     # slower build, better optimization
```

To use the size-optimized Pi profile:
```bash
cross build --profile rpi --target armv7-unknown-linux-gnueabihf -p seqterm-app
```

---

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `cpal-backend` | on | CPAL audio I/O (required for audio output) |
| `jack-backend` | on (via `cpal/jack`) | JACK support (Linux/macOS) |
| `fluidsynth` | off | **Embedded** FluidSynth SF2 engine (FluidLite, statically compiled — no external libs; all platforms) |
| `fluidsynth-system` | off | System FluidSynth 2.x (dynamic link; requires `libfluidsynth` ≥ 2.0) |
| `vst3` | off | VST3 plugin host (future) |
| `clap` | off | CLAP plugin host (future) |
| `wasm` | off | WebAssembly target (future) |

Feature-flagged builds:
```bash
# Minimal build (no JACK, no FluidSynth)
cargo build --release --no-default-features --features cpal-backend

# Embedded FluidSynth (FluidLite) — no external library, all targets identical
cargo build --release --features fluidsynth

# System FluidSynth 2.x (dynamic link, requires libfluidsynth ≥ 2.0)
#   Linux/RPi : sudo apt install libfluidsynth-dev   (pkg-config resolves it)
#   macOS     : brew install fluid-synth
#   Windows   : vcpkg install fluidsynth && set FLUIDSYNTH_LIB_DIR=...\lib
cargo build --release --features fluidsynth-system
```

**Embedded (`fluidsynth`)** compiles FluidLite's bundled C with the `cc` crate, so it
cross-compiles like any other Rust code — no sysroot library needed. The `cc` crate
honours `CC`/`CFLAGS` and the `cross` Docker images already, so `cross build
--features fluidsynth` works for ARM targets out of the box.

**System (`fluidsynth-system`)** linking is handled by
`crates/seqterm-fluidsynth/build.rs` (`FLUIDSYNTH_LIB_DIR` → `pkg-config` →
bare `-lfluidsynth`). When cross-compiling, point
`PKG_CONFIG_PATH`/`PKG_CONFIG_SYSROOT_DIR` at the target's fluidsynth, or set
`FLUIDSYNTH_LIB_DIR`; and bundle the dylib/DLL for macOS/Windows artifacts — see
[`fluidsynth-evaluation.md`](fluidsynth-evaluation.md#implementation-2026-06).

---

## Cross-Compilation Notes

### ALSA dependency

`libasound2-dev` must be available for the target architecture. `cross` handles this automatically via its Docker images. For native cross-compilation, install the multiarch package:

```bash
sudo dpkg --add-architecture arm64
sudo apt update
sudo apt install libasound2-dev:arm64
```

### No C dependencies in the hot path

All audio callback code is pure Rust. The only C dependency is `libasound2` for MIDI I/O (`midir` uses it). This means:
- The binary is statically linked except for `libasound2.so`
- On Raspberry Pi, `libasound2` is pre-installed in the standard image

### macOS Universal Binary

The macOS Universal binary is created by building two separate targets and combining with `lipo`:
```bash
cargo build --release --target x86_64-apple-darwin  -p seqterm-app
cargo build --release --target aarch64-apple-darwin -p seqterm-app
lipo -create \
  target/x86_64-apple-darwin/release/seqterm \
  target/aarch64-apple-darwin/release/seqterm \
  -output dist/seqterm-universal
```

This is handled automatically by the GitHub Actions release workflow.

---

## Testing Cross-Compiled Binaries

For ARM targets, test on real hardware or via QEMU:
```bash
# Install QEMU user-mode emulation
sudo apt install qemu-user-static

# Run an ARM64 binary on x86_64
qemu-aarch64-static ./target/aarch64-unknown-linux-gnu/release/seqterm --version
```

Note: QEMU doesn't emulate audio hardware, so functional audio tests must run on real ARM hardware.
