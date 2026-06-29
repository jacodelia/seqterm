# Space Echo

Vintage tape-delay + spring-reverb, modelled on the *acoustic character* of the
Roland RE-201 Space Echo — **not** a clone of its circuit or firmware. Original
DSP throughout. Kind id `spaceecho`. Source: `crates/seqterm-audio-engine/src/fx/space_echo.rs`.

## Signal flow

```
 in ──┬──────────────────────────────────────────────────────────► dry
      │     ┌────────────────── feedback loop ──────────────────┐
      ▼     ▼                                                    │
   write → [ tape buffer (≤2 s) ] → 3 playback heads → Σ ──┐    │
              ▲ wow + flutter modulate the read position   │    │
              │                                            ▼    │
              │        tape colour:  HP110 → LP(age) → tanh(age)│
              │                                            │×fb  │
              └────────────────────────────────────────────┴────┘
   echo Σ ──► spring reverb: 3× allpass → 2× damped comb ──► wet tail
   out = dry·(1−wet) + (echo + spring·tail)·wet
```

## DSP decisions (why it sounds like tape)

1. **Three fixed playback heads.** Read taps at delay ratios `[1.0, 0.68, 0.40]`
   with gains `[1.0, 0.70, 0.45]`. The RE-201's multiple heads produce a
   rhythmic, slightly blurred repeat rather than a single clean tap.

2. **Fractional read + wow/flutter.** Each head reads at `write − (base·ratio + m(t))`
   with linear interpolation between adjacent samples. The modulation term is

   ```
   m(t) = A_wow·sin(2π·0.6·t)  +  A_flut·sin(2π·7·t)
   ```

   *Wow* (≈0.6 Hz, ≤4 ms) is the slow speed drift of the capstan; *flutter*
   (≈7 Hz, ≤0.9 ms) is fast scrape. The right channel uses phase-offset LFOs
   (`+1.7`, `+0.9` rad) so the wobble drifts across the stereo field.

   *ponytail:* linear interpolation, not cubic — the wow/flutter detune already
   masks the interpolation error; upgrade to cubic only if a clean long delay
   reveals zipper artefacts.

3. **Colour lives in the feedback path only.** Each repeat is therefore darker
   and more saturated than the last — cumulative tape degradation. Per repeat:

   - 1-pole **highpass** at 110 Hz (tape low-end rolloff): `y = x − LP(x)`.
   - 1-pole **lowpass** whose corner falls with `age`:
     `f_c = (1800 + (1−age)·8200)·(0.4 + 0.6·tone)` Hz. New tape ≈10 kHz, worn
     tape ≈1.8 kHz. The 1-pole coefficient is `a = e^(−2π f_c / sr)`.
   - **tanh** soft-saturation with drive `1 + 3·age`.

4. **Self-oscillation.** `feedback` maps to `0…1.1`. Above unity the loop runs
   away — but the `tanh` in the loop bounds it, giving the classic controllable
   runaway howl instead of a digital blow-up. (Test asserts |out| < 8 under an
   impulse with fb≈0.66.)

5. **Spring reverb.** Schroeder topology on the mono echo sum: 3 allpass stages
   (lengths 225/556/341 @ 44.1 k, g=0.6) for dispersion, then 2 damped feedback
   combs (1557/1116, fb=0.7, HF-damped at 2.6 kHz) for the metallic resonant
   tail. Mixed in by `spring`. All delay lengths scale by `sr/44100`.

## Parameters (normalised 0..1, mapped at build)

| # | Name     | Range / effect |
|---|----------|----------------|
| 0 | Time     | 50…1500 ms base delay |
| 1 | Feedback | 0…1.1 (≥1 self-oscillates) |
| 2 | Wow      | slow pitch drift depth (0…4 ms) |
| 3 | Flutter  | fast pitch drift depth (0…0.9 ms) |
| 4 | Age      | tape wear: HF loss + saturation drive |
| 5 | Spring   | spring-reverb tail amount |
| 6 | Tone     | global hi-cut on the repeats |
| 7 | Wet      | dry/wet mix |

`set_param` updates these **live** (no processor rebuild), so automating Time,
Feedback or Wow stays click-free and preserves the tape buffer + reverb tail
(registered in `kind_supports_live_param`).

## Suggested presets (dial these param values)

| Preset    | Time | FB   | Wow  | Flut | Age  | Spring | Tone | Wet |
|-----------|------|------|------|------|------|--------|------|-----|
| Slapback  | 0.10 | 0.20 | 0.10 | 0.10 | 0.30 | 0.10   | 0.70 | 0.35|
| Tape Dub  | 0.40 | 0.55 | 0.30 | 0.20 | 0.50 | 0.20   | 0.55 | 0.45|
| Wobble    | 0.50 | 0.50 | 0.80 | 0.60 | 0.60 | 0.25   | 0.45 | 0.50|
| Space     | 0.60 | 0.60 | 0.35 | 0.25 | 0.45 | 0.75   | 0.55 | 0.55|
| Runaway   | 0.45 | 0.95 | 0.40 | 0.30 | 0.70 | 0.40   | 0.40 | 0.60|

## Cost / latency

- **Latency:** 0 samples (sample-by-sample; dry passes straight through).
- **CPU:** O(frames) — 3 head reads + 2 one-poles + tanh + 5 reverb stages per
  sample, per channel. No allocation in `process_block`.
- **Memory:** two `≈2 s` f32 buffers + small reverb delay lines (≈0.8 MB @ 48 k).
