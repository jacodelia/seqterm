# Phase 4 — Open Platform

Phase 4 turns SeqTerm into an open, extensible platform. It stabilises the public API, ships an official Lua scripting engine, opens the project format for third-party tooling, and targets cross-platform binary releases.

---

## Public API Stabilisation

### `seqterm-sdk` Crate ✅

**Status: Complete (initial version)**

The `seqterm-sdk` crate ships with `prelude`, `core`, and `ports` re-exports, plus helpers `project_to_json`, `from_json`, `new_project`, and `sdk_version`. Full `docs.rs` integration and `#[doc(hidden)]` cleanup are pending.

### C FFI Layer

Expose a C-compatible header (`seqterm.h`) for embedding SeqTerm in non-Rust hosts:

```c
seqterm_project_t* seqterm_project_open(const char* path);
void               seqterm_project_save(seqterm_project_t* p, const char* path);
void               seqterm_transport_play(seqterm_project_t* p);
void               seqterm_transport_stop(seqterm_project_t* p);
void               seqterm_project_free(seqterm_project_t* p);
```

Generated with `cbindgen`. Enables embedding SeqTerm in Max/MSP, SuperCollider, or custom hardware firmware.

---

## Scripting Engine

### Lua Integration

Embed a `mlua`-based scripting engine in `seqterm-application`:

- Scripts run on a dedicated Lua thread (not the audio callback, not the UI thread).
- Scripts communicate with the engine via `AppCommand` dispatch — the same mechanism used by the UI.
- Scripts can read project state via a safe read-only snapshot API.
- Script files live in `scripts/` inside the `.stz` container.

#### Core Script API

```lua
-- Read transport state
local step = seqterm.current_step()
local bpm  = seqterm.bpm()

-- Modify a pattern
local pat = seqterm.pattern("KICK01")
pat:set_step(0, { note="C-4", vel=100, gate=100 })

-- Dispatch commands
seqterm.dispatch("SetBpm", 140.0)
seqterm.dispatch("Play")

-- Register a callback (called every step)
seqterm.on_step(function(step)
    if step % 16 == 0 then
        pat:randomize(0.3)
    end
end)
```

#### Use Cases

- Generative composition: procedural pattern generation beyond the built-in Markov/Euclidean tools.
- Live coding: real-time pattern transformation from the Lua REPL.
- Macro automation: multi-step operations scripted as a single command.
- Hardware integration: custom MIDI controller mappings that go beyond MIDI Learn.

---

## Project Format

### STZ v2

Extend the `.stz` container format for Phase 4 requirements:

- **Script storage**: `scripts/{name}.lua` — Lua scripts embedded in the project.
- **Custom metadata**: `metadata/notes.md` and `metadata/tags.json` — freeform annotations.
- **Waveform cache**: `cache/waveforms/{uuid}.f32` — pre-computed waveform display data. Cache is disposable; SeqTerm regenerates it automatically if missing.
- **Format version 2** — `format_version: 2` in manifest. The `ProjectMigrator` handles v1 → v2 migration (adds script and cache directories; no structural changes to existing objects).

### STZ CLI Tool

A standalone `stz` command-line tool:

```bash
stz inspect   project.stz              # print manifest and registry
stz extract   project.stz audio/       # extract all audio assets
stz pack      project/                 # create .stz from directory
stz migrate   project.stz              # apply all migrations in-place
stz validate  project.stz              # check integrity (hashes, UUIDs)
stz diff      a.stz b.stz              # show changed objects
```

Implemented as a thin binary wrapping `seqterm-stz` public types.

---

## Cross-Platform Releases

### Binary Distribution ✅

**Status: Complete**

Pre-built binaries ship for all primary targets via GitHub Actions release workflow:

| Platform | Target | Status |
|----------|--------|--------|
| Linux x86-64 | `x86_64-unknown-linux-gnu` (glibc ≥ 2.31) | ✅ .deb / .rpm / .tar.gz |
| Linux ARM64 | `aarch64-unknown-linux-gnu` (Raspberry Pi 4/5) | ✅ .deb (cross-compiled) |
| Linux ARMv7 | `armv7-unknown-linux-gnueabihf` (Raspberry Pi OS 32-bit) | ✅ .deb (cross-compiled) |
| macOS Universal | `x86_64` + `aarch64` fat binary | ✅ .dmg |
| Windows x86-64 | `x86_64-pc-windows-msvc` | ✅ .msi |
| Windows ARM64 | `aarch64-pc-windows-msvc` | ✅ |

CI pipeline (GitHub Actions) — complete:

1. `cargo test --all` on every PR.
2. `cargo clippy -- -D warnings` on every PR.
3. Cross-compile via `cross` + `Cross.toml` for ARM targets.
4. `cargo build --release` + strip + compress on release tags.
5. Automated changelog via `git-cliff` (`orhun/git-cliff-action@v3`; `cliff.toml` with conventional-commits grouping).
6. Semantic versioning (vMAJOR.MINOR.PATCH) enforced on release tags.

### Package Managers

- **Homebrew** (macOS/Linux): formula in a tap repository.
- **AUR** (Arch Linux): PKGBUILD maintained in the community.
- **Cargo**: `cargo install seqterm-app` as the primary install method for Rust users.

---

## Documentation

### docs.rs Integration

All public types and functions in `seqterm-sdk` must have doc comments. CI enforces `#![deny(missing_docs)]` on the SDK crate. Examples in doc comments are run as doctests.

### Interactive Tutorial

An embedded tutorial mode (`AppCommand::StartTutorial`) guides new users through:

1. Creating a project.
2. Building a drum pattern in the Matrix.
3. Assigning an SF2 instrument.
4. Recording an automation lane.
5. Arranging a simple song.
6. Exporting a WAV mixdown.

Tutorial steps are stored as a `Vec<TutorialStep>` in a JSON resource file and rendered as modal overlays.

---

## Testing

Phase 4 targets 400 passing unit tests, adding:

- Lua scripting: `on_step` callback fires at correct intervals.
- STZ v2 migration correctness.
- C FFI: project open/save/play round-trip via the C API.
- Cross-platform path normalisation (Windows forward-slash audit).
- `stz` CLI tool integration tests.
