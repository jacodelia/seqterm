# Canonical Note Representation — Architecture Decision

**Status:** DECIDED (migration deferred, direction locked) · **Date:** 2026-06-13
· **Scope:** `01_patternUpdate` core data model; gates Arrangement, Automation,
and all future editing work.

This document resolves the single recurring architectural uncertainty in the
roadmap: whether `Pattern.steps` or `NoteEvent` is the canonical note store. It
records the decision, the evidence behind it, the migration impact, and the
incremental strategy — so feature work can proceed without deepening the current
compromise.

---

## 1. Current state

### Canonical store today: the step grid

```rust
pub struct Pattern {
    pub steps: Vec<Note>,   // one Note per step index (the source of truth)
    pub length: usize,      // active step count
    pub resolution: Resolution,
    // …
}
```

Musical timing is **projected onto the step grid** through two `Note` fields:

- `micro: i8` — onset offset, `±99` = ±99 % of one step (`step_start`).
- `gate: u8` — sounding length, `% of one step` (`step_duration`).

The rational view is **derived**, not stored:

```rust
pub fn to_events(&self) -> Vec<NoteEvent>   // pattern.rs:486
```

folds `micro`/`gate` into exact `start`/`duration` and normalizes the payload
note (`micro = 0`, `gate = 100`) so the derived event has one timing source.

```rust
pub struct NoteEvent {        // note.rs:214
    pub start: RationalTime,  // beats from pattern origin
    pub duration: RationalTime,
    pub note: Note,           // expressive payload only
}
```

### Consequences of the compromise

1. **1 %-of-step granularity.** Snap *targets* can be any rational (`5/7`,
   `17/12`), but what is **stored** is the nearest 1 %-of-step `micro`/`gate`.
   Arbitrary-rational positions are not bit-exact after a round-trip.
2. **One event per step index.** Two events at different sub-step times on the
   same step cannot both exist (chord voices share a step's timing). Coarsening
   via `set_resolution` reports a `dropped` count when positions collide.
3. **Every consumer reads `.steps`.** The grid shape is baked into the playback,
   persistence, history, MIDI-I/O, `.stz`, and UI layers (see §4).

These limitations are inherited by any feature built on patterns — including
Arrangement clips (which reference patterns), Automation, and advanced editing.

---

## 2. Options considered

### Option A — Keep `steps` canonical (status quo)

- **Pro:** zero migration cost; all 148 call sites stay valid.
- **Con:** the granularity ceiling and one-event-per-step limit are permanent;
  every future editor feature is built on a representation we know is wrong.
- **Verdict:** rejected as the *long-term* model; acceptable only as the
  transitional state during migration.

### Option B — Big-bang `NoteEvent` canonical

`Pattern` stores `events: Vec<NoteEvent>` as truth; the step grid becomes a
derived view computed on demand for the tracker.

- **Pro:** exact, unlimited sub-step events, clean model.
- **Con:** rewrites ~148 `.steps` call sites + the tracker/piano-roll editing
  paths + a persistence migration, all in one non-shippable change. High risk,
  long red-build window, hard to review.
- **Verdict:** rejected as a single step; correct target, wrong delivery.

### Option C — Incremental `NoteEvent` canonical via an adaptor *(CHOSEN)*

Make `NoteEvent` the canonical target, but migrate **behind an adaptor** so the
codebase stays green at every step:

1. Patterns gain an authoritative `events` representation in core; `steps`
   becomes a **derived, cached view** produced by a `steps_view()` adaptor.
2. Consumers migrate from `.steps` to `to_events()` / `NoteEvent` **incrementally**,
   crate by crate, lowest-risk first (engine/midi-io/stz already mostly read
   events conceptually; UI is the long tail).
3. Serialization is **additive** (schema v4): persist `events`; on load, legacy
   step-grid projects derive `events` once (lossless for existing 1 %-of-step
   data). The step view is never serialized as truth again.
4. The grid-based tracker keeps editing through the `steps_view()` adaptor until
   its editing paths are ported to operate on events directly.

- **Pro:** every commit ships; risk is spread; reviewable; new features can
  target `NoteEvent` immediately and never touch `.steps`.
- **Con:** a transitional period where both representations coexist (the adaptor
  must keep them consistent); slightly more total code than a big-bang.
- **Verdict:** **chosen.** Lowest risk for the largest architectural change.

---

## 3. Decision

1. **`NoteEvent` is the canonical representation** the project is migrating
   toward. `Pattern.steps` is, from now on, a *legacy/derived* concept.
2. **The migration is deferred until after Arrangement playback (Milestone B).**
   It is large and should not block the highest-value user feature.
3. **Hard constraint, effective immediately:** *no new feature may add new
   `.steps` dependencies.* New code (Arrangement, Automation, scheduler routing,
   audio/MIDI clips) consumes `to_events()` / `NoteEvent`. This stops the
   compromise from deepening while the migration is scheduled.
4. The migration follows **Option C** (incremental adaptor), schema **v4**,
   additive and lossless for existing projects.

---

## 4. Affected call-site inventory

`.steps` references outside tests (snapshot 2026-06-13): **148** across 9 crates.

| Crate | `.steps` refs | Migration character |
|-------|--------------:|---------------------|
| `seqterm-ui` | 75 | **Long tail.** Tracker/piano-roll editing + rendering. Migrate last, via the `steps_view()` adaptor; the editing paths are the real work. |
| `seqterm-core` | 40 | The adaptor itself + pattern ops (`set_resolution`, resize, quantize, humanize). Migrate first — defines `events`↔`steps` equivalence. |
| `seqterm-history` | 10 | Snapshots whole patterns; mostly mechanical once `Pattern` shape settles. |
| `seqterm-midi-io` | 8 | Import/export — already event-shaped; port to `NoteEvent` directly. |
| `seqterm-stz` | 7 | Bridge already carries `resolution_den`; extend to events. |
| `seqterm-persistence` | 3 | Schema v4 migration site. |
| `seqterm-sdk` | 2 | Public surface — coordinate with any external consumers. |
| `seqterm-engine` | 2 | Scheduler — should read `to_events()`; small. |
| `seqterm-audio-engine` | 1 | Trivial. |

**Critical path:** core adaptor → engine/midi-io/stz/persistence (event-shaped,
low risk) → UI editing paths (the bulk of the effort and the only place that
truly needs new design, because grid editing must become event editing).

---

## 5. Incremental migration plan

- **M-A.1** Introduce `Pattern::events` (canonical) + `steps_view()` adaptor in
  core; keep `to_events()` as the read API. Property test: `events ==
  to_events(steps_view(events))` for all 1 %-of-step-representable inputs.
- **M-A.2** Schema v4: serialize `events`; legacy load derives them once
  (lossless). Persistence round-trip tests for v1→v4.
- **M-A.3** Port event-shaped consumers (engine, midi-io, stz, audio-engine,
  persistence, sdk) off `.steps`. Each is independently shippable.
- **M-A.4** Port `seqterm-core` pattern ops to operate on `events`; `steps_view`
  becomes pure derivation (no write-back through `micro`/`gate`). This is where
  the 1 %-of-step ceiling is actually removed.
- **M-A.5** Port the tracker/piano-roll editing paths (the 75 UI sites). Grid
  cells become a view over events; sub-step and multi-event-per-column become
  possible. Largest, most design-heavy step.
- **M-A.6** Remove the legacy `steps` field from the serialized model (keep a
  derived accessor for any remaining grid rendering). Final cleanup.

Each step keeps the suite green; the build is never broken across a milestone
boundary.

---

## 6. Cost estimate (order-of-magnitude)

| Step | Effort | Risk |
|------|--------|------|
| M-A.1 adaptor + props | S–M | Low |
| M-A.2 schema v4 | S | Low (additive) |
| M-A.3 event-shaped consumers | M | Low |
| M-A.4 core ops on events | M | Medium (removes ceiling — behavior change) |
| M-A.5 UI editing paths | **L** | Medium–High (real redesign of grid editing) |
| M-A.6 cleanup | S | Low |

The cost is concentrated in **M-A.5** (UI). Everything before it is mechanical
and de-risking. This ordering means the exactness win (M-A.4) lands well before
the expensive UI rework, and external/playback consumers are correct early.

---

## 7. Implications for current work

- **Arrangement playback (Milestone B)** reads `to_events()` per active clip via
  `clips_active_at()` — it never touches `.steps`, so it is forward-compatible
  with the migration with no rework.
- **Automation (Milestone F)** stores rational points/segments natively; it does
  not depend on the step grid and is unaffected.
- **The headless test harness (Milestone D)** should assert against `NoteEvent`
  output, not grid cells, so its tests survive the migration.
