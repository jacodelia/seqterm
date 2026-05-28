use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use seqterm_command::HelpTopic;
use crate::modal::HelpState;

const BG: Color     = Color::Rgb(22, 27, 34);
const BORDER: Color = Color::Rgb(48, 54, 61);
const HEADER: Color = Color::Rgb(240, 136, 62);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const SEL: Color    = Color::Rgb(56, 139, 253);

pub fn draw_help(f: &mut Frame, state: &mut HelpState, area: Rect) {
    // Outer block.
    let block = Block::default()
        .title(" HELP ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(10)])
        .split(inner);

    draw_sidebar(f, state, chunks[0]);
    draw_content(f, state, chunks[1]);
}

fn draw_sidebar(f: &mut Frame, state: &mut HelpState, area: Rect) {
    let topics = HelpTopic::all();
    let items: Vec<ListItem> = topics
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let selected = i == state.sidebar_cursor;
            let style = if selected {
                Style::default().fg(Color::Black).bg(SEL).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(
                format!("  {}", t.label()),
                style,
            )))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(BG)),
    );
    f.render_widget(list, area);
}

fn draw_content(f: &mut Frame, state: &mut HelpState, area: Rect) {
    let content = topic_content(&state.topic);
    let lines: Vec<Line> = content
        .lines()
        .skip(state.scroll)
        .map(|l| {
            if l.starts_with("##") {
                Line::from(Span::styled(
                    l.trim_start_matches('#').trim().to_string(),
                    Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
                ))
            } else if l.starts_with('#') {
                Line::from(Span::styled(
                    l.trim_start_matches('#').trim().to_string(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ))
            } else if l.starts_with("  ") && l.contains('=') {
                // Key binding line: "  key = description"
                let parts: Vec<&str> = l.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Line::from(vec![
                        Span::styled(
                            format!("{:>18}  ", parts[0].trim()),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(parts[1].trim(), Style::default().fg(Color::White)),
                    ])
                } else {
                    Line::from(Span::styled(l, Style::default().fg(Color::White)))
                }
            } else {
                Line::from(Span::styled(l, Style::default().fg(Color::White)))
            }
        })
        .collect();

    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .style(Style::default().bg(BG))
                .borders(Borders::NONE),
        );
    f.render_widget(p, area);

    // Scroll hint.
    let hint = Span::styled(
        " ↑↓/PgUp/PgDn=scroll  ←→=topic  Esc=close ",
        Style::default().fg(BORDER),
    );
    if area.height > 1 {
        let hint_area = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
        f.render_widget(Paragraph::new(Line::from(hint)), hint_area);
    }
}

// ─── Static topic content ─────────────────────────────────────────────────────

fn topic_content(topic: &HelpTopic) -> &'static str {
    match topic {
        HelpTopic::KeyboardShortcuts   => KEYS,
        HelpTopic::WorkflowGuide       => WORKFLOW,
        HelpTopic::MidiImport          => MIDI_IMPORT,
        HelpTopic::Routing             => ROUTING,
        HelpTopic::PatternEditor       => PATTERN_EDITOR,
        HelpTopic::Troubleshooting     => TROUBLESHOOTING,
        HelpTopic::LatencyOptimization => LATENCY,
    }
}

static KEYS: &str = "\
# Keyboard Shortcuts

## Global
  q               = Quit (outside edit mode)
  Ctrl+Q          = Quit (always)
  Space           = Play / Stop
  s               = Stop
  r               = Record toggle
  + / =           = BPM +1
  -               = BPM -1
  Tab             = Next subsection / next view
  1-5             = Switch view (Matrix/Tracker/Arranger/Mixer/Config)
  Alt+F           = Open FILE menu
  Alt+E           = Open EDIT menu
  Alt+A           = Open ABOUT menu
  Alt+H           = Open HELP menu
  F1              = Keyboard shortcuts help
  F12             = About dialog
  Ctrl+N          = New project
  Ctrl+O          = Open project
  Ctrl+S          = Save project
  Ctrl+I          = Import MIDI
  Ctrl+E          = Export MIDI
  Ctrl+Z          = Undo
  Ctrl+Y          = Redo

## Matrix view (1)
  h/j/k/l         = Navigate grid (←↓↑→)
  Enter           = Open pattern in Tracker
  e               = Enable / disable clip
  Del/Backspace   = Remove clip from slot
  Tab             = Cycle: Grid → Transport → Polymeter → Routing

## Tracker view (2)
  h/j/k/l         = Navigate steps / columns
  Enter           = Edit mode (adjust with ↑↓)
  Esc             = Exit edit mode
  [ / ]           = Decrease / increase pattern length
  d               = Toggle piano roll draw/select mode
  Tab             = Cycle: Step Table → Piano Roll → Generative → Modulation

## Piano Roll (in Tracker)
  L-click         = Place note
  R-click         = Erase note
  L-drag          = Extend gate duration
  R-drag          = Paint-erase notes
  ↑↓              = Move note row
  ←→              = Move step

## Arranger view (3)
  ↑↓              = Select track
  ←→              = Scroll bars
  Space           = Song play / stop
  Enter           = Edit track name / trigger transport
  Tab             = Cycle: Tracks → Automation → Song Transport

## Mixer view (4)
  ←→              = Select channel
  ↑↓              = Adjust volume
  m               = Mute / unmute
  Enter           = Enter edit mode
  Tab             = Switch to Automation section

## Config view (5)
  h/l             = Navigate panels
  ↑↓              = Navigate items
  e               = Enable / disable item
";

static WORKFLOW: &str = "\
# Workflow Guide

## Basic Session Setup
1. Start SeqTerm — the demo project loads automatically.
2. Press Space to start playback.
3. Navigate to the Matrix view (key 1) to see your clips.
4. Press Enter on any clip to open the Pattern Editor (Tracker view).

## Creating Patterns
1. In Matrix view, move the cursor to an empty slot and press Enter.
   A new pattern is created and the Tracker opens.
2. Navigate steps with hjkl (or arrow keys).
3. Press Enter to enter edit mode, then ↑↓ to adjust the note value.
4. Press Esc to return to navigate mode.
5. Use Tab to switch between Step Table, Piano Roll, Generative Engine,
   and Track Modulation panels.

## Piano Roll Editing
1. Tab to the Piano Roll panel in Tracker view.
2. Left-click to place notes; right-click to erase.
3. Left-drag to extend note gate length.
4. Scroll wheel to zoom note rows.

## Generative Engine
Each pattern has a built-in generative engine:
- SWING (50-80%): adds rhythmic feel
- PROB (0-100%): probability of triggering the full pattern
- RANDOM (0-100%): randomizes step activation
- EUCLID: generates Euclidean rhythms
- HUMANIZE: adds subtle velocity variation

## MIDI Routing
1. In Matrix view, Tab to the Routing panel.
2. Press ↑↓ to select a MIDI output port.
3. Press Enter to assign the selected port to the current clip.
4. SeqTerm creates one virtual ALSA port per pattern — visible in Carla.

## Saving Your Work
- Ctrl+S saves to the current file.
- Ctrl+Shift+S opens Save As dialog.
- The project auto-saves every 60 seconds.
- An asterisk (*) in the title bar indicates unsaved changes.
";

static MIDI_IMPORT: &str = "\
# MIDI Import Guide

## Supported Formats
- SMF Type 0 (single track)
- SMF Type 1 (multi-track, most common)
- Standard .mid and .midi extensions

## Import Steps
1. Press Ctrl+I or use FILE → Import MIDI…
2. Navigate to your MIDI file in the file picker.
3. Press Enter to begin import.

## What Gets Imported
- Each MIDI track becomes a matrix row (A, B, C…)
- Long tracks are split into pattern slices (default: 4 bars each)
- Note pitches, velocities, and gate lengths are preserved
- CC01 (modulation) and CC74 (filter) are imported per step
- The first tempo event sets the project BPM

## Pattern Naming
- MIDI track names become pattern names (uppercase, 8 chars max)
- Unnamed tracks are named TRK01, TRK02, etc.
- Multi-slice patterns: TRK0101, TRK0102 (track + slice number)

## After Import
- All imported patterns appear in the matrix
- Open each pattern in the Tracker to edit
- Assign MIDI output ports in the Routing panel
- The original MIDI file is added to Recent MIDI Imports

## Tips
- For best results, export from your DAW at 480 PPQ
- Quantize to 16th notes before exporting for clean step alignment
- Drum tracks (channel 10) are automatically detected
";

static ROUTING: &str = "\
# Routing Guide

## Virtual MIDI Ports
SeqTerm creates one ALSA virtual MIDI output port per pattern.
These ports appear in Carla, QjackCtl, and other patchbay tools
under the pattern's name (e.g. 'KCK01', 'BASS1').

## Auto-Patching
On startup, SeqTerm automatically runs aconnect to wire each clip's
pattern port to its configured MIDI destination.

Requirements:
- alsa-utils must be installed (for aconnect)
- Destination synth must be running before SeqTerm starts

## Manual Routing in SeqTerm
1. Go to Matrix view (key 1).
2. Tab to the ROUTING panel (section 3).
3. Use ↑↓ to highlight a MIDI output port.
4. Press Enter to assign it to the selected clip.

## PipeWire Compatibility
SeqTerm's ALSA virtual ports work transparently through pipewire-alsa.
No special configuration is needed on PipeWire systems.

## JACK Mode
Build with --features seqterm-midi/jack for native JACK support.
In JACK mode, ports appear in the JACK graph instead of ALSA.

## Unavailable Routes
If a configured MIDI output disappears:
- The clip cell turns amber in the matrix grid
- The routing panel shows '! UNAVAILABLE' next to the port name
- SeqTerm checks port availability every 3 seconds
";

static PATTERN_EDITOR: &str = "\
# Pattern Editor Guide

## Step Table
Each row is one 16th-note step. Columns:
  NOTE   = pitch name (C-4, D#3, etc.) or '---' for silence
  INS    = instrument number (0-15, for multi-timbral synths)
  VEL    = MIDI velocity (0-127)
  FX1/2  = Effect commands (Vxx=volume, Dxx=delay, Sxx=slide)
  CC01   = Modulation wheel value (0-127)
  CC74   = Filter cutoff value (0-127)
  GATE   = Note length in % of one step (10-400%)
  MICRO  = Microtiming offset (-99 to +99 ticks)
  PROB   = Step trigger probability (0-100%)

## Pattern Length
Press [ to decrease length, ] to increase (1-128 steps).

## Time Signatures
In the Generative Engine panel, adjust TIME_N and TIME_D.
Beat groups control how measures are subdivided.

## Generative Engine
  NAME       = Pattern name (press Enter to rename)
  LEN        = Pattern length in measures
  TIME_N/D   = Time signature numerator / denominator
  BEAT_GROUP = Rhythmic subdivision grouping
  SWING      = Swing percentage (50-80%)
  PROB       = Pattern trigger probability
  RANDOM     = Step randomization amount
  EUCLID F/L = Euclidean rhythm fill and length
  PROB LOCK  = Freeze randomization state
  MICROSHIFT = Pattern-wide timing offset
  EVOLUTION  = Gradual pattern mutation rate
  HUMANIZE   = Velocity humanization amount

## Track Modulation
The bottom panel shows per-step envelope for:
VEL, GAIN, PAN, LP (low-pass), HP (high-pass), LFO, SPD, AMP

Click or drag in the bar chart to paint values across steps.
";

static TROUBLESHOOTING: &str = "\
# Troubleshooting

## No sound
1. Check that your JACK or ALSA server is running.
2. Open Config view (key 5) and verify audio backend status.
3. Ensure MIDI output ports are assigned in Matrix → Routing.
4. Check that the target synthesizer is running and receiving MIDI.

## MIDI ports not visible in Carla
1. SeqTerm must be running — ports are created at startup.
2. Each pattern creates one virtual port. Verify patterns exist.
3. On ALSA: run 'aconnect -l' to list ports.
4. On PipeWire: run 'pw-cli ls Port' or use qpwgraph.
5. Restart both Carla and SeqTerm if ports still don't appear.

## XRun counter increasing
XRuns indicate audio buffer underruns:
1. Increase buffer size in EDIT → Audio Settings.
2. Reduce CPU load: close other applications.
3. Set your kernel to real-time scheduling:
   sudo rtcred add username && sudo reboot

## Import fails
- Ensure the file is a valid SMF (.mid) Type 0 or Type 1.
- Type 2 (sequential) files are not supported.
- SMPTE timecode timing is not supported; use metric (PPQ).

## Project won't save
- Check disk space and write permissions to the project directory.
- Try Save As (Ctrl+Shift+S) to a different location.
- The autosave file (project.autosave.json) may be used for recovery.

## Undo not working
- Undo history is cleared on New Project.
- Some operations (MIDI port changes, audio settings) are not undoable.
- Undo depth is limited to 200 operations.
";

static LATENCY: &str = "\
# Latency Optimization

## Understanding Latency
Total latency = audio buffer latency + MIDI processing latency.
Audio buffer latency = buffer_size / sample_rate × 1000 ms.

Examples:
  256 samples @ 48kHz = 5.3 ms
  128 samples @ 48kHz = 2.7 ms
  64  samples @ 48kHz = 1.3 ms

## Recommended Settings
For live performance (lowest latency):
  Sample rate:  48000 Hz
  Buffer size:  128 samples (2.7 ms)

For production (stability over latency):
  Sample rate:  48000 Hz
  Buffer size:  256 or 512 samples

## JACK Configuration
Start JACK with real-time priority:
  jackd -R -d alsa -r 48000 -p 128

Or via QjackCtl: enable 'Realtime' checkbox.

## Kernel Tuning
1. Install a real-time kernel (linux-rt on Arch/Fedora, etc.)
2. Add your user to the 'audio' and 'realtime' groups:
   sudo usermod -aG audio,realtime $USER
3. Set CPU governor to 'performance':
   cpupower frequency-set -g performance

## PipeWire Latency
Set quantum in /etc/pipewire/pipewire.conf:
  default.clock.quantum = 128
  default.clock.min-quantum = 64

Then restart PipeWire:
  systemctl --user restart pipewire pipewire-pulse

## MIDI Timing
SeqTerm's internal scheduler runs at 24 PPQN.
The scheduler thread runs at high priority (SCHED_FIFO).
Avoid holding the project mutex for long periods.
";
