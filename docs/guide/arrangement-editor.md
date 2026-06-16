# Arrangement Editor — User Guide

The Arrangement Editor is SeqTerm's song-composition view: a rational-time
timeline where you place pattern/audio clips on tracks, route them to
instruments, and shape the whole song with automation, markers, regions,
sections, and a cycle loop.

Open the **Arranger** view, then press **`g`** to switch into the rational
timeline (press `g` again to return to the legacy bar-block view). Everything
below is undoable with **Ctrl+Z** / redo with **Ctrl+Y**.

## Concepts

- **Track** — a row routed to a matrix row's instrument (`source_row`). Unrouted
  tracks are dimmed and silent.
- **Clip** — a Pattern, Audio, or MIDI block at an exact beat position/length.
- **Beat cursor** — the insertion point (cyan `╎`); new clips/markers land here.
- **Cycle** — a loop span; when set, playback loops the arrangement over it.

## Keyboard reference (rational timeline)

### Navigate
| Key | Action |
|-----|--------|
| `h` / `l` | Move the beat cursor ∓/± one bar |
| `j` / `k` | Focus the previous / next track |
| `<` / `>` | Jump the cursor to the previous / next **marker** |

### Clips
| Key | Action |
|-----|--------|
| `n` | New clip from a pattern (picker) at the cursor |
| `A` | New **audio** clip from a file at the cursor |
| `d` | Duplicate the cursor clip |
| `s` | Split the cursor clip at the beat cursor |
| `[` / `]` | Trim the cursor clip's start / end to the cursor |
| `,` / `.` | Nudge the cursor clip ∓/± one beat |
| `x` / `Del` | Delete the selection (or the cursor clip) |

### Tracks
| Key | Action |
|-----|--------|
| `t` | Add a track |
| `T` | Cycle the track's kind (MIDI→Audio→Drum→Group→Bus→Auto) |
| `K` / `J` | Move the focused track up / down |
| `X` | Delete the focused track |
| `r` | Rename the focused track (type, Enter to confirm, Esc to cancel) |
| `R` | Cycle the track's instrument route (matrix row A–H / off) |
| `a` / `o` / `u` / `y` | Toggle arm / solo / mute / monitor |

### Playback
| Key | Action |
|-----|--------|
| `P` | Toggle arrangement playback (then `Space` to run transport) |

### Automation
| Key | Action |
|-----|--------|
| `V` | Show/hide the automation sub-lane on the focused track |
| `b` / `B` | Pick the destination (volume/pan/cutoff/resonance/reverb/chorus) |
| `+` / `-` | Raise / lower the value cursor |
| `p` | Set a breakpoint at the beat cursor |
| `c` | Remove the nearest breakpoint |

### Markers, regions & cycle
| Key | Action |
|-----|--------|
| `m` | Add a marker at the cursor (auto-named Intro/Verse/Chorus/…) |
| `M` | Remove the nearest marker |
| `i` | Set a region/section/cycle **start** at the cursor |
| `e` | Close a **region** `[start, cursor)` |
| `E` | Remove the region under the cursor |
| `L` | Toggle the **cycle** (loop) over the region/pending span |

### Sections
| Key | Action |
|-----|--------|
| `S` | Create a section `[i-start, cursor)`, or remove the one under the cursor |
| `(` / `)` | Shift the section (and its clips) ∓/± one bar |
| `D` | Duplicate the section (clips + marker) |

## Mouse

| Gesture | Action |
|---------|--------|
| Click a clip | Select it (sets track + beat cursor) |
| Drag a clip | Move it (snapped to 1/4 beat); one undo step |
| Alt+Drag | Duplicate the clip and drag the copy |
| Shift+click clips | Add/remove clips from the multi-selection (`x` deletes all) |
| Click the **OVERVIEW** strip | Jump the beat cursor to that position |

## Ruler rows

Above and below the track lanes the timeline shows:

- **MARKERS** — `▼name` at each marker.
- **REGIONS** — `[name…]` color bars; the cycle span is reversed with `↺`.
- **SECTIONS** — `◖name◗` blocks grouping clips.
- **OVERVIEW** — a minimap of the whole song (clip density, section tints, marker
  ticks, a bracket showing the visible window, and the cursor) — click to navigate.

## Tips

- Route a track (`R`) before expecting it to play; unrouted tracks are silent.
- Automation sends MIDI CC to the routed instrument only when the value changes.
- A cycle set with `L` loops the arrangement clock independently of the matrix.

See `docs/architecture/arranger.md` for the data model and internals, and
`docs/roadmap/STATUS.md` for the current feature status and known gaps.
