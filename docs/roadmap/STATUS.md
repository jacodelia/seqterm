# Roadmap Status & Pending Issues

Honest, end-to-end status of the `01/02/03` roadmap (see `README.md` for the
plan). Last updated **2026-06-15**. Test baseline: **457 passing, 0 failing**;
new code clippy-clean. Song-section coverage: full-arrangement persistence
round-trip (automation/markers/regions/sections/cycle + exact 2/7 event),
end-to-end scheduler playback of an **exact off-grid tuplet event** through a
routed row with sub-step tick precision, and multi-track section-shift integrity.

> **Phase 6 — Complex/irregular rhythms (canonical-note layer):** patterns carry
> an exact rational note layer (`Pattern.events: Vec<NoteEvent>`, schema v4) merged
> into `to_events()` so it **plays + persists**. Piano roll **and** tracker step
> table: **Ctrl++/Ctrl+-** (or the RHYTHM toolbar `ZOOM±`) zoom the **displayed
> grid** to 1/64 (semifusa) by stepping the *edit* resolution — this subdivides
> each step into `pdiv` sub-cells so you can place corcheas…semifusas **within a
> beat** without changing the pattern's length (fixed: zoom no longer re-grids the
> pattern). Sub-cell clicks + the fine cursor drop those notes into the exact
> rational `events` layer. **Piano-roll mouse map:** **left-drag** = rubber-band
> note selection (variable rectangle); **middle/scroll-wheel button** = insert
> notes (press to place, drag to paint); **right-click** = erase; **Ctrl+scroll** =
> zoom the grid; **Delete/Backspace** erase precisely (the exact event under the
> cursor, else the step note — batch-deletes when a selection is active). The
> left-drag **marquee draws a visible rectangle border** while selecting; the
> selection is **zoom-aware** and grabs every note in the box — step notes **and**
> exact events. With a selection, **RHYTHM → FIGURE** (button or `g`) opens a
> **modal of irregular groupings (2…12)** that **regroups the selected notes into
> an N-tuplet confined to the selection's own span** — it never touches notes
> outside the selection or extends the pattern. **Selected notes are recoloured**
> (bright magenta) so the marked set is unmistakable, step notes and exact events
> alike. Applying a figure also drops a **score-style bracket annotation** above
> the regrouped notes in the piano roll (e.g. `⌐──5──¬`), persisted per-pattern
> (`Pattern.tuplet_marks`, schema-additive) and undoable; it's purely visual and
> never affects playback. The RHYTHM toolbar is now `[ZOOM−][ZOOM+][FIGURE]` (TUPLET/TRIPLET removed). The tracker NOTE column
> now mirrors the piano roll: it shows the **top** note sounding at each step
> (step note + chord voices + exact events; amber when poly). **Shift+T** sets
> an arbitrary tuplet ratio (e.g. 7:9); **g** drops that tuplet figure (any ratio;
> several tuplets coexist exactly in one voice = polyrhythm); **`[`/`]`** move a
> **fine exact-rational cursor** and **`\`** toggles an exact note there
> (sound/MIDI precise; UI shows it only approximately, by design); **Ctrl+C/Ctrl+V**
> rhythm-aware copy/paste (carries step notes AND exact events). A visible,
> **TRANSPORT-style RHYTHM toolbar** under the piano roll exposes all of this with
> the mouse — `[ZOOM−][ZOOM+][TUPLET][FIGURE][TRIPLET]` boxes + a live `GRID …`
> readout — so the feature is discoverable and works even on terminals that don't
> transmit `Ctrl+=`. MIXER/FX gained **Add / Move** buttons (clickable + `a`/`,`/`.`).
> PATTERN view opens on the user's **favourite tab** (`*` marks it; ★ shown; persisted).

> **Execution plan (post-Phase-4-editing).** Work is now sequenced by the
> milestone plan, not strict roadmap order: **A** canonical-note decision ✅
> (`docs/rational-storage.md`), **B** arrangement playback ✅ (routing + scheduler
> + toggle; emit-parity follow-ups remain), **C** audio clips/waveforms/MIDI-viz ✅
> (audio-clip *playback* still to wire), **D** headless test harness ✅
> (`seqterm_ui::testkit::HeadlessApp`), **E** mouse editing 🚧 (arrangement
> drag-move + Alt+Drag duplicate + sub-beat snap done; piano-roll Shift+drag
> rect-select + batch-delete done; piano drag-move-notes + zoom + resize-handles
> remain), **F** automation 🚧 (per-track rational automation lanes: model +
commands + keyboard edit + timeline sub-lane render + realtime CC apply + dest
picker done; only mouse point-editing remains), then **G** Phase 5 🚧 (Fase 8
Fase 8 (markers/regions/cycle+loop) DONE, Fase 9 track reorder/delete/kind DONE, Fase 9 (reorder/delete/kind/rename) + Fase 10 (sections+rearrange+overview+nav) + Fase 13 docs DONE; nav/zoom (Fase 7), UX model (Fase 11), virtualization+folders (Fase 12) remain — see Remaining gaps).

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
- 🚧 **Fase 6 — inline automation (Milestone F):** per-track lanes now use the
  **rational, beat-based** `automation::AutomationLane` (breakpoints with
  Linear/Exp/Log/Bézier curves + Read/Write/Touch/Latch modes — interpolation
  already battle-tested in core). `Arrangement::{set,remove}_automation_point` +
  `automation_value` (tested); undoable commands `Arrangement{Set,Remove}AutomationPoint`;
  keyboard edit in the timeline (`V` show/hide sub-lane, `+/-` value cursor,
  `p` set point at beat cursor, `c` clear nearest); 8-level breakpoint-curve
  sub-row render under the focused track. **Realtime apply DONE**: the scheduler's
  `process_arrangement_automation` evaluates each routed, unmuted track's lanes
  every tick and emits a MIDI CC (dest→CC map: volume→7, pan→10, cutoff→74,
  resonance→71, send_a/reverb→91, send_b/chorus→93, `ccNN` literal) to the routed
  instrument's audio slot (`AudioControlChange`) and/or MIDI port, only when the
  quantised `0..127` value changes. **Destination picker DONE**: `b`/`B` cycle
  the edited lane through volume/pan/cutoff/resonance/reverb/chorus (the value
  cursor re-syncs to the picked lane). Remaining: mouse point drag/insert.
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

## Phase 5 — Arrangement Editor: Pro Workflow & Scale 🚧 (markers landed)

Full scope in `phase-5.md`: navigation/zoom/fit, markers/regions/cycle, track
types (audio/instrument/hybrid/folder/group), sections + overview, UX
consistency, virtualization/scale for large projects, integration, docs.

- 🚧 **Fase 8 — markers (first slice DONE):** rational beat-based `Marker{beat,
  name,color}` on `Arrangement` with sorted insert/dedupe (1/64-beat tol),
  `add_marker`/`remove_marker`/`neighbor_marker` (core-tested). Undoable commands
  `Arrangement{Add,Remove}Marker`; keyboard in the rational timeline: `m` add at
  beat cursor (auto-named Intro/Verse/Chorus/Bridge/Outro→`Marker N`), `M` remove
  nearest, `<`/`>` jump cursor to prev/next marker. Marker ruler row rendered
  (`▼name`, amber). The legacy bar-based `proj.markers` `m` handler is now gated
  to `!arrangement_mode` so the two systems don't collide. Workflow-tested
  (add/jump/remove + undo).
- ✅ **Fase 8 — regions & cycle (DONE, incl. loop playback):** rational
  `Region{start,end,name,color}` + `cycle: Option<(start,end)>` on `Arrangement`
  with `add_region`/`region_at`/`remove_region`/`toggle_cycle` (core-tested,
  half-open spans). Undoable commands `Arrangement{AddRegion,RemoveRegion,
  ToggleCycle}`; keyboard `i` set region-in, `e` close `[in,cursor)` (auto-named),
  `E` remove region under cursor, `L` toggle cycle over the region/pending span.
  REGIONS band rendered (`[name…]` color bars; cycle span reversed with `↺`).
  **Loop playback wired**: `Scheduler::maybe_loop_arrangement` wraps the
  arrangement clock (`absolute_step`, 4 steps/beat) back to the cycle start at the
  end — only the arrangement path loops; the matrix transport (`current_step`) is
  untouched, so the two stay independent. Tested at core + scheduler + workflow.
- 🚧 **Fase 9 — track management (reorder/delete/kind/rename DONE):** core
  `Arrangement::move_track(idx,up)`/`remove_track(idx)` (clip-carrying, edge-safe,
  tested). Undoable commands `Arrangement{MoveTrack,RemoveTrack,CycleTrackKind}`;
  keyboard `K`/`J` move focused track up/down (selection follows), `T` cycle kind
  (MIDI→Audio→Drum→Group→Bus→Auto), `X` delete focused track, `r` inline rename
  (reuses the name editor, undoable), `t` create. Workflow-tested. Remaining Fase 9:
  Folder/Group **collapse/expand** — deferred to Fase 12 because hiding tracks
  needs the visible-track indirection that virtualization introduces (the `row→track`
  hit-test currently assumes contiguous rows).
- 🚧 **Fase 10 — sections (blocks + rearrangement DONE):** rational
  `Section{start,end,name,color}` on `Arrangement`; `add_section`/`section_at`/
  `remove_section`, `shift_section(idx,delta)` (moves the span + all contained
  clips, edge-safe), `duplicate_section(idx)` (copies contained clips with fresh
  ids one length later + inserts a matching section) — all core-tested. Undoable
  commands `Arrangement{AddSection,RemoveSection,ShiftSection,DuplicateSection}`;
  keyboard `S` create (`i`-anchor…`S`) / remove-under-cursor, `(`/`)` shift section
  ∓/± a bar, `D` duplicate. SECTIONS band rendered (`◖name◗` blocks). Workflow-
  tested (create/shift+clip-carry/duplicate + undo).
- ✅ **Fase 10 — overview minimap (DONE):** an OVERVIEW strip compresses the whole
  arrangement (independent of zoom/scroll) into the lane width — `overview_coverage`
  (clip-overlap density per column, unit-tested) shown as `·▃▆█` shades, section
  background tints, amber marker ticks `│`, a gray visible-window bracket `▕…▏`, and
  the cyan cursor `▮`. **Mouse click-to-navigate** jumps the cursor (cached
  `arr_overview_rect`, workflow-tested).
- ✅ **Fase 13 — docs:** `docs/architecture/arranger.md` extended with the rational
  timeline model/playback/editing + known-gaps; `docs/guide/arrangement-editor.md`
  user manual (full keyboard/mouse reference) added.
- ⬜ **Fase 7** navigation/zoom/fit, **Fase 11** UX model, **Fase 12**
  virtualization — not started (see "Remaining gaps" below).
- ✅ **Multi-select (Milestone E):** Shift+click toggles clips into `arr_selection`
  (magenta highlight); `x` deletes the whole set as one undo step. Workflow-tested.

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

---

## Remaining gaps (deferred, with rationale)

These are knowingly **not** done. Each is deferred for a concrete reason, not
forgotten — they cluster into a few real bodies of work:

| Gap | Why deferred |
|-----|--------------|
| **Timeline zoom-to-fit (Fase 7)**, **piano-roll drag-move + zoom (Milestone E)** | All hinge on the hardcoded cell width baked across the renderer and ~5 hit-test sites. Parameterizing it is a focused refactor with high regression risk; do it deliberately, not piecemeal. Keyboard move/resize already cover the operations. |
| **Folder collapse/expand (Fase 9)** + **virtualization (Fase 12)** | Both require a **visible-track indirection** (a visible→absolute row map) so hiding/culling rows doesn't break the `row→track` hit-test. They share that core change and should land together. Virtualization also needs profiling against a large project. |
| **Arrangement audio-clip playback (Phase 4)** | Needs app-side loading of each audio clip's file into an audio slot + a `clip_id→slot` map before the scheduler can trigger it. Not meaningfully testable here (no audio hardware in CI). Pattern/MIDI playback + automation are wired. |
| **Automation mouse point-editing (Milestone F)** | The automation sub-lane is one row tall, so vertical value-drag is impossible; the keyboard (`p`/`c`/`+`/`-`/`b`) is the editing path. Closed by design. |
| **Unified mouse/keyboard UX model (Fase 11)** | A cross-view consistency pass (double-click-to-open, triple-click, constrained drag) best done once the above mouse work settles. |
| **`.stz` arrangement bridge** | Separate serialization-format work in `seqterm-stz`; independent of the editor. |

Everything else in Phases 4–5 (model, timeline, clip/track/marker/region/section
editing, automation incl. realtime + dest picker, cycle loop **playback**,
overview minimap + nav, multi-select, rename, docs) is **done, undoable, and
tested**.
