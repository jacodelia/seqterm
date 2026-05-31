# SeqTerm — Master Roadmap & Todo List

Tracking all tasks from `SEQTERM_DAW_MASTER_REFACTOR.md` and organic feature work.

**Status key:** `[x]` = done · `[ ]` = pending · `[~]` = partial / in progress

Priority: **P0** blocking · **P1** core · **P2** important · **P3** polish

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
- [ ] **P2** Automated changelog generation (e.g. `git-cliff` or `conventional-changelog`) on release tag

### Documentation

- [x] **P1** `docs/architecture/` — audio-engine, scheduler, arranger, mixer, midi, sf2-engine, plugin-hosting
- [x] **P1** `docs/roadmap/` — phase-1 through phase-4
- [x] **P1** `docs/design-principles.md` — 10 architectural principles

### Phase 1 Deliverables (pending analysis docs)

- [ ] **P1** FluidSynth Evaluation — formal comparison of options A/B/C/D (replace / runtime backend / hybrid / improve) with recommendation
- [ ] **P1** ARM Compatibility Analysis — per-subsystem evaluation (audio, MIDI, scheduler, SF2, mixer, DSP, rendering)
- [ ] **P1** Raspberry Pi Deployment Strategy — Pi 4/5 use cases, performance budget, optimization guidance
- [ ] **P2** Cross-Compilation Strategy document — Cargo config, build/release profiles, feature flags, platform-specific opts
- [ ] **P2** Platform-Specific Optimization Recommendations — SIMD guards, buffer size tuning, latency targets per OS

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
- [ ] **P2** LUFS meter architecture (sliding window RMS → ITU-R BS.1770 gate)
- [ ] **P2** Correlation Meter architecture (M/S correlation coefficient, -1 to +1 display)
- [ ] **P2** Spectrum Analyzer architecture (FFT-based, 1/3 octave or full FFT, overlay on channel)
- [ ] **P2** Channel type labels in mixer strips (Audio / Instr / Bus / Return / Master)
- [ ] **P2** Channel color in mixer strips (serde field + colored label render)
- [ ] **P2** Phase invert toggle in mixer strip (⊘ button)
- [ ] **P2** Width knob in mixer strip (stereo widener)
- [ ] **P2** Mono button in mixer strip
- [ ] **P2** Record arm button in mixer strip (●)
- [ ] **P2** Group bus routing (send channel to group bus, not just aux A/B)
- [ ] **P2** Per-channel Input / Output routing selector (source bus, destination bus)
- [ ] **P2** Routing Matrix view — full grid of sends/receives between all channels

### Arranger Redesign (FL Studio Playlist-style)

- [x] **P1** Basic track lanes with clip blocks
- [x] **P1** Automation lanes
- [x] **P1** Bar ruler with playhead
- [ ] **P1** Track types: MIDI / Audio / Automation / Group / Bus
- [ ] **P1** Track visibility toggle (show/hide track row)
- [ ] **P1** Track color (serde field + render)
- [ ] **P1** Track height (variable row height in arranger)
- [ ] **P1** Beat sub-divisions in bar ruler (beats within each bar)
- [ ] **P1** Snap-to-grid: Off / Bar / ½Bar / ¼Bar / 1/8 / 1/16 / 1/32
- [ ] **P1** Multi-select clips (Shift+click or drag selection box)
- [ ] **P1** Clip operations: Move, Copy (Ctrl+drag), Paste, Duplicate, Delete
- [ ] **P1** Clip resize (drag clip edge to extend/shorten)
- [ ] **P1** Clip split at playhead position
- [ ] **P1** Clip glue (merge adjacent clips of same pattern)
- [ ] **P2** Viewport: horizontal zoom (zoom in/out with Ctrl+scroll)
- [ ] **P2** Viewport: vertical zoom (variable track height)
- [ ] **P2** Loop region (set loop in/out points from arranger)
- [ ] **P2** Timeline markers (add/remove named markers)
- [ ] **P2** Tool modes: Select / Draw / Slice / Paint / Mute

### FX — Missing Processors

- [ ] **P2** `Expander` — upward/downward expander (complement to Gate, threshold/ratio/range)
- [ ] **P2** `Pan` — utility stereo pan effect (law-configurable: linear / constant-power)
- [ ] **P2** FX slot reorder — drag-to-reorder within `EffectChain` in Mixer and Channel strip UI

### Drum Workflow

- [ ] **P1** MIDI Channel 10 designation in `Channel` domain model (drum channel flag)
- [ ] **P1** Drum Mapping — map drum pad steps (0–15) to GM note numbers (35–81)
- [ ] **P1** Drum Kit Browser — SF2 preset browser filtered to percussion banks (bank 128)
- [ ] **P1** Multiple drum kits per project — each drum track can reference a different SF2 preset
- [ ] **P1** Bank Switching from drum track (MSB/LSB selectable per drum channel)
- [ ] **P1** Program Switching from drum track (GM percussion presets)
- [ ] **P2** Drum pattern matrix — dedicated per-pad step grid view (16 pads × N steps)

### SF2 Engine — Missing Features

- [ ] **P1** Multiple simultaneous SoundFonts — load and mix SF2s across channels independently
- [ ] **P1** Bank Select MSB (CC0) + LSB (CC32) — full GM2 / XG bank navigation
- [ ] **P1** Expression (CC11) — velocity-scaled amplitude modulation per voice
- [ ] **P1** Chorus Send (CC93) — route voice output to chorus bus in SF2 engine
- [ ] **P2** Velocity Layers — explicit layer selection based on note velocity (SF2 spec compliance)
- [ ] **P2** ADSR per preset — attack / decay / sustain / release from SF2 modulator data
- [ ] **P2** Sustain Pedal (CC64) — hold active voices past note-off

### MIDI Engine — Missing Features

- [ ] **P1** Tempo Change events (FF 51) — parse and apply BPM changes mid-file during SMF playback
- [ ] **P1** Time Signature Change events (FF 58) — update scheduler meter mid-playback
- [ ] **P2** Program Change events — route to SF2 engine preset switch during playback
- [ ] **P2** Meta Events — full FF parsing (track name, copyright, marker, cue point, end-of-track)

### Time-Stretch & Audio Recording

- [ ] **P2** Time-stretch via `rubato` — offline render pass in `AudioClipPlayer`
- [ ] **P2** CPAL duplex stream — live audio input routing
- [ ] **P2** Overdub recording (requires duplex)
- [ ] **P2** Audio quantisation (snap audio to grid after recording)

### Plugin State

- [ ] **P2** VST2 state save/restore (`effGetChunk` / `effSetChunk`)
- [ ] **P2** Plugin parameter automation lanes
- [ ] **P2** Plugin state stored in `.stz` `plugins/state/{uuid}.state`

### Project Format

- [ ] **P2** `.stz` snapshot system (take/restore snapshots)
- [ ] **P2** Autosave writes snapshot into active `.stz` file
- [ ] **P2** Incremental `.stz` save (only rewrite dirty objects)

---

## 🔬 PHASE 3 — Plugin Ecosystem & Collaboration

### Plugin Hosting

- [x] **P1** VST2 host (`seqterm-plugin-vst2`) — scan/load/process
- [ ] **P2** VST3 crate (`seqterm-plugin-vst3`)
- [ ] **P2** CLAP crate (`seqterm-plugin-clap`)
- [ ] **P3** AU crate (`seqterm-plugin-au`, macOS only)
- [ ] **P3** Plugin sandbox (separate process + shared-memory bridge)

### Backend Abstraction

- [x] **P1** `AudioBackendPort` trait (CPAL adapter)
- [x] **P1** `SoundFontBackend` via `AudioSynthPort` trait
- [ ] **P1** `MidiBackend` trait — abstract MIDI I/O port (midir adapter + virtual port adapter)
- [ ] **P1** `InstrumentBackend` trait — abstract instrument engine (SF2 / SFZ / VST3 / CLAP)
- [ ] **P2** FluidSynth binding as alternative `SoundFontBackend` adapter
- [ ] **P2** SFZ format support (`InstrumentBackend` trait + adapter)

### Collaboration

- [ ] **P3** Object-level merge (UUID-based three-way diff)
- [ ] **P3** CRDT delta operations (Lamport timestamps)
- [ ] **P3** WebSocket collaboration session
- [ ] **P3** Cloud sync adapter

---

## 🚀 PHASE 4 — Open Platform

### SDK & Scripting

- [ ] **P2** `seqterm-sdk` crate — stable public API
- [ ] **P2** C FFI layer (`seqterm.h`, generated with `cbindgen`)
- [ ] **P2** Lua scripting engine (`mlua`) — `on_step` callbacks, `AppCommand` dispatch
- [ ] **P3** Lua REPL in terminal (live coding mode)

### STZ v2

- [ ] **P2** Script storage: `scripts/{name}.lua` in archive
- [ ] **P2** Waveform cache: `cache/waveforms/{uuid}.f32`
- [ ] **P2** Format version 2 migration
- [ ] **P3** `stz` CLI tool (inspect / extract / pack / migrate / validate / diff)

### Cross-Platform & Distribution

- [x] **P0** Linux x86_64 release artifacts (.deb, .rpm, .tar.gz)
- [x] **P0** Linux ARM64 release artifacts (.deb, cross-compiled)
- [x] **P0** macOS Universal binary (.dmg)
- [x] **P0** Windows x86_64 release artifacts (.msi)
- [ ] **P1** Linux ARMv7 (Raspberry Pi OS 32-bit) release artifacts
- [ ] **P1** Windows ARM64 release artifacts
- [ ] **P2** `Cross.toml` configuration for all ARM targets
- [ ] **P2** Cargo build profiles — dev / release / release-lto / rpi (size-optimized for ARM)
- [ ] **P2** Feature flags — `feature = ["fluidsynth"]`, `feature = ["vst3"]`, `feature = ["clap"]`, `feature = ["wasm"]`
- [ ] **P2** Platform-specific Cargo optimizations — strip symbols on ARM, codegen-units=1 on release
- [ ] **P2** Raspberry Pi performance analysis — benchmark large MIDI + multi-SF2 + effects on Pi 4/5
- [ ] **P2** Homebrew formula
- [ ] **P2** AUR PKGBUILD
- [ ] **P3** WebAssembly build (`seqterm-wasm`, Web MIDI + Web Audio)

### Documentation & Community

- [x] **P0** `docs/architecture/` (7 files)
- [x] **P0** `docs/roadmap/` (4 phases)
- [x] **P0** `docs/design-principles.md`
- [ ] **P1** `README.md` — professional redesign (platform matrix, features, install, audio backends, MIDI, SF2, Raspberry Pi recommendations)
- [ ] **P1** `CONTRIBUTING.md` — contribution guidelines, coding standards
- [ ] **P1** `RELEASE_PROCESS.md` — versioning policy (Major/Minor/Patch), release checklist
- [ ] **P1** `docs/raspberry-pi.md` — Pi 4/5 setup, audio latency tuning, headless use cases
- [ ] **P2** Interactive tutorial mode (`AppCommand::StartTutorial`)
- [ ] **P2** `docs.rs` integration for `seqterm-sdk`

---

## 🛠 Architecture Debt

- [x] Routing panel click — gated by `sidebar_tab` to avoid spurious activations
- [x] `AudioControlChange` event in engine for CC01/CC74 → SF2 per step
- [x] Pattern defaults neutral (swing=50→displays 0, prob=0, random=0)
- [ ] **P2** `FxKind` enum in `seqterm-core` needs expansion to match all 22 processors
- [ ] **P2** `AudioFxEntry` in `seqterm-ui` needs new variants for Compressor/Gate/EQ/Chorus/Flanger/Phaser/Widener/Utility types
- [ ] **P2** Mixer strips should apply `channel.phase_invert`, `channel.width`, `channel.mono` via the new FX processors (wire in `rebuild_audio_slots`)
- [ ] **P3** `FxProcessor::name()` method for UI display
- [ ] **P3** `FxProcessor::params() -> Vec<FxParam>` for generic automation binding
- [ ] **P2** TUI focus management system — unified focus ring across all views and modals
- [ ] **P3** TUI render caching — skip re-rendering unchanged widgets (dirty flag per widget)

---

## 🧪 Tests

- [x] 153+ passing across all crates (seqterm-core, routing, audio-engine, engine, persistence, history, midi, application, stz)
- [x] **P1** Add tests for Compressor: unity gain when below threshold, gain reduction above
- [x] **P1** Add tests for Gate: opens/closes on threshold crossing, hold phase respected
- [x] **P1** Add tests for ParametricEq: bypass=unity, peak boost at freq, HP attenuates DC, LowShelf cut
- [x] **P1** Add tests for StereoWidener: width=0 produces mono, width=1 produces unity
- [x] **P1** Add tests for PhaseInvert: inverts L only, R only, or both
- [x] **P1** Add tests for RMS metering: EMA converges to correct level for DC and master output
- [ ] **P2** Add tests for Chorus/Flanger: output differs from dry after first block
- [ ] **P2** Add tests for Expander: gain increases above threshold (upward) / below threshold (downward)
- [ ] **P2** Add tests for drum channel: Channel 10 flag routes to percussion bank, ignores note pitch
- [ ] **P2** Add tests for SF2 Bank Select: MSB+LSB combination selects correct preset
- [ ] **P2** Add tests for MIDI Tempo Change: BPM updates mid-playback from FF 51 meta event
- [ ] **P2** Channel serialisation roundtrip with new fields (phase_invert, width, mono, channel_type)

---

## Performance Targets

| Target | Status |
|--------|--------|
| 128+ tracks | Architecture supports 32 audio slots + unlimited MIDI |
| 10,000+ clips in arranger | [ ] pending virtualized clip rendering |
| 256+ voices | ✅ oxisynth configured at 256 MAX_VOICES |
| Multiple simultaneous SoundFonts | [ ] pending multi-SF2 loader |
| Hundreds of effects | [ ] pending FX pool pre-allocation |
| Sub-millisecond timing | ✅ 480 PPQN ≈ 1ms resolution at 120 BPM |
| No allocation in callback | ✅ enforced by design |
| No mutex in callback | ✅ rtrb ring + atomic stats |
| ARM64 / Raspberry Pi | ✅ pure Rust, no x86 SIMD in hot path |
| Linux/macOS/Windows | ✅ CPAL backend |

---

## 🔮 Future Features — Architecture Readiness

Architecture must support the following without major rewrites.
Track readiness here as design decisions are made.

- [ ] **P2** Piano Roll — note-level MIDI editor (per-clip, pitch × time grid)
- [ ] **P2** Audio Editing — waveform view, trim/fade/normalize for audio clips
- [ ] **P2** Freeze Tracks — render MIDI+FX chain to audio, bypass live processing
- [ ] **P2** Bounce In Place — export clip/track to audio file, re-import as AudioClip
- [ ] **P2** Offline Rendering — export full project mix to WAV/FLAC without real-time constraint
- [ ] **P3** Clip Stretching — time-stretch audio clips to match project BPM (rubato integration)
- [ ] **P3** Quantization — snap MIDI notes to grid (strength 0–100%, swing-aware)
- [ ] **P3** Spectrum Analyzer — FFT overlay widget, 1/3 octave bands, usable in Mixer + Master
- [ ] **P3** Correlation Meter — M/S correlation coefficient display on Master channel
- [ ] **P3** Loudness Metering — ITU-R BS.1770-4 integrated LUFS, short-term, momentary

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
