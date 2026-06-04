# FluidSynth Evaluation

**Decision date:** 2026-05  
**Status:** ✅ **Implemented (2026-06)** — Option B shipped. oxisynth remains the
default; FluidSynth is an opt-in engine behind `--features fluidsynth`. See the
[Implementation](#implementation-2026-06) section below and
[`docs/architecture/sf2-engine.md`](architecture/sf2-engine.md).

---

## Options Evaluated

### Option A — Replace oxisynth with FluidSynth (C binding)

**Pros:**
- Mature, battle-tested SF2 implementation
- Full GM2 / GS / XG spec compliance including velocity layers, modulators, envelope interpolation
- Actively maintained; broad SF2 file compatibility

**Cons:**
- C library dependency: breaks the "pure Rust" build; requires `libfluidsynth-dev` on every target
- Cross-compilation to ARM/Windows/macOS requires packaging the shared library with the binary
- No Raspberry Pi OS ARMv7 package in the default apt repo (must build from source)
- Heap allocations in the render callback (FluidSynth is not realtime-safe in strict sense)
- License: LGPL 2.1 — imposes linking requirements on proprietary builds

**Verdict:** Not suitable as the default backend. Breaks the Raspberry Pi deployment story and adds a non-Rust dependency.

---

### Option B — Runtime backend via AudioSynthPort trait (current architecture)

**Pros:**
- `InstrumentBackend` trait in `seqterm-ports` abstracts over any synth engine
- Can plug in FluidSynth as an *opt-in* adapter via `feature = ["fluidsynth"]` without affecting default builds
- oxisynth remains the default; FluidSynth enhances quality on capable systems

**Cons:**
- Two codepaths to maintain
- oxisynth lacks velocity layers and full modulator support (limited SF2 fidelity)

**Verdict:** ✅ Chosen approach. Implement a `FluidSynthAdapter` crate behind a feature flag.

---

### Option C — Hybrid (oxisynth for small presets, FluidSynth for complex SF2s)

**Pros:**
- Best of both worlds for quality vs. portability

**Cons:**
- Significant complexity: two synths running simultaneously, preset routing logic
- Integration points for transport Stop (all-notes-off on two engines)
- Memory overhead: both synths pre-allocated

**Verdict:** Too complex for the current project scope.

---

### Option D — Improve oxisynth

**Pros:**
- Pure Rust, no new dependencies
- Upstream contributions benefit the whole ecosystem

**Cons:**
- oxisynth is a research/educational library; upstream is not actively maintained for production DAW use
- Adding velocity layers and full GM2 modulators is months of work

**Verdict:** Not a near-term option, but worth contributing upstream.

---

## Recommendation

**Keep oxisynth as the default.** Add a `seqterm-fluidsynth` adapter crate behind `feature = ["fluidsynth"]` when:
1. A concrete production use case requires better SF2 fidelity (velocity layers, ADSR from modulators)
2. The `InstrumentBackend` trait is stable enough to abstract the differences

For Raspberry Pi and low-memory targets: oxisynth is the right choice — small binary, no shared library dependency, predictable memory use.

---

## SF2 File Compatibility Notes

oxisynth handles:
- ✅ Single SF2 load, multiple channels with independent bank/preset
- ✅ Bank 128 percussion (after the bank-clamping bug fix in v0.1.1)
- ✅ CC events (CC01 mod, CC11 expression, CC74 filter, CC93 chorus send, CC64 sustain)
- ✅ Note on/off, pitch bend, aftertouch
- ⚠️ Velocity layers — selects the first matching sample range only
- ⚠️ SF2 modulators — not fully implemented; ADSR comes from hardcoded defaults
- ❌ SF3 (OGG-compressed samples) — not supported

For the vast majority of free GM SoundFonts (GeneralUser GS, TimGM6mb, FluidR3), oxisynth produces correct output.

---

## Implementation (2026-06)

Option B was implemented with one design refinement: instead of exposing a second
`InstrumentBackend` to the whole engine (which would have meant replacing the many
`downcast_mut::<SoundFontSynth>()` call-sites in the mixer/scheduler), the engines
live **inside** `SoundFontSynth` as a private enum. The public surface is unchanged, so
nothing downstream had to move.

```rust
// crates/seqterm-audio-engine/src/sf2_synth.rs
enum Sf2Engine {
    Oxi(oxisynth::Synth),            // default, pure Rust
    Fluid(seqterm_fluidsynth::FluidSynthBackend),
}
```

`load_multi()` picks the engine via `sf2_prefer_fluidsynth()`; FluidSynth only "wins"
when a real engine is compiled in (`FluidSynthBackend::is_real()`), otherwise it logs
and falls back to oxisynth.

### Two FluidSynth flavours — and why embedded is the default

Option A's headline objection was the GLib dependency: full FluidSynth 2.x **cannot**
be statically embedded portably because it hard-depends on GLib. That objection was
resolved by recognising it only applies to *full* FluidSynth. `seqterm-fluidsynth`
therefore offers two engines behind one identical API
(`crates/seqterm-fluidsynth/src/engine_*.rs`):

| Feature | Engine | C dependency | Cross-platform story |
|---------|--------|--------------|----------------------|
| `fluidlite` *(audio-engine/app feature `fluidsynth`)* | **FluidLite** — FluidSynth's synth core with GLib + drivers stripped out | bundled C, compiled by `cc` into the binary | **zero external deps**; one command, every OS |
| `fluidsynth` *(app feature `fluidsynth-system`)* | system **libfluidsynth 2.x** | dynamic `libfluidsynth` + GLib | per-platform install + bundling |
| *(neither)* | silent stub | none | falls back to oxisynth |

The **embedded** path is the default `fluidsynth` feature and the answer to "no external
dependencies": the [`fluidlite`](https://crates.io/crates/fluidlite) crate ships the C
source plus prebuilt bindings, so with `default-features = false, features = ["builtin",
"with-sf3", "with-stb"]` it needs **no system library, no GLib, no `pkg-config`, no
`bindgen`, no `cmake`** — only the C compiler the toolchain already uses. SF2 and SF3
(Vorbis, via bundled `stb_vorbis`) both work. Verified: `ldd seqterm` shows no
`libfluidsynth`/`glib`, and the bundled engine renders non-silent audio.

The old crates.io `fluidsynth 0.0.1` binding was rejected for the system path too — its
API doesn't match modern libfluidsynth and it has no preset enumeration; `seqterm-fluidsynth`
carries **hand-written FFI** (`src/ffi.rs`) against the stable libfluidsynth 2.x ABI.
Both engines render into SeqTerm's own buffers and flow through the normal mixer/FX chain
— FluidSynth is a *sample engine*, never a standalone audio server.

### How Windows & macOS were solved

- **Embedded (default).** Nothing platform-specific. The `cc` crate compiles FluidLite's
  bundled C with the right toolchain on each OS (gcc/clang on Linux/macOS, MSVC on
  Windows) and statically links it. No DLL/dylib to ship, no GLib, no `@rpath` fixups,
  no vcpkg. `cross build --features fluidsynth` likewise just works for ARM/Raspberry Pi.

- **System (opt-in `fluidsynth-system`).** Handled by
  `crates/seqterm-fluidsynth/build.rs`, which resolves the library in three steps —
  `FLUIDSYNTH_LIB_DIR` env override → `pkg-config` → bare `-lfluidsynth` — with **no**
  `cfg!(target_os)` branches:

  | Platform | How it links | Runtime |
  |----------|--------------|---------|
  | **Linux / Raspberry Pi OS** | `libfluidsynth-dev` ships `fluidsynth.pc`; pkg-config supplies arch-correct paths (arm64/armhf too). | `libfluidsynth.so.3` |
  | **macOS (Homebrew)** | `brew install fluid-synth` installs `fluidsynth.pc` under the brew prefix (`/usr/local` Intel, `/opt/homebrew` Apple Silicon); pkg-config resolves it, else `FLUIDSYNTH_LIB_DIR=$(brew --prefix fluid-synth)/lib`. | `libfluidsynth.3.dylib` — bundle in the `.app`. |
  | **Windows (vcpkg)** | No pkg-config by convention → use the env override: `vcpkg install fluidsynth`, `set FLUIDSYNTH_LIB_DIR=…\lib` (and `FLUIDSYNTH_LIB_NAME=libfluidsynth` if the import lib is so named). | `fluidsynth.dll` + deps (`glib`, `libsndfile`, …) next to `seqterm.exe`. |

> **Bottom line:** the embedded FluidLite engine removes the packaging cost that kept
> FluidSynth opt-in, so a FluidSynth-quality SF2 engine now ships in the binary on every
> platform with nothing to install. The system path remains for users who specifically
> want full FluidSynth 2.x.
