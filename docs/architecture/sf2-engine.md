# SF2 Engine

**Crate:** `seqterm-audio-engine`  
**Module:** `sf2_synth.rs`  
**Layer:** Realtime (called from the CPAL audio callback via `AudioSource::render()`)

SeqTerm's SF2 engine wraps **oxisynth** — a pure-Rust SoundFont 2 synthesiser — to provide General MIDI-compatible instrument and drum kit rendering inside the audio callback.

---

## Architecture

```
Application layer (non-RT)          Realtime callback
───────────────────────────          ─────────────────
AudioCommand::NoteOn ──rtrb──►       SoundFontSynth::note_on()
AudioCommand::NoteOff ─rtrb──►       SoundFontSynth::note_off()
AudioCommand::ControlChange ─►       SoundFontSynth::control_change()
                                     SoundFontSynth::render(output, sr)
                                       └─ synth.write((l, r))   [oxisynth]
                                       └─ apply fade-out envelope
                                       └─ interleave L/R → output
```

Loading is non-RT: `SoundFontSynth::load_multi()` reads the `.sf2` file, decodes sample data, and instantiates an `oxisynth::Synth`. The fully-constructed synth is then installed into the mixer slot via the `install_rx` channel — the only point where a heap allocation enters the RT path.

---

## Polyphony

```rust
const MAX_VOICES: u16 = 256;
```

256 simultaneous voices, matching FluidSynth's General MIDI default. The voice allocator inside oxisynth steals the oldest voice when the limit is reached.

---

## Loading: Single File, Multiple Channels

`load_multi(path, channels: &[(ch, bank, preset)], sample_rate)` is the primary constructor. It:

1. Reads the `.sf2` file into memory (`std::fs::read`).
2. Parses with `oxisynth::SoundFont::load()`.
3. Adds the font to a new `Synth` with `synth.add_font(sf, true)`.
4. Calls `synth.select_program(ch, sfont_id, bank, preset)` for each entry in `channels`. If a bank/preset combination is missing in the SF2 file (common for `bank 128` in non-GM files), the call logs a warning and falls back to bank 0 / preset 0.
5. Applies GM default CC values on **all 16 channels**:
   - CC 7 (volume) = 100
   - CC 10 (pan) = 64 (centre)
   - CC 91 (reverb send) = 40
   - CC 93 (chorus send) = 0

This multi-channel design means that all clips sharing the same `.sf2` file path share **one** `SoundFontSynth` instance in **one** mixer slot. Channel isolation is achieved via the MIDI channel number (`0–15`). The application layer maps each clip's `(row, col)` key to the same `slot_id` but a different `channel` index.

---

## General MIDI Drum Mapping

Channel 9 (index 9, 0-based) is the GM percussion channel. When an SF2 file follows the GM standard, bank 128 on channel 9 contains the drum kit. The MIDI import code uses `gm_sf2_preset(channel, program)` from `seqterm-midi-io` to derive the correct `(bank, preset)` pair:

```
channel 9  → (bank=128, preset=0)   Standard Drum Kit
channel 10 → (bank=0,   preset=...)  Normal instrument
```

The `load_multi` fallback (bank 0 / preset 0) activates silently for SF2 files that omit bank 128, avoiding a hard error at the cost of using the piano sound for drums.

---

## Realtime Interface

`SoundFontSynth` implements two port traits:

### `AudioSource`

```rust
fn render(&mut self, output: &mut [f32], _sample_rate: u32) -> usize
```

Writes up to `output.len() / 2` stereo frames by calling `synth.write((l_buf, r_buf))` then interleaving into the output slice. If a fade-out is active, each frame is multiplied by a linearly decreasing gain.

Returns the number of frames actually rendered (`buf_frames`).

### `AudioSynthPort`

```rust
fn note_on (&mut self, channel, note, velocity)
fn note_off (&mut self, channel, note)
fn control_change(&mut self, channel, cc, value)
fn pitch_bend    (&mut self, channel, value: i16)
```

All calls forward to `synth.send_event(MidiEvent::...)`. `pitch_bend` converts the signed ±8192 range to the unsigned 0–16383 range: `u14 = (value + 8192).clamp(0, 16383)`.

`note_on()` re-activates the synth and clears any pending fade-out, so notes played after a transport stop are heard at full gain.

---

## CC Forwarding from the Scheduler

When the step sequencer fires an SF2 note that has non-default `cc01` or `cc74` values (from MIDI import), the scheduler emits `EngineEvent::AudioControlChange` **before** the NoteOn. The application layer converts this to `AudioCommand::ControlChange`, which arrives at the mixer callback and calls `control_change()` on the synth before the note sounds.

This ensures that imported MIDI files with modulation or filter automation render correctly through the SF2 engine.

---

## Stop / Fade-Out

`stop()` queues a 50 ms fade-out (at the configured sample rate):

```rust
let fade_frames = sample_rate as usize * 50 / 1000;
self.fade_out = Some((fade_frames, fade_frames));
// AllNotesOff on all 16 channels
```

During the fade, each rendered frame is multiplied by `remaining / total` (linear gain). When `remaining == 0`, `self.active` is set to `false` and the mixer slot stops calling `render()`.

---

## Preset Enumeration

`enumerate_sf2_presets(path) -> Vec<(u8, u8, String)>` is a **non-RT** function that reads the SF2 header using the `soundfont` crate (which only parses metadata, not sample data) and returns a sorted list of `(bank, preset_num, name)` tuples. This is used by the SF2 Browser modal to populate the bank and preset selection lists without loading the full sample memory.

Results are deduplicated (some SF2 files list the same preset at multiple banks) and sorted by `bank * 128 + preset`.

---

## SF2 Browser Modal

The SF2 Browser (`Sf2BrowserState`) provides the user interface for assigning a bank and preset to a clip. It is opened in two ways:

1. **`AppCommand::OpenSf2Browser { row, col, path }`** — after the user picks an `.sf2` file via the file picker.
2. **`AppCommand::ReopenSf2Browser { row, col }`** — directly from the routing panel when the clip already has an SF2 source, skipping the file picker. The browser pre-selects the clip's current bank and preset.

The browser enumerates presets in a background thread and pre-selects the current bank/preset cursor once the results arrive.

Mouse interaction:

| Element | Action |
|---------|--------|
| ◄ arrow | `shift_bank(-1)` — previous bank |
| ► arrow | `shift_bank(+1)` — next bank |
| Preset row click | Set cursor to that preset |
| Mouse scroll | Scroll the preset list |
| `[ Accept ]` button | `ConfirmSf2Assignment` → assigns source to clip, loads synth |
| `[ Cancel ]` button | Closes modal without changes |

---

## Preset Name Caching

After `ConfirmSf2Assignment`, the assigned source is stored as:

```rust
PatternSource::Sf2 {
    path,
    bank,
    preset,
    preset_name: format!("Bank:{bank} Prog:{preset}"),
}
```

The `preset_name` field is a cached display string updated on each confirm. It is shown in the routing panel and is non-authoritative (not used during playback).
