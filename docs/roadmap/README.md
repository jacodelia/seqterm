# SeqTerm Roadmap

> **Current status & all pending issues:** see **[STATUS.md](STATUS.md)** —
> Phases 1–2 done, Phase 3 partial, Phases 4–5 not started (as of 2026-06-12).

This roadmap drives three feature specifications into SeqTerm:

- **`01_patternUpdate.md`** — a rational Time-Resolution System for Pattern / Tracker / Piano-Roll editing (arbitrary subdivisions, tuplets, polyrhythms, graphical note resize, free-time editing).
- **`02_songUpdate.md`** — turn the SONG view into a professional timeline **Arrangement Editor** (clips, editing tools, automation lanes, markers, track types, virtualization) — **without** absorbing any mixer responsibilities.
- **`03_matrixUpdate.md`** — Matrix-View **Copy/Paste between patterns** + a complete session **Undo/Redo** system (no audio-engine changes).

## Baseline (already shipped)

The new roadmap starts on top of a complete foundation: hexagonal core, realtime-safe 32-slot audio engine, SF2 (oxisynth/FluidSynth) + SF2 preset editor, granular engine, sampler, 26 FX, MIDI I/O + MPE, OSC, persistence (JSON/MessagePack/`.stz`), plugin hosting (VST2 + **CLAP** with polyphonic expression and state persistence), system-wide undo/redo (`seqterm-history`: `EditCommand` + `ProjectSnapshot` + `App::record_edit`), and universal MIDI-Learn. The phases below build on these.

## Phase ordering & rationale

| Phase | Spec | Theme | Why this order |
|-------|------|-------|----------------|
| **1** | 03 | Matrix Copy/Paste + Session Undo/Redo | Smallest, self-contained, **no engine changes**. Hardens the undo/redo + selection foundation that Phases 2–5 reuse. Fastest user-visible win. |
| **2** | 01 | Rational Time — Core | Foundational engine change (rational time model + migration). Must land before precise editing (P3) and accurate clip timing (P4–P5). |
| **3** | 01 | Rational Editing — Tracker / Pattern / Piano-Roll | The editing UX (snap, resize, tuplets, free-time) built on the P2 core. |
| **4** | 02 | Arrangement Editor — Foundation | New SONG data model, layout, clip system, editing tools, automation lanes. Benefits from P2's rational time for clip positions. |
| **5** | 02 | Arrangement Editor — Pro Workflow & Scale | Navigation, markers/regions, track types, sections, UX polish, virtualization, integration. |

Phases 1 → 2 → 3 are strictly sequential (each depends on the prior). Phase 4 may begin its data-model/layout work in parallel with Phase 3 but should consume the Phase 2 rational-time API for clip timing. Phase 5 follows Phase 4.

## Cross-cutting principles

- **No data loss on migration.** Existing projects load unchanged; new fields are `#[serde(default)]`; rational time defaults to `1/16` and converts old step/`micro` data losslessly.
- **Undo/redo everywhere.** Every new mutation is a reversible `EditCommand` (typed where cheap, `ProjectSnapshot` otherwise) with a human-readable description shown in the status bar.
- **Keyboard- and mouse-complete.** Every operation is reachable from both; shortcuts are configurable via the keybindings system.
- **Mixer stays separate.** The Arrangement Editor never embeds mixer controls (`02_songUpdate` is explicit on this).
- **Realtime contract preserved.** No allocation/locks added to the audio callback; the scheduler consumes rational time without per-frame heap work.

See each `phase-N.md` for goals, current state, work breakdown, data-model changes, affected crates, migration, tests, risks, and exit criteria.
