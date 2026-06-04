# Phase 2 — Professional Audio & Project Management

Phase 2 focuses on closing the gap between SeqTerm and established DAWs at the audio quality and project management levels.

**Status (2026-05-31):** Major audio, mixer, arranger, and MIDI items are complete. Remaining open items: CPAL duplex stream, overdub, VST2 state persistence, plugin automation lanes, group bus routing, routing matrix view, drum pattern matrix, .stz autosave, incremental .stz save.

---

## Priority Order

Features are ordered by user-facing impact. High-priority items ship first; the phase is complete when all are done.

---

## Audio Quality

### Time-Stretch via rubato ✅

**Status: Complete**

`LoadedClip::time_stretch(ratio)` and `time_stretch_to_bpm(orig, proj)` implemented using `rubato::FastFixedIn` sinc interpolation. Applied offline from a background thread; pitch remains independent of stretch.

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

**Status: Pending**  
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

### Snapshot System ✅

**Status: Complete**

`StzContainer.take_snapshot(name, json)` / `restore_snapshot(id)` / `list_snapshots()` implemented. Snapshots stored as `snapshots/{uuid}/meta.json` + `project.json` inside the `.stz` archive. `Ctrl+T` in the App takes a snapshot. `App.stz_container` holds the active container.

### Autosave to `.stz`

**Status: Pending**  
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

**Status: Pending**  
**Priority: Medium**

For projects already stored as `.stz`, only changed objects should be rewritten on save. Implement a dirty-tracking layer:

- Each domain object carries a `modified: bool` flag set by the history system.
- `save()` opens the existing archive, replaces only dirty objects, and writes a new archive atomically.

### Cross-Platform Path Normalisation

**Priority: Low**

Audit all `PathBuf` → string conversions to ensure forward-slash normalisation on Windows (ZIP paths must use `/`).

---

## Mixer Redesign ✅

**Status: Complete** (group bus routing, per-channel I/O selector, and routing matrix view pending)

- Per-slot peak metering with exponential decay; master peak L/R
- RMS metering — EMA per slot and master L/R (teal ▬ bar above peak VU in Mixer view)
- Clip indicator — red CLIP label when peak > 0 dBFS since last reset; `c` resets all
- Headroom display — "HR+x.x" label when peak > −6 dBFS
- LUFS meter — K-weighted 2-stage biquad, 400 ms blocks, short-term 3 s, integrated gated (ITU-R BS.1770-4); M/S/I rows on MASTER R strip
- Correlation meter — Pearson L/R, EMA-smoothed; φ row with colour coding on MASTER R strip
- Spectrum analyzer — 2048-pt FFT, 32 log-spaced bands, Hann window; bar chart overlay on MASTER L strip
- Channel type labels (AU/IN/GR/RE/MA badge); `t` cycles type
- Channel colour (`Channel.color: u8`, 0=auto, 1–7=palette); `K` cycles palette
- Phase invert toggle — `⊘` indicator + `P` key
- Width knob — `W`/`w` adjust ±0.1; mono button `◉` + uppercase `M`; record arm `●` + uppercase `R`

---

## Arranger Redesign ✅

**Status: Mostly complete** (tool modes and vertical zoom pending)

- Track types: MIDI / Audio / Drum / Group / Bus / Auto — `t` cycles, stored in `proj.track_types`
- Track visibility toggle (`H`), track colour (8-colour palette, `c` cycles), track height 2–6 lines (`+`/`-`)
- Beat sub-divisions in bar ruler (dots between bar labels); snap-to-grid Off/Bar/½Bar/¼Bar/1/8/1/16/1/32 (`S` cycles)
- Multi-select clips — `Space` toggles; `Shift+↑↓` extends selection to adjacent rows
- Clip operations: duplicate (`d`), delete (`Del`/`Backspace`)
- Clip resize — `r` enters resize mode; `[`/`]` shrink/grow by one bar; `r`/`Esc` exits
- Clip split at playhead position (`x`); clip glue — merge adjacent same-pattern clips (`g`)
- Horizontal zoom — `ArrangerState.bar_width` 2–8; Ctrl+scroll
- Loop region — `I`=loop in, `O`=loop out, `L`=toggle; green tint + `[I`/`O]` markers in beat row
- Timeline markers — `m` adds/removes at current bar; shown as `▼name` in marker row

---

## Dynamics & EQ Integration ✅

**Status: Complete**

All dynamics, EQ, and modulation processors wired into the Mixer FX selector with full parameter exposure in the FX detail panel:

- **Compressor** — threshold, ratio, attack, release, makeup, knee; `Compressor::limiter()` hard preset
- **Gate** — threshold, attack, hold, release, range floor
- **ParametricEq** — 4-band biquad (HP · LowShelf · Peak · HighShelf/LP); freq, gain, Q per band
- **Chorus** — rate, depth, feedback, stereo width (π phase offset)
- **Flanger** — rate, depth, feedback, optional stereo mode
- **Phaser** — 2–8 all-pass stages, LFO rate/depth
- **StereoWidener** — M/S processing (0=mono, 1=unity, 2=wide)
- **Gain** — utility gain stage (dB); **PhaseInvert** — per-channel polarity flip; **MonoMaker** — L+R sum to mono

---

## FX — Expander & Pan ✅

**Status: Complete**

- `Expander` — downward/upward expansion; threshold, ratio, attack, release, range; 2 unit tests
- `Pan` — linear and constant-power law; 3 unit tests (center=unity, full-left, full-right)

Both wired into `build_fx_chain`; all 26 processors now active.

---

## Drum Workflow ✅

**Status: Complete** (dedicated drum pattern matrix view pending)

- `Channel::is_drum: bool` — MIDI channel 10 designation; `bank_msb`/`bank_lsb` fields; mixer `D` toggles
- `drum_map: [u8; 16]` — per-channel pad-to-MIDI-note mapping; default is GM General MIDI drum map
- Drum Kit Browser — `Sf2BrowserState.drum_mode` auto-selects bank 128 on load
- Multiple drum kits per project — each Channel has independent `sf2_path`/`sf2_bank`/`sf2_preset` + `drum_map`
- Bank switching — `B`/`b` keys in mixer cycle `bank_msb` (0–127); sends CC0 to audio engine slot
- Program switching — mixer `f` opens SF2 browser in drum mode (bank 128); falls back to file picker if no SF2 assigned

---

## SF2 Engine Improvements ✅

**Status: Complete** (velocity layers and ADSR per preset pending)

- Multiple simultaneous SoundFonts — each unique SF2 path loads independently via `load_sf2_multi`
- Bank Select MSB (CC0) + LSB (CC32) — `SoundFontSynth.control_change()` passes both to oxisynth; bank 128 percussion bug fixed
- Expression CC11 — `Note.cc11` field (default 127); scheduler sends `AudioControlChange { cc: 11 }` when non-default
- Chorus Send CC93 — `Note.cc93` field (default 0)
- Sustain Pedal CC64 — `Note.cc64: u8`; oxisynth handles it natively

---

## MIDI Engine Improvements ✅

**Status: Complete**

- Tempo Change (FF 51) — SMF import builds a "BPM" automation lane; scheduler applies it via `process_automation`
- Time Signature Change (FF 58) — applied per pattern at import; sets `time_sig_num`/`den`
- Program Change events — `Note.program_change: Option<u8>`; CC 0xFE sentinel → `AudioCommand::ProgramChange` → oxisynth `select_preset`
- Meta Events — FF 06 Marker + FF 07 CuePoint parsed in SMF import → merged into `proj.markers`

---

## Testing

**Current:** 215 passing tests (as of 2026-05-31).

Phase 2 targets 220+ tests. Remaining additions:
- Plugin state save/restore (chunk and parameter modes).
- Autosave recovery simulation.
- Duplex stream integration test (mock input → granular output).
- SF2 Bank Select MSB+LSB combination.
- MIDI Tempo Change FF 51 mid-playback update.
