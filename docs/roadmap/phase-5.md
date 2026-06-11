# Phase 5 — Arrangement Editor: Pro Workflow & Scale

**Spec:** `02_songUpdate.md` (Fases 7–13) · **Status:** Planned · **Depends on:** Phase 4

Complete the Arrangement Editor with professional navigation, markers/regions, advanced track types, arranger sections, UX consistency, and the performance work needed for large projects — plus full integration with the rest of SeqTerm. The Mixer remains a separate view throughout.

---

## Goals

- Pro navigation: horizontal/vertical zoom, scroll, zoom-to-selection, fit project/track.
- Markers, named/colored regions, and cycle (loop) regions.
- Track types: Audio, Instrument, Hybrid, **Folder**, **Group**.
- Arranger sections (Intro/Verse/Chorus/Bridge/Outro) as movable blocks, rearrangement, and an overview minimap.
- Consistent mouse/keyboard interaction model.
- Scalability for hundreds of tracks / thousands of clips via virtualization and incremental redraw.
- Integration: SONG consumes content from sampler, audio editor, granular, sequencer, instruments — without taking on mixing.

---

## Current state (post Phase 4)

- Timeline, track inspector, clip system, editing tools, and inline automation exist (Phase 4).
- Zoom/scroll are functional but not yet "fit/zoom-to-selection"; no marker/region/cycle system; only basic track kinds; no sections/overview; rendering redraws broadly.

---

## Work breakdown

### Fase 7 — Advanced navigation
- [ ] Zoom: horizontal (`Ctrl`+wheel), vertical (`Alt`+wheel); smooth scroll (H/V, optional inertia).
- [ ] `Z` zoom-to-selection; `Shift+F` fit project; `F` fit track.

### Fase 8 — Markers & regions
- [ ] Markers (Intro/Verse/Chorus/Bridge/Outro) on the ruler.
- [ ] Regions with start/end, color, name.
- [ ] Cycle regions for repeated playback (loop).

### Fase 9 — Track management
- [ ] Track kinds: Audio (audio clips), Instrument (MIDI events), Hybrid (audio+MIDI), Folder (visual grouping), Group (logical organization).
- [ ] Create/delete/rename/reorder; collapse/expand folders.

### Fase 10 — Advanced arranger
- [ ] **Sections** as visual blocks (Intro/Verse/Chorus/Bridge/Outro).
- [ ] **Rearrangement**: move/duplicate/reorder whole sections.
- [ ] **Arrangement Overview**: global minimap with quick navigation.

### Fase 11 — Critical UX
- [ ] Mouse: click = select; double-click = open associated editor; triple-click = select whole clip; drag = move / rectangular select; `Alt+Drag` = duplicate; `Shift+Drag` = constrained move.
- [ ] Keyboard — transport: `Space` play/stop, `Enter` return to start, `R` record. Navigation: `Home`/`End` project start/end, `PageUp`/`PageDown` zoom in/out. Edit: `Ctrl+C/V/X`, `Delete`, `Ctrl+D` duplicate, `S` split, `Z` zoom-selection.

### Fase 12 — Scalability
- [ ] Virtualize tracks/clips (render only what's visible); waveform cache; incremental/dirty-region redraw.
- [ ] Avoid full-timeline re-render, redundant waveform recompute, and per-frame heavy work.
- [ ] Validate with hundreds of tracks / thousands of clips.

### Fase 13 — Integration
- [ ] SONG as project center, consuming sampler / audio-editor / granular / sequencer / instrument content; clear flow Editor → Pattern → Song → Mixer → Master, with no mixing responsibilities in SONG.

### Documentation
- [ ] Technical documentation of the arrangement architecture and data model.
- [ ] User manual for the Arrangement Editor (tools, shortcuts, workflows).

---

## Affected crates / files

- `crates/seqterm-ui/src/views/arranger.rs`, `app.rs`, `lib.rs`, `widgets/` (navigation, markers, sections, overview, virtualization).
- `crates/seqterm-core` (track kinds, sections, regions, cycle).
- `crates/seqterm-command` (navigation, marker/region, section commands).
- `docs/architecture/arranger.md` (rewrite) + a new user-manual doc.

## Tests

- [ ] Zoom/scroll/fit/zoom-to-selection bounds and stability.
- [ ] Marker/region/cycle create/edit/playback.
- [ ] Folder/group collapse/expand; track reorder.
- [ ] Section move/duplicate/reorder integrity.
- [ ] Large-project rendering performance (virtualization correctness; no dropped clips).
- [ ] Undo/redo for all new operations.

## Risks

- Virtualization correctness (off-by-one in visible-range culling) — cover with tests over scroll/zoom extremes.
- UX consistency across mouse gestures — centralize the interaction model so SONG matches the rest of the app.

## Exit criteria

The Arrangement Editor offers pro navigation, markers/regions/cycle, all five track types, sections + overview, a consistent mouse/keyboard model, and scales to large projects via virtualization — fully integrated with the rest of SeqTerm while keeping the Mixer separate; technical docs and a user manual ship; tests green.
