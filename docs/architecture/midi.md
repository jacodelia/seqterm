# MIDI

**Crates:** `seqterm-midi`, `seqterm-midi-io`  
**Layer:** Infrastructure adapter

SeqTerm handles MIDI at two levels: low-level I/O and port management (`seqterm-midi`) and file-level import/export and OSC (`seqterm-midi-io`).

---

## seqterm-midi

### Module Map

```
seqterm-midi/src/
├── lib.rs           MidiMessage enum, suppress_alsa_stderr(), open_output_connections()
├── router.rs        MidiRouter + SharedMidiRouter
├── midir_adapter.rs MidirMidiAdapter — implements MidiBackendPort
├── input_bus.rs     MidiInputBus — fan-in from all enabled input ports
└── midi2.rs         MIDI 2.0 UMP helpers + MIDI 1.0 ↔ 2.0 conversion utilities
```

### MidiMessage

Canonical in-process MIDI representation:

```rust
pub enum MidiMessage {
    NoteOn    { channel: u8, note: u8, velocity: u8 },
    NoteOff   { channel: u8, note: u8 },
    CC        { channel: u8, control: u8, value: u8 },
    ProgramChange { channel: u8, program: u8 },
    PitchBend { channel: u8, value: i16 },   // -8192..+8191
    Clock,                                    // 0xF8
    Start,                                    // 0xFA
    Stop,                                     // 0xFC
    Continue,                                 // 0xFB
    ActiveSensing,                            // 0xFE
}
```

All internal MIDI routing uses `MidiMessage`. Raw bytes (`Vec<u8>`) are used only on the wire (scheduler → midir output thread).

### Output Port Management

`open_output_connections(destinations: &[String]) -> HashMap<String, flume::Sender<Vec<u8>>>` opens a `midir` output connection for each named destination and spawns a thread that reads from the flume channel and calls `midir_out.send(&bytes)`. This isolates the scheduler from midir's non-RT locking.

`create_pattern_ports(keys: &[String])` creates virtual MIDI output ports (one per pattern key) on Linux via `midir::VirtualOutput`, allowing DAWs or hardware to receive SeqTerm's MIDI output.

### ALSA Stderr Suppression

On Linux, ALSA's C library prints diagnostic messages directly to stderr, corrupting the ratatui TUI. `suppress_alsa_stderr()` installs a no-op handler via `snd_lib_error_set_handler()` before any ALSA operations. The call is idempotent and compiled out on non-Linux platforms.

### MidiInputBus

`MidiInputBus` aggregates incoming MIDI from all enabled input ports into a single `flume::Receiver<(String, MidiMessage)>` channel. The application loop polls this channel each frame to:

- Feed the MIDI Learn system.
- Forward clock pulses to the scheduler when `midi_clock_sync` is enabled.
- Trigger OSC-mapped actions.

### Port Watcher

`spawn_port_watcher(interval)` spawns a background thread that polls the available MIDI port list every `interval` and sends updates over a channel. The UI polls this channel once per frame to detect plug/unplug events without blocking.

### MIDI 2.0 Utilities (`midi2.rs`)

Provides conversion helpers between MIDI 1.0 bytes and MIDI 2.0 Universal MIDI Packets (UMP):

| Function | Description |
|----------|-------------|
| `ump_from_midi1(bytes)` | Wrap MIDI 1.0 message in a UMP packet |
| `midi1_from_ump(packet)` | Extract MIDI 1.0 bytes from a UMP |
| `velocity_midi1_to_midi2(v)` | Scale 7-bit velocity to 16-bit |
| `pitch_bend_midi1_to_midi2(pb)` | Scale 14-bit PB to 32-bit |
| `cc_midi1_to_midi2(cc, val)` | Map CC to MIDI 2.0 relative/absolute |
| `parse_ump_stream(bytes)` | Decode a UMP byte stream |

> **Note:** Full MIDI 2.0 CI negotiation requires physical MIDI 2.0 hardware and is currently marked N/A.

---

## seqterm-midi-io

### Module Map

```
seqterm-midi-io/src/
├── lib.rs     import_midi(), export_musicxml(), probe_midi(), gm_sf2_preset()
└── osc.rs     OscServer — UDP OSC listener + OscMsg event type
```

### MIDI Import

`import_midi(path, opts: &MidiImportOptions) -> Result<ImportedMidi>`

Converts a Standard MIDI File (Type 0 or Type 1) into SeqTerm's internal pattern matrix:

**Step 1 — Parse**: Uses `midly` to parse the SMF into tracks and extract:
- Tempo map (tick → µs/beat, supporting multiple tempo changes).
- Time signature map (bar → numerator/denominator).
- Note-on/note-off events quantised to the step grid.
- CC1 (modulation), CC74 (filter cutoff), pitch-bend values per step.
- Program changes and bank selects (for GM preset mapping).

**Step 2 — Slice**: Each MIDI track is split into pattern slices of `bars_per_pattern` bars. If `bars_per_pattern == 0` ("Full" mode), one pattern per track covers the entire piece aligned to the nearest complete bar.

**Step 3 — Deduplicate**: Each pattern is fingerprinted (`FNV-1a` hash over all step data including velocities, gate times, CCs). Identical patterns share the same key, reducing memory usage for repeated sections.

**Step 4 — Assign**: If `opts.sf2_path` is set, each clip is assigned `PatternSource::Sf2` with bank/preset derived from `gm_sf2_preset(channel, program)`. Channel 9 (percussion) maps to bank 128.

`MidiImportOptions`:

| Field | Default | Description |
|-------|---------|-------------|
| `bars_per_pattern` | 0 (Full) | Slice length in bars (0 = whole piece) |
| `steps_per_beat` | 4 | 4 = 16th notes, 8 = 32nd notes |
| `detect_drums` | true | Flag channel 9 clips as percussion |
| `sf2_path` | None | Pre-assign SF2 source to all clips |

### MIDI Export (MusicXML)

`export_musicxml(project, path)` serialises the project's pattern arrangement into MusicXML format, suitable for import into MuseScore, Sibelius, Finale, and most notation software.

### Track Probe

`probe_midi(path) -> Vec<MidiTrackInfo>` performs a fast scan without full import. Returns name, channel, note count, drum flag, and program number for each track. Used by the MIDI import dialog to populate the track selection list before the user confirms options.

### OSC Server

`OscServer::start(port) -> Result<OscServer>` binds a UDP socket and spawns a background thread that parses incoming OSC packets using the `rosc` crate. Parsed messages are sent as `OscMsg` events to the application loop via a `flume` channel.

`OscMsg { address: String, args: Vec<OscArg> }` — the address is matched against `project.osc_routes` to trigger commands (e.g. `"/play"` → transport play, `"/bpm"` → set BPM).

---

## MIDI Learn

When `app.midi_learn` is `Some(MidiLearnTarget)`, the next incoming CC event on any input port is bound to the target. Targets include:

- Channel volume, pan, send levels (by channel index).
- Project BPM.
- Per-slot FX parameters.

Bindings are persisted in `AppSettings` and saved across sessions.
