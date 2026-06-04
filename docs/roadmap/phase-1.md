# Phase 1 — Foundation (Complete)

Phase 1 establishes the core architecture and delivers a fully functional terminal-based sequencer with audio and MIDI playback.

---

## Goals

- Hexagonal architecture with clean separation of domain, ports, and adapters.
- Realtime-safe audio callback with no allocation, no mutex.
- Polymetric step sequencer with sub-step precision.
- SF2 SoundFont synthesis (oxisynth, 256 voices).
- MIDI I/O via midir (ALSA/CoreMIDI/WinMM).
- Terminal UI via ratatui + crossterm.
- JSON and MessagePack project persistence.
- Full undo/redo history.

---

## Delivered Features

### Core Domain (`seqterm-core`)

- [x] `Note` — 16-field step event (note, velocity, gate, micro, prob, CC01, CC74, pitch bend, chord voices, …)
- [x] `Pattern` — variable-length step grid (1–8192 steps), time signature, swing, Euclidean fill
- [x] `Clip` — matrix slot referencing a pattern; supports MIDI, SF2, and AudioFile sources; MPE zone
- [x] `Channel` — mixer channel strip with volume, pan, 3 FX slots, SF2 assignment, EQ
- [x] `Project` — 8×8 session matrix, named patterns, channels, tracks, scenes, buses, automation, routing graph, granular presets
- [x] Polymetric scheduling — each pattern loops at its own length independently

### Audio Engine (`seqterm-audio-engine`)

- [x] CPAL backend with PipeWire/JACK/ALSA auto-selection
- [x] PipeWire quantum configuration (`PIPEWIRE_QUANTUM=N/SR`)
- [x] 32-slot realtime mixer with bus sends and master FX
- [x] `SoundFontSynth` — oxisynth, 256 voices, GM channel init
- [x] `AudioClipPlayer` — Symphonia decode, trim, loop, pitch, reverse, normalize
- [x] `GranularEngine` — 32-voice, Linear/RandomWalk/Freeze scan modes
- [x] 12 original FX processors: Svf, FilterBankFx, DelayLine, Reverb, Bitcrusher, VinylSim, Cassette, Isolator, GranularDelay, SidechainDuck, Looper (Phase 1); 14 additional added in Phase 2 — total 26
- [x] Live oscilloscope — 1024-sample waveform ring published via atomics
- [x] Peak metering — exponential decay per slot and master
- [x] RMS metering — EMA per slot and master L/R
- [x] LUFS metering — K-weighted, momentary/short-term/integrated gated (ITU-R BS.1770-4)
- [x] Correlation meter — Pearson L/R, EMA-smoothed
- [x] Spectrum analyzer — 2048-pt FFT, 32 log bands, shown on MASTER L strip
- [x] Offline mixdown and stem rendering (`OfflineRenderer`)

### Scheduler (`seqterm-engine`)

- [x] 480 PPQN tick clock
- [x] Polymeter: each clip fires at `global_step % pattern.length`
- [x] Sub-step NoteOn/NoteOff precision via `micro` field
- [x] Audio lookahead compensation for buffer latency
- [x] MIDI clock output (24 PPQ pulses, Start/Stop)
- [x] Song-mode pattern chain
- [x] Automation lane playback (linear interpolation per bar)
- [x] MPE channel allocation per clip

### MIDI (`seqterm-midi`, `seqterm-midi-io`)

- [x] MIDI input bus (fan-in from all enabled inputs)
- [x] MIDI output routing per clip
- [x] Virtual port creation (one per pattern key)
- [x] MIDI Learn — bind any CC to volume, pan, send, BPM
- [x] SMF Type 0/1 import — full piece or bar-sliced patterns
- [x] MusicXML export
- [x] OSC server (UDP, rosc)
- [x] MIDI 2.0 UMP utilities

### Persistence

- [x] JSON project format (`.json`)
- [x] MessagePack binary format (`.seqterm`)
- [x] Atomic save (write-rename)
- [x] Schema migration (v0 → v1)
- [x] Autosave thread (configurable interval)
- [x] Recent files list

### `.stz` Container Format (`seqterm-stz`)

- [x] ZIP-based container with structured directory layout
- [x] UUID-based object identity
- [x] Asset registry (hash deduplication)
- [x] Object registry (fast loading / validation)
- [x] Migration system (trait-based, chainable)
- [x] Atomic save (write-validate-rename)
- [x] Bridge adapter: `seqterm_core::Project` ↔ `StzContainer`

### UI (`seqterm-ui`)

- [x] 7 views: Matrix · Tracker · Arranger · Mixer · Granular · Config · About
- [x] Matrix: 8×16 session grid, transport buttons, polymeter visualiser, routing panel
- [x] Tracker: step editor, piano roll, generative engine panel, track modulation
- [x] Arranger: track lanes, automation lanes, song transport, chain editor
- [x] Mixer: per-channel strips (volume, pan, EQ, FX slots, sends)
- [x] Granular: zone editor, modulation matrix, grain envelope
- [x] Config: MIDI I/O, audio settings, OSC, key bindings
- [x] Modal system: file picker, SF2 browser, MIDI import, alert, confirm, progress
- [x] Full mouse support (click, drag, scroll) across all views and modals
- [x] Vim-mode editing in the Tracker (Normal/Visual/Insert)
- [x] Multi-tab project support

### Settings & History

- [x] `AppSettings` — key bindings, audio config, MIDI Learn map, last SF2 path
- [x] JSON serialisation with MessagePack fallback
- [x] Edit history — unlimited undo/redo via `EditCommand` trait

---

## Test Coverage

215 unit tests passing across all crates (153 at Phase 1 cut; grew to 215 with Phase 2 additions). Tests cover:
- Project serialisation roundtrip (JSON, MessagePack)
- Schema migration
- Note arithmetic and MIDI conversion
- Atomic save behaviour
- STZ container roundtrip, UUID persistence, registry consistency, cycle detection
- MIDI import quantisation
- Euclidean and Markov generation

---

## Known Limitations (N/A — hardware/OS blocked)

| Feature | Reason blocked |
|---------|----------------|
| Gate trigger mode (`TriggerMode::Gate`) | crossterm `KeyRelease` not available on Linux terminals |
| Live granular input / overdub | CPAL duplex stream not yet wired |
| Time-stretch (rubato offline pass) | ✅ Completed in Phase 2 — `LoadedClip::time_stretch` via `rubato::FastFixedIn` |
| Stutter / Pattern Roll / Combo FX | Require simultaneous key tracking |
| MIDI 2.0 CI negotiation | Requires physical MIDI 2.0 hardware |
| MusicXML validation | Manual QA with MuseScore |
