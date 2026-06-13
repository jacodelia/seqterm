# Phase 3 — Rational Editing: Tracker / Pattern / Piano-Roll

**Spec:** `01_patternUpdate.md` (editing UX) · **Status:** 🚧 Mostly done (functional editing complete 2026-06-12) · **Depends on:** Phase 2

> **Progress note (2026-06-12).** The shared edit engine, core editing ops, and a
> **complete keyboard + basic-mouse editing workflow** are done and undoable:
> place/erase/toggle, move (`,`/`.`), duplicate (`D`), resize (`+`/`-` + drag),
> quantize (`Q`, resolution+tuplet), grid controls (`<>tsfR`), per-note rational
> readout, and edit-grid tick lines. **Remaining is UI polish:** dedicated resize
> *handles* + ghost preview, mouse drag-move / rectangular select, distinct
> piano-roll tuplet/grouping grid layers, a Pattern-View selector widget, a
> quantize *modal*, configurable-keybinding routing, and the expand/collapse
> resolution view toggle. The step grid is still the canonical store (positions
> kept to 1%-of-step granularity); making `NoteEvent` canonical stays a later step.

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

### 1. Edit-resolution & snap engine (shared) — DONE
- [x] `EditState { resolution, tuplet: Option<Tuplet>, snap: SnapMode, free_time }` in `seqterm-core/edit.rs`; `App.edit_state` shared by the views.
- [x] `snap_pos`/`snap_duration` snap a `RationalTime` to resolution/tuplet (exact rational), bypassed in Free-Time / `SnapMode::Off`; `grid_beats`, `cycle_resolution`, `toggle_triplet`, `summary`.
- [x] Tracker keyboard (Normal mode): `<`/`>` resolution, `t` triplet, `s` snap, `f` free-time, `R` apply-resolution-to-pattern.

### 2. Keyboard editing
- [x] Insert/toggle (Enter), delete (vim), **move** (`,`/`.` by snap unit), **duplicate** (`D`), **change duration** (`+`/`-`), change resolution/tuplet/snap/free-time, **quantize** (`Q` to active resolution+tuplet) — all undoable.
- [x] Duration editing by keyboard grows/shrinks by the active snap unit (tuplet-aware).
- [ ] Not yet routed through the **configurable keybindings system** (hardcoded in `lib.rs`); no apply-tuplet-to-selection / multi-note keyboard ops.

### 3. Mouse editing (complete)
- [ ] Create / select / multi-select / rectangular-select / drag / duplicate notes.
- [ ] Adjust start and end; horizontal and vertical zoom.

### 4. Graphical note resize (DAW-style)
- [x] Core ops: `Pattern::set_note_duration` (resize end) + `resize_note_start` (resize start, end fixed); commands `ResizeNoteEnd`/`ResizeNoteStart`. Target arbitrary rational durations (kept to `gate`'s 1%-of-step granularity).
- [x] Piano-roll resize wired: left-drag snaps the dragged duration to the active edit grid (free-time = raw steps); keyboard `+`/`-` grow/shrink the cursor note by one snap unit (undoable, uncapped to 64 steps). Live status shows `dur N/D beat`.
- [ ] Left/right edge *handle* hit-testing (distinct grab zones) + ghost preview remain.

### 5. Piano Roll
- [~] Create (L-click) / erase (R-click) / **resize** (L-drag + `+`/`-`) by mouse & keyboard; grid summary in title. Main grid + beat-group separators + a faint **edit-grid tick** (exact rational, marks where snap lands) render today.
- [ ] Distinct subgrid + tuplet-grid + grouping-grid line *layers*, resolution-dependent zoom; move/duplicate by mouse remain.

### 6. Tracker View
- [x] Keyboard duration editing (`+`/`-`) in the step table; navigation/grid driven by the active resolution; edit-grid summary in the panel title.
- [ ] `Expand Resolution` / `Collapse Resolution` non-destructive *view* toggle remains.

### 7. Pattern View
- [x] Core: `Pattern::set_resolution` preserves exact positions/durations (lossless on commensurate grids; nearest+`micro` otherwise; returns dropped-on-collision count). Command `ChangePatternResolution`; Tracker `R` applies it.
- [ ] Pattern-View selector widget (dropdown/cycle UI) remains.

### 8. Quantize / Snap UI
- [x] Reachable: `Q` quantizes the pattern to the active resolution **and tuplet** (rational, undoable) via `QuantizeToResolution`. Snap modes live in `EditState`/`SnapMode` and cycle from the keyboard (`s`/`f`).
- [ ] Dedicated quantize **modal** (strength slider, grouping/custom presets) + per-view snap-mode picker remain.

### 9. Visual feedback
- [x] Tracker + Piano-Roll titles show the active grid via `EditState::summary()`; piano-roll edit-grid tick lines; command status messages on every change.
- [x] Per-note exact readout in the piano-roll title: `pos N/D (dec)  len N/D (dec)` for the cursor note.

### 10. Undo/Redo integration — DONE
- [x] `ChangePatternResolution`, `QuantizeToResolution`, `ResizeNote*` route through `record_edit`. **Piano-roll create / erase / gate-drag / move / duplicate are now undoable** via a gesture snapshot (`begin_piano_gesture` on mouse-down/keypress, `commit_piano_gesture` on mouse-up — one undo step per gesture, no-op skipped via `Pattern: PartialEq`). Grid/snap/tuplet/free-time are view state (not undoable, by design).

---

## Affected crates / files

- `crates/seqterm-ui/src/views/{tracker.rs, matrix.rs, granular.rs (piano-roll/editor)}`, `app.rs`, `lib.rs`, `widgets/`.
- `crates/seqterm-command` (edit-resolution, tuplet, resize, quantize commands).
- `crates/seqterm-core` (snap/quantize helpers on rational time, if not already in Phase 2).

## Tests

- [x] Snap (grid/fine), Free-Time, triplet snap-to-thirds, resize start/end (`edit.rs` 10 tests, `pattern.rs` resize tests).
- [x] Resolution change preserves event positions/durations exactly on power-of-two; musical length preserved.
- [x] Quantize to resolution + tuplet grid (`quantize_to_*` tests).
- [ ] Tracker/Piano-Roll/Pattern mouse-vs-keyboard parity (needs mouse wiring).
- [x] Undo/redo path: edit commands route through `record_edit` (core ops tested; app-level harness still deferred).

**+14 tests this slice (383 total).** `edit.rs` snap engine (10) + `pattern.rs` resolution/resize (4).

## Risks

- Mouse hit-testing for handles in a TUI grid — reuse the existing rect/bar hit-test tables; define clear handle zones.
- View consistency: keep `EditState` and snap logic shared so the three views never diverge.

## Exit criteria

Tracker, Pattern, and Piano Roll offer uniform rational editing — editable resolution, tuplets, groupings, polyrhythm, graphical resize, free-time mode, advanced snap/quantize, full keyboard+mouse parity, rich visual feedback — all undoable, with no data loss across resolution changes; tests green.
