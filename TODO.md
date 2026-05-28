# SeqTerm-rs — TODO

Priority: **P0** = blocking · **P1** = core feature · **P2** = important · **P3** = polish

---

## IMPLEMENTED ✅

### Hexagonal Architecture (Ports & Adapters)
- [x] **P0** `seqterm-ports` — pure traits: `AudioBackendPort`, `MidiBackendPort`, `ProjectRepository`, `AudioSource`, `AudioSynthPort`, `RealtimeEventSink`, `ExporterPort`, `PluginHostPort`
- [x] **P0** `seqterm-application` — `AppCmd` enum, `DomainEvent`, `EventBus` (flume pub/sub), `CommandBus`, use cases
- [x] **P0** `AutoProjectRepository` / `JsonProjectRepository` / `BinaryProjectRepository`
- [x] **P0** `MidirMidiAdapter` in `seqterm-midi` — full `MidiBackendPort` implementation
- [x] **P0** Domain isolation — `seqterm-core` depends only on `serde` + `thiserror` + `seqterm-routing`

### Real-Time Audio Engine (`seqterm-audio-engine`)
- [x] **P0** `CpalAudioBackend` — CPAL stream, lock-free `rtrb` ring buffer to RT callback
- [x] **P0** `Mixer` — 32 pre-allocated slots, stereo mix without alloc, soft-clip
- [x] **P0** `SoundFontSynth` — SF2 via oxisynth, pre-allocated render buffers, 50ms fade-out, `AllNotesOff`
- [x] **P0** `AudioClipPlayer` — WAV/FLAC/MP3/OGG via symphonia, linear interpolation, loop mode
- [x] **P0** `AssetCache` — background SF2/audio loading, LRU for clips
- [x] **P0** `AudioEngine` — non-RT control: `load_sf2`, `load_audio_file`, `drain_events`
- [x] **P2** Audio bus sends/returns — post-fader send_a/b → bus A/B return; `SetSlotSends`, `SetBusVolume`, `SetBusMuted`
- [x] **P2** SIMD in hot paths — AVX2+FMA `_mm256_fmadd_ps` in `Mixer::mix()`; scalar fallback
- [x] **P2** Realtime capture — `SkipBackBuffer` lock-free circular stereo buffer (last 30 s); wired into CPAL callback via `try_write()`; accessible via `AudioEngine::skip_back()`
- [x] **P2** Offline render — `OfflineRenderer` in `offline.rs`; `render_offline_mixdown/stem`; real audio export

### FX Chain (NEW — `seqterm-audio-engine/src/fx/`)
- [x] **P1** `FxProcessor` trait — `process_block()` + `reset()` + `set_mix()`
- [x] **P1** `Bitcrusher` — bit-depth reduction + sample-hold decimation
- [x] **P1** `Svf` — topology-preserving state-variable filter (LP/HP/BP/Notch, Simper 2012)
- [x] **P1** `DelayLine` — stereo delay, feedback, 1-pole LP damping, ping-pong
- [x] **P1** `VinylSim` — LFO wow/flutter + LCG crackle noise
- [x] **P1** `Reverb` — Freeverb: 8 comb + 4 allpass filters, stereo spread
- [x] **P1** `MixerSlot.fx_chain` — pre-fader insert chain; `AudioCommand::SetSlotFxChain / ClearSlotFx`
- [x] **P1** `Isolator` — 3-band (bass/mid/treble) SVF isolator; 4× cascaded stages for 48 dB/oct; Butterworth k=√2; `fx/isolator.rs`
- [x] **P1** `Cassette` — tape saturation: pre-emphasis HP shelf + tanh drive + de-emphasis LP + LCG flutter noise; `fx/cassette.rs`
- [x] **P1** `Looper` — stereo loop recorder; pre-allocated 32-s buffer; Idle→Recording→Playing→Overdub state machine; RT-safe pending-cmd pattern; `fx/looper.rs`
- [x] **P1** `SidechainDuck` — LFO or externally triggered volume duck; `AtomicBool` trigger for non-RT callers; exponential release; `fx/sidechain.rs`

### Granular Engine (NEW — `seqterm-audio-engine/src/granular/`)
- [x] **P1** `EnvelopeTables` — precomputed Hann, Gaussian, Triangle, Exponential (1024-point LUTs)
- [x] **P1** `Grain` — linear interpolation read, envelope phase, bidirectional playback, pan
- [x] **P1** `GranularEngine` — 32-voice pool, spray/jitter/density scheduling, freeze buffer, zero-alloc render; implements `AudioSource` (owns `params`/`zone`; activated via `activate()`)

### Sampler Domain (NEW — `seqterm-core/src/pad.rs`)
- [x] **P1** `TriggerMode` — OneShot, Loop, Gate, Retrigger
- [x] **P1** `MuteGroup` / `ChokeGroup` — exclusive mute and instant-choke groups
- [x] **P1** `PadSlot` — path, trigger params, pitch_st, gain, pan, reverse, trim, loop points, vel_to_vol
- [x] **P1** `PadBank` — 16 pads per bank
- [x] **P1** `SamplerConfig` — banks + active_bank + skip_back_secs; stored in `Project`

### Granular Params (NEW — `seqterm-core/src/granular.rs`)
- [x] **P1** `GrainEnvelope` — Hann, Gaussian, Triangle, Exponential
- [x] **P1** `GrainParams` — size_ms, density, spray, overlap, pitch_st, direction, pan, gain, jitter, stereo_spread
- [x] **P1** `GranularZone` — position, range, scan_speed, scan_mode, frozen
- [x] **P1** `GranularPreset` — named snapshot of params + zone

### Scheduler / MIDI Engine (`seqterm-engine`)
- [x] **P0** Dedicated `seqterm-scheduler` thread with PPQN=24 clock
- [x] **P0** Independent polymeter per clip (`step % pattern.length`)
- [x] **P0** Routing via `PatternSource`: MIDI → external port, SF2/AudioFile → audio engine
- [x] **P0** Latency compensation — `audio_lookahead_steps`, `SetAudioLatency`
- [x] **P0** Triple-buffer transport state (lock-free scheduler→UI)
- [x] **P2** MPE — `MpeZone`, `MpeChannelMap`; per-note pitch bend, pressure, timbre; MPE allocation in scheduler

### Domain (`seqterm-core`)
- [x] **P0** `PatternSource` — `Midi` / `Sf2` / `AudioFile`
- [x] **P0** `Note` — pitch, velocity, gate, prob, pitch_bend, microshift, cc01, cc74, pressure, timbre
- [x] **P0** `Pattern` — swing, euclidean, humanization, evolution, time signature, beat groups
- [x] **P0** `Channel` — volume_db, pan, mute, solo, send_a/b, sidechain_source
- [x] **P0** `Project` with schema versioning and migrations; `sampler: SamplerConfig` field

### Persistence
- [x] **P0** Relative paths for `PatternSource::Sf2.path` and `AudioFile.path`
- [x] **P0** Atomic save: `.tmp` → `rename`
- [x] **P0** JSON + MessagePack with auto-detection; forward-only schema migrations
- [x] **P0** `Autosave` thread — every 60 s to `.autosave.json`
- [x] **P2** Undo/Redo history serialised to `<project>.history.json`

### UI / TUI (`seqterm-ui`)
- [x] **P0** 7 views: Matrix (8×8), Tracker+PianoRoll, Arranger, Mixer, Config, Sampler, Granular
- [x] **P0** 15 modal types: FilePicker, Sf2Browser, PluginParams, CommandPalette, Alert, Confirm, Input, Progress, AudioSettings, MidiSettings, KeybindingsEditor, AudioExportOptions, MidiImportOptions, Help, About
- [x] **P0** `AppCommand` enum (100+ variants) + `dispatch_command()` centralised handler
- [x] **P2** MIDI Learn — bind CC to mixer/transport parameters
- [x] **P2** Drag-and-drop clip movement in Matrix view
- [x] **P2** SF2 preset browser with audio preview (Space key)
- [x] **P3** OSC server — UDP listener, `/seq/play`, `/seq/stop`, `/seq/bpm`, `/mixer/vol/<n>`
- [x] **P3** Screen reader accessibility — `announce_status()` → `/tmp/seqterm.announce`
- [x] **P3** Real-time capture toggle (WAV output)

### MIDI 2.0 (`seqterm-midi/src/midi2.rs`)
- [x] **P3** `UmpPacket` — Single/Double/Quad (32/64/128-bit UMP words)
- [x] **P3** `ump_from_midi1` / `midi1_from_ump` — bidirectional conversion with exact bit-replication scaling
- [x] **P3** `MidiCiMessage` / `Muid` / `MidiCiSubId` — MIDI CI encode/parse (Discovery, Profile, Property Exchange)
- [x] **P3** `parse_ump_stream` / `encode_ump_stream` — big-endian byte stream codec

### Plugin System
- [x] **P1** `seqterm-plugin-vst2` — VST2 host; scan `.so/.dll/.vst`; dynamic load via `libloading`; audio process
- [x] **P1** `PluginRegistry` — global lifecycle manager (Active/Suspended/Destroyed); param proxy
- [x] **P2** Plugin UI bridge — `Modal::PluginParams` floating overlay; ←→ nudge; `r` refresh

### Tests (155 passing)
- [x] **P0** `seqterm-core` — note parse, MIDI conversion, MPE, pad, granular params
- [x] **P0** `seqterm-routing` — cycle detection, add/remove nodes/edges (9 tests)
- [x] **P0** `seqterm-audio-engine` — mixer, FX chain, granular engine, skip-back, clip (53 tests)
- [x] **P0** `seqterm-engine` — polymeter, transport, play/stop/restart (6 tests)
- [x] **P0** `seqterm-persistence` — JSON/MessagePack, atomic write, migration (8 tests)
- [x] **P0** `seqterm-history` — push, undo, redo, group (9 tests)
- [x] **P0** `seqterm-midi` — MIDI 2.0 UMP, CI, velocity/CC/pitchbend scaling (14 tests)
- [x] **P0** `seqterm-application` — plugin registry lifecycle (10 tests)

### CI / CD
- [x] **P0** `.github/workflows/ci.yml` — fmt + clippy + build + test on Linux/macOS/Windows
- [x] **P0** `.github/workflows/release.yml` — Linux x86_64/ARM64, macOS Universal, Windows .msi

---

## PENDING — Next Steps 🔧

### P1 — Sampler Engine Integration

- [x] **P1** **Wire PadSlot → AudioEngine slot** — `TriggerPad` handler looks up `PadSlot`, calls `ae.load_audio_file()` on first hit; `pending_plays: HashSet<u32>` queues slot IDs until `AudioFileLoaded` fires, then sends `PlayAudioClip`
- [x] **P1** **Mute/choke group enforcement** — `TriggerPad` iterates all active slots in same `MuteGroup`/`ChokeGroup`; sends `StopAudioClip` (fade-out for mute, instant via `StopAudioClip` for choke)
- [x] **P1** **Sampler view (View 6)** — 4×4 pad grid with bank tab bar; per-pad label/filename/trigger-mode/gain; colour-coded by state (cursor/loaded/assigned/empty); `'6'` key to enter; `[`/`]` for banks; `Space` trigger, `s` stop, `a` assign, `d` clear, `c` capture
- [x] **P1** **Skip-back capture → pad** — `CaptureSkipBackToPad`: reads `SkipBackBuffer`, writes WAV via `hound` on background thread, creates `PadSlot`, assigns to pad grid
- [x] **P1** **TriggerMode: Loop** — `AudioClipPlayer.loop_start/end` set via `AudioCommand::SetLoopPoints`; fractions from `PadSlot.loop_start/end` sent before `PlayAudioClip`
- [ ] **P1** **TriggerMode: Gate** — hold pad key → `NoteOn`/`PlayAudioClip`; release → `StopPad`; needs key-up event routing from TUI (requires crossterm `KeyRelease` support)

### P1 — Granular Engine Integration

- [x] **P1** **Wire GranularEngine to Mixer** — `GranularEngine` implements `AudioSource`; owns `params`/`zone`; `activate()` sets source on Mixer slot; renders into Mixer via `render_block()`
- [x] **P1** **Granular view** — `ViewKind::Granular` (key `7`); ASCII waveform strip with zone/spray overlay; 12 GrainParams + 5 GranularZone params edited with `↑↓`/`←→`; `g` from Sampler opens it; `f`/`F` freeze/unfreeze; changes sent via `SetGranularParams`/`SetGranularZone`
- [ ] **P1** **Live granular input** — when `GranularZone.frozen = false` and no source loaded, granularise audio input from CPAL input stream (requires CPAL duplex)
- [x] **P1** **Freeze shortcut** — `AppCommand::GranularFreeze/GranularUnfreeze` → `AudioCommand::FreezeGranular/UnfreezeGranular` → downcast to `GranularEngine`, call `freeze()`/`set_frozen(false)`

### P1 — FX Integration

- [x] **P1** **Wire FX chain per Mixer slot** — `MixerSlot.fx_chain: Vec<Box<dyn FxProcessor>>`; processed pre-fader after `src.render()`; updated via `AudioCommand::SetSlotFxChain / ClearSlotFx`
- [x] **P1** **FX routing UI** — Mixer FX sidebar detects audio vs MIDI slot; audio slots: `Tab` focus, `↑↓` navigate, `←→` cycle type, `a` add, `Del` remove, `J`/`K` reorder, `+`/`-` wet; changes rebuild and send `SetSlotFxChain`
- [x] **P1** **Isolator FX** — 3-band (bass/mid/treble) SVF isolator; 4× cascaded stages (48 dB/oct); Butterworth k=√2; `fx/isolator.rs`
- [x] **P1** **Cassette saturation FX** — pre-emphasis HP + tanh drive + de-emphasis LP + LCG flutter noise; `fx/cassette.rs`
- [x] **P1** **Looper/stutter FX** — stereo loop recorder; pre-allocated 32-s buffer; Idle→Recording→Playing→Overdub state machine; RT-safe `pending_cmd` transitions; `fx/looper.rs`
- [x] **P1** **Sidechain pump FX** — LFO or externally triggered volume duck; `AtomicBool` for non-RT `trigger()` calls; exponential release; `fx/sidechain.rs`

### P2 — Sampler Features

- [x] **P2** **Pitch shifting** — `AudioClipPlayer.set_pitch_st(st)` adjusts `rate = base_rate * 2^(st/12)` (vinyl-style); `AudioCommand::SetPitchSt` sent from `TriggerPad` in both load and retrigger branches
- [ ] **P2** **Time-stretch** — `rubato::SincFixedOut` to produce a version at `original_bpm / current_bpm` ratio; cache result per (path, bpm) key
- [x] **P2** **Reverse** — `AudioClipPlayer.reverse: bool`; render loop reads `loop_end - 1 - pos` when reversed; `AudioCommand::SetReverse` sent from `TriggerPad`; `PadSlot.reverse` wired
- [x] **P2** **Normalize** — `PadSlot.normalize: bool`; `load_audio_file_ex(path, loop, normalize)` computes peak gain and calls `player.set_gain(1/peak)` in the asset thread; `TriggerPad` extracts and passes flag
- [x] **P2** **Non-destructive trim** — `AudioClipPlayer.trim_start/end` hard limits; `AudioCommand::SetPlaybackRange` sets them; sent from `TriggerPad` in both branches; loop points clamped within trim
- [ ] **P2** **Pattern bounce to pad** — `BouncePatternToPad`: run `render_offline_mixdown()` for N bars of a specific pattern, save to temp WAV, assign to pad
- [ ] **P2** **Overdub** — record audio input on top of existing pad sample; mix input buffer with existing `LoadedClip` buffer

### P2 — Pattern Sequencer Extensions

- [ ] **P2** **Retrigger** — within one step, trigger pad multiple times at sub-step intervals; configure count (1–8) per step
- [ ] **P2** **Stutter** — hold a key to gate-repeat a clip in real time at the current step division rate
- [ ] **P2** **Pattern roll** — hold pattern trigger key to replay continuously at 1/16 or 1/32 intervals
- [ ] **P2** **Pattern chaining** — define a play order for patterns (A1→A2→B1); transport follows the chain
- [ ] **P2** **Combo FX** — hold two pad keys simultaneously to apply a temporary FX (distortion, reverse, stutter) for that press duration

### P2 — Granular Features

- [x] **P2** **Scan modes** — `Linear`: playhead advances `scan_speed * zone_span / (sr*2)` per frame, wraps within zone; `RandomWalk`: Brownian step clamped to zone; `Freeze`: static; effective_pos passed to grain spawner; `set_zone()` snaps positions on large position changes
- [ ] **P2** **Modulation matrix** — LFO sources (Sine, Square, S&H, Euclidean) → granular params (spray, density, pitch, pan, grain_size); `seqterm-generative` extension
- [ ] **P2** **Macro controls** — 4 macro knobs that fan out to multiple params via a modulation matrix; assignable from granular view
- [ ] **P2** **Scene snapshots** — save/recall `GranularPreset`; `SeqTerm::scenes: Vec<GranularPreset>` in project
- [ ] **P2** **Scene morphing** — interpolate between two `GranularPreset` states over N beats; `Morph { from, to, steps }` automation
- [ ] **P2** **Granular delay** — delay line filled with granular output; feeds back into granular source for infinite texture
- [ ] **P2** **Spectral smear mode** — overlap granular output with high-overlap settings + random pitch ±cents per grain for spectral wash effect

### P2 — Mixer / Routing

- [ ] **P2** **48-band resonant filter bank** — `seqterm-audio-engine/src/fx/filterbank.rs`; 48 SVF bandpass filters; morphing between classic filter models (LP/HP/bandpass bell); `FilterBankFx` implementing `FxProcessor`
- [ ] **P2** **Per-pattern FX** — FX chain assigned to a pattern's output slot (not per channel); useful for clip-specific processing
- [x] **P2** **Master bus FX chain** — `Mixer.master_fx: Vec<Box<dyn FxProcessor>>`; applied after bus returns, before soft-clip; `AudioCommand::SetMasterFxChain / ClearMasterFx`; `App.master_fx: Vec<AudioFxEntry>` + `rebuild_master_fx_chain()`; Mixer view sidebar shows master FX when MASTER channel selected; `handle_master_fx_key` wired with full CRUD

### P3 — Performance / Live Features

- [ ] **P3** **Perform page with macros** — dedicated performance overlay (`Ctrl+K`): 4 macro sliders (←→ to move), 4 punch-in FX buttons (hold to activate)
- [ ] **P3** **128 scenes** — `Project::scenes: [Option<SceneSnapshot>; 128]`; `SceneSnapshot` captures sampler active bank, granular presets, mixer volumes; MIDI Program Change triggers recall
- [ ] **P3** **MIDI clock sync** — receive MIDI clock from external source; `MidiMessage::Clock` already parsed; wire to `Scheduler::set_external_tempo()`
- [ ] **P3** **MIDI 2.0 protocol upgrade** — when connected device supports MIDI 2.0 (via CI Discovery), switch to sending Type 4 UMP packets; `MidiRouterV2` adapter
- [ ] **P3** **Live texture capture** — press `L` while granular is playing → record output to new pad in real time (like MPC live resampling)
- [ ] **P3** **Happy accidents** — `Randomise` command on granular preset: randomise spray, jitter, pitch ±12st, envelope shape; seed from system time for reproducibility
- [ ] **P3** **VU meters** — ASCII block meters (▁▂▃▄▅▆▇█) per Mixer channel and master; update from `AudioEngineEvent::DspLoad` + peak tracking in Mixer
- [ ] **P3** **Waveform preview in pad grid** — scan sample (already `scan_waveform()` exists) → ASCII mini-waveform in pad cell; display in Sampler view
- [ ] **P3** **Validate MusicXML in MuseScore 4** — open exported `.musicxml` in MuseScore 4 and verify note/time rendering (manual QA step)

### P3 — MIDI 2.0 Polish

- [ ] **P3** **UmpPacket segmentation for long SysEx** — `ump_from_midi1` currently truncates SysEx to 6 bytes; implement multi-packet segmentation (Start/Continue/End status nibbles in Type 3 packets)
- [ ] **P3** **Per-note controllers** — `UmpPacket::per_note_ctrl()` for registered per-note controllers (spec §4.2.4); useful for MPE extension
- [ ] **P3** **MIDI CI profile negotiation** — respond to `ProfileInquiry` with supported profiles; auto-enable MPE when peer requests it

---

## Architecture Debt 🔧

- [x] `GranularEngine` implements `AudioSource` — owns `params`/`zone`; `render_block()` is the internal entry point; `activate()`/`deactivate()` for lifecycle; can be `set_slot()`ed into Mixer directly
- [x] `FxProcessor` chain plumbed into `MixerSlot.fx_chain: Vec<Box<dyn FxProcessor>>` — processed pre-fader after `src.render()`; updated via `AudioCommand::SetSlotFxChain / ClearSlotFx`
- [x] `SkipBackBuffer` wired into CPAL callback — `skip_back_rt.try_write()` called after `mixer.mix()`; 30-second buffer at configured sample rate; accessible via `AudioEngine::skip_back()`
- [x] `AppCommand::TriggerPad / StopPad` handlers — full implementation: slot lookup via `sampler_slots: HashMap<(bank,pad),u32>`; mute/choke enforcement; velocity scaling; loop-points wiring; `pending_plays` for async load
- [x] Granular view (`OpenGranularView`) — full view implemented; waveform + param editor; key bindings wired
- [ ] `BouncePatternToPad` — needs integration between offline renderer and sampler pad assignment

---

## Key Dependencies

| Crate           | Version | Purpose                                       |
|-----------------|---------|-----------------------------------------------|
| `ratatui`       | 0.29    | TUI rendering                                 |
| `crossterm`     | 0.28    | Terminal I/O                                  |
| `cpal`          | 0.15    | Audio I/O cross-platform                      |
| `oxisynth`      | 0.0.2   | SF2 synthesis (pure Rust)                     |
| `symphonia`     | 0.5     | WAV/FLAC/MP3/OGG decode                       |
| `rubato`        | 0.15    | High-quality resampling / time-stretch        |
| `rtrb`          | 0.3     | Lock-free ring buffer (RT↔non-RT)             |
| `triple_buffer` | 9       | Transport state UI↔scheduler                  |
| `flume`         | 0.11    | MPSC channels for EventBus                    |
| `midir`         | 0.10    | MIDI I/O cross-platform                       |
| `midly`         | 0.5     | MIDI file parser                              |
| `parking_lot`   | 0.12    | Fast mutex                                    |
| `rmp-serde`     | 1       | MessagePack serialisation                     |
| `alsa`          | 0.9     | ALSA seq direct (Linux)                       |
| `libloading`    | 0.8     | VST2 dynamic library loading                  |

---

## Build Status

```
cargo test --workspace
  148 tests passed, 0 failed, 0 ignored  (2026-05-27)

cargo check --workspace
  Finished dev — 0 errors, 0 warnings
```
