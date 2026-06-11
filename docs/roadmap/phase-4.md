# Phase 4 — Arrangement Editor: Foundation

**Spec:** `02_songUpdate.md` (Fases 1–6) · **Status:** Planned · **Constraint:** no mixer functionality in this view

Transform the SONG view from a basic scene/chain sequencer into a professional timeline **Arrangement Editor** — the project's center of composition, arrangement, and temporal editing. This phase delivers the audit, the new data model, the pro layout, the clip system, the core editing tools, and timeline automation. Navigation polish, markers, advanced track types, sections, and scaling are Phase 5.

---

## Goals

- A global project **timeline** as the primary visual focus.
- Data hierarchy: **Project → Tracks → Lanes → Clips → Events**, each independently editable.
- Pro layout: global transport header, left **Track Inspector** (no mixer controls), main timeline with ruler/markers/clips/automation.
- Clip system: **Audio / MIDI / Pattern** clips with move, trim, split, duplicate, loop, (stretch).
- Standard editing tools: Selection, Split (`S`), Trim, Move (snap), Duplicate (`Alt+Drag`), Time-Stretch.
- Automation lanes in the timeline (points, curves, ramps, Bézier).
- Strict separation from the Mixer view.

---

## Current state

- The Arranger (SONG) view stores `tracks` with `blocks (start_bar, length_bars, pattern_label)`, plus `scenes` and a `chain`. There is a loop region and basic markers; zoom is coarse; clips are pattern-block references only.
- Automation exists as project lanes evaluated by the scheduler; it is not yet edited inline on the timeline.
- Rational time (Phase 2) is available for exact clip positions/lengths.

---

## Work breakdown

### Fase 1 — Audit (deliverable: written report)
- [ ] Document the current SONG architecture: track/clip model, playback, time sync, events, rendering, selection, zoom.
- [ ] Identify limitations, UX problems, bottlenecks, scalability risks; mark reusable vs. refactor-needed components.

### Fase 2 — Conceptual model
- [ ] Introduce `ArrangementTrack`, `Lane`, `Clip`, `ClipRef` types (Project → Tracks → Lanes → Clips → Events), each editable independently. Clip positions/lengths use `RationalTime` (Phase 2).

### Fase 3 — Pro layout
- [ ] **Global header**: transport (Play/Stop/Record/Loop/Metronome), Tempo, Time Signature; indicators (current time, bar, BPM, record state).
- [ ] **Track Inspector** (left): name, color, type, arm, solo, mute, monitor, optional instrument/audio icon. **No mixer controls.**
- [ ] **Main timeline**: time ruler, markers, tracks, clips, automation; the timeline is the focal area.

### Fase 4 — Clip system
- [ ] **Audio clips**: waveform + name + color; move, trim, split, duplicate, loop, stretch.
- [ ] **MIDI clips**: name + color + content indicators; move, resize, duplicate, loop.
- [ ] **Pattern clips**: references to internal sequencer patterns; shared references + multiple instancing (non-destructive reuse).

### Fase 5 — Editing tools
- [ ] **Selection** (single / multiple / rectangular).
- [ ] **Split** at the playhead/cursor (`S`).
- [ ] **Trim** (change duration, non-destructive on source).
- [ ] **Move** with configurable snap.
- [ ] **Duplicate** (`Alt+Drag`).
- [ ] **Time-Stretch** (length change; preserve pitch where possible).

### Fase 6 — Automation
- [ ] Per-track automation lanes for: track volume, pan, instrument params, FX params, granular params, sampler params.
- [ ] Each lane supports points, curves, ramps, and Bézier; rendered inline in the timeline.
- [ ] All clip and automation edits are undoable (Phase 1 command system).

---

## Data-model changes

- `seqterm-core`: arrangement types (`ArrangementTrack`, `Lane`, `Clip`, `ClipKind::{Audio,Midi,Pattern}`), clip timing in `RationalTime`, per-track automation lane references. New fields `#[serde(default)]`; old `tracks/blocks/chain` migrate forward.
- `.stz` bridge extended for the new arrangement model.

## Affected crates / files

- `crates/seqterm-core/src/project.rs` (+ new `arrangement.rs`).
- `crates/seqterm-ui/src/views/arranger.rs`, `app.rs`, `lib.rs`, `widgets/`.
- `crates/seqterm-command` (clip + automation edit commands).
- `crates/seqterm-engine/src/scheduler.rs` (play arrangement clips on the timeline).
- `crates/seqterm-persistence`, `crates/seqterm-stz` (migration).

## Tests

- [ ] Clip move/trim/split/duplicate/loop preserve source content and timing.
- [ ] Pattern clips share a reference; editing the source updates all instances.
- [ ] Automation point add/move/delete + curve types; inline evaluation matches the scheduler.
- [ ] Migration of existing arranger projects is lossless.

## Risks

- Scope: this is a large UI surface — keep the audit (Fase 1) gate-first so the data model is right before building tools.
- Avoid mixer bleed: enforce the inspector excludes volume/EQ/sends (those stay in the Mixer view).

## Exit criteria

SONG presents a timeline with a global header, a (mixer-free) track inspector, audio/MIDI/pattern clips with the six editing tools, and inline automation lanes — all on rational time and fully undoable; existing projects migrate losslessly; tests green. Navigation, markers, advanced tracks, sections, and scaling follow in Phase 5.
