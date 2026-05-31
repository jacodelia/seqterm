# Phase 3 — Plugin Ecosystem & Collaboration Infrastructure

Phase 3 expands SeqTerm's plugin support to modern formats (VST3, CLAP, AU) and lays the infrastructure for multi-user collaboration and cloud sync on top of the `.stz` format.

---

## Plugin Hosting — Modern Formats

### VST3

**Priority: High**

Implement a `seqterm-plugin-vst3` crate:

- Uses the `vst3-sys` or `vst3` crate for the VST3 C++ ABI bindings.
- `Vst3PluginHost` implements `PluginHostPort`.
- VST3 plugins expose `IComponent` (audio processing) and `IEditController` (parameter UI) as separate objects — SeqTerm's separation maps naturally to this.
- State persistence uses `IBStream` serialisation into the `.stz` `plugins/state/` directory.
- Supports MIDI 2.0 per-note controllers (`INoteExpressionController`) when hardware is available.

### CLAP

**Priority: High**

Implement a `seqterm-plugin-clap` crate:

- Uses the `clap-sys` crate for the C ABI.
- CLAP's audio thread safety model explicitly declares which functions are RT-safe — use this to validate plugins before calling from the audio callback.
- CLAP's state extension maps directly to the `.stz` state blob.
- CLAP's note ports extension enables polyphonic expression without the MPE workaround.

### AU (macOS)

**Priority: Medium**

- Implement `seqterm-plugin-au` using the `AudioUnit` framework bindings.
- AU plugins are inherently macOS-only; the crate is cfg-gated with `#[cfg(target_os = "macos")]`.
- AU state serialisation uses `AudioUnitGetProperty(kAudioUnitProperty_ClassInfo)`.

### Plugin Sandbox

**Priority: Low**

Run plugins in a separate OS process (using `fork`/`exec` on Linux/macOS) with a shared-memory audio bridge. Provides crash isolation: a misbehaving plugin cannot bring down the audio engine.

---

## Collaboration Infrastructure

### Object-Level Merge

**Priority: High**

The `.stz` format stores each object as an independent UUID-keyed file. This enables Git-style three-way merges:

- Define a `merge(base, ours, theirs) -> MergeResult` operation for each object type.
- For patterns: merge at the step level (last-write-wins per step, flag conflicts).
- For automation: merge at the point level.
- For project metadata (BPM, name): flag as conflicted if both parties changed.

### Collaborative Session (CRDT)

**Priority: Medium**

Introduce a `seqterm-collab` crate:

- Each object change is represented as a delta operation with a Lamport timestamp and author UUID.
- Delta operations are broadcast over a WebSocket connection.
- Receiving peers apply deltas using CRDT rules (conflict-free for commutative operations; last-write-wins for non-commutative).
- The UI displays a "collaborators" indicator in the transport bar when a session is active.

### Cloud Sync

**Priority: Low**

- `seqterm-cloud` crate: authenticated upload/download of `.stz` files.
- Background sync thread detects remote changes and applies them as delta operations.
- Conflict resolution uses the collaborative merge logic from above.
- Authentication is provider-agnostic (OAuth2 token, stored in `AppSettings`).

---

## Audio

### Distributed Rendering

**Priority: Medium**

The `.stz` format reserves a `render/` directory for offline stems. Phase 3 enables distributed rendering:

- A `render-worker` binary reads a `.stz` file, renders a subset of tracks, and writes stems back.
- The main application orchestrates workers over local IPC or a network socket.
- Workers share the same `seqterm-audio-engine` code; no additional rendering engine is needed.

### MIDI 2.0 Full Support

**Priority: Low** (hardware-dependent)

When a MIDI 2.0 device is connected:

- Switch the MIDI I/O path from MIDI 1.0 byte streams to UMP packets.
- Enable CI capability exchange (`seqterm-midi::midi2::MidiCiMessage`).
- Support per-note pitch, pressure, and timbre as independent automation parameters.

---

## UI

### GUI Rendering Mode (Experimental)

**Priority: Low**

Investigate a `seqterm-gui` crate using `wgpu` or `egui` as an alternative frontend:

- The domain, scheduler, and audio engine are frontend-agnostic (hexagonal architecture).
- The GUI frontend would reuse all existing `AppCommand` dispatch logic.
- The TUI frontend remains the primary supported mode.

### Browser MIDI (WebAssembly)

**Priority: Low**

A `seqterm-wasm` crate targeting WebAssembly:

- Uses Web MIDI API for I/O.
- Uses Web Audio API for output (no CPAL).
- Runs the scheduler and domain logic unchanged.
- Limited to the core sequencer; audio engine features that require native codecs are disabled.

---

## Testing

Phase 3 targets 300 passing unit tests, adding:

- VST3 and CLAP plugin load/process tests against bundled test plugins.
- CRDT merge correctness tests (concurrent pattern edits, conflict scenarios).
- Cloud sync round-trip tests against a mock server.
- Distributed render consistency tests (offline == live output).
