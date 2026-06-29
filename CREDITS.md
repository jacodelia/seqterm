# Credits & acknowledgments

seqterm is MIT-licensed. The DSP below was written from scratch for this project;
the references listed were studied for *technique and character* only — **no code
was copied**, so no copyleft licenses are inherited.

## Audio FX references

- **ZynAddSubFX** (GPLv2, Nasca Octavian Paul / Mark McCurry et al.) — studied as
  a reference for classic effect structure. No code copied; the items below are
  generic, long-standing techniques reimplemented independently:
  - **L/R crossfeed** (`Cross`) on the stereo delay — the idea behind its `Echo`
    effect's `lrcross` parameter (`fx/delay.rs`).
  - **Reverse delay** (`reverse`) — the reversed-segment concept of its
    `Reverse`/`Reverter` effect (GPLv2, Michael Kirchner). Reimplemented as a
    free-running overlap-add (granular) crossfade, no host/MIDI sync
    (`fx/reverse.rs`).
  - The 2× **anti-alias oversampling** added to `softclip`/`tubesat`
    (`fx/utility.rs`) is standard band-limited-waveshaping practice (textbook,
    not specific to ZynAddSubFX).

- **Roland RE-201 Space Echo** (hardware) — inspiration for the *acoustic
  character* modelled by the `Space Echo` effect (multi-head tape delay, wow/
  flutter, tape saturation, spring reverb). Original DSP; not a circuit clone.
  See `docs/audio/space-echo.md`.

- **Hologram Microcosm** (hardware) — inspiration for the *types of processing*
  in the `Protocosmos` granular effect (grain clouds, freeze, pitch/reverse,
  diffusion). Original DSP; not an algorithm clone. See `docs/audio/protocosmos.md`.

- **Freeverb** (Jezar at Dreampoint, public domain) — the Schroeder comb/allpass
  reverb topology reused across `reverb`, `space_echo` and `protocosmos`.
