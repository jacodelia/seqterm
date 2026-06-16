# Phase 4 — Arrangement Editor: Foundation

**Spec:** `02_songUpdate.md` (Fases 1–6) · **Status:** 🚧 Started (data model + migration done 2026-06-12) · **Constraint:** no mixer functionality in this view

> **Progress note (2026-06-12).** Fase 1 (audit) and Fase 2 (data model) are
> done: `seqterm-core/arrangement.rs` provides the rational `Arrangement →
> ArrangementTrack → Lane → Clip` model with stable clip ids, `ClipKind`
> (Pattern/Audio/MIDI), and clip ops (`split_at`, `trim_start`/`trim_end`,
> `shift`, `overlaps`, `contains`). `Project.arrangement` is additive
> (`#[serde(default)]`, schema v3); legacy bar-block `tracks` migrate forward
> losslessly on load (`migrate_legacy_arrangement`). 12 new tests. **Remaining:**
> the pro 3-pane UI (Fase 3), clip/track editing tools wired to these ops (Fase
> 4–5), inline automation (Fase 6), scheduler playback of arrangement clips, and
> the `.stz` bridge extension.

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

### Fase 1 — Audit (deliverable: written report) — DONE

**Current SONG architecture.**
- *Data:* `Project.tracks: Vec<Track>`; `Track { name, blocks: Vec<(start_bar:u32, length_bars:u32, label:String)>, mute }`. Clips are **bar-granular tuples** referencing a pattern by `label` (string). Separately: `scenes: Vec<Scene>` (per-row active pattern + mute/fx masks) and `chain: Vec<ChainEntry{scene_idx,bars}>` for song-mode playback. Automation lives in project-level `automation: Vec<AutomationLane{target, points:(bar,u8)}>`.
- *Playback:* the scheduler plays the **matrix** clips (per-pattern polymeter) and follows the `chain` (scene advance every N bars). The arranger `blocks` are **not** independently scheduled — playback is matrix/scene/chain driven, not timeline-clip driven.
- *Time:* everything arranger-side is **integer bars** (`u32`); no sub-bar precision; zoom is coarse (bars). Rational time (Phase 2) is not used here yet.
- *Rendering/selection/zoom:* `arranger.rs` renders tracks×bars with a loop region, basic markers, and a coarse horizontal scroll; selection is a single (track,col) cursor; there is no clip-level move/trim/split.

**Limitations / risks.**
- No clip identity (clips are positional tuples) → no stable selection, no per-clip undo, no shared/instanced references.
- Bar-only timing → can't place or trim clips at beat/rational positions; can't honor Phase 2 exact time.
- Only one clip kind (pattern label); no audio/MIDI clips, no trim/loop/content-offset, no per-clip color/mute.
- Automation is project-global and bar-quantized; not inline per-track, no curves/bézier.

**Reuse vs. refactor.** Reuse: `TrackKind` enum, `AutomationLane` (extend), the matrix/scene/chain playback for the *session* view (kept separate). Refactor: introduce a parallel **`Arrangement`** model (rational clips with identity) alongside the legacy `tracks/blocks` so existing projects keep working; migrate `blocks → clips` losslessly. New model is **additive** (`#[serde(default)]`).

### Fase 2 — Conceptual model — DONE (core)
- [x] `Arrangement`, `ArrangementTrack`, `Lane`, `Clip`, `ClipKind{Pattern,Audio,Midi}` in `seqterm-core/arrangement.rs`; clip positions/lengths in `RationalTime`; stable monotonic clip ids (`alloc_id`); clip ops `split_at`/`trim_start`/`trim_end`/`shift`/`overlaps`/`contains`; `Lane::clip_at`/`sort`; `Arrangement::clip_mut`/`length_beats`.
- [x] `Project.arrangement` field (`#[serde(default)]`, schema **v3**); `from_legacy_tracks` + `Project::migrate_legacy_arrangement` populate it from legacy bar-blocks on load (lossless, idempotent). Legacy `tracks` preserved.
- [ ] `.stz` bridge extension for the arrangement model remains.

### Fase 3 — Pro layout (timeline + inline inspector DONE; header polish remains)
- [~] **Global header**: the existing song-transport row + bar ruler provide
  transport/BPM/loop/markers; a dedicated time-signature + record-state indicator
  block is not yet broken out.
- [x] **Track Inspector** (inline, mixer-free): each timeline row shows the kind
  badge, name, and **arm / solo / mute / monitor** flags (`A`/`S`/`M`/`I`),
  toggled with `a`/`o`/`u`/`y` (undoable). A dedicated wide left pane is not
  needed at this density. **No mixer controls** (volume/EQ/sends stay in Mixer).
- [x] **Main timeline (rational)**: `draw_arrangement_timeline` renders the
  `Arrangement` model — one row per track, clips as colored bars positioned by
  exact rational `[start,end)` on the bar grid, a per-kind leading glyph
  (`▏`pattern / `≈`audio / `♪`midi), the cursor clip highlighted, a `╎`
  beat-cursor (insertion point) marker, and a per-clip readout in the title.
  Toggled with **`g`** (additive; legacy matrix view kept).

### Fase 4 — Clip system (creation + distinct rendering + waveforms DONE)
- [x] **Pattern clips**: created via the pattern picker — `n` on the timeline
  opens `PatternPicker` (targeted at the arrangement) and places a clip whose
  length = the referenced pattern's bar-length. Duplicating (`d`) keeps the same
  `pattern_key`, so the instances are **shared references** (editing the source
  pattern updates every instance — non-destructive reuse, inherent in the model).
- [x] **Audio clips (Milestone C)**: `A` opens the file picker
  (`AssignAudioToArrangement`) → `ConfirmArrangementAudioClip` creates a
  `ClipKind::Audio` clip whose **length is derived from the file duration at the
  project BPM** (undoable, serialized). A background scan fills `waveform_cache`.
- [x] **Waveform rendering**: audio clips render a peak waveform
  (`▁▂▃▄▅▆▇█` from `waveform_cache`) across their columns; the periodic scan now
  includes arrangement audio clips.
- [x] **MIDI / pattern visualization**: pattern & MIDI clips render a **note-
  density preview** (`•` per event onset of the referenced pattern) after the name.
- [x] **Distinct kind rendering**: pattern (`▏`), audio (`≈`), MIDI (`♪`) leading
  glyphs; per-clip palette color; muted clips dimmed.
- [ ] **Remaining**: scheduler **playback of arrangement audio clips** (creation
  + display done; triggering the file's audio slot at the clip start is not wired —
  `playback_hits` covers pattern/MIDI clips only). Inline MIDI-event editing too.

### Fase 5 — Editing tools (core + command layer DONE; UI wiring remains)
- [x] **Core ops** (tested): `Clip::split_at`/`trim_start`/`trim_end`/`shift`; `Arrangement::add_clip`/`delete_clip`/`duplicate_clip`/`split_clip`/`clip_mut`.
- [x] **Command layer + undoable handlers**: `ArrangementAddTrack/AddClip/MoveClip/SplitClip/DuplicateClip/DeleteClip/TrimClip` route through `record_edit`.
- [x] **Active-clip query**: `Arrangement::clips_active_at(beat) -> Vec<ClipHit>` (track/lane/clip + resolved local source position) — the query the scheduler and timeline UI consume.
- [x] **UI wiring (keyboard + mouse)**: in `arrangement_mode` a beat cursor +
  clip cursor drive everything — `h`/`l` move the beat cursor ∓1 bar (auto-select
  the clip under it), `j`/`k` switch tracks, `n` new clip, `t` new track, `d`
  duplicate, `s` split at the beat cursor (else midpoint), `[`/`]` trim start/end
  to the cursor, `x`/`Del` delete, `,`/`.` move ±1 beat, `a`/`o`/`u`/`y` toggle
  arm/solo/mute/monitor. A **left click** on the timeline focuses the track, moves
  the beat cursor (rounded to the beat) and selects the clip under it. All edits
  route through the undoable `Arrangement*` commands; navigation uses tested core
  helpers (`track_clip_ids`/`neighbor_clip`/`nearest_clip_on_track`/`clip_at_on_track`/
  `first_clip_on_track`/`locate_clip`/`clip`).
- [x] **Mouse editing (Milestone E)**: press-drag **moves** the clip under the
  cursor (snapped to 1/4 beat), **`Alt+Drag` duplicates** then drags the copy;
  each drag is one undo step (arrangement gesture snapshot). Driven through the
  real mouse dispatcher and covered by headless-harness tests.
- [ ] **Remaining**: rectangular/multi-select, mouse resize-handles + ghost
  preview, time-stretch (keyboard `[`/`]` trim already exists).

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
- `crates/seqterm-engine/src/scheduler.rs` — arrangement playback **DONE**
  (`fire_arrangement_clips` + `source_row` routing + `SetArrangementPlayback`).
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
