# Roadmap Status & Pending Issues

Honest, end-to-end status of the `01/02/03` roadmap (see `README.md` for the
plan). Last updated **2026-06-13**. Test baseline: **415 passing, 0 failing**;
new code clippy-clean.

> **Execution plan (post-Phase-4-editing).** Work is now sequenced by the
> milestone plan, not strict roadmap order: **A** canonical-note decision ✅
> (`docs/rational-storage.md`), **B** arrangement playback ✅ (routing + scheduler
> + toggle; emit-parity follow-ups remain), **C** audio clips/waveforms/MIDI-viz ✅
> (audio-clip *playback* still to wire), **D** headless test harness ✅
> (`seqterm_ui::testkit::HeadlessApp`), **E** mouse editing 🚧 (arrangement
> drag-move + Alt+Drag duplicate + sub-beat snap done; piano-roll Shift+drag
> rect-select + batch-delete done; piano drag-move-notes + zoom + resize-handles
> remain), then **F** automation, **G** Phase 5.

Legend: ✅ done · 🚧 partial (foundation/slice landed, more remains) · ⬜ not started.

| Phase | Spec | Theme | Status |
|-------|------|-------|--------|
| 1 | 03 | Matrix Copy/Paste + Session Undo/Redo | ✅ Done |
| 2 | 01 | Rational Time — Core | ✅ Done |
| 3 | 01 | Rational Editing — Tracker / Pattern / Piano-Roll | 🚧 Mostly done (functional; UI polish remains) |
| 4 | 02 | Arrangement Editor — Foundation | 🚧 In progress (model + timeline + clip creation + inspector + mouse) |
| 5 | 02 | Arrangement Editor — Pro Workflow & Scale | ⬜ Not started |

---

## The one cross-cutting issue that gates "exact" everywhere

**The step grid (`Pattern.steps: Vec<Note>`) is still the canonical store.** The
rational `NoteEvent` model (Phase 2) is *derived* from it, not the source of
truth. Consequences that recur in every item below:

- Note **positions** are stored via `micro` (±99 = ±99 % of a step) and
  **durations** via `gate` (% of a step) — both at **1 %-of-step granularity**.
  So arbitrary rationals (`5/7`, `17/12`) snap *targets* are exact, but what gets
  **stored** is the nearest 1 %-of-step value.
- One note per step (plus `chord_notes`): two events at different sub-step times
  on the *same* step index cannot both be stored. `set_resolution` reports a
  `dropped` count when a coarsening collapses distinct positions.
- True bit-exact arbitrary-rational editing and full mouse↔keyboard parity
  require making `NoteEvent` canonical (rewriting the ~105 `.steps` call sites +
  the tracker/piano-roll editing paths). **This is the single biggest remaining
  piece of `01_patternUpdate`** and is deliberately deferred.

---

## Phase 1 — Matrix Copy/Paste + Undo/Redo ✅

Done (see `phase-1.md`, `memory/phase1-matrix-clipboard.md`). Known deferrals:

- Undo uses full `ProjectSnapshot` per edit (cut/paste), not minimal diffs —
  intentional (matches the proven pattern); diff-optimization deferred.
- ✅ **App-level integration-test harness landed (Milestone D)** —
  `seqterm_ui::testkit::HeadlessApp` drives the real `handle_key`/`handle_mouse`
  dispatchers with no TTY/audio. Arrangement create/dup/delete/route/playback +
  a **Ctrl+Z undo** round-trip are now covered end-to-end
  (`tests/arrangement_workflow.rs`). Phase-1 paste-mode and Phase-3 tracker-edit
  workflow tests are now *unblocked* and can be added the same way.

## Phase 2 — Rational Time Core ✅

Done (see `phase-2.md`, `memory/phase2-rational-time.md`). Known deferrals/risks:

- Events derived, not stored (the cross-cutting issue above).
- **Scheduler keeps the fixed 1/16 master clock.** Per-pattern resolution plays
  correctly by scanning a rational window per master step and deferring off-grid
  hits to the tick queue — but the *granularity ceiling* is the tick clock
  (`ppqn`, default 480/beat). Extreme subdivisions beyond tick resolution would
  quantize to the nearest tick. Parity on 1/16 grids is byte-identical.
- `quantize_to`/`humanize_rational` write back through `micro` (1 %-of-step).
- `.stz` carries `resolution_den` but not tuplet metadata (patterns store a
  resolution, not a tuplet; tuplets live only in the transient `EditState`).

## Phase 3 — Rational Editing 🚧 (functional editing complete; UI polish remains)

Done (see `phase-3.md`, `memory/phase3-rational-editing.md`):

- ✅ **Item 1** shared `EditState` + snap engine (`seqterm-core/edit.rs`): exact
  rational snap, resolution ladder, triplet toggle, `summary()`. Fully tested.
- ✅ **Item 2** keyboard editing: insert/toggle, move (`,`/`.`), duplicate (`D`),
  resize (`+`/`-`), grid controls (`<>tsfR`), quantize (`Q`). All undoable.
- ✅ **Item 6 (core)** keyboard duration editing in the step table.
- ✅ **Item 7 core** `Pattern::set_resolution` (lossless on commensurate grids).
- ✅ **Item 4 core + wiring** `set_note_duration` / `resize_note_start`; piano-roll
  left-drag (snapped) + keyboard `+`/`-`; commands route through undo.
- ✅ **Item 8 (functional)** `Q` quantizes to active resolution **and tuplet**.
- ✅ **Item 9** edit-grid summary + tick lines + per-note `pos/len` rational readout.
- ✅ **Item 10** create / erase / gate-drag / move / duplicate are undoable via
  gesture snapshots; structural commands via `record_edit`.

### Pending in Phase 3 (UI polish)

- 🚧 **Item 3 — mouse editing:** **rectangular selection DONE** (Milestone E) —
  `Shift+drag` rubber-bands a step×pitch rectangle, selected notes are
  highlighted, `Del`/`Backspace` batch-deletes them (one undo step), `Esc`
  clears. Driven through the real dispatcher; harness-tested. Remaining: mouse
  drag-move notes and vertical/horizontal zoom (would require parameterizing the
  hardcoded 2-col cell width across the renderer + hit-tests).
- 🚧 **Item 4 — graphical resize handles:** resize works (drag + keys), but no
  dedicated left/right **edge grab zones**, no **ghost preview**, no mouse
  resize-*start* (only resize-end via drag).
- 🚧 **Item 5 — Piano Roll grids:** beat-group separators + one edit-grid tick
  render; dedicated **subgrid / tuplet-grid / grouping-grid layers** and
  resolution-dependent zoom not done; move/duplicate by *mouse* missing.
- ⬜ **Item 6 — expand/collapse resolution view toggle** (the non-destructive
  Tracker view that shows subdivisions) remains.
- ⬜ **Item 7 UI — Pattern-View resolution selector widget** (resolution is
  changed via the `R` key today).
- ⬜ **Item 8 UI — Quantize modal** (strength slider, grouping/custom presets)
  and per-view snap-mode picker. The functional path (`Q`) exists.
- ⬜ **Configurable keybindings:** the new edit keys are hardcoded in `lib.rs`,
  not yet routed through the keybindings system.
- 🚧 **Tests:** the headless harness now exists (Milestone D) and covers
  arrangement edit-command round-trips + undo; mouse-vs-keyboard parity tests and
  Phase-3 tracker-edit workflow tests remain to be *written* (no longer blocked).

## Phase 4 — Arrangement Editor: Foundation 🚧 (data model + migration)

Done (see `phase-4.md`):

- ✅ **Fase 1 audit** — written into `phase-4.md` (current bar-block model,
  limitations, reuse-vs-refactor).
- ✅ **Fase 2 data model** — `seqterm-core/arrangement.rs`: rational
  `Arrangement → ArrangementTrack → Lane → Clip` with stable ids, `ClipKind`
  (Pattern/Audio/MIDI), tested clip ops. Schema **v3**; legacy bar-blocks migrate
  forward losslessly on load.
- ✅ **Fase 5 core + command layer** — `add/delete/duplicate/split_clip`,
  `clips_active_at(beat) -> ClipHit`; commands `Arrangement{AddTrack,AddClip,
  MoveClip,SplitClip,DuplicateClip,DeleteClip,TrimClip}` with undoable handlers.
  19 arrangement tests (incl. clip-cursor navigation helpers).
- ✅ **Gate cleared — timeline render + clip cursor:** `draw_arrangement_timeline`
  renders the rational `Arrangement` (toggle **`g`**), and a keyboard clip cursor
  (`h`/`l` clips, `j`/`k` tracks, `d`/`s`/`x`/`,`/`.`) invokes the `Arrangement*`
  commands. The model is now **reachable and editable** end-to-end.

### Pending in Phase 4 (UI polish / playback)

- 🚧 **Fase 3 — pro layout:** timeline rendering **DONE** (colored clip bars on
  the bar grid, per-kind glyph, cursor-clip highlight, `╎` beat-cursor marker,
  per-clip readout) and a **mixer-free inline Track Inspector** **DONE**
  (arm/solo/mute/monitor flags + `a`/`o`/`u`/`y` toggles). Remaining: a broken-out
  time-signature + record-state header block.
- 🚧 **Fase 4 — clip system UI (Milestone C):** **DONE** — `n` places a Pattern
  clip (length = pattern bars), `A` places an **audio clip** from a file (length
  from duration×BPM, undoable, serialized), `t` adds a track. Rendering: audio
  **waveforms** (`▁..█` from `waveform_cache`, periodic scan includes arrangement
  clips), pattern/MIDI **note-density** preview (`•`), distinct kind glyphs,
  shared-reference pattern instancing. **Remaining:** scheduler **playback of
  arrangement audio clips** (creation/display done; `playback_hits` is pattern/
  MIDI-only — triggering the audio slot at the clip start isn't wired) + inline
  MIDI-event editing.
- 🚧 **Fase 5 — editing tools UI:** keyboard clip cursor + ops **DONE**
  (move/split/trim/duplicate/delete, undoable), **left-click select** **DONE**,
  and **mouse drag-move + `Alt+Drag` duplicate + sub-beat (1/4) snap** **DONE**
  (Milestone E; one undo step per drag via an arrangement gesture snapshot, tested
  through the headless harness). Remaining: rectangular/multi-select,
  mouse resize-handles + ghost preview, time-stretch.
- ⬜ **Fase 6 — inline automation:** per-track lanes with points/curves/ramps/
  Bézier on the timeline (today automation is project-global, bar-quantized).
- 🚧 **Scheduler playback (Milestone B):** **DONE for the common path.**
  `ArrangementTrack.source_row` routes a track through a matrix row's configured
  instrument; `Arrangement::playback_hits(beat)` (tested) resolves what to play;
  `Scheduler::fire_arrangement_clips` emits it (gated by
  `EngineCommand::SetArrangementPlayback`, toggled with `P`, routed with `R`).
  Tested end-to-end (`arrangement_clip_plays_through_routed_row`). **Remaining
  parity:** MPE / drum-map / per-step CC on the arrangement path, audio-buffer
  lookahead compensation, and richer routing (the row's *first* clip supplies the
  instrument today).
- ⬜ **`.stz` bridge** extension for the arrangement model.

## Phase 5 — Arrangement Editor: Pro Workflow & Scale ⬜

Not started. Full scope in `phase-5.md`: navigation/zoom/fit, markers/regions/
cycle, track types (audio/instrument/hybrid/folder/group), sections + overview,
UX consistency, virtualization/scale for large projects, integration, docs.

---

## Suggested next steps (in priority order)

1. ✅ ~~Make piano-roll create/erase/gate undoable~~ — **done** (gesture snapshots).
2. **Piano-roll mouse: drag-move + rectangular select** (item 3) — high user value,
   builds on the existing hit-testing.
3. **Quantize modal** (item 8 UI) — functional `Q` exists; this is the strength/
   preset picker.
4. **Pattern-View resolution selector** (item 7 UI) + expand/collapse view (item 6).
5. ✅ ~~**Decide on `NoteEvent`-canonical migration**~~ — **done**
   (`docs/rational-storage.md`): NoteEvent canonical, incremental Option-C, schema
   v4, deferred past playback; hard rule = no new `.steps` deps.
6. ✅ ~~**Stand up a headless `App` test harness**~~ — **done** (Milestone D):
   `seqterm_ui::testkit::HeadlessApp`.

> **Note:** Phase 3 is "functionally complete" — every editing operation is
> reachable, rational, and undoable. The remaining items are TUI polish (mouse
> ergonomics, dedicated grid layers, modals) and the configurable-keybinding
> routing. They can proceed in parallel with Phase 4.
