# Phase 2 — Rational Time: Core

**Spec:** `01_patternUpdate.md` (foundation) · **Status:** Planned · **Risk:** High (core temporal model)

Introduce an exact, rational representation of musical time and migrate the event model, scheduler, and persistence onto it — **without** changing the editing UX yet (Phase 3). This is the foundational lift the rest of `01_patternUpdate` depends on.

---

## Goals

- A `RationalTime { num: i64, den: i64 }` (always reduced, `den > 0`) used for all positions and durations. **No `f32`/`f64` for stored musical time.**
- Note/event model carries rational `start` + `duration` (absolute, in beats), not step indices, ticks, or row counts.
- Arbitrary resolutions and tuplets are representable exactly (`1/7`, `5/7`, `11/8`, `17/12`, `13:8`, …).
- Per-pattern resolution; polyrhythm across patterns; LCM used **only** for grid rendering, never as the internal representation.
- Lossless migration of existing projects (default edit resolution `1/16`); old `step + micro + gate` converts to rational `start`/`duration`.
- The scheduler plays rational time with no accumulation error and no per-frame heap work.

---

## Current state

- `Note` is a fixed step event: integer step index (implicit in `Pattern.steps: Vec<Note>`), `gate` (% of step), `micro` (% sub-step offset), plus pitch/velocity/CC/MPE fields.
- `Pattern` is a `Vec<Note>` grid with a length, time signature, and swing; `quantize`/`humanize` operate on the step grid.
- The scheduler advances on `elapsed_ticks` (PPQN 480) and derives step timing from the grid; sub-step offset comes from `micro`.
- Persistence serializes patterns as step grids (JSON/MessagePack/`.stz`).

---

## Work breakdown

### 1. RationalTime type (`seqterm-core`)
- [ ] `RationalTime` with reduce-on-construct (gcd), `Add/Sub/Mul/Div` by `RationalTime`/`i64`, `Ord`, `from_beats_decimal` (best-rational), `to_f64` (display only), `to_beats`.
- [ ] `Tuplet { num: i64, den: i64 }` and helpers: subdivide a span into N parts; map a step index under a resolution/tuplet to a `RationalTime`.
- [ ] `Resolution` enum (`1/1 … 1/128`, incl. non-powers `1/3,1/5,1/6,1/7,1/12,1/24,1/48,1/96`) + arbitrary custom denominators.
- [ ] Unit tests: odd denominators, tuplet math, grouping (e.g. 4 beats → 7 divisions), LCM grid for polyrhythm.

### 2. Event model migration
- [ ] `NoteEvent { start: RationalTime, duration: RationalTime, note, velocity, channel, … }` (keep the existing expressive fields: prob, CCs, pitch-bend, pressure, timbre, chord voices).
- [ ] `Pattern` gains a rational event list + a per-pattern `resolution` (default `1/16`) and `length` in beats (`RationalTime`). The legacy `Vec<Note>` step grid becomes a *view* derived for the current resolution, not the source of truth.
- [ ] Keep a compatibility accessor so existing tracker/scheduler code paths compile during the transition.

### 3. Migration & persistence (no data loss)
- [ ] `#[serde(default)]` on all new fields; a `from_legacy_steps()` that converts `step + micro + gate` → `start`/`duration` exactly under the project's stored time signature.
- [ ] Schema-version bump with an automatic up-migration; round-trip test old → new → play is sample-identical.
- [ ] `.stz` bridge updated to carry rational events.

### 4. Scheduler on rational time (`seqterm-engine`)
- [ ] Convert pattern playback to evaluate `NoteEvent.start`/`duration` against the transport in rational beats; note-off at `start + duration`.
- [ ] Polyrhythm: each pattern keeps its own resolution; the scheduler never materializes an LCM grid at runtime.
- [ ] Preserve existing features: swing, probability, micro-timing (now a rational offset), MPE forwarding, audio-slot routing.
- [ ] No allocation/locks added to the audio callback.

### 5. Quantize/humanize on rational time
- [ ] Reimplement `quantize` to snap `start` to a chosen `Resolution`/`Tuplet`; `humanize` adds bounded rational jitter. (UI exposure is Phase 3.)

---

## Data-model changes

- `seqterm-core`: `RationalTime`, `Tuplet`, `Resolution`, `NoteEvent`; `Pattern { events: Vec<NoteEvent>, resolution, length_beats, … }` with legacy step-grid derivation.
- Persistence: new serialized shape + migration; `.stz` domain/bridge updated.

## Affected crates / files

- `crates/seqterm-core/src/{lib.rs, note.rs, pattern.rs}` (+ new `rational.rs`).
- `crates/seqterm-engine/src/scheduler.rs`.
- `crates/seqterm-persistence`, `crates/seqterm-stz` (migration + bridge).

## Tests

- [ ] Rational arithmetic (reduction, ordering, no drift over 10⁶ ops).
- [ ] Simple, odd (`1/7,1/11,1/13,1/17`), tuplet (`3:2,5:4,7:4,11:8,13:8,17:16`), and grouping resolutions.
- [ ] Polyrhythm: 4/5/7/11/13 divisions sync correctly over a bar.
- [ ] Legacy-project migration is lossless; playback parity vs the old scheduler for power-of-two grids.

## Risks

- Touching the scheduler is high-risk for timing regressions — gate behind parity tests and keep the legacy path until parity is proven.
- Performance: rational ops in the hot path — precompute per-block boundaries; keep `RationalTime` `Copy` and branch-light.

## Exit criteria

All musical time is rational end-to-end; existing projects migrate losslessly and play identically on power-of-two grids while now supporting arbitrary/odd resolutions, tuplets, and polyrhythms; scheduler stays realtime-safe; tests green. Editing UX lands in Phase 3.
