# SeqTerm Design Principles

SeqTerm is a professional-grade terminal-based MIDI and audio sequencer written in Rust. Every architectural decision flows from a small set of non-negotiable principles.

---

## 1. Realtime Safety Is a Hard Boundary

The audio callback is law. Everything that executes between the OS handing control to the CPAL callback and the callback returning its filled buffer must satisfy:

- **No heap allocation** — all buffers are pre-allocated at startup.
- **No mutex acquisition** — shared state flows through lock-free channels (`rtrb` ring buffers) and atomic variables.
- **No blocking** — no `sleep`, no `Mutex::lock`, no file I/O.

This boundary is not merely a guideline; it is enforced structurally. The `Mixer`, `SoundFontSynth`, `AudioClipPlayer`, and `GranularEngine` types contain no `Mutex` fields. The scheduler communicates with the audio engine exclusively through a `rtrb::Producer<AudioCommand>` ring buffer.

When in doubt, move work to a non-RT thread.

---

## 2. Hexagonal Architecture

SeqTerm separates domain logic, port definitions, and infrastructure adapters into distinct crates:

```
Domain          seqterm-core, seqterm-routing, seqterm-generative
Ports           seqterm-ports  (trait definitions only — no implementations)
Application     seqterm-application, seqterm-command
Infrastructure  seqterm-audio-engine, seqterm-midi, seqterm-persistence, seqterm-stz
Frontend        seqterm-ui, seqterm-app
```

The domain layer has zero knowledge of CPAL, midir, ratatui, or ZIP files. Infrastructure adapters implement port traits and are injected at startup. This makes the core logic testable in isolation and allows future backends (e.g. JACK-only, WebAssembly) to be added without touching domain code.

---

## 3. UUID-Based Object Identity

Every persistent object — patterns, clips, channels, buses, automation lanes, plugins — carries a globally unique UUID that never changes for the life of the project. Array indices are never used as identifiers. This design:

- Enables the `.stz` container format to store each object as an independent file.
- Supports future collaborative editing and conflict-free merges.
- Makes undo/redo operations safe when the collection is reordered.

---

## 4. Polymeter by Default

The step sequencer is inherently polymetric. Each pattern in the matrix has its own length and loops independently. The scheduler fires all active clips simultaneously; each clip calculates its playback position as `global_step % pattern.length`. No pattern is forced to align to a common bar boundary. Time-signature metadata is carried per-pattern and used for display only.

---

## 5. Lazy Everything

Resources are loaded on demand and never duplicated:

- **SF2 fonts** are loaded once per path; all clips sharing the same SF2 path share a single `SoundFontSynth` instance via multi-channel program selection.
- **Audio waveform caches** are generated in background threads and stored in an in-memory map keyed by path.
- **MIDI ports** are opened on first use and kept alive only as long as at least one clip routes to them.
- **Asset data in `.stz` archives** is written and read per-UUID — only referenced objects are loaded.

---

## 6. Non-Destructive by Default

No user action permanently destroys data:

- **Clip trim** and **loop points** are offset metadata on top of the original decoded PCM; the audio file is never modified.
- **Pattern mutations** from the Generative Engine are applied to a working copy; the original is kept in undo history.
- **Project saves** use atomic write-rename: a temporary file is fully written and validated before it replaces the original.
- **Snapshots** reference existing objects by UUID; they never copy sample data.

---

## 7. The UI Is a Pure Projection

The `seqterm-ui` crate reads project state, transport state, and audio stats every frame and renders them to the terminal. It never holds authoritative state beyond UI cursor positions. Any mutation goes through `AppCommand`, which is dispatched through `dispatch_command()` and recorded in the undo history where appropriate. This keeps the render path referentially transparent and simplifies debugging.

---

## 8. Prefer Composition Over Abstraction

Three similar lines of code are better than a premature abstraction. SeqTerm does not introduce helper traits, builder patterns, or extension methods unless the same logic appears four or more times across distinct call sites. The FX chain is a `Vec<Box<dyn FxProcessor>>`; it is not wrapped in a `ChainManager` struct with lifecycle callbacks.

---

## 9. Errors Are Structured, Not Strings

All error types use `thiserror` with named variants that carry structured context. `anyhow` is used only at the application boundary (I/O, startup, CLI parsing). Functions that can fail in the audio path return `Option` or a domain-specific `Result` — they never panic and never allocate in the error path.

---

## 10. Cross-Platform from Day One

Every file path stored in project files is relativized to the project root on save and resolved back to absolute on load. SF2 and audio paths written into patterns use `PathBuf`, not `String`. The audio backend selection logic (`pipewire_is_running()`, JACK fallback, ALSA fallback) is encapsulated in `CpalAudioBackend` and hidden behind the `AudioBackendPort` trait so upper layers see the same interface on all platforms.
