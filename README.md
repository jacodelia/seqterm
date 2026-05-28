# SEQTERM

> Terminal-based modular music sequencer, sampler, and granular synthesizer for Linux

```
╔══════════════════════════════════════════════════════════════════════╗
║  SEQTERM :: PROJECT "live_set" :: 128 BPM :: JACK :: CPU 03%        ║
╠══════════════════════════════════════════════════════════════════════╣
║ ▶ 1.MATRIX  2.TRACKER  3.ARRANGER  4.MIXER  5.CONFIG  6.SAMPLER     ║
╚══════════════════════════════════════════════════════════════════════╝

  MATRIX LAUNCHER 8×8             PAD SAMPLER — Bank A
  ┌──────┬──────┬──────┬──────┐   ┌────┬────┬────┬────┐
  │▶KCK  │      │♪SF2  │      │   │ 01 │ 02 │ 03 │ 04 │  ← kick
  │16/16 │░░░░░ │08/16 │░░░░░ │   ├────┼────┼────┼────┤
  ├──────┼──────┼──────┼──────┤   │ 05 │ 06 │ 07 │ 08 │  ← snare
  │▶SNR  │      │▶BASS │      │   ├────┼────┼────┼────┤
  │16/16 │░░░░░ │04/16 │░░░░░ │   │ 09 │ 10 │ 11 │ 12 │  ← hats
  └──────┴──────┴──────┴──────┘   └────┴────┴────┴────┘
```

---

## What is SeqTerm?

SeqTerm is a real-time music production environment that runs entirely inside a terminal. It combines:

- **Step sequencer** with polymeter support (independent pattern lengths per row)
- **SP-404-style pad sampler** with 16-pad banks, mute/choke groups, and skip-back capture
- **Granular synthesis engine** inspired by the Torso S-4 — freeze, spray, cloud generation
- **Mixer** with 32 slots, per-channel FX chains, bus sends/returns, and SIMD mixing
- **MIDI 2.0** (UMP packets, MIDI CI, bidirectional MIDI 1↔2 conversion)
- **Plugin support** (VST2 via `seqterm-plugin-vst2`, live parameter browser)
- **OSC server** for external control (TouchOSC, SuperCollider, etc.)

---

## Requirements

| Dependency    | Min version | Notes                                   |
|---------------|-------------|------------------------------------------|
| Rust          | 1.85+       | Edition 2024                             |
| libasound2    | 1.2+        | ALSA MIDI I/O (Linux)                    |
| libjack       | 1.9+ (opt)  | JACK audio — enable with `--features jack` |
| PipeWire      | 1.0+ (opt)  | Via JACK bridge (`pw-jack seqterm`)      |
| Terminal      | any TrueColor + UTF-8 | kitty / wezterm / alacritty / foot |

---

## Install

### Debian / Ubuntu

```bash
sudo apt install build-essential libasound2-dev pkg-config
# Optional JACK support
sudo apt install libjack-jackd2-dev
```

### Arch Linux

```bash
sudo pacman -S base-devel alsa-lib
sudo pacman -S jack2          # optional
```

### Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

---

## Build & Run

```bash
# Development build
cargo build

# Optimised release build (recommended for live use)
cargo build --release

# With JACK backend
cargo build --release --features jack -p seqterm-midi

# Run
cargo run --release -p seqterm-app

# Run with a project file
SEQTERM_PROJECT=projects/my_set.json cargo run --release -p seqterm-app

# Debug logging
RUST_LOG=seqterm=debug cargo run -p seqterm-app
```

---

## Tests

```bash
# All tests (138 unit + integration tests)
cargo test --workspace

# Specific crate
cargo test -p seqterm-audio-engine   # mixer, FX, granular engine, skip-back
cargo test -p seqterm-core           # notes, patterns, pad domain, granular params
cargo test -p seqterm-midi           # MIDI 2.0 UMP, CI, scaling roundtrips
cargo test -p seqterm-engine         # scheduler, polymeter, transport
cargo test -p seqterm-persistence    # JSON/MessagePack, atomic save, migration
cargo test -p seqterm-history        # undo/redo stack
```

---

## Key Bindings

### Global

| Key         | Action                              |
|-------------|-------------------------------------|
| `1–6`       | Switch view (Matrix / Tracker / Arranger / Mixer / Config / Sampler) |
| `Space`     | Play / Stop                         |
| `R`         | Record toggle                       |
| `Ctrl+S`    | Save project                        |
| `Ctrl+Z/Y`  | Undo / Redo                         |
| `Ctrl+P`    | Command Palette                     |
| `?`         | Help                                |
| `Q` / `Esc` | Close modal / Quit                  |

### Matrix (View 1)

| Key      | Action                         |
|----------|-------------------------------|
| `↑↓←→`   | Move cursor                   |
| `Enter`  | Launch / stop clip            |
| `e`      | Edit clip in Tracker          |
| `f`      | Assign SF2 source             |
| `F`      | Assign audio file source      |
| `x`      | Clear clip source             |
| `m`      | Grab clip (drag mode)         |
| `Del`    | Delete pattern                |

### Sampler (View 6)

| Key          | Action                                    |
|--------------|-------------------------------------------|
| `1–9,0,a–f`  | Trigger pads 1–16                         |
| `Tab`        | Next pad bank (A→P)                       |
| `Shift+Tab`  | Previous pad bank                         |
| `s`          | Assign sample to cursor pad               |
| `x`          | Clear cursor pad                          |
| `c`          | Capture skip-back buffer to cursor pad    |
| `g`          | Open granular engine for cursor pad       |
| `f`          | Freeze granular buffer                    |

### Tracker (View 2)

| Key        | Action                         |
|-----------|-------------------------------|
| `↑↓`      | Move step cursor              |
| `Enter`   | Toggle note on/off            |
| `←→`      | Adjust note length            |
| `Shift+↑↓`| Adjust pitch                  |
| `v`       | Adjust velocity               |
| `p`       | Adjust probability            |

---

## Crate Map

```
seqterm-rs/
├── seqterm-app/           Binary entry point — wires all crates together
├── seqterm-core/          Domain types (Note, Pattern, Clip, Channel, Project)
│   ├── pad.rs             SP-404 sampler: PadSlot, PadBank, SamplerConfig
│   └── granular.rs        Granular params: GrainParams, GranularZone, GranularPreset
├── seqterm-engine/        Scheduler (PPQN clock, polymeter, MPE)
├── seqterm-audio-engine/  Real-time audio (CPAL, SF2, audio clips, mixer)
│   ├── fx/                FX chain: Bitcrusher, SVF filter, Delay, VinylSim, Reverb
│   ├── granular/          Grain engine: GranularEngine, EnvelopeTables, Grain
│   └── skip_back.rs       Lock-free circular skip-back buffer
├── seqterm-midi/          MIDI I/O (midir, ALSA multi-port, MIDI 2.0 UMP + CI)
├── seqterm-midi-io/       MIDI import/export, OSC server
├── seqterm-generative/    Euclidean, Markov chain, mutation engine
├── seqterm-application/   Use cases, plugin registry, EventBus
├── seqterm-persistence/   JSON/MessagePack save/load, autosave, migrations
├── seqterm-history/       Undo/redo command stack
├── seqterm-ui/            TUI (ratatui): 6 views, 14 modal types, drag-drop
├── seqterm-command/       AppCommand enum (100+ variants)
├── seqterm-ports/         Port traits (AudioSource, PluginHostPort, etc.)
├── seqterm-settings/      Audio/MIDI/keybinding settings persistence
├── seqterm-plugin-vst2/   VST2 host adapter (dynamic loading, parameter bridge)
└── seqterm-routing/       Routing graph (DFS cycle detection)
```

---

## Audio Routing

```
MIDI Pattern / AudioFile / SF2
        │
        ▼
   Scheduler (PPQN clock)
        │  AudioNoteOn/Off / ClipTrigger
        ▼
   AudioEngine
        │
        ├── Slot 0..31  ─── SoundFontSynth / AudioClipPlayer / GranularEngine
        │                         │
        │                    Mixer (32 slots, SIMD AVX2+FMA)
        │                         │
        │                    Bus A / Bus B (send/return)
        │                         │
        │                    FX Chain (Bitcrusher / SVF / Delay / VinylSim / Reverb)
        │                         │
        ▼                    Master Out → CPAL (ALSA / JACK / PipeWire)
   SkipBackBuffer ──────────────────────────────────────────────┘
```

---

## MIDI 2.0

SeqTerm includes full Universal MIDI Packet support:

```rust
use seqterm_midi::{ump_from_midi1, midi1_from_ump, MidiMessage};

let msg = MidiMessage::NoteOn { channel: 0, note: 60, velocity: 100 };
let pkt = ump_from_midi1(&msg, 0 /* group */);
// pkt is a Type 4 (MIDI 2.0 Channel Voice) with 16-bit velocity

let back = midi1_from_ump(&pkt).unwrap();
// back == MidiMessage::NoteOn { channel: 0, note: 60, velocity: 100 }
```

MIDI CI (Capability Inquiry) for protocol negotiation:

```rust
use seqterm_midi::{MidiCiMessage, Muid};

let ci = MidiCiMessage::discovery(Muid(0x1234), [0x41,0,0], [1,0], [1,0], [1,0,0,0], 0x7F, 512);
let sysex = ci.to_sysex(); // wraps in MidiMessage::SysEx
```

---

## Project Format

Projects are saved as JSON (`.json`) or MessagePack (`.seqterm`), with forward-only schema migrations:

```json
{
  "schema_version": 5,
  "name": "live_set_01",
  "bpm": 128.0,
  "matrix": { "A": [{ "pattern_key": "KCK01", "source": "Midi" }, ...] },
  "patterns": { "KCK01": { "length": 16, "swing": 54, "steps": [...] } },
  "channels": [...],
  "sampler": {
    "active_bank": 0,
    "banks": [{ "name": "A", "slots": [{ "path": "samples/kick.wav", ... }, ...] }]
  }
}
```

Autosave runs every 60 seconds to `<project>.autosave.json`.

---

## OSC

Start the OSC server from Config view (View 5) or via command:

| Route              | Action                    |
|--------------------|---------------------------|
| `/seq/play`        | Start transport           |
| `/seq/stop`        | Stop transport            |
| `/seq/bpm <f>`     | Set BPM                   |
| `/mixer/vol/<n> <f>` | Set channel n volume    |
| `/pad/<bank>/<pad>` | Trigger sampler pad      |

---

## License

MIT — Jorge Codelia, 2026
