# Protocosmos

Granular cloud / glitch / particle-delay texture processor, inspired by the
*types* of processing in the Hologram Microcosm — **not** its proprietary
algorithm. Original DSP. Kind id `protocosmos`. Source:
`crates/seqterm-audio-engine/src/fx/protocosmos.rs`.

## Signal flow

```
 in ──┬───────────────────────────────────────────────────────────► dry
      ▼ (write skipped when Freeze on)
   [ circular buffer (≤4 s) ] ◄─── + grain·0.35 feedback (cloud sustain)
      ▲                                                  │
   grain pool (≤12): pos = write − rand·spray,           │
                     speed = ±2^(±12 st/12),  Hann window │
      │ Σ                                                 │
      ▼                                                   │
   diffusion: 3× allpass → 2× damped comb (× Diffuse) ───┘──► wet tail
   out = dry·(1−wet) + (grainΣ + tail)·wet
```

## DSP decisions

1. **Shared circular buffer, grain pool.** Up to 12 concurrent grains read one
   stereo ring buffer. A grain is spawned every `sr / density` samples
   (`density` → 1…80 grains/s). Each grain:

   - **Position:** `pos = write − (rand·spray·0.5 s + 1)`, i.e. scattered back
     from the write head by up to `spray` × 0.5 s. Wider spray ⇒ mosaic scatter.
   - **Pitch:** `speed = 2^(st/12)`, `st = (pitch−0.5)·24` → ±12 semitones,
     applied as a fractional resample rate.
   - **Reverse:** with probability `reverse`, `speed = −speed` (grain reads
     backwards) → glitch / mosaic motion.
   - **Length:** `20 + size·180` ms.
   - **Window:** Hann, `w(age) = sin²(π·age/life)` — overlapping grains
     crossfade smoothly into clouds with no edge clicks.

2. **Freeze = infinite hold.** When `Freeze > 0.5` the buffer **stops writing**;
   grains keep scanning the held audio forever. Because `Freeze` is updated via
   the live `set_param` path (it's in `kind_supports_live_param`), engaging it
   does **not** rebuild the processor — the rolling buffer is preserved, so it
   captures exactly what was playing. (A rebuild would wipe the buffer; this is
   why a live param path is mandatory for freeze.)

3. **Cloud feedback.** Grain output is mixed back into the buffer at 0.35 (fixed,
   bounded) so textures sustain and bloom rather than dying with the input.

4. **Diffusion + integrated reverb.** The grain sum is dispersed through 3
   allpass stages (441/341/225 @ 44.1 k, g=0.7) then 2 damped feedback combs
   (1617/1277, fb=0.78). The reverb tail is scaled by `Diffuse` and added to the
   dry grains → ambient pads and cinematic tails. Lengths scale by `sr/44100`.

## Achievable textures

micro-looping · glitch · mosaic · granular clouds · particle delays ·
granulated arpeggios (high density + pitch + spray) · reverse ambience
(`reverse`≈1 + `diffuse`) · infinite freeze · harmonic cascades (`pitch` up +
feedback) · ambient pads · cinematic textures · organic movement.

## Parameters (normalised 0..1)

| # | Name    | Range / effect |
|---|---------|----------------|
| 0 | Size    | grain length 20…200 ms |
| 1 | Density | 1…80 grains/s |
| 2 | Pitch   | ±12 semitones (0.5 = unison) |
| 3 | Spray   | position scatter (0…0.5 s) |
| 4 | Reverse | probability a grain plays backwards |
| 5 | Freeze  | >0.5 holds the buffer (infinite) |
| 6 | Diffuse | diffusion + reverb tail amount |
| 7 | Wet     | dry/wet mix |

## Suggested presets

| Preset    | Size | Dens | Pitch | Spray | Rev  | Freeze | Diff | Wet |
|-----------|------|------|-------|-------|------|--------|------|-----|
| Mosaic    | 0.30 | 0.70 | 0.50  | 0.55  | 0.30 | 0.00   | 0.30 | 0.60|
| Glitch    | 0.15 | 0.85 | 0.50  | 0.70  | 0.50 | 0.00   | 0.20 | 0.70|
| Arp       | 0.25 | 0.60 | 0.70  | 0.20  | 0.00 | 0.00   | 0.25 | 0.60|
| Cloud     | 0.60 | 0.55 | 0.50  | 0.45  | 0.20 | 0.00   | 0.55 | 0.70|
| Reverse   | 0.45 | 0.50 | 0.50  | 0.40  | 1.00 | 0.00   | 0.50 | 0.65|
| Shimmer   | 0.40 | 0.65 | 0.75  | 0.35  | 0.10 | 0.00   | 0.60 | 0.65|
| Infinite  | 0.55 | 0.70 | 0.50  | 0.30  | 0.20 | 1.00   | 0.45 | 0.80|
| Cathedral | 0.70 | 0.45 | 0.62  | 0.40  | 0.15 | 0.00   | 0.80 | 0.70|

*(Presets are param recipes — there is no per-effect preset selector yet; the
default param set ships as a neutral "Cloud"-ish texture.)*

## Cost / latency

- **Latency:** 0 samples (dry passes through).
- **CPU:** O(frames · active_grains), ≤12 grains; + 5 reverb stages/sample.
- **Memory:** two `≈4 s` f32 buffers + reverb lines (≈1.6 MB @ 48 k). No
  allocation in `process_block`.

## Known limitation

Manual knob edits in the mixer UI currently rebuild the chain (clearing the
buffer). Drive `Freeze` (and other params) from **automation / CC / a macro** to
get the click-free, state-preserving live path. Wiring the manual-edit path
through `set_param` for live-param kinds is a tracked follow-up.
