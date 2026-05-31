# Mixer

**Crate:** `seqterm-audio-engine`  
**Module:** `mixer.rs`, `fx/`  
**Layer:** Realtime (called exclusively from the CPAL audio callback)

The mixer accumulates audio from up to 32 independent sources into a single stereo output, applies per-slot and master FX, routes to aux buses, and publishes metering data.

---

## Data Model

```
Mixer
├── slots[0..31]     MixerSlot (SF2 synth, AudioClipPlayer, or GranularEngine)
├── master_volume    f32  (linear amplitude, default 1.0)
├── bus_scratch[A,B] Vec<f32>  (aux bus accumulation buffers)
├── bus_volumes[A,B] f32
├── bus_muted[A,B]   bool
├── master_fx        Vec<Box<dyn FxProcessor>>  (post-bus, pre-clip)
├── slot_peaks[0..31] f32  (peak with exponential decay)
├── master_peak[L,R] f32
├── waveform_slot    i32   (-1 = off, ≥0 = capture this slot)
└── waveform_buf     Vec<f32>  (1024-sample ring, L channel)
```

---

## Signal Flow

```
for each active slot:
    source.render(scratch)         → raw PCM into scratch buffer
    for fx in slot.fx_chain:
        fx.process_block(scratch)  → in-place transform
    master_sum += scratch × volume
    bus_A      += scratch × send_a
    bus_B      += scratch × send_b
    update slot_peaks[i]

master_sum += bus_A × bus_vol_A  (if not muted)
master_sum += bus_B × bus_vol_B  (if not muted)

for fx in master_fx:
    fx.process_block(master_sum)   → master bus insert chain

soft_clip(master_sum)              → tanh saturation limiter
update master_peak

if waveform_slot >= 0:
    capture L samples into waveform_buf ring
```

The method signature is `mix(&mut self, output: &mut [f32], sample_rate: u32)`. The `output` slice is the CPAL output buffer (interleaved L/R). No allocation occurs during `mix()`; all scratch and bus buffers are pre-sized at `Mixer::new(max_block)`.

---

## Slot Management

Slots are addressed by a stable `slot_id: u32` (index into `slots[0..31]`). The non-RT engine assigns IDs when clips are loaded:

```
AudioCommand::LoadSf2     { slot_id, path, bank, preset }
AudioCommand::LoadAudioFile { slot_id, path, looping, original_bpm }
AudioCommand::UnloadSlot  { slot_id }
```

Loading happens on a background thread (non-RT). The loaded `Box<dyn AudioSource>` is shipped back through a separate `(slot_id, source)` install channel and installed into the mixer slot at the top of the next audio callback — the only point where the RT thread touches heap allocation (installing the box pointer).

---

## Per-Slot FX Chain

Each `MixerSlot.fx_chain` is a `Vec<Box<dyn FxProcessor>>` that runs **pre-fader** (before volume scaling). The chain is replaced atomically via:

```
AudioCommand::SetSlotFxChain { slot_id, chain: Vec<Box<dyn FxProcessor>> }
AudioCommand::ClearSlotFx    { slot_id }
```

Pre-constructed processors are sent through the command channel — no allocation happens during processing.

---

## Master FX Chain

`Mixer.master_fx` is a post-bus, pre-clip chain. It processes the fully summed stereo output after all bus returns have been mixed in. Useful for mastering-style processing: limiting, EQ, stereo width. Replaced via `AudioCommand::SetMasterFxChain`.

---

## Aux Buses

Two buses (A and B) are available. Each slot can send to either or both via post-fader send levels:

```
AudioCommand::SetSlotSends { slot_id, send_a: f32, send_b: f32 }
```

Bus return volumes and mute state:

```
AudioCommand::SetBusVolume { bus_idx: usize, volume: f32 }
AudioCommand::SetBusMuted  { bus_idx: usize, muted: bool  }
```

Buses can model traditional effects sends (reverb return, delay return) or parallel compression sidechains.

---

## Peak Metering

Peak levels use **exponential decay** with `PEAK_DECAY = 0.98` applied per block. For a 256-frame block at 44.1 kHz:

```
time_per_block ≈ 5.8 ms
decay per second ≈ 0.98^(1000/5.8) ≈ 0.98^172 ≈ 0.031
-30 dB release in ≈ 172 blocks ≈ 1 s
```

This gives the classic "fast attack, slow release" VU-meter feel. Peaks are published to `AudioStats` atomics using `Relaxed` ordering — the UI reads them once per frame.

---

## Live Oscilloscope Capture

When `waveform_slot >= 0`, the mixer writes left-channel post-FX samples of that slot into a 1024-element ring buffer. The write index is `waveform_pos % WAVE_LEN`. The UI reads the ring buffer at 60 Hz, resamples it to the display width, and renders a bipolar waveform centred on zero.

The slot ID to capture is set by `AudioCommand` passthrough from `AudioEngineHandle::set_waveform_slot()`.

---

## Volume Units

Slot and master volumes are linear amplitudes, not dB. The conversion for the UI:

```
dBFS = 20 × log10(amplitude)
amplitude = 10^(dBFS / 20)
```

The mixer itself does not convert; the application layer is responsible for converting user-facing dB values before sending `SetSlotVolume` or `SetMasterVolume`.

---

## Live Links (Granular Engine)

`Mixer.live_links: Vec<(source_slot_idx, granular_slot_idx)>` connects a rendered mixer slot as the live audio input of a granular engine slot. After the source slot renders its scratch buffer, the mixer copies those samples into the granular engine's live ring buffer. This enables real-time granular processing of any audio source without a separate recording pass.

---

## FX Processor Catalogue

All processors implement `FxProcessor`:

```rust
pub trait FxProcessor: Send {
    fn process_block(&mut self, buf: &mut [f32], sample_rate: u32);
    fn reset(&mut self);
    fn set_mix(&mut self, wet: f32);
}
```

| Name | Type | Key Parameters |
|------|------|----------------|
| `Svf` | State-variable filter | mode (LP/HP/BP/Notch), cutoff, resonance |
| `FilterBankFx` | 48-band graphic EQ | per-band gain (dB) |
| `DelayLine` | Ping-pong delay | delay_ms, feedback, pan |
| `Reverb` | Schroeder reverb | room_size, damping, width |
| `Bitcrusher` | Bit depth + sample rate | bits (1–16), rate_divisor |
| `VinylSim` | Vinyl emulation | crackle_density, flutter_rate, wow_depth |
| `Cassette` | Tape saturation | drive, tone, flutter |
| `Isolator` | Frequency isolator | lo_gain, mid_gain, hi_gain |
| `GranularDelay` | Granular feedback | grain_ms, feedback, density |
| `SidechainDuck` | Sidechain ducking | threshold, attack_ms, release_ms |
| `Looper` | Real-time looper | state (Record/Play/Overdub/Stop) |
