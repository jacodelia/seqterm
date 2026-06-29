# SeqTerm Roadmap

Phased TODO of the **remaining** work, focused on the SONG (Arrangement) view
plus the adjacent gaps it depends on. Each phase is independently shippable,
ordered by value/risk. Check items off as they land.

Conventions: `core` = `seqterm-core`, `ui` = `seqterm-ui`, `stz` = `seqterm-stz`,
`engine` = `seqterm-engine`/`seqterm-audio-engine`.

Status legend: ⬜ todo · 🟡 partial · ✅ done.

---

## Already done (context — do not redo)

SONG has: rational `Arrangement` model (tracks/lanes/clips/markers/regions/
sections), timeline + overview minimap, clip move/trim/split/duplicate, audio-clip
waveforms, multi-select, undo, automation lanes (4 curves) + realtime CC,
pattern/MIDI scheduler playback, track reorder/rename/delete/kind, and (latest)
Fase 7 nav/zoom/fit (`Home`/`End`/`PageUp`/`PageDown`/`Z`/`Shift+F`), clip
clipboard (`Ctrl+C/V/X/D`), clip length-stretch (`{`/`}`), double-click → open
pattern editor.

---

## Phase A — `.stz` arrangement persistence  🟡

**Finding:** lossless persistence **already works**. `Project.arrangement`
(`core/src/project.rs:259`) is serde, and `bridge.rs:118` writes the whole
`Project` to `core_project_json` inside the `.stz` (read back at `bridge.rs:249`).
So clips/markers/regions/sections/automation already round-trip when SeqTerm saves.

Remaining (optional, low priority):

- [ ] **Lock it in:** a round-trip test in `stz` (or `ui`) — build an arrangement,
      `save` → `load`, assert clips/markers/sections/automation identical. Guards
      against future regressions.
- [ ] **Structured interchange view (optional):** also emit `arrangement/*.json`
      entries in `stz.rs` so *foreign* tools / the spec view can read the
      arrangement (the structured `objects/**` view today omits it). Not needed for
      SeqTerm itself.

**Files:** `stz/src/{stz,bridge}.rs` (interchange only), test anywhere.
**Risk:** low — core path unchanged; only adds a test (+ optional extra entries).

---

## Phase B — Arrangement audio-clip playback  ⬜

**Goal:** `ClipKind::Audio` clips actually sound during SONG playback (today
pattern/MIDI clips play; audio clips are silent).

- [ ] On entering SONG playback (or on load), load each audio clip's file into an
      audio engine slot; build a `clip_id → slot_id` map on `App`.
- [ ] Scheduler triggers the slot at the clip's `start`, honoring `content_offset`,
      `length`, `loop_enabled`, and `gain`.
- [ ] Release slots when playback stops / clips change.
- [ ] Manual verify (no audio in CI): place an audio clip, play SONG, hear it.

**Files:** `ui/src/lib.rs` (slot loading), `engine/src/scheduler.rs` (`playback_hits`
already returns audio hits — wire to `AudioCommand`).
**Risk:** medium — not CI-testable; gate behind a feature/manual check.
**Depends on:** nothing (independent of A).

---

## Phase C — Track types: Hybrid + Folder, with collapse  ⬜

**Goal:** complete the track-kind set (Audio / Instrument / Hybrid / Folder /
Group) and let Folder/Group tracks collapse/expand to hide their children.

- [ ] Add `Hybrid` (audio+MIDI) and `Folder` variants to `TrackKind`
      (`core/src/project.rs`); update every exhaustive `match` (`short_label`,
      scheduler routing, `T`-cycle in the arranger).
- [ ] **Visible→track indirection**: a `visible_rows: Vec<usize>` (absolute track
      indices) computed each frame, honoring folder collapse state. ALL render +
      hit-test sites use this instead of raw indices.
- [ ] Folder collapse/expand toggle (e.g. `Enter`/click on the folder row).
- [ ] Drag/assign child tracks into a folder (or auto by adjacency).

**Files:** `core/src/{project,arrangement}.rs`, `ui/src/views/arranger.rs`,
`ui/src/lib.rs` (hit-tests).
**Risk:** HIGH — the indirection is the shared prerequisite for Phase D. Do it
here, deliberately. High regression risk in mouse hit-testing.
**Depends on:** none, but **blocks** Phase D.

---

## Phase D — Scalability / virtualization  ⬜

**Goal:** hundreds of tracks / thousands of clips without per-frame full re-render.

- [ ] Virtualized track rendering: only build rows for the visible window (reuses
      the `visible_rows` indirection from Phase C).
- [ ] Clip culling: skip clips fully outside the visible beat range.
- [ ] Avoid recomputing waveforms every frame (cache per clip; invalidate on edit).
- [ ] Profile against a synthetic large project (e.g. 200 tracks × 2k clips).

**Files:** `ui/src/views/arranger.rs`, waveform cache on `App`.
**Risk:** medium — correctness of the visible window; needs profiling.
**Depends on:** Phase C (shares `visible_rows`).

---

## Phase E — Automation mouse point-editing  ⬜

**Goal:** edit automation breakpoints with the mouse (today keyboard-only:
`p`/`c`/`+`/`-`/`b`).

- [ ] Make the automation sub-lane ≥ 3 rows tall when focused so vertical
      value-drag is meaningful.
- [ ] Click = add/select point; drag = move (beat + value); right-click = delete;
      cycle curve via existing `b`/`B` or a click target.
- [ ] Snap horizontal to grid; clamp value 0..1.

**Files:** `ui/src/views/arranger.rs` (taller lane + point rects), `ui/src/lib.rs`
(mouse). 
**Risk:** low–medium.
**Depends on:** none.

---

## Phase F — Real audio time-stretch (DSP)  ⬜

**Goal:** pitch-preserving time-stretch for audio clips (today `{`/`}` only change
clip length and loop the source — no true stretch).

- [ ] Implement/integrate a stretch algorithm (WSOLA or phase-vocoder) in the
      audio engine; expose a `stretch_ratio` per audio clip.
- [ ] UI: a stretch handle/gesture mapping clip-length change to ratio when the
      clip is audio (vs. loop for pattern/MIDI).
- [ ] Quality pass on transients.

**Files:** `audio-engine/src/*` (DSP), `core/src/arrangement.rs` (`stretch_ratio`
field), `ui` gesture.
**Risk:** HIGH — real DSP, not CI-verifiable; largest standalone effort.
**Depends on:** Phase B (audio clips must play first).

---

## Phase G — Piano-roll mouse parity (adjacent, PATTERN view)  ⬜

Not SONG, but the same deferred "hardcoded cell-width" refactor; tracked here so
it isn't forgotten.

- [ ] Parameterize the piano-roll cell width across the renderer + ~5 hit-test
      sites.
- [ ] Drag-move notes, resize handles + ghost preview, vertical/horizontal zoom.

**Files:** `ui/src/views/tracker.rs`, `ui/src/lib.rs`.
**Risk:** medium — focused refactor, high regression risk in hit-tests.

---

## Suggested order

A (persistence — already lossless; just add the guard test, quick) → B (audio playback) → C (track types +
indirection) → D (virtualization, needs C) → E (automation mouse) → F (DSP
stretch, needs B) → G (piano-roll, independent).

Do **one phase per change**, keep the build + tests green, commit per phase.
