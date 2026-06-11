# Phase 3 — Rational Editing: Tracker / Pattern / Piano-Roll

**Spec:** `01_patternUpdate.md` (editing UX) · **Status:** Planned · **Depends on:** Phase 2

Build the professional editing experience on top of the Phase 2 rational-time core: editable resolution, tuplets, graphical note resize, free-time mode, advanced snap/quantize, and full keyboard + mouse parity — uniform across Tracker View, Pattern View, and Piano Roll.

---

## Goals

- A **Current Edit Resolution** that governs cursor, insertion, movement, snap, selection, and resize.
- Tuplets, temporal groupings, and per-pattern resolution exposed in the UI without data loss when changing resolution.
- **Graphical note resize** (left/right handles, real-time preview, optional snap, optional free edit).
- **Free Time Edit Mode** (ignore grid/snap; place at exact `RationalTime`).
- Advanced quantize (to resolution / tuplet / grouping / custom) and snap.
- Rich visual feedback (active resolution/tuplet/snap, exact position & duration as both rational and decimal).
- Every operation reversible and reachable by keyboard and mouse; shortcuts configurable.

---

## Current state (post Phase 2)

- Rational events, per-pattern resolution, and a realtime scheduler exist (Phase 2).
- Tracker View edits steps with vim-like keys; Piano Roll / Pattern rendering exist but are step-grid oriented; mouse editing is partial.
- Quantize/humanize exist on the rational core but are not yet surfaced with tuplet/grouping/custom options.

---

## Work breakdown

### 1. Edit-resolution & snap engine (shared)
- [ ] `EditState { resolution: Resolution, tuplet: Option<Tuplet>, snap: SnapMode, free_time: bool }` shared by the three views.
- [ ] Snap a `RationalTime` to the active resolution/tuplet/grouping; bypass entirely in Free Time mode.
- [ ] Keyboard: change resolution, toggle tuplet, toggle snap, toggle free-time, expand/collapse resolution.

### 2. Keyboard editing (complete, configurable)
- [ ] Temporal navigation, insert, delete, move, change duration, change resolution, apply tuplet, quantize, change snap, expand/collapse — all via the keybindings system.
- [ ] Duration editing by keyboard (grow/shrink by the active resolution or a tuplet unit).

### 3. Mouse editing (complete)
- [ ] Create / select / multi-select / rectangular-select / drag / duplicate notes.
- [ ] Adjust start and end; horizontal and vertical zoom.

### 4. Graphical note resize (DAW-style)
- [ ] Each note shows left/right handles; `ResizeStart` / `ResizeEnd` operations.
- [ ] Real-time preview while dragging; snap optional; free (rational) resize optional.
- [ ] Targets arbitrary rational durations (`1.25`, `5/7`, `11/8`, `17/12`, `13/16`, …).

### 5. Piano Roll
- [ ] Main grid + subgrid + **tuplet grid** + **grouping grid**, zoom dependent on resolution.
- [ ] Create / move / duplicate / resize entirely by mouse; vertical = pitch, horizontal = rational time.

### 6. Tracker View
- [ ] `Expand Resolution` / `Collapse Resolution`; visualize complex subdivisions and tuplets; duration editing by keyboard; navigation driven by the active resolution.

### 7. Pattern View
- [ ] Per-pattern resolution selector; changing resolution preserves exact positions/durations (never destroys data).

### 8. Quantize / Snap UI
- [ ] Quantize to Resolution / Tuplet / Grouping / Custom (`1/5,1/7,1/11,1/24,1/48`; `3:2,5:4,7:4,11:8`).
- [ ] Snap modes selectable per view.

### 9. Visual feedback
- [ ] Status/overlay shows active resolution, tuplet, snap; exact `Position: 13/7`, `Length: 5/7`, `Beat: 3.714…` (rational + readable).

### 10. Undo/Redo integration
- [ ] All operations (create/delete/move/duration/resize-start/resize-end/change-resolution/apply-tuplet/quantize) are reversible via the Phase 1 command system.

---

## Affected crates / files

- `crates/seqterm-ui/src/views/{tracker.rs, matrix.rs, granular.rs (piano-roll/editor)}`, `app.rs`, `lib.rs`, `widgets/`.
- `crates/seqterm-command` (edit-resolution, tuplet, resize, quantize commands).
- `crates/seqterm-core` (snap/quantize helpers on rational time, if not already in Phase 2).

## Tests

- [ ] Snap, Free Time, Resize Start/End at odd resolutions and tuplets.
- [ ] Resolution change preserves event positions/durations exactly.
- [ ] Quantize to resolution/tuplet/grouping/custom.
- [ ] Tracker, Piano Roll, and Pattern View editing parity (keyboard == mouse result).
- [ ] Undo/redo of every edit operation.

## Risks

- Mouse hit-testing for handles in a TUI grid — reuse the existing rect/bar hit-test tables; define clear handle zones.
- View consistency: keep `EditState` and snap logic shared so the three views never diverge.

## Exit criteria

Tracker, Pattern, and Piano Roll offer uniform rational editing — editable resolution, tuplets, groupings, polyrhythm, graphical resize, free-time mode, advanced snap/quantize, full keyboard+mouse parity, rich visual feedback — all undoable, with no data loss across resolution changes; tests green.
