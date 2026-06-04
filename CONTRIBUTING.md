# Contributing to SeqTerm

Thank you for wanting to contribute. This document explains how the project is structured and what we expect from contributions.

---

## Getting Started

```bash
git clone https://github.com/your-org/seqterm
cd seqterm
cargo build
cargo test --workspace
```

Requirements: Rust 1.85+, `libasound2-dev` (Linux), `pkg-config`.

---

## Project Structure

SeqTerm follows a **hexagonal architecture** (Ports and Adapters):

```
seqterm-core     ← domain model; zero runtime dependencies except serde
seqterm-ports    ← pure trait definitions (AudioBackendPort, MidiBackendPort, InstrumentBackend…)
seqterm-engine   ← scheduler; depends only on core + ports
seqterm-audio-engine ← CPAL adapter + realtime FX; implements ports
seqterm-ui       ← ratatui TUI; depends on all crates
```

**Key rule:** `seqterm-core` must not depend on any runtime library. If you need to add a dependency to core, open an issue first.

---

## Code Standards

### Rust style

- Format with `cargo fmt` before committing. CI enforces `cargo fmt --check`.
- Fix all `cargo clippy -- -D warnings` before submitting a PR.
- No `unsafe` without a `// SAFETY:` comment explaining why it is sound.
- No `unwrap()` / `expect()` in library code that can be called from user contexts. Return `Result` or `Option`.

### Comments

- Default is **no comments**. Add a comment only when the *why* is non-obvious.
- Never write comments that repeat what the code says.
- No multi-paragraph docstrings on private items.

### Tests

- Every new FX processor must have at least two tests: output differs from dry, and zero-mix is passthrough.
- Every new domain field that is serialized must have a roundtrip serialization test.
- Tests live in `#[cfg(test)] mod tests` at the bottom of the source file, not in a separate file.

### Realtime safety

**The audio callback must be allocation-free, lock-free, and mutex-free.**

If your code runs in `Mixer::mix()` or any `FxProcessor::process_block()`:
- Do not allocate (no `Vec::new()`, no `String::new()`, no `Box::new()`)
- Do not lock a `Mutex` or `RwLock`
- Do not call `std::thread::sleep`
- Do not call `std::fs` or any blocking I/O
- Pre-allocate all state in `new()` / `reset()` and reuse it in the hot path

---

## Adding an FX Processor

1. Create `crates/seqterm-audio-engine/src/fx/your_fx.rs`
2. Implement `FxProcessor`: `process_block`, `reset`, `set_mix`
3. Export from `fx/mod.rs`
4. Add variant to `AudioFxKind` in `seqterm-ui/src/app.rs`
5. Add to `ALL_FX_KINDS` and `build_fx_chain` match
6. Add to `FxKind` in `seqterm-core/src/channel.rs` with param labels
7. Add tests (output differs from dry; zero-mix passthrough)

---

## Adding a Domain Field

When adding a field to `Note`, `Pattern`, `Channel`, or `Project`:

1. Add `#[serde(default)]` or a serde default function — this ensures old project files continue to deserialize.
2. Initialize the field in `new()` / `Default` impls.
3. Export from `seqterm-core/src/lib.rs` if it needs to be public.
4. Add a roundtrip serialization test.
5. If the field is a new enum variant that changes behavior, bump the schema version in `Project::CURRENT_VERSION` and add a migration in `seqterm-persistence`.

---

## Pull Request Process

1. Fork → branch → commits → PR against `main`.
2. Branch naming: `feat/short-description`, `fix/short-description`, `docs/short-description`.
3. PR title: keep it under 70 characters. Use the imperative mood: "Add Expander FX processor", not "Added" or "Adding".
4. Description: explain *what* and *why*. Reference the TODO.md item if applicable.
5. All CI checks must pass (fmt, clippy, build, test on Linux/macOS/Windows).
6. At least one approving review required for merges to `main`.

### Commit messages

Use conventional commits:
```
feat(audio-engine): add Expander FX processor with downward/upward modes
fix(scheduler): apply "bpm" automation lane alias for SMF import
docs(raspberry-pi): add Pi 4/5 latency tuning guide
test(core): add Channel serialization roundtrip test
```

---

## Reporting Issues

- Bug reports: include OS, Rust version (`rustc --version`), audio backend, and a minimal reproduction.
- Feature requests: check `TODO.md` first — it may already be tracked.
- Performance issues: include `RUST_LOG=seqterm=debug` output and CPU/buffer size.

---

## License

By contributing, you agree that your contributions are licensed under the MIT License.
