# Phase 2 — Professional Audio & Project Management

Phase 2 focuses on closing the gap between SeqTerm and established DAWs at the audio quality and project management levels. It introduces offline time-stretch, live audio input, plugin state persistence, and the full `.stz` project workflow including snapshots and cloud-ready autosave.

---

## Priority Order

Features are ordered by user-facing impact. High-priority items ship first; the phase is complete when all are done.

---

## Audio Quality

### Time-Stretch via rubato

**Priority: High**

Integrate the `rubato` crate for offline sample-rate conversion and time-stretching:

- `AudioClipPlayer` gains a `stretch_ratio: f64` field (1.0 = no stretch, 0.5 = half speed, 2.0 = double speed).
- Stretch is applied at load time (non-RT), producing a resampled PCM buffer.
- The pitch remains independent of stretch when `pitch_semitones` is non-zero.
- Exposed in the matrix routing panel as "Time Stretch %" and in the Tracker track modulation panel.

### CPAL Duplex Stream (Live Audio Input)

**Priority: High**

Open a CPAL input stream alongside the output stream:

- `AudioCallback` gains an `input` slice in addition to `output`.
- A pre-allocated ring buffer routes input samples to any slot configured as a live granular source.
- Enables real-time granular processing and live recording.
- `TriggerMode::Gate` becomes feasible once key-release events are available (dependent on crossterm upstream).

### Overdub Recording

**Priority: Medium**

When the transport is recording and a clip is playing, capture audio into `audio/recordings/{uuid}.wav` inside the `.stz` container. The recorded file is registered in the asset registry and assigned as the clip's audio source.

---

## Plugin Hosting

### VST2 Plugin State Save/Restore

**Priority: High**

- `Vst2Instance` gains `get_chunk() -> Vec<u8>` (calls `effGetChunk`) and `set_chunk(data)` (calls `effSetChunk`).
- On project save, each plugin instance's state blob is written to `plugins/state/{uuid}.state` inside the `.stz` archive.
- On load, the blob is passed back to the plugin via `set_chunk`.
- Fallback: if `effGetChunk` is unsupported (flag `effFlagsProgramChunks` absent), parameter values are serialised individually.

### Plugin Parameter Automation

**Priority: Medium**

- Automation lanes gain a `PluginParam { instance_uuid: Uuid, param_idx: u32 }` target variant.
- The scheduler maps parameter values (0–127) to the plugin's 0.0–1.0 normalised range and calls `set_param` each bar.

---

## Project Format

### Snapshot System

**Priority: High**

Fully wire the `snapshots/` directory in the `.stz` container:

- `AppCommand::TakeSnapshot { name }` serialises the current manifest into `snapshots/{uuid}.json`.
- The snapshot list is accessible via the Project menu → Snapshots.
- `AppCommand::RestoreSnapshot { uuid }` reloads the project state from a snapshot.
- Snapshots are incremental: they only reference existing UUID objects; sample data is not duplicated.

### Autosave to `.stz`

**Priority: Medium**

The autosave thread currently writes `.autosave.json`. In Phase 2:

- Autosave targets the `.stz` container directly, writing a `snapshots/autosave.json` entry inside the active project file.
- On crash recovery, SeqTerm detects the autosave snapshot on next open and offers to restore it.

### Pattern Chain Persistence

**Priority: Medium**

- `project.chain` is already serialised in the JSON format.
- Phase 2 adds the chain to the Arranger's song-export path so that a full piece export includes the chain order, not just individual patterns.

---

## UI Improvements

### Hybrid View Enhancements

**Priority: Medium**

- Tracker Monitor: add step seek by clicking — `app.current_step = clicked_step`.
- Active Patterns: show step progress as animated fill during playback.
- Voice Activity: display per-MIDI-channel activity (not just per-slot peaks).

### Piano Roll Improvements

**Priority: Medium**

- Velocity lane below the grid (horizontal bars per note).
- Chord detection: when two notes start at the same step, display as a chord label.
- Snap-to-grid with configurable resolution (1/4, 1/8, 1/16, 1/32).

### SF2 Multi-Bank Preview

**Priority: Low**

When the SF2 Browser loads a drum kit (bank 128), fire a short drum roll preview instead of a single C4 note.

---

## Persistence & Formats

### STZ — Incremental Save

**Priority: Medium**

For projects already stored as `.stz`, only changed objects should be rewritten on save. Implement a dirty-tracking layer:

- Each domain object carries a `modified: bool` flag set by the history system.
- `save()` opens the existing archive, replaces only dirty objects, and writes a new archive atomically.

### Cross-Platform Path Normalisation

**Priority: Low**

Audit all `PathBuf` → string conversions to ensure forward-slash normalisation on Windows (ZIP paths must use `/`).

---

## Testing

Phase 2 targets 220 passing unit tests, including:

- Time-stretch roundtrip (pitch and tempo independent).
- Plugin state save/restore (chunk and parameter modes).
- Snapshot take/restore.
- Autosave recovery simulation.
- Duplex stream integration test (mock input → granular output).
