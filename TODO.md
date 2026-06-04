# SeqTerm — Master Roadmap & Todo List

Tracking all tasks from `SEQTERM_DAW_MASTER_REFACTOR.md` and organic feature work.

**Status key:** `[x]` = done · `[ ]` = pending · `[~]` = partial / in progress

Priority: **P0** blocking · **P1** core · **P2** important · **P3** polish

> **Audit 2026-06-02 (Opus 4.8):** Phases 1–2 (sequencer, realtime audio engine,
> mixer, FX, UI, persistence) are genuinely implemented, integrated and tested.
> Phases 3–4 contained many `[x]` items that were in fact **orphaned stub crates**
> not wired into the app; these have been re-marked `[~]` with honest notes.
> This pass also: wired the **VST2 host** into the registry (functional), added
> `PluginHostPort` adapters + feature plumbing for **VST3/CLAP** (scanning real,
> processing pending the format SDKs), added `seqterm-wasm` to the workspace, and
> cleaned compiler warnings. Build: green. Tests: **258 pass / 0 fail / 2 ignored**.

---

## ✅ PHASE 1 — Foundation (Complete)

### Architecture

- [x] **P0** Hexagonal architecture: domain / ports / adapters / frontend layers
- [x] **P0** `seqterm-ports` — pure trait definitions (`AudioBackendPort`, `MidiBackendPort`, `ProjectRepository`, `AudioSynthPort`, `PluginHostPort`, …)
- [x] **P0** `seqterm-application` — `AppCommand` enum, `EventBus` (flume pub/sub), `CommandBus`, use cases
- [x] **P0** Domain isolation — `seqterm-core` depends only on `serde` + `thiserror`
- [x] **P0** Single unified scheduler (no duplicate timing systems)
- [x] **P0** Transport state published via triple buffer (lock-free UI read)

### Realtime Audio Engine

- [x] **P0** CPAL backend — PipeWire/JACK/ALSA auto-selection
- [x] **P0** Lock-free `rtrb` ring buffer between non-RT control and RT callback
- [x] **P0** `Mixer` — 32 pre-allocated slots, stereo mix, soft-clip limiter
- [x] **P0** No allocation / no mutex / no blocking in audio callback
- [x] **P0** Aux bus sends A/B with return volumes and mute
- [x] **P0** Master bus insert FX chain (post-bus, pre-clip)
- [x] **P0** Peak metering with exponential decay (PEAK_DECAY=0.98)
- [x] **P1** RMS metering — exponential moving average per slot + master L/R  ← NEW
- [x] **P1** Live oscilloscope — 1024-sample waveform ring via atomics
- [x] **P0** Audio lookahead compensation for buffer latency

### Scheduler

- [x] **P0** 480 PPQN tick clock, dedicated OS thread
- [x] **P0** Polymeter — each clip fires at `global_step % pattern.length`
- [x] **P0** Sub-step precision: `micro` field ±99% of step (~116ms at 128 BPM)
- [x] **P0** Gate duration tracking (`note.gate` as % of step in ticks)
- [x] **P0** MIDI clock output (24 PPQ), Start, Stop
- [x] **P1** MPE channel allocation per clip
- [x] **P1** Song-mode pattern chain following
- [x] **P1** Automation lane interpolation (per bar)
- [x] **P1** `AudioControlChange` event for CC01/CC74 → SF2 synths

### MIDI

- [x] **P0** MIDI input bus (fan-in from all enabled ports)
- [x] **P0** MIDI output routing per clip
- [x] **P0** Virtual port creation (one per pattern key)
- [x] **P0** SMF Type 0/1 import — quantised to step grid, CC1/CC74/PB preserved
- [x] **P0** MusicXML export
- [x] **P1** MIDI Learn — bind CC to volume/pan/send/BPM
- [x] **P1** OSC server (UDP, rosc)
- [x] **P3** MIDI 2.0 UMP utilities + MIDI 1↔2 conversion

### FX Chain — Original Set

- [x] **P1** `FxProcessor` trait (process_block / reset / set_mix)
- [x] **P1** `Bitcrusher` — bit-depth + sample-rate reduction
- [x] **P1** `Svf` — topology-preserving state-variable filter (LP/HP/BP/Notch)
- [x] **P1** `DelayLine` — stereo ping-pong delay
- [x] **P1** `Reverb` — Freeverb (8 comb + 4 allpass)
- [x] **P1** `VinylSim` — wow/flutter/crackle
- [x] **P1** `Cassette` — tape saturation
- [x] **P1** `Isolator` — 3-band (bass/mid/treble) SVF, 48 dB/oct
- [x] **P1** `FilterBankFx` — 48-band graphic EQ
- [x] **P1** `Looper` — RT loop recorder (Idle/Record/Play/Overdub)
- [x] **P1** `SidechainDuck` — LFO/trigger ducking
- [x] **P1** `GranularDelay` — granular feedback delay

### FX Chain — DAW Refactor Additions  ← ALL NEW

- [x] **P1** `Compressor` — feed-forward peak, soft-knee, threshold/ratio/attack/release/makeup
- [x] **P1** `Compressor::limiter()` — hard limiter preset
- [x] **P1** `Gate` — noise gate with hold phase and range floor
- [x] **P1** `ParametricEq` — 4-band biquad (HP · LowShelf · Peak · HighShelf/LP)
- [x] **P1** `Chorus` — LFO-modulated delay lines, stereo width via π phase offset
- [x] **P1** `Flanger` — short-delay with feedback, optional stereo mode
- [x] **P1** `Phaser` — all-pass chain (2–8 stages), LFO sweep
- [x] **P1** `StereoWidener` — M/S processing (0=mono, 1=unity, 2=wide)
- [x] **P1** `Gain` — utility gain stage (dB)
- [x] **P1** `PhaseInvert` — per-channel polarity flip
- [x] **P1** `MonoMaker` — sum L+R to mono
- [x] **P1** `SoftClipper` — tanh waveshaper with drive
- [x] **P1** `TubeSaturation` — asymmetric triode waveshaper + HP tone

### Domain Model

- [x] **P0** `Note` — 16 fields including cc01, cc74, pitch_bend, micro, prob, chord voices
- [x] **P0** `Pattern` — variable length, time signature, swing (50=neutral), prob, random
- [x] **P0** `Channel` — volume, pan, mute, solo, send_a/b, EQ, FX slots
- [x] **P1** `Channel::channel_type` — Audio / Instrument / GroupBus / Return / Master  ← NEW
- [x] **P1** `Channel::phase_invert` — polarity flip flag  ← NEW
- [x] **P1** `Channel::width` — stereo width 0.0–2.0  ← NEW
- [x] **P1** `Channel::mono` — force mono output  ← NEW
- [x] **P1** `Channel::record_arm` — live recording flag  ← NEW
- [x] **P1** `PatternSource` — Midi / Sf2 / AudioFile
- [x] **P0** `Project` — matrix, patterns, channels, scenes, buses, routing graph

### Persistence

- [x] **P0** JSON + MessagePack with atomic write-rename
- [x] **P0** Schema migrations (v0→v1)
- [x] **P0** Autosave thread (60 s interval)
- [x] **P1** `.stz` ZIP container format (UUID objects, asset registry, migration system)

### UI

- [x] **P0** 7 views: Matrix, Tracker, Arranger, Mixer, Config, Sampler, Granular
- [x] **P0** Full mouse support across all views and modals
- [x] **P1** Matrix sidebar tabs: PANELS (Poly + Routing) and HYBRID
- [x] **P1** Hybrid View: Active Patterns, Tracker Monitor, Voice Activity
- [x] **P1** SF2 Bank/Preset modal with full mouse: bank ◄►, list clicks, Accept/Cancel
- [x] **P1** Pattern defaults: swing=0, prob=0, random=0 for new projects and MIDI imports
- [x] **P1** New project → always 8×8 matrix

### CI/CD & Release

- [x] **P0** `.github/workflows/ci.yml` — fmt + clippy + build + test on Linux/macOS/Windows
- [x] **P0** `.github/workflows/release.yml` — Linux x86_64 (.deb/.rpm), ARM64 (.deb), macOS Universal (.dmg), Windows (.msi)
- [x] **P0** Semantic versioning (vMAJOR.MINOR.PATCH) on release tags
- [x] **P2** Automated changelog generation — `git-cliff` via `orhun/git-cliff-action@v3` in release.yml; `cliff.toml` with conventional-commits grouping

### Documentation

- [x] **P1** `docs/architecture/` — audio-engine, scheduler, arranger, mixer, midi, sf2-engine, plugin-hosting
- [x] **P1** `docs/roadmap/` — phase-1 through phase-4
- [x] **P1** `docs/design-principles.md` — 10 architectural principles

### Phase 1 Deliverables (pending analysis docs)

- [x] **P1** FluidSynth Evaluation — `docs/fluidsynth-evaluation.md`; recommends oxisynth default + optional FluidSynth adapter behind feature flag
- [x] **P1** ARM Compatibility Analysis — `docs/arm-compatibility.md`; all subsystems verified; CPU budget table for Pi 4/5
- [x] **P1** Raspberry Pi Deployment Strategy — `docs/raspberry-pi.md`; Pi 4/5 install, ALSA tuning, RT scheduling, SF2 budget, headless tips
- [x] **P2** Cross-Compilation Strategy document — `docs/cross-compilation.md`: targets, native/cross toolchain, cargo config, feature flags
- [x] **P2** Platform-Specific Optimization Recommendations — `docs/platform-optimizations.md`: SIMD guards, latency targets, JACK/PipeWire/ALSA/WASAPI tuning, build profiles, memory budget

---

## 🔧 PHASE 2 — Professional Audio & Mixer (In Progress)

### Dynamics & EQ Integration

- [x] **P1** Wire Compressor/Gate/Limiter into FX selector in Mixer UI
- [x] **P1** Wire ParametricEq into FX selector
- [x] **P1** Expose Compressor params in FX detail panel (threshold, ratio, attack, release, makeup, knee)
- [x] **P1** Expose Gate params in FX detail panel (threshold, attack, hold, release, floor)
- [x] **P1** Expose ParametricEq band params in FX detail panel (freq, gain, Q)
- [x] **P1** Wire Chorus/Flanger/Phaser into FX selector with rate/depth/feedback params
- [x] **P1** Wire StereoWidener/Gain/PhaseInvert/MonoMaker into FX selector
- [x] **P1** Show RMS meters in Mixer view (audio slot strips, alongside peak bar)
- [x] **P1** Show master RMS L/R in Mixer MASTER strips (teal ▬ bar above peak VU)

### Mixer Redesign

- [x] **P1** Per-slot peak metering with decay
- [x] **P1** Master peak L/R
- [x] **P1** RMS metering per-slot and master
- [x] **P1** Clip indicator (peak > 0 dBFS since last reset) — red CLIP label in strip + overlay on MASTER; 'c' resets
- [x] **P1** Headroom display (dB below 0 dBFS) — shown as "HR+x.x" in strip dB label when peak > -6 dBFS
- [x] **P2** LUFS meter architecture — `LufsIntegrator` in `seqterm-audio-engine/src/lufs.rs`: K-weighting (2-stage biquad), 400ms blocks, short-term 3s, integrated gated; published via atomics in cpal_backend; read by App; shown on MASTER R strip (M/S/I rows)
- [x] **P2** Correlation Meter architecture — Pearson L/R correlation computed in `Mixer::mix()`, EMA-smoothed; published as `master_correlation` atomic; shown on MASTER R strip (φ row with color coding)
- [x] **P2** Spectrum Analyzer architecture — `spectrum.rs`: rustfft 2048-point FFT, 32 log-spaced bands, Hann window; published via `spectrum_bands` atomics in cpal_backend; `App.master_spectrum` polled per frame; shown as `draw_spectrum_overlay` bar chart on MASTER L strip in Mixer
- [x] **P2** Channel type labels in mixer strips — AU/IN/GR/RE/MA badge in title; `t` cycles
- [x] **P2** Channel color in mixer strips — `Channel.color: u8` (0=auto, 1-7=palette); mixer `K` cycles; `collect_mixer_entries` uses it
- [x] **P2** Phase invert toggle in mixer strip — `⊘` indicator + `P` key
- [x] **P2** Width knob in mixer strip — shown implicitly; `W`/`w` adjust ±0.1
- [x] **P2** Mono button in mixer strip — `◉` indicator + `M` (uppercase) key
- [x] **P2** Record arm button in mixer strip — `●` indicator + `R` (uppercase) key
- [x] **P2** Group bus routing (send channel to group bus, not just aux A/B)
- [x] **P2** Routing Matrix view — full grid of sends/receives between all channels (audio routing matrix in Mixer, `\` to open, hjkl+Enter to assign, ↑↓ on send columns to adjust)
- [x] **P2** Per-channel routing selector — output destination shown in FX row of mixer strip (`→G3` or `→MST` in teal); `G` key cycles group bus; `\` opens full routing matrix; title badge shows `AU→G3`

### Arranger Redesign (FL Studio Playlist-style)

- [x] **P1** Basic track lanes with clip blocks
- [x] **P1** Automation lanes
- [x] **P1** Bar ruler with playhead
- [x] **P1** Track types: MIDI / Audio / Drum / Group / Bus / Auto — `t` cycles, stored in `proj.track_types`
- [x] **P1** Track visibility toggle — `H` toggles hidden, stored in `proj.track_hidden`
- [x] **P1** Track color — 8-color palette, `c` cycles, stored in `proj.track_colors`
- [x] **P2** Track height — `proj.track_heights: HashMap<String, u8>` (2-6 lines); `+`/`-` in arranger track section
- [x] **P1** Beat sub-divisions in bar ruler (beats within each bar) — dots between bar labels
- [x] **P1** Snap-to-grid: Off / Bar / ½Bar / ¼Bar / 1/8 / 1/16 / 1/32 — `S` cycles, shown in header
- [x] **P1** Multi-select clips — `Space` toggles clip in multi_select; `Shift+↑↓` extends selection to adjacent rows
- [x] **P1** Clip operations: Duplicate (`d`), Delete (`Del`/`Backspace`) — selected by `[`/`]` column cursor
- [x] **P1** Clip resize — `r` enters resize mode; `[`/`]` shrink/grow pattern by one bar; `r`/`Esc` exits
- [x] **P1** Clip split at playhead position — `x` key
- [x] **P1** Clip glue (merge adjacent clips of same pattern) — `g` key
- [x] **P2** Viewport: horizontal zoom — `ArrangerState.bar_width: u8` (2-8); Ctrl+scroll; all renders use it
- [x] **P2** Viewport: vertical zoom (variable track height) — renderer reads `track_heights`; extra rows fill clip body; `+`/`-` keys in arranger track section
- [x] **P2** Loop region — `proj.loop_region: Option<(u32, u32)>`; arranger `I`=loop in, `O`=loop out, `L`=toggle; shown as green tint in beat row + `[I`/`O]` markers
- [x] **P2** Timeline markers — `proj.markers: Vec<(u32, String)>`; arranger `m` adds/removes at current bar; shown as `▼name` in marker row
- [x] **P2** Tool modes: Select / Draw / Slice / Paint / Mute — `ArrangerTool` enum; `T` cycles; shown in TRACKS title; Enter dispatches per-tool: rename/create/mute/slice/paint-toggle

### FX — Missing Processors

- [x] **P2** `Expander` — downward/upward, threshold/ratio/attack/release/range; 2 tests
- [x] **P2** `Pan` — linear + constant-power law; 3 tests (center=unity, full-left/right)
- [x] **P2** FX slot reorder — `J`/`K` in the FX sidebar moves the selected processor down/up within `audio_slot_fx` and `master_fx` chains; rebuilds RT chain immediately

### Drum Workflow

- [x] **P1** MIDI Channel 10 designation in `Channel` domain model — `is_drum: bool`, `bank_msb`, `bank_lsb`; mixer `D` key toggles
- [x] **P1** Drum Mapping — `drum_map: [u8; 16]` in Channel; default GM_DRUM_MAP (Kick/Snare/HH/Toms/etc.); scheduler uses it when `is_drum`
- [x] **P1** Drum Kit Browser — `Sf2BrowserState.drum_mode` auto-selects bank 128 on load
- [x] **P1** Multiple drum kits per project — each Channel has independent `sf2_path`/`sf2_bank`/`sf2_preset` + `drum_map`
- [x] **P1** Bank Switching from drum track — `B`/`b` keys in mixer cycle `bank_msb` (0-127); sends CC0 to audio engine slot
- [x] **P1** Program Switching from drum track — mixer `f` key opens SF2 browser (drum_mode=true, auto-jumps bank 128); falls back to file picker if no SF2 assigned
- [x] **P2** Drum pattern matrix — interactive 16-pad × N-step grid in Matrix sidebar tab "DRUM"; Tab to focus, hjkl=navigate, Space/Enter=toggle, e=euclidean fill, x=clear pad; polyphonic via chord_notes

### SF2 Engine — Missing Features

- [x] **P1** Multiple simultaneous SoundFonts — each unique SF2 path gets one `load_sf2_multi` slot; different paths load independently
- [x] **P1** Bank Select MSB (CC0) + LSB (CC32) — `SoundFontSynth.control_change()` passes CC0/CC32 to oxisynth; bank 128 percussion bug fixed
- [x] **P1** Expression (CC11) — `Note.cc11` field added (default 127); scheduler sends `AudioControlChange { cc: 11 }` when non-default
- [x] **P1** Chorus Send (CC93) — `Note.cc93` field added (default 0); scheduler sends `AudioControlChange { cc: 93 }` when non-default
- [x] **P2** Velocity Layers — oxisynth selects sample region by key+velocity range per SF2 spec (midi.rs checks key/vel range on NoteOn); velocity forwarded via `MidiEvent::NoteOn { vel }` in `sf2_synth.rs`
- [x] **P2** ADSR per preset — oxisynth reads Volume envelope generators (attack/hold/decay/sustain/release) from SF2 modulator data natively; no override needed
- [x] **P2** Sustain Pedal (CC64) — `Note.cc64: u8`; scheduler fires `AudioControlChange { cc: 64 }` when non-default; oxisynth handles it natively

### MIDI Engine — Missing Features

- [x] **P1** Tempo Change events (FF 51) — import builds "BPM" automation lane; scheduler applies it via `process_automation`; fixed "bpm" target alias
- [x] **P1** Time Signature Change events (FF 58) — `time_sig_at_tick()` applied per pattern at import; pattern gets correct `time_sig_num/den`
- [x] **P2** Program Change events — `Note.program_change: Option<u8>`; CC 0xFE sentinel → `AudioCommand::ProgramChange` → oxisynth `select_preset`
- [x] **P2** Meta Events — FF 06 Marker + FF 07 CuePoint parsed in SMF import → `ImportedMidi.markers` → merged into `proj.markers`

### Time-Stretch & Audio Recording

- [x] **P2** Time-stretch via `rubato` — `LoadedClip::time_stretch(ratio)` + `time_stretch_to_bpm(orig, proj)` using `FastFixedIn` sinc interpolation; called offline from background thread
- [x] **P2** CPAL duplex stream — live audio input routing
- [x] **P2** Overdub recording (requires duplex)
- [x] **P2** Audio quantisation (snap audio to grid after recording)

### Plugin State

- [x] **P2** VST2 state save/restore — `PluginRegistry::collect_states()` calls `effGetChunk`; blobs saved via `StzContainer::set_plugin_state(id, data)` on every snapshot; `effSetChunk` called on `.stz` open
- [x] **P2** Plugin parameter automation lanes — automation target `slot.N.fx.M.param.P` → `EngineEvent::AudioFxParam` → `AudioCommand::SetSlotFxParam`; routes through scheduler `process_automation` and `App::process_events` to the RT mixer; `FxProcessor::set_param(idx, val)` applies the value per block
- [x] **P2** Plugin state stored in `.stz` `plugins/state/{plugin_id}.state` — via asset_data + asset_registry in StzContainer

### Project Format

- [x] **P2** `.stz` snapshot system — `StzContainer.take_snapshot(name, json)` / `restore_snapshot(id)` / `list_snapshots()`; snapshots stored as `snapshots/{uuid}/meta.json` + `project.json`; `Ctrl+T` to take snapshot; `App.stz_container` holds active container
- [x] **P2** Autosave writes snapshot into active `.stz` file — `App::write_stz_autosave()` every 60s when `stz_path` set + `project_dirty`; includes plugin state blobs
- [x] **P2** Incremental `.stz` save — `seqterm_stz::incremental_save(container, path)`: reads existing archive, overlays serializations of dirty UUIDs only, writes merged archive atomically; `container.clear_dirty()` on success

---

## 🔬 PHASE 3 — Plugin Ecosystem & Collaboration (Partial — scaffolds)

### Plugin Hosting

- [x] **P1** VST2 host (`seqterm-plugin-vst2`) — scan/load/process; **registered in `PluginRegistry::with_default_adapters` (default `vst2` feature) → reachable from the running app via `ScanPlugins`/`LoadPlugin`**
- [~] **P2** VST3 crate (`seqterm-plugin-vst3`) — `Vst3Host` implements `PluginHostPort`, scanning of `.vst3` bundles + registry wiring functional behind `vst3` feature; **real-time processing through a loaded bundle still needs the Steinberg VST3 COM SDK**
- [~] **P2** CLAP crate (`seqterm-plugin-clap`) — `ClapHost` implements `PluginHostPort`, `.clap` scanning + registry wiring functional behind `clap-host` feature; **real-time processing still needs `clack-host` (CLAP C ABI)**
- [~] **P3** AU crate (`seqterm-plugin-au`, macOS only) — descriptor/scan scaffold + `InstrumentBackend` stub; **CoreAudio hosting unimplemented, builds empty on non-macOS**
- [~] **P3** Plugin sandbox — `seqterm-plugin-sandbox` shared-memory header + `SandboxedPlugin` scaffold; **process spawn + IPC bridge not yet wired into the registry**

### Backend Abstraction

- [x] **P1** `AudioBackendPort` trait (CPAL adapter)
- [x] **P1** `SoundFontBackend` via `AudioSynthPort` trait
- [x] **P1** `MidiBackendPort` trait — `seqterm-ports/src/midi.rs`; implemented by `MidirMidiAdapter`
- [x] **P1** `InstrumentBackend` trait — added to `seqterm-ports/src/realtime.rs`; `SoundFontSynth` implements it; provides `backend_name`, `select_preset`, `list_presets`, `all_notes_off`
- [x] **P2** FluidSynth as alternative SF2 engine — `seqterm-fluidsynth` offers an **embedded** engine (FluidLite, statically compiled, *no external deps*, feature `fluidsynth`) and a system libfluidsynth path (FFI + cross-platform `build.rs`, feature `fluidsynth-system`); `SoundFontSynth` drives oxisynth or FluidSynth via an internal `Sf2Engine` enum, selectable in **Audio Settings → SF2 engine** / `SEQTERM_SF2_BACKEND` / `set_sf2_prefer_fluidsynth()`, with automatic fallback to oxisynth; silent stub without either feature. See `docs/architecture/sf2-engine.md`
- [~] **P2** SFZ format support — `seqterm-sfz` minimal parser (sections/opcodes/region matching); **not yet wired as a selectable `InstrumentBackend` in the app**

### Collaboration

> ⚠️ `seqterm-collab` and `seqterm-cloud` exist and compile but are **orphaned** —
> no other crate depends on them and they are not wired into the app. Treat the
> items below as standalone scaffolds, not shipping features.

- [~] **P3** Object-level merge (UUID-based three-way diff) — in `seqterm-collab`, not integrated
- [~] **P3** CRDT delta operations (Lamport timestamps) — in `seqterm-collab`, not integrated
- [~] **P3** WebSocket collaboration session — scaffold only, no transport wired
- [~] **P3** Cloud sync adapter — `seqterm-cloud` trait + scaffold, no concrete backend

---

## 🚀 PHASE 4 — Open Platform (Partial — SDK/Lua real; FFI/WASM/collab scaffolds)

### SDK & Scripting

- [x] **P2** `seqterm-sdk` crate — `prelude`, `core`, `ports` re-exports; `project_to_json`/`from_json`/`new_project`/`sdk_version` helpers
- [~] **P2** C FFI layer (`seqterm-ffi`, `seqterm.h` via `cbindgen`) — crate exists but is **orphaned** (no consumer); not built/validated by CI
- [x] **P2** Lua scripting engine (`mlua`) — `on_step` callbacks, `AppCommand` dispatch; **wired into `seqterm-ui` (only Phase 3/4 extra crate that is)**
- [x] **P3** Lua REPL in terminal (live coding mode)

### STZ v2

- [x] **P2** Script storage: `scripts/{name}.lua` in archive
- [x] **P2** Waveform cache — `waveform_cache.rs`: `load_cached`, `write_cached`, `waveform_bands`, `evict_old`; `~/.cache/seqterm/waveforms/`
- [x] **P2** Format version 2 migration — `V1ToV2` migration in `migration.rs` stamps `format_version=2` and adds default fields for `track_colors`, `track_heights`, `track_types`, `markers`, `loop_region`; `STZ_FORMAT_VERSION=2`
- [x] **P3** `stz` CLI tool — `crates/seqterm-stz-cli`; binary `stz`; subcommands: inspect, extract, pack, migrate, validate, diff, snapshot

### Cross-Platform & Distribution

- [x] **P0** Linux x86_64 release artifacts (.deb, .rpm, .tar.gz)
- [x] **P0** Linux ARM64 release artifacts (.deb, cross-compiled)
- [x] **P0** macOS Universal binary (.dmg)
- [x] **P0** Windows x86_64 release artifacts (.msi)
- [x] **P1** Linux ARMv7 (Raspberry Pi OS 32-bit) release artifacts — `linux-armv7` job in release.yml
- [x] **P1** Windows ARM64 release artifacts — `windows-arm64` job in release.yml
- [x] **P2** `Cross.toml` — ARM64 + ARMv7 cross-rs images configured
- [x] **P2** Cargo build profiles — `docs/platform-optimizations.md` documents dev/release/release-lto/rpi profiles
- [x] **P2** Feature flags — `seqterm-audio-engine`: `fluidsynth`, `vst3`, `clap-host`, `wasm` stubs; `seqterm-app`: `fluidsynth`, `vst3`, `clap-host` feature gates that forward to audio-engine
- [x] **P2** Platform-specific Cargo optimizations — `[profile.release-arm64]` (LTO + strip + codegen-units=1) and `[profile.release-win-arm64]` added to workspace Cargo.toml; existing `rpi` profile already optimal
- [x] **P2** Raspberry Pi performance analysis — benchmark large MIDI + multi-SF2 + effects on Pi 4/5
- [x] **P2** Homebrew formula
- [x] **P2** AUR PKGBUILD
- [~] **P3** WebAssembly build (`seqterm-wasm`, Web MIDI + Web Audio) — `wasm-bindgen` bindings; **now a workspace member (compiles on native + wasm32)**, but only a thin binding surface, not a browser-hosted app

### Documentation & Community

- [x] **P0** `docs/architecture/` (7 files)
- [x] **P0** `docs/roadmap/` (4 phases)
- [x] **P0** `docs/design-principles.md`
- [x] **P1** `README.md` — platform matrix, features, install all platforms, audio backends, SF2, Raspberry Pi
- [x] **P1** `CONTRIBUTING.md` — contribution guidelines, coding standards, FX/domain field procedures
- [x] **P1** `RELEASE_PROCESS.md` — SemVer policy, release checklist, changelog format, schema migration rules
- [x] **P1** `docs/raspberry-pi.md` — Pi 4/5 models, install, ALSA tuning, RT scheduling, SF2 budget, headless tips
- [x] **P2** Interactive tutorial mode (`AppCommand::StartTutorial`)
- [x] **P2** `docs.rs` integration for `seqterm-sdk` — `#![deny(missing_docs)]`; `[package.metadata.docs.rs]` with all-features + docsrs cfg; comprehensive module-level docs + 4 working doctests for `project_to_json`, `project_from_json`, `new_project`, `sdk_version`

---

## 🛠 Architecture Debt

- [x] Routing panel click — gated by `sidebar_tab` to avoid spurious activations
- [x] `AudioControlChange` event in engine for CC01/CC74 → SF2 per step
- [x] Pattern defaults neutral (swing=50→displays 0, prob=0, random=0)
- [x] **P2** `FxKind` enum in `seqterm-core` expanded from 9 → 29 variants; covers all audio-engine processors with CC param labels
- [x] **P2** `AudioFxEntry`/`AudioFxKind` in `seqterm-ui` — added `Expander` and `Pan` variants; all 26 processors wired in `build_fx_chain`
- [x] **P2** Mixer strips apply `channel.phase_invert`, `channel.width`, `channel.mono` via `sync_slot_channel_flags` in `rebuild_audio_slots`
- [x] **P3** `FxProcessor::name()` method for UI display — default impl returns `"FX"` in `fx/mod.rs`
- [x] **P3** `FxProcessor::params() -> Vec<FxParam>` + `set_param(idx, val)` — `FxParam` struct (name/value/min/max/unit); implemented on Compressor, Gate, Reverb, DelayLine, Chorus
- [x] **P2** TUI focus management system — `FocusId` drives Mixer FX sidebar + routing matrix; `fx_panel_focused` removed; `Tab`/Ctrl+Tab/`\` all sync `app.focus`; border colors read from `app.focus`
- [x] **P3** TUI render caching — skip re-rendering unchanged widgets (dirty flag per widget)

---

## 🧪 Tests

- [x] **258 passing, 0 failing, 2 ignored** across all crates (`cargo test --workspace`, audited 2026-06-02) — audio-engine (90), core (35), stz (17), midi (14), persistence (8), application (11), routing (9), plugin adapters (vst2 5 / vst3 3 / clap 3 / au 2 / vst3-host…), sfz (3) …
- [x] **P1** Add tests for Compressor: unity gain when below threshold, gain reduction above
- [x] **P1** Add tests for Gate: opens/closes on threshold crossing, hold phase respected
- [x] **P1** Add tests for ParametricEq: bypass=unity, peak boost at freq, HP attenuates DC, LowShelf cut
- [x] **P1** Add tests for StereoWidener: width=0 produces mono, width=1 produces unity
- [x] **P1** Add tests for PhaseInvert: inverts L only, R only, or both
- [x] **P1** Add tests for RMS metering: EMA converges to correct level for DC and master output
- [x] **P2** Add tests for Chorus/Flanger: output differs from dry after first block; zero-mix passthrough
- [x] **P2** Add tests for Expander: downward attenuates below threshold; ratio=1 bypass
- [x] **P2** Add tests for drum channel: `is_drum` defaults false; drum_map matches GM_DRUM_MAP
- [x] **P2** Add tests for SF2 Bank Select: MSB+LSB combination selects correct preset — `bank_select_msb_lsb_encoding` + `bank_select_roundtrip` in `sf2_synth.rs`
- [x] **P2** Add tests for MIDI Tempo Change: BPM updates mid-playback from FF 51 meta event — `bpm_automation_fires_bpm_changed_event` + `bpm_automation_interpolates_between_points` in `scheduler.rs`
- [x] **P2** Channel serialisation roundtrip with new fields (phase_invert, width, mono, channel_type, is_drum, color, drum_map)

---

## Performance Targets

| Target | Status |
|--------|--------|
| 128+ tracks | Architecture supports 32 audio slots + unlimited MIDI |
| 10,000+ clips in arranger | ✅ virtualized clip + track rendering (viewport culling, early-out) |
| 256+ voices | ✅ oxisynth configured at 256 MAX_VOICES |
| Multiple simultaneous SoundFonts | ✅ `load_sf2_multi` groups by path |
| Hundreds of effects | ✅ 26 FX processors, chain pre-allocated |
| Sub-millisecond timing | ✅ 480 PPQN ≈ 1ms resolution at 120 BPM |
| No allocation in callback | ✅ enforced by design |
| No mutex in callback | ✅ rtrb ring + atomic stats |
| ARM64 / Raspberry Pi | ✅ pure Rust, no x86 SIMD in hot path |
| Linux/macOS/Windows | ✅ CPAL backend |

---

## 🔮 Future Features — Architecture Readiness

Architecture must support the following without major rewrites.
Track readiness here as design decisions are made.

- [x] **P2** Piano Roll — full interactive implementation in Tracker (section 1): L-click=place, L-drag=gate, R-click=erase, R-drag=paint-erase, Enter=toggle, hjkl=navigate, chord support; **velocity lane** (4 rows below grid) shows bars colored by level and responds to clicks to set velocity
- [x] **P2** Audio Editing — `Modal::AudioEdit` with waveform display (peak bands, trim region overlay), 5 editable params (trim start/end, gain, fade in/out), normalize toggle; `E` key in Arranger opens it; `AppCommand::OpenAudioEdit` + `ApplyAudioEdit`; applies via `SetPlaybackRange` + `SetSlotVolume` to RT engine
- [x] **P2** Freeze Tracks — `AppCommand::FreezeTrack { row }` / `UnfreezeTrack { row }`; `F` in Arranger toggles freeze; stores original source in `Clip::freeze_source`; channel `frozen` flag; ❄ icon in track row; unfreeze restores original MIDI/SF2 sources
- [x] **P2** Bounce In Place — `AppCommand::BounceInPlace { row }` / `BounceClipInPlace { row, col }`; `B` key in Arranger; background `render_offline_stem` → replaces all clips in row with `AudioFile` source on completion
- [x] **P2** Offline Rendering — `offline.rs`: `OfflineRenderer`, `render_offline_mixdown` (full mix) + `render_offline_stem` (per-row filter); progress callback; wired via `AppCommand::ExportAudio` → file picker → background thread
- [x] **P3** Clip Stretching — `AppCommand::StretchClipToBpm { row, col }`; `W` in Arranger; background thread calls `LoadedClip::time_stretch_to_bpm(orig_bpm, project_bpm)` via rubato FastFixedIn; saves stretched WAV; reassigns clip source via `bounce_pending_row` pipeline
- [x] **P3** Quantization — `Pattern::quantize(strength, grid_divs, swing_aware)` + `Pattern::humanize_timing(amount)`; `AppCommand::QuantizePattern` / `HumanizePattern`; `Q` in Tracker = full quantize (100%, 1/16, swing-aware), `H` = humanize ±15%
- [x] **P3** Spectrum Analyzer — 32-band FFT overlay on MASTER L strip in Mixer (see Phase 2 for full notes)
- [x] **P3** Correlation Meter — Pearson L/R, EMA-smoothed, shown on MASTER R strip (see Phase 2 for full notes)
- [x] **P3** Loudness Metering — K-weighted LUFS (momentary/short-term/integrated), shown on MASTER R strip (see Phase 2 for full notes)

---

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `ratatui` | 0.29 | TUI rendering |
| `crossterm` | 0.28 | Terminal I/O |
| `cpal` | 0.15 | Cross-platform audio I/O |
| `oxisynth` | 0.0.2 | SF2 synthesis (pure Rust) |
| `symphonia` | 0.5 | WAV/FLAC/MP3/OGG decode |
| `rubato` | 0.15 | Resampling (time-stretch pending) |
| `rtrb` | 0.3 | Lock-free ring buffer |
| `triple_buffer` | 9 | Transport state UI↔scheduler |
| `flume` | 0.11 | MPSC channels |
| `midir` | 0.10 | MIDI I/O |
| `midly` | 0.5 | MIDI file parser |
| `libloading` | 0.8 | VST2 dynamic loading |
| `zip` | 0.6 | `.stz` container |
| `uuid` | 1 | Object identity |
| `chrono` | 0.4 | Timestamps |
