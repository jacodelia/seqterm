# Phase 2 — Rational Time: Core

**Spec:** `01_patternUpdate.md` (foundation) · **Status:** ✅ Done (2026-06-12) · **Risk:** High (core temporal model)

> **Implementation note.** Delivered as a *dual representation* per the doc's
> "compatibility accessor during the transition" guidance: the `Vec<Note>` step
> grid stays the editing **source of truth**, and the exact rational `NoteEvent`
> view is **derived** from it (`Pattern::to_events`) — losslessly for the legacy
> `1/16` grid and for any resolution. The scheduler and persistence run on the
> rational view; flipping events to *canonical* (and the editing UX) is Phase 3.
> Realtime note: the "no allocation/locks" contract binds the **audio-engine
> callback**, not the scheduler thread (which already clones the whole project
> per step), so deriving events on the scheduler thread is in-budget.

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

### 1. RationalTime type (`seqterm-core`) — `rational.rs`
- [x] `RationalTime` with reduce-on-construct (gcd), `Add/Sub/Mul/Div` by `RationalTime`/`i64`, `Ord` (i128 cross-mul, no overflow), `from_beats_decimal` (Stern–Brocot best-rational), `to_f64`/`to_beats` (display only), plus `floor`/`frac`/`rem_euclid`/`div_floor`.
- [x] `Tuplet { num, den }` (`scale = den/num`) + helpers `step_to_beats`, `subdivide`.
- [x] `Resolution` enum (`Whole(den)` / `Custom(den)`, incl. non-powers `3,5,6,7,12,24,48,96`; `step_beats`); `default_edit = 1/16`.
- [x] Unit tests: odd denominators, tuplet math, 4 beats → 7 divisions, LCM polyrhythm grid, no-drift over 10⁶ ops, serde.

### 2. Event model migration
- [x] `NoteEvent { start, duration, note: Note }` — embeds `Note`, preserving **all** expressive fields (prob, CCs, pitch-bend, pressure, timbre, chord voices) losslessly.
- [x] `Pattern` gains `resolution` (`#[serde(default)] = 1/16`); rational view derived via `step_beats`/`length_beats`/`step_start`/`step_duration`/`to_events`. (Step grid stays canonical — see note above; events derived, not stored.)
- [x] Compatibility preserved: no change to the 105 `.steps` call sites; everything compiles.

### 3. Migration & persistence (no data loss)
- [x] `#[serde(default)]` on `resolution`; legacy `step + micro + gate` → `start`/`duration` via `to_events` (no separate `from_legacy_steps` needed — default reconstructs identical timing).
- [x] Schema bump `v1 → v2` with a documented no-op up-migration (serde default covers it); `legacy_v1_project_migrates_losslessly_to_rational` test.
- [x] `.stz` bridge carries per-pattern resolution (`StzPattern.resolution_den`, default 16); round-trip test.

### 4. Scheduler on rational time (`seqterm-engine`)
- [x] `fire_all_clips` scans a rational beat-window per master step via `hits_in_window`; note-off at `start + duration` (`gate_ticks = duration * ppqn`); off-grid hits deferred through the existing pending-note tick queue.
- [x] Polyrhythm: each pattern loops on its own `length_beats`; no runtime LCM grid.
- [x] Preserved: swing, probability, micro-timing (now rational `offset`), MPE forwarding, audio-slot routing, audio lookahead.
- [x] Master 1/16 clock unchanged → byte-identical timing for `1/16` patterns (`rational_scheduler_parity_on_sixteenth_grid`); triplet deferral tested. No audio-callback changes.

### 5. Quantize/humanize on rational time
- [x] `Pattern::quantize_to(Resolution, Tuplet, strength_pct)` snaps exact rational `start` to a grid line; `humanize_rational(amount_pct)` adds bounded deterministic jitter. Results stored through `micro` (1%-of-step granularity until events go canonical in Phase 3). UI exposure is Phase 3.

---

## Data-model changes

- `seqterm-core`: `RationalTime`, `Tuplet`, `Resolution`, `NoteEvent`; `Pattern { events: Vec<NoteEvent>, resolution, length_beats, … }` with legacy step-grid derivation.
- Persistence: new serialized shape + migration; `.stz` domain/bridge updated.

## Affected crates / files

- `crates/seqterm-core/src/{lib.rs, note.rs, pattern.rs}` (+ new `rational.rs`).
- `crates/seqterm-engine/src/scheduler.rs`.
- `crates/seqterm-persistence`, `crates/seqterm-stz` (migration + bridge).

## Tests

- [x] Rational arithmetic (reduction, ordering, no drift over 10⁶ ops).
- [x] Odd denominators, tuplet math, grouping (4 beats → 7), LCM polyrhythm grid.
- [x] Polyrhythm: independent loop lengths realign only at the LCM (`hits_in_window_polyrhythm_independent_loops`).
- [x] Legacy-project migration is lossless; scheduler parity vs the old grid on `1/16` (`rational_scheduler_parity_on_sixteenth_grid`).

**+35 tests, 369 total green.** New: `rational.rs` (20), `pattern.rs` rational view/window/quantize (11), persistence migration (2), `.stz` resolution (1), scheduler parity+triplet (2). Clippy-clean.

## Risks

- Touching the scheduler is high-risk for timing regressions — gate behind parity tests and keep the legacy path until parity is proven.
- Performance: rational ops in the hot path — precompute per-block boundaries; keep `RationalTime` `Copy` and branch-light.

## Exit criteria

All musical time is rational end-to-end; existing projects migrate losslessly and play identically on power-of-two grids while now supporting arbitrary/odd resolutions, tuplets, and polyrhythms; scheduler stays realtime-safe; tests green. Editing UX lands in Phase 3.
