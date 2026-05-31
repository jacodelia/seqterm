# Arranger

**Crate:** `seqterm-ui`  
**Module:** `views/arranger.rs`  
**Layer:** Frontend adapter

The Arranger is SeqTerm's song-composition view. It shows the project's pattern arrangement as a timeline of block clips, provides automation lane editing, and hosts the song-mode chain editor.

---

## View Layout

```
┌── Bar ruler ─────────────────────────────────────────────────────────────────┐
│  01──  05──  09──  13──  17──  21──  25──  29──  33──  37──  41──            │
├── Track lanes ───────────────────────────────────────────────────────────────┤
│ KICK01  ████████████████████████████████████████████                         │
│ BASS01          ████████████████████████████                                 │
│ LEAD01                                   ████████████████                   │
│ HIHAT   ████████████████████████████████████████████████████████             │
├── Automation lanes ──────────────────────────────────────────────────────────┤
│ BPM     ·····-·····-·····                                                    │
│ CH1.vol ∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿                                                   │
├── Song transport + Chain editor ─────────────────────────────────────────────┤
│  ► PLAY  ■ STOP  ↩ RWD  ● REC  ↺ LOOP  BPM 128.0      [Scene A: 4 bars]    │
└──────────────────────────────────────────────────────────────────────────────┘
```

The view is split into four vertical sections by `draw_arranger()`:

| Section | Height | Description |
|---------|--------|-------------|
| Bar ruler | 2 rows | Numbered bar markers, current position indicator |
| Track lanes | flexible | One row per arranger track |
| Automation lanes | 9 rows | Per-lane parameter curves |
| Song transport | 9 rows | Transport controls + chain editor |

---

## Track Lanes

Each track corresponds to a `seqterm_core::project::Track`:

```rust
pub struct Track {
    pub name: String,
    pub blocks: Vec<(u32, u32, String)>,  // (start_bar, length_bars, pattern_key)
    pub mute: bool,
}
```

`draw_track_lanes()` renders each block as a filled rectangle proportional to its bar length. Block colours reflect playback state:

- **White** — block currently playing.
- **Blue** — block active but not at playback position.
- **Gray** — muted track.

Track names are drawn on the left side with a fixed 14-character label column. Horizontal scrolling is controlled by `arranger_state.bar_offset`.

### Keyboard Navigation

| Key | Action |
|-----|--------|
| `←` / `→` | Scroll bar view (`bar_offset`) |
| `↑` / `↓` | Select track |
| `a` | Add block at cursor bar (creates a new pattern and clips it here) |
| `Delete` | Remove block under cursor |
| `m` | Toggle mute on selected track |
| `Enter` | Open the pattern in the Tracker/Piano Roll view |

---

## Automation Lanes

Automation lanes are stored in `project.automation: Vec<AutomationLane>`:

```rust
pub struct AutomationLane {
    pub name: String,
    pub target: String,  // e.g. "project.bpm", "channel.0.cc74"
    pub points: Vec<(u32, u8)>,  // (bar, value 0-127)
    pub enabled: bool,
}
```

`draw_automation_lanes()` renders each lane as a polyline connecting its automation points. Points are drawn as bright dots; lines between them represent the linear interpolation that the scheduler applies each bar.

### Target Syntax

Automation targets use a dot-path syntax:

| Target | Effect |
|--------|--------|
| `"project.bpm"` | Maps 0–127 → 20–300 BPM |
| `"channel.N.cc74"` | Sends CC 74 to MIDI output N |
| `"channel.N.send_a"` | Maps to CC 91 (reverb send) |
| `"channel.N.send_b"` | Maps to CC 92 (chorus send) |

The scheduler evaluates automation once per bar in `process_automation()`, interpolates linearly between surrounding points, and dispatches the result as a MIDI CC or BPM change.

### Editing Points

| Key | Action |
|-----|--------|
| `↑` / `↓` | Select lane |
| `a` | Add point at current bar with current value |
| `Delete` | Remove nearest point |
| `←` / `→` | Move point in time |
| `[` / `]` | Decrease / increase point value |

---

## Song Transport

`draw_song_transport()` renders the standard transport controls (Play, Stop, Rewind, Record) in the lower-left area of the Arranger.

Additionally:

- **Loop toggle** — sets `project.loop_enabled` (used by the scheduler to loop between `loop_start_bar` and `loop_end_bar`).
- **BPM display** — editable by clicking and scrolling.
- **CHAIN toggle** — activates song-mode pattern chaining (the chain editor on the right).

---

## Chain Editor

The chain editor is the right half of the song-transport section. It displays `project.chain: Vec<ChainEntry>`:

```rust
pub struct ChainEntry {
    pub scene_idx: usize,  // index into project.scenes
    pub bars: u32,         // how many bars to play this scene
}
```

Each entry is rendered as a coloured block showing the scene name and bar count. When `chain_mode` is active, the scheduler advances through entries sequentially and fires `EngineEvent::ChainAdvanced` on each transition.

### Editing

| Key | Action |
|-----|--------|
| `↑` / `↓` | Select chain entry |
| `a` | Append current scene with default 4 bars |
| `Delete` | Remove selected entry |
| `[` / `]` | Decrease / increase bar count of selected entry |
| `C` (global) | Toggle chain mode on/off |

---

## Arranger State

The `ArrangerState` struct (inside `App`) tracks:

```rust
pub struct ArrangerState {
    pub section: usize,       // 0=tracks, 1=automation, 2=transport
    pub track_cursor: usize,  // selected track
    pub lane_cursor: usize,   // selected automation lane
    pub bar_cursor: u32,      // current bar for block operations
    pub bar_offset: u32,      // horizontal scroll offset
}
```

---

## Mouse Support

| Area | Click behaviour |
|------|-----------------|
| Bar ruler | Set `bar_cursor` to clicked bar |
| Track lane block | Select track + set `bar_cursor` |
| Transport buttons | Same as keyboard shortcuts |
| Chain entry | Select entry |
| Automation point | Select lane; scroll adjusts value |

---

## Relationship to the Matrix

The Arranger and Matrix views share the same underlying data (`project.matrix`, `project.tracks`, `project.patterns`). The Matrix is the **live performance** view (clip launching, step editing); the Arranger is the **composition** view (linear arrangement, automation, song structure). They can be used together: patterns built in the Matrix appear as blocks in the Arranger, and changes made in either view are immediately reflected in the other.
