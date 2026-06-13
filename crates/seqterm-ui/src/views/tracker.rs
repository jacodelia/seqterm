use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Table, TableState,
    },
    Frame,
};

use crate::app::{App, fx_param_descs};

const PANEL: Color = Color::Rgb(22, 27, 34);
const PANEL_DARK: Color = Color::Rgb(18, 22, 28);
const BORDER: Color = Color::Rgb(48, 54, 61);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const HEADER: Color = Color::Rgb(240, 136, 62);
const BEAT_MARKER: Color = Color::Rgb(35, 42, 52);
const BAR_MARKER: Color = Color::Rgb(45, 52, 65);

/// Black key note names (sharps).
const BLACK_KEYS: &[&str] = &[
    "A#", "G#", "F#", "D#", "C#",
];

/// All 88 piano keys, top = C9 (MIDI 108) → bottom = A1 (MIDI 21).
/// Row index = 108 - MIDI.
pub const NOTE_ROWS: &[&str] = &[
    // Octave 9 (partial): C9 only
    "C9",
    // Octave 8: B8 … C#8
    "B8","A#8","A8","G#8","G8","F#8","F8","E8","D#8","D8","C#8",
    // Octave 7
    "C8","B7","A#7","A7","G#7","G7","F#7","F7","E7","D#7","D7","C#7",
    // Octave 6
    "C7","B6","A#6","A6","G#6","G6","F#6","F6","E6","D#6","D6","C#6",
    // Octave 5
    "C6","B5","A#5","A5","G#5","G5","F#5","F5","E5","D#5","D5","C#5",
    // Octave 4 (middle C = C5 in this codebase = MIDI 60)
    "C5","B4","A#4","A4","G#4","G4","F#4","F4","E4","D#4","D4","C#4",
    // Octave 3
    "C4","B3","A#3","A3","G#3","G3","F#3","F3","E3","D#3","D3","C#3",
    // Octave 2
    "C3","B2","A#2","A2","G#2","G2","F#2","F2","E2","D#2","D2","C#2",
    // Octave 1 (bottom of standard piano): C2 … A1
    "C2","B1","A#1","A1",
];

fn is_black_key(note_name: &str) -> bool {
    BLACK_KEYS.iter().any(|&b| note_name.starts_with(b))
}

pub fn draw_tracker(f: &mut Frame, app: &App, area: Rect) {
    // The velocity bar-chart click is not guarded by a parent area, so clear its
    // rect each frame; the TRACK MODULATION panel re-sets it only when that tab
    // is the one actually drawn below the piano roll.
    app.vel_chart_area.set(Rect::default());

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left column: just the step table (top) and the transport (bottom). SOURCE,
    // TRACK MODULATION, FX CHAIN and GENERATIVE ENGINE now live in a tabbed area
    // below the piano roll (right column).
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),      // 0: step table
            Constraint::Length(7),   // 6: transport (matrix-style: play/stop/rwd/rec/quantize)
        ])
        .split(chunks[0]);

    // Cache section rects. Sections 1 (piano roll), 2 (generative), 3 (modulation),
    // 4 (fx) and 5 (source) are cached inside draw_piano_roll_panel's tabbed area.
    let mut tr = app.tracker_panel_rects.get();
    tr[0] = left_chunks[0];
    tr[6] = left_chunks[1];
    app.tracker_panel_rects.set(tr);

    draw_step_table(f, app, left_chunks[0]);
    draw_tracker_transport(f, app, left_chunks[1]);
    draw_piano_roll_panel(f, app, chunks[1]);
}

/// TRANSPORT subsection — matrix-style transport box for the current pattern,
/// with only the buttons PLAY | STOP | RWD | REC | QUANTIZE. PLAY/STOP drive the
/// pattern in isolation (solos the pattern; all other clips are muted while it
/// plays). Styled to match the MATRIX → TRANSPORT panel.
fn draw_tracker_transport(f: &mut Frame, app: &App, area: Rect) {
    let active  = app.tracker_section == 6;
    let cur     = app.tracker_transport_cursor;
    let playing = app.pattern_solo_playing;

    // Per-button base colours (match the matrix transport palette).
    let play_col = if playing { Color::Green } else { Color::Rgb(20, 80, 30) };
    let stop_col = Color::Rgb(80, 80, 95);
    let rwd_col  = Color::Rgb(60, 80, 120);
    let rec_col  = if app.recording { Color::Red } else { Color::Rgb(100, 25, 25) };
    let qnt_col  = Color::Rgb(150, 110, 200);
    let cols = [play_col, stop_col, rwd_col, rec_col, qnt_col];

    // Border style: yellow when this button is the active cursor; bold when "on".
    let bolds = [playing, false, false, app.recording, false];
    let border_s = |idx: usize| -> Style {
        if active && cur == idx {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if bolds[idx] {
            Style::default().fg(cols[idx]).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(cols[idx])
        }
    };
    let face_s = |idx: usize| -> Style {
        Style::default().fg(cols[idx]).add_modifier(
            if bolds[idx] { Modifier::BOLD } else { Modifier::empty() })
    };

    // Fixed-width button faces (7 inner chars), matrix-style boxes.
    let play_face = if playing { "│■ STOP│" } else { "│▶ PLAY│" };
    let labels    = [play_face, "│■ STOP│", "│◀◀ RWD│",
                     if app.recording { "│● REC │" } else { "│  REC │" }, "│◈ QUAN│"];
    let tops      = "╭──────╮";
    let bots      = "╰──────╯";

    let mut top = Vec::new();
    let mut mid = Vec::new();
    let mut bot = Vec::new();
    for i in 0..5usize {
        if i > 0 { top.push(Span::raw(" ")); mid.push(Span::raw(" ")); bot.push(Span::raw(" ")); }
        top.push(Span::styled(tops, border_s(i)));
        mid.push(Span::styled(labels[i], face_s(i)));
        bot.push(Span::styled(bots, border_s(i)));
    }

    let hint = if active {
        "  ←→=button  Enter=trigger  Tab=back"
    } else {
        "  Tab=transport"
    };

    let block = Block::default()
        .title(if active { " TRANSPORT [ACTIVE] " } else { " TRANSPORT " })
        .title_style(Style::default().fg(HEADER))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if active { Color::Yellow } else { BORDER }))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 { return; }

    // Store per-button hit-test rects (each box is 8 cols wide + 1 gap).
    let mut rects = [Rect::default(); 5];
    let mut x = inner.x;
    for r in rects.iter_mut() {
        *r = Rect::new(x, inner.y, 8.min(inner.width.saturating_sub(x - inner.x)), 3.min(inner.height));
        x += 9;
    }
    app.tracker_transport_btn_rects.set(rects);
    // Keep the legacy single-button rect pointing at PLAY for back-compat.
    app.tracker_transport_btn_rect.set(rects[0]);

    let pat = app.tracker_state.pattern_key.as_deref().unwrap_or("—");
    let lines = vec![
        Line::from(top),
        Line::from(mid),
        Line::from(bot),
        Line::from(Span::styled(
            format!("{}   pat:{}", hint, pat),
            Style::default().fg(if active { Color::Yellow } else { Color::DarkGray }),
        )),
    ];
    f.render_widget(Paragraph::new(lines).style(Style::default().bg(PANEL)), inner);
}

// ─────────────────────────────────────────────────────────────────── Tracker ──

fn draw_step_table(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let pat_key = app.tracker_state.pattern_key.as_deref().unwrap_or("KCK01");
    let pattern = proj.patterns.get(pat_key);

    // Column widths (LN + NOTE + INS + VEL + FX1 + FX2 + CC01 + CC74 + GATE + MICRO + PROB).
    let col_widths: &[u16] = &[4, 5, 4, 4, 4, 4, 5, 5, 5, 6, 5];
    let col_names: &[&str] = &["LN", "NOTE", "INS", "VEL", "FX1", "FX2", "CC01", "CC74", "GATE", "MICRO", "PROB"];

    // Build header row with edit-column highlight.
    let header_cells: Vec<Cell> = col_names
        .iter()
        .enumerate()
        .map(|(ci, &h)| {
            let is_edit_col = app.tracker_editing && ci == app.tracker_edit_field + 1;
            let style = if is_edit_col {
                Style::default()
                    .fg(Color::Black)
                    .bg(HEADER)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(HEADER).add_modifier(Modifier::BOLD)
            };
            Cell::from(h).style(style)
        })
        .collect();
    let header = Row::new(header_cells).height(1);

    // Pattern info for title.
    let (pat_len, pat_swing, pat_prob, pat_random) = pattern
        .map(|p| (p.length, p.swing.saturating_sub(50), p.prob, p.random))
        .unwrap_or((16, 0, 0, 0));

    // Beat-group subdivision derived from the pattern's time signature.
    let (time_sig_num, eff_groups) = pattern
        .map(|p| (p.time_sig_num.max(1) as usize, p.effective_groups()))
        .unwrap_or((4, vec![4u8]));

    // Positions within a measure that begin a sub-group (not including beat 0).
    let mut group_starts = std::collections::HashSet::<usize>::new();
    {
        let mut pos = 0usize;
        for (i, &g) in eff_groups.iter().enumerate() {
            if i > 0 { group_starts.insert(pos); }
            pos += g as usize;
        }
    }

    let title = format!(
        " PAT:{} LEN:{} SWING:{}% PROB:{}% RNDM:{}% ",
        pat_key, pat_len, pat_swing, pat_prob, pat_random
    );

    // Visible area height — store for scroll clamping and mouse hit-testing.
    let inner_h = area.height.saturating_sub(3) as usize; // borders + header
    app.tracker_view_height.set(inner_h);
    app.tracker_table_area.set(area);
    let scroll = app.tracker_scroll;

    // When editing, the active column gets bright-yellow; all others get dim gold.
    let edit_col_idx = app.tracker_edit_field + 1; // col index (0=LN, 1=NOTE, ..., 10=PROB)

    let rows: Vec<Row> = if let Some(pat) = pattern {
        (0..pat.length)
            .map(|step| {
                let note = pat.steps.get(step).cloned().unwrap_or_default();
                let is_cursor = app.tracker_state.cursor.0 == step;
                let is_playing = app.playing && app.current_step % pat.length == step;

                // Beat position within the current measure.
                let beat = step % time_sig_num;
                let is_measure_start = beat == 0;
                let is_group_start   = !is_measure_start && group_starts.contains(&beat);

                // Beat marker backgrounds derived from beat grouping.
                let beat_bg = if is_measure_start {
                    BAR_MARKER
                } else if is_group_start {
                    BEAT_MARKER
                } else {
                    PANEL
                };

                let row_fg = if is_measure_start || is_group_start {
                    Color::White
                } else {
                    Color::Rgb(160, 160, 160)
                };

                // Visual selection range.
                let is_in_visual = app.vim_mode == crate::app::VimMode::Visual
                    && app.visual_start.map(|vs| {
                        let cursor = app.tracker_state.cursor.0;
                        let lo = vs.min(cursor);
                        let hi = vs.max(cursor);
                        step >= lo && step <= hi
                    }).unwrap_or(false);

                let base_style = if is_in_visual && is_cursor {
                    Style::default().fg(Color::Black).bg(Color::Magenta).add_modifier(Modifier::BOLD)
                } else if is_in_visual {
                    Style::default().fg(Color::White).bg(Color::Rgb(80, 0, 120))
                } else if is_cursor && app.tracker_editing {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if is_cursor {
                    Style::default()
                        .fg(Color::Yellow)
                        .bg(BEAT_MARKER)
                        .add_modifier(Modifier::REVERSED)
                } else if is_playing {
                    Style::default().fg(Color::Black).bg(Color::Green)
                } else {
                    Style::default().fg(row_fg).bg(beat_bg)
                };

                // Note display with color coding.
                let note_str = if note.is_empty() {
                    "···".to_string()
                } else {
                    note.note.clone()
                };
                let note_style = if is_cursor || is_playing {
                    base_style
                } else if note.is_empty() {
                    Style::default().fg(Color::DarkGray).bg(beat_bg)
                } else {
                    Style::default().fg(Color::Rgb(100, 200, 255)).bg(beat_bg)
                };

                // Velocity as hex.
                let vel_str = if note.is_empty() {
                    "···".to_string()
                } else {
                    format!("{:02X}", note.velocity)
                };
                let vel_style = if is_cursor || is_playing {
                    base_style
                } else if note.is_empty() {
                    Style::default().fg(Color::DarkGray).bg(beat_bg)
                } else {
                    Style::default().fg(Color::Green).bg(beat_bg)
                };

                let ins_str = if note.is_empty() {
                    "··".to_string()
                } else {
                    format!("{:02}", note.instrument)
                };

                let fx1_str = if note.fx1 == "--" { "··".to_string() } else { note.fx1.clone() };
                let fx2_str = if note.fx2 == "--" { "··".to_string() } else { note.fx2.clone() };

                let cc01_str = if note.is_empty() { "···".to_string() } else { format!("{:03}", note.cc01) };
                let cc74_str = if note.is_empty() { "···".to_string() } else { format!("{:03}", note.cc74) };
                let gate_str = if note.is_empty() { "···".to_string() } else { format!("{:03}", note.gate) };
                let micro_str = if note.is_empty() { "····".to_string() } else { format!("{:>+4}", note.micro) };
                let prob_str = if note.is_empty() { "···".to_string() } else { format!("{:03}", note.prob) };

                let line_num = format!("{:03}", step);
                let ln_style = if is_cursor || is_playing {
                    base_style
                } else if is_measure_start {
                    Style::default().fg(HEADER).bg(beat_bg)
                } else if is_group_start {
                    Style::default().fg(ACCENT).bg(beat_bg)
                } else {
                    Style::default().fg(Color::DarkGray).bg(beat_bg)
                };

                let cc_style = |s: Style| {
                    if is_cursor || is_playing {
                        base_style
                    } else {
                        s
                    }
                };

                // In edit mode, highlight only the active column; dim all others.
                let is_cursor_editing = is_cursor && app.tracker_editing;
                let es = |ci: usize, s: Style| -> Style {
                    if is_cursor_editing {
                        if ci == edit_col_idx {
                            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Rgb(100, 85, 10)).bg(Color::Rgb(45, 40, 5))
                        }
                    } else {
                        s
                    }
                };

                let cells = vec![
                    Cell::from(line_num).style(es(0, ln_style)),
                    Cell::from(note_str).style(es(1, note_style)),
                    Cell::from(ins_str).style(es(2, base_style)),
                    Cell::from(vel_str).style(es(3, vel_style)),
                    Cell::from(fx1_str).style(es(4, cc_style(Style::default().fg(Color::Yellow).bg(beat_bg)))),
                    Cell::from(fx2_str).style(es(5, cc_style(Style::default().fg(Color::Yellow).bg(beat_bg)))),
                    Cell::from(cc01_str).style(es(6, cc_style(Style::default().fg(Color::Cyan).bg(beat_bg)))),
                    Cell::from(cc74_str).style(es(7, cc_style(Style::default().fg(Color::Cyan).bg(beat_bg)))),
                    Cell::from(gate_str).style(es(8, cc_style(Style::default().fg(Color::Magenta).bg(beat_bg)))),
                    Cell::from(micro_str).style(es(9, cc_style(Style::default().fg(Color::Rgb(255, 165, 0)).bg(beat_bg)))),
                    Cell::from(prob_str).style(es(10, cc_style(Style::default().fg(Color::Rgb(200, 100, 255)).bg(beat_bg)))),
                ];
                Row::new(cells).height(1)
            })
            .collect()
    } else {
        vec![Row::new(vec![Cell::from("No pattern selected").style(Style::default().fg(Color::DarkGray))])]
    };

    let widths: Vec<Constraint> = col_widths.iter().map(|&w| Constraint::Length(w)).collect();

    let tracker_active = app.tracker_section == 0;
    let (table_border, mode_col) = match (tracker_active, app.vim_mode) {
        (false, _) => (Style::default().fg(BORDER), BORDER),
        (true, crate::app::VimMode::Normal) => (Style::default().fg(Color::Yellow), Color::Yellow),
        (true, crate::app::VimMode::Insert) => (Style::default().fg(Color::Green), Color::Green),
        (true, crate::app::VimMode::Visual) => (Style::default().fg(Color::Magenta), Color::Magenta),
    };
    let mode_badge = if tracker_active {
        format!(" [{}] ", app.vim_mode.label())
    } else {
        String::new()
    };
    let table_title = format!(
        "{}{}{}",
        title,
        mode_badge,
        if tracker_active {
            format!(" i=ins v=vis Esc=norm │ GRID {} ", app.edit_state.summary())
        } else {
            String::new()
        }
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(table_title)
                .title_style(Style::default().fg(mode_col).add_modifier(Modifier::BOLD))
                .borders(Borders::ALL)
                .border_style(table_border)
                .style(Style::default().bg(PANEL)),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));

    // Use TableState with scroll offset. `select` must be the ABSOLUTE row index
    // (not relative to `scroll`); otherwise ratatui — which keeps the selected row
    // visible — recomputes the offset and the wrong lines show for long patterns.
    let cursor_row = app.tracker_state.cursor.0;
    let mut table_state = TableState::default();
    *table_state.offset_mut() = scroll;
    table_state.select(Some(cursor_row));

    // Render scrollbar on the right.
    let scrollbar_area = Rect {
        x: area.x + area.width.saturating_sub(1),
        y: area.y + 2,
        width: 1,
        height: area.height.saturating_sub(3),
    };

    f.render_stateful_widget(table, area, &mut table_state);

    let total = pat_len;
    if total > inner_h {
        let mut sb_state = ScrollbarState::new(total)
            .viewport_content_length(inner_h)
            .position(scroll);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .thumb_symbol("█")
                .track_symbol(Some("│")),
            scrollbar_area,
            &mut sb_state,
        );
    }
}

// ──────────────────────────────────────────────────── Generative/Status panel ──

fn draw_generative_panel(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let pat_key = app.tracker_state.pattern_key.as_deref().unwrap_or("KCK01");

    let (swing, random, prob, pat_len, pat_name, euclid_fill, euclid_len,
         humanization, evolution, prob_lock, microshift, time_sig_num, time_sig_den,
         beat_groups) = proj
        .patterns
        .get(pat_key)
        .map(|p| (p.swing.saturating_sub(50), p.random, p.prob, p.length, p.name.clone(),
                  p.euclid_fill, p.euclid_len, p.humanization, p.evolution,
                  p.prob_lock, p.microshift, p.time_sig_num, p.time_sig_den,
                  p.effective_groups()))
        .unwrap_or((0, 0, 0, 16, pat_key.to_string(), 3, 16, 0, 0, false, 0, 4, 4,
                    vec![4u8]));

    let effective_len = {
        let num = time_sig_num.max(1) as usize;
        ((pat_len + num - 1) / num) * num
    };
    let measures = effective_len / time_sig_num.max(1) as usize;

    let group_display: String = beat_groups.iter()
        .map(|g| g.to_string())
        .collect::<Vec<_>>()
        .join("+");

    // Euclidean pattern using per-pattern fill/len.
    let euclid = seqterm_generative::euclidean_rhythm(
        euclid_fill.max(1),
        euclid_len.max(2),
    );
    let euclid_vis: String = euclid.iter().map(|&b| if b { '●' } else { '─' }).collect();

    let evolution_label = match evolution {
        1 => "SLOW  ",
        2 => "MEDIUM",
        3 => "FAST  ",
        _ => "OFF   ",
    };

    let gen_active = app.tracker_section == 2;
    let gc = app.generative_cursor;

    // Style helpers.
    let row_style = |idx: usize| -> Style {
        if gen_active && gc == idx {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        }
    };
    let lbl_style = |idx: usize| -> Style {
        if gen_active && gc == idx {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(ACCENT)
        }
    };

    // Name display: if editing, show buffer with cursor.
    let name_display = if app.pattern_name_editing && gen_active && gc == 0 {
        format!("{}_", app.pattern_name_buffer)
    } else {
        format!("\"{}\"", pat_name)
    };

    let mut lines: Vec<Line> = Vec::new();

    // gc=0: Pattern name.
    lines.push(Line::from(vec![
        Span::styled("PAT NAME  : ", lbl_style(0)),
        Span::styled(
            format!("{:<10}", name_display),
            if gen_active && gc == 0 {
                Style::default().fg(Color::Yellow).bg(PANEL_DARK).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            },
        ),
        Span::styled(
            if gen_active && gc == 0 { " Enter=edit " } else { "" },
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // gc=1: LEN — always a complete number of measures.
    lines.push(Line::from(vec![
        Span::styled("PAT LEN   : ", lbl_style(1)),
        Span::styled(format!("{:>3}", pat_len), row_style(1)),
        Span::styled(
            format!("  ({} bars × {}/{})", measures, time_sig_num, time_sig_den),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // gc=2+3: Time signature (N / D on same row, like EUCL STEPS).
    lines.push(Line::from(vec![
        Span::styled("TIME SIG  : ", lbl_style(2)),
        Span::styled(format!("{:>3}", time_sig_num), row_style(2)),
        Span::styled(" / ", Style::default().fg(ACCENT)),
        Span::styled(format!("{:>3}", time_sig_den), row_style(3)),
        Span::styled(
            if gen_active && (gc == 2 || gc == 3) { "  ←→=adjust" } else { "" },
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // gc=4: Beat grouping (cycles valid compositions with ←→).
    let total_groups = seqterm_core::musical_groupings(time_sig_num).len();
    let group_hint = if gen_active && gc == 4 {
        format!("  ←→=cycle ({} options)", total_groups)
    } else {
        String::new()
    };
    lines.push(Line::from(vec![
        Span::styled("BEAT GROUP: ", lbl_style(4)),
        Span::styled(format!("{:<12}", group_display), row_style(4)),
        Span::styled(group_hint, Style::default().fg(Color::DarkGray)),
    ]));

    // Separator.
    lines.push(Line::from(Span::styled(
        "─".repeat(area.width.saturating_sub(2) as usize),
        Style::default().fg(BORDER),
    )));

    // gc=5: Swing.
    lines.push(Line::from(vec![
        Span::styled("SWING          : ", lbl_style(5)),
        Span::styled(format!("{:>3}%", swing), row_style(5)),
    ]));

    // gc=6: Prob.
    lines.push(Line::from(vec![
        Span::styled("PROB           : ", lbl_style(6)),
        Span::styled(format!("{:>3}%", prob), row_style(6)),
    ]));

    // gc=7: Random mutation.
    lines.push(Line::from(vec![
        Span::styled("RANDOM MUTATION: ", lbl_style(7)),
        Span::styled(format!("{:>3}%", random), row_style(7)),
        Span::styled(
            format!("  {}", make_bar(random, 100, 10)),
            Style::default().fg(Color::Rgb(180, 80, 200)),
        ),
    ]));

    // gc=8+9: Euclidean steps (fill / len on same row).
    lines.push(Line::from(vec![
        Span::styled("EUCL STEPS     : ", lbl_style(8)),
        Span::styled(format!("{:>2}", euclid_fill), row_style(8)),
        Span::styled(" / ", Style::default().fg(ACCENT)),
        Span::styled(format!("{:>2}", euclid_len), row_style(9)),
        Span::styled(
            if gen_active && (gc == 8 || gc == 9) { "  ←→=adjust" } else { "" },
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // gc=10: Probability lock.
    lines.push(Line::from(vec![
        Span::styled("PROB LOCK      : ", lbl_style(10)),
        Span::styled(
            if prob_lock { "ACTIVE" } else { "OFF   " },
            if prob_lock {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else if gen_active && gc == 10 {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
        Span::styled(
            if gen_active && gc == 10 { "  Enter=toggle" } else { "" },
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // gc=11: Microshift.
    lines.push(Line::from(vec![
        Span::styled("MICROSHIFT     : ", lbl_style(11)),
        Span::styled(format!("{:>+4}", microshift), row_style(11)),
    ]));

    // Euclidean visualization.
    lines.push(Line::from(Span::styled("", Style::default())));
    lines.push(Line::from(vec![
        Span::styled("PATTERN  ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(
            euclid_vis,
            Style::default().fg(if gen_active { Color::Green } else { Color::Rgb(60, 160, 80) }),
        ),
    ]));

    // gc=12: Evolution mode.
    lines.push(Line::from(vec![
        Span::styled("EVOLUTION MODE : ", lbl_style(12)),
        Span::styled(evolution_label, row_style(12)),
    ]));

    // gc=13: Humanization.
    lines.push(Line::from(vec![
        Span::styled("HUMANIZATION   : ", lbl_style(13)),
        Span::styled(format!("{:>3}%", humanization), row_style(13)),
        Span::styled(
            format!("  {}", make_bar(humanization, 100, 10)),
            Style::default().fg(Color::Rgb(255, 160, 0)),
        ),
    ]));

    // Hint line.
    let hint = if gen_active {
        if app.pattern_name_editing {
            " TYPE=edit name  Enter=confirm  Esc=cancel"
        } else {
            " ↑↓=row  ←→=adjust  Enter=toggle/edit  Tab=next"
        }
    } else {
        " Tab=activate generative engine"
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(if gen_active { Color::Yellow } else { Color::DarkGray }),
    )));

    let gen_active_str = if gen_active { " [ACTIVE] " } else { "" };
    let title = format!(" GENERATIVE ENGINE :: TRACK \"{}\"{}", pat_key, gen_active_str);
    let gen_border = if gen_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(BORDER)
    };

    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(gen_border)
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 { return; }

    // Two columns so the engine fits the uniform tab height: the first
    // GEN_SPLIT rows go left, the rest go right. `generative_row_to_gc` mirrors
    // this split for mouse hit-testing.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);
    let split = crate::GEN_SPLIT.min(lines.len());
    let left  = lines[..split].to_vec();
    let right = lines[split..].to_vec();
    let bg = Style::default().bg(PANEL);
    f.render_widget(Paragraph::new(left).style(bg), cols[0]);
    f.render_widget(Paragraph::new(right).style(bg), cols[1]);
}

fn make_bar(val: u8, max: u8, width: usize) -> String {
    let filled = (val as usize * width / max as usize).min(width);
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width - filled)
    )
}

// ─────────────────────────────────────────────────────────────── Piano Roll ──

/// Tab display order for the panel area below the piano roll.
/// Index → tracker_section: SOURCE=5, TRACK MODULATION=3, FX CHAIN=4, GENERATIVE=2.
pub const TRACKER_TAB_LABELS: [&str; 4] =
    ["SOURCE", "MODULATION", "FX", "SETTINGS"];
pub const TRACKER_TAB_SECTIONS: [usize; 4] = [5, 3, 4, 2];

/// Uniform height (rows) of the tabbed panel below the piano roll. Every tab uses
/// the same height so the piano roll never resizes when switching tabs; the taller
/// tabs (FX CHAIN, GENERATIVE) lay their content out in columns to fit.
const TRACKER_TAB_PANEL_H: u16 = 13;

fn draw_piano_roll_panel(f: &mut Frame, app: &App, area: Rect) {
    // Right column: piano roll (top) | tab bar (1 row) | selected panel (fixed).
    let tab = app.tracker_tab.min(3);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(7),
            Constraint::Length(1),
            Constraint::Length(TRACKER_TAB_PANEL_H),
        ])
        .split(area);

    let mut tr = app.tracker_panel_rects.get();
    tr[1] = chunks[0];
    app.tracker_panel_rects.set(tr);

    draw_piano_roll(f, app, chunks[0]);
    draw_tracker_tab_bar(f, app, chunks[1]);

    // Render the selected tab's panel in the remaining area, and record its rect
    // for the matching tracker_section so existing mouse hit-testing keeps working.
    // Only one tab is visible at a time, so clear all four tab-section rects first
    // and set just the active one (otherwise stale rects from a previously-shown
    // tab would still match clicks).
    let panel_area = chunks[2];
    let section = TRACKER_TAB_SECTIONS[tab];
    let mut tr = app.tracker_panel_rects.get();
    for &s in &TRACKER_TAB_SECTIONS { tr[s] = Rect::default(); }
    tr[section] = panel_area;
    app.tracker_panel_rects.set(tr);

    match tab {
        0 => crate::views::matrix::draw_tracker_source_panel(f, app, panel_area, app.tracker_section == 5),
        1 => draw_modulation_panel(f, app, panel_area),
        2 => draw_fx_chain_panel(f, app, panel_area),
        _ => draw_generative_panel(f, app, panel_area),
    }
}

/// One-row tab selector: SOURCE | TRACK MODULATION | FX CHAIN | GENERATIVE ENGINE.
/// Highlights the active tab; the focused tab (matching tracker_section) is bright.
fn draw_tracker_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();
    let mut rects = [Rect::default(); 4];
    let mut x = area.x;
    for (i, label) in TRACKER_TAB_LABELS.iter().enumerate() {
        let is_sel     = app.tracker_tab == i;
        let is_focused = is_sel && app.tracker_section == TRACKER_TAB_SECTIONS[i];
        let text = format!(" {} ", label);
        let w = text.chars().count() as u16;
        if x + w <= area.x + area.width {
            rects[i] = Rect::new(x, area.y, w, 1);
        }
        let style = if is_focused {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if is_sel {
            Style::default().fg(Color::Black).bg(HEADER).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(150, 160, 180)).bg(PANEL_DARK)
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::styled("│", Style::default().fg(BORDER).bg(PANEL_DARK)));
        x += w + 1;
    }
    app.tracker_tab_rects.set(rects);
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(PANEL_DARK)),
        area,
    );
}

fn draw_piano_roll(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let pat_key = app.tracker_state.pattern_key.as_deref().unwrap_or("KCK01");

    let pat = match proj.patterns.get(pat_key) {
        Some(p) => p,
        None => {
            let p = Paragraph::new("No pattern").block(
                Block::default()
                    .title(" PIANO ROLL ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BORDER))
                    .style(Style::default().bg(PANEL)),
            );
            f.render_widget(p, area);
            return;
        }
    };

    // The velocity lane has been removed from the piano roll; velocity is now
    // shown (and edited) in the aligned TRACK MODULATION panel below. The note
    // grid uses the full panel height.

    // Inner area after block borders (borders + header row + bottom scrollbar row).
    let inner_h = area.height.saturating_sub(4) as usize;
    let key_w: usize = 5; // piano key label width in columns
    // Columns available for step cells (subtract key area, borders, scrollbar).
    let step_display_w = area.width.saturating_sub(key_w as u16 + 3) as usize;
    // Each step cell is 2 columns wide.
    let visible_steps = (step_display_w / 2).min(pat.length);
    // Publish step viewport width so process_events can clamp horizontal scroll.
    app.piano_visible_steps.set(visible_steps.max(1));
    let step_scroll = app.piano_step_scroll.min(pat.length.saturating_sub(1));
    let note_scroll = app.piano_note_scroll.min(NOTE_ROWS.len().saturating_sub(1));

    let time_sig_num = pat.time_sig_num.max(1) as usize;
    let eff_groups = pat.effective_groups();

    // Compute which beat positions within a measure begin a sub-group (not including 0).
    let mut group_starts: std::collections::HashSet<usize> = std::collections::HashSet::new();
    {
        let mut pos = 0usize;
        for (i, &g) in eff_groups.iter().enumerate() {
            if i > 0 { group_starts.insert(pos); }
            pos += g as usize;
        }
    }

    // Edit-resolution grid: a faint tick marks steps that fall on an edit-grid
    // boundary (where snap/placement aligns), so the active resolution/tuplet is
    // visible while editing. Computed in exact rational beats; a step is on-grid
    // when its absolute position is an integer multiple of the edit grid cell.
    let edit_grid = app.edit_state.grid_beats();
    let pat_step_beats = pat.step_beats();
    let on_edit_grid = |step: usize| -> bool {
        if edit_grid.is_zero() {
            return false;
        }
        let abs = pat_step_beats * step as i64;
        (abs / edit_grid).frac().is_zero()
    };

    // Build step header: beat numbers 01..N repeating, colored by grouping.
    let mut hdr_spans: Vec<Span> =
        vec![Span::styled(format!("{:<5}", " "), Style::default())];
    for i in 0..visible_steps {
        let step = step_scroll + i;
        let beat = step % time_sig_num; // 0-based position within measure
        let is_measure_start = beat == 0;
        let is_group_start = group_starts.contains(&beat);
        let style = if is_measure_start {
            Style::default().fg(HEADER).add_modifier(Modifier::BOLD)
        } else if is_group_start {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let label = format!("{:02}", beat + 1); // 01..N within measure
        hdr_spans.push(Span::styled(label, style));
    }
    let hdr_line = Line::from(hdr_spans);

    // Visible note rows (scroll-adjusted; published for scrollbar thumb sizing).
    let visible_rows = inner_h.min(NOTE_ROWS.len() - note_scroll);
    app.piano_visible_rows.set(visible_rows.max(1));

    // Build note grid lines.
    let mut grid_lines: Vec<Line> = Vec::with_capacity(visible_rows + 1);
    grid_lines.push(hdr_line);

    // Polyphonic note map: step → Vec<(row_idx, gate_steps)>.
    // Each step can have multiple voices (primary note + chord_notes).
    let mut note_map: Vec<Vec<(usize, usize)>> = vec![vec![]; pat.length];
    for (step, note) in pat.steps.iter().enumerate() {
        if note.is_empty() { continue; }
        let gate_steps = ((note.gate as usize + 99) / 100).max(1);
        // Primary note voice.
        if let Some(midi) = seqterm_core::note::parse_note_name(&note.note) {
            if let Some(row_idx) = midi_to_row_idx(midi) {
                note_map[step].push((row_idx, gate_steps));
            }
        }
        // Chord voices.
        for chord_name in &note.chord_notes {
            if let Some(midi) = seqterm_core::note::parse_note_name(chord_name) {
                if let Some(row_idx) = midi_to_row_idx(midi) {
                    note_map[step].push((row_idx, gate_steps));
                }
            }
        }
    }

    let piano_active = app.tracker_section == 1;
    let piano_cursor_step = app.piano_cursor.1;
    let piano_cursor_row = app.piano_cursor.0;

    for row_rel in 0..visible_rows {
        let row_idx = note_scroll + row_rel;
        let row_name = NOTE_ROWS[row_idx];
        let black = is_black_key(row_name);
        // C notes mark octave boundaries (exclude C# which is also a black key).
        let is_c_note = !black && row_name.starts_with('C');

        // FL Studio-style piano key label (5 chars):
        //   Black key:  " ▐A#4"  — right-half-block gives an inset/recessed look.
        //   C note:     "  C4─"  — dash at right marks octave boundary.
        //   White key:  "  A4 "  — plain label with trailing space.
        let key_label = if black {
            format!(" ▐{}", row_name)    // 1 + 1 + 3 chars = 5 ✓
        } else if is_c_note {
            format!("  {}─", row_name)   // 2 + 2 + 1 chars = 5 ✓
        } else {
            format!("  {} ", row_name)   // 2 + 2 + 1 chars = 5 ✓
        };

        // Subtle grid background: black-key rows are darker than white-key rows.
        let grid_empty_bg = if black {
            Color::Rgb(12, 12, 16)
        } else {
            Color::Rgb(22, 22, 28)
        };

        // Piano cursor: highlight the cursor row label when active.
        let is_piano_cursor_row = piano_active && piano_cursor_row == row_idx;
        let label_style = if is_piano_cursor_row {
            Style::default().fg(Color::Yellow).bg(Color::Rgb(50, 42, 0)).add_modifier(Modifier::BOLD)
        } else if black {
            Style::default().fg(Color::Rgb(150, 150, 150)).bg(Color::Rgb(10, 10, 14))
        } else if is_c_note {
            Style::default().fg(Color::Rgb(255, 180, 60)).bg(Color::Rgb(30, 26, 12))
        } else {
            Style::default().fg(Color::Rgb(200, 200, 210)).bg(Color::Rgb(24, 24, 30))
        };

        let mut spans: Vec<Span> = vec![Span::styled(key_label, label_style)];

        for i in 0..visible_steps {
            let step = step_scroll + i;
            // When piano is active, the cursor column is piano_cursor_step;
            // otherwise it follows the tracker row cursor.
            let is_cursor_col = if piano_active {
                piano_cursor_step == step
            } else {
                app.tracker_state.cursor.0 == step
            };
            let is_piano_cursor = piano_active && piano_cursor_row == row_idx && piano_cursor_step == step;
            let is_piano_cursor_row_cell = piano_active && piano_cursor_row == row_idx;
            let is_playing = app.playing && app.current_step == step;

            // Check if this row/step has a note start (any polyphonic voice).
            let has_start = note_map
                .get(step)
                .map(|voices| voices.iter().any(|(nr, _)| *nr == row_idx))
                .unwrap_or(false);

            // Check if this step is a gate continuation of a note started earlier.
            let has_continuation = {
                let mut cont = false;
                for back in 1..=16usize {
                    if step < back { break; }
                    if let Some(voices) = note_map.get(step - back) {
                        if voices.iter().any(|(nr, gates)| *nr == row_idx && *gates > back) {
                            cont = true;
                            break;
                        }
                    }
                }
                cont
            };

            let (cell_str, cell_fg, cell_bg) = if is_piano_cursor {
                // Crosshair intersection: distinctive marker regardless of note state.
                if has_start {
                    ("██", Color::White, Color::Rgb(80, 60, 0))
                } else {
                    ("◆◆", Color::White, Color::Rgb(60, 45, 0))
                }
            } else if has_start {
                // Note start: bright FL Studio green.
                if is_playing {
                    ("██", Color::Rgb(50, 255, 80), PANEL_DARK)
                } else if is_cursor_col {
                    ("██", Color::Rgb(230, 220, 60), PANEL_DARK)
                } else {
                    ("██", Color::Rgb(110, 215, 75), PANEL_DARK)
                }
            } else if has_continuation {
                // Note body: darker shade of green (distinct from start).
                if is_piano_cursor_row_cell {
                    ("▓▓", Color::Rgb(190, 175, 55), PANEL_DARK)
                } else if is_cursor_col {
                    ("▓▓", Color::Rgb(170, 165, 50), PANEL_DARK)
                } else {
                    ("▓▓", Color::Rgb(65, 155, 45), PANEL_DARK)
                }
            } else if is_playing {
                ("  ", Color::DarkGray, Color::Rgb(15, 30, 15))
            } else if is_piano_cursor_row_cell {
                // Horizontal crosshair row: warm tint.
                if black {
                    ("  ", Color::DarkGray, Color::Rgb(22, 18, 2))
                } else {
                    ("  ", Color::DarkGray, Color::Rgb(35, 28, 3))
                }
            } else if is_cursor_col {
                // Vertical crosshair column: subtle highlight.
                if black {
                    ("  ", Color::DarkGray, Color::Rgb(18, 18, 22))
                } else {
                    ("  ", Color::DarkGray, Color::Rgb(28, 28, 36))
                }
            } else {
                // Empty cell: background color distinguishes black/white key rows.
                ("  ", Color::DarkGray, grid_empty_bg)
            };

            // Rectangular-selection highlight (Shift+drag): selected note-start
            // cells get a magenta wash.
            let (cell_str, cell_fg, cell_bg) =
                if piano_active && has_start && app.piano_selection.contains(&step) {
                    (cell_str, Color::Black, Color::Rgb(200, 120, 220))
                } else {
                    (cell_str, cell_fg, cell_bg)
                };

            // Beat-group separator: measure start = amber │, sub-group start = dim blue │.
            let beat = step % time_sig_num;
            let is_meas_sep = i > 0 && beat == 0;
            let is_grp_sep  = i > 0 && !is_meas_sep && group_starts.contains(&beat);
            let is_edit_tick = i > 0 && !is_meas_sep && !is_grp_sep && on_edit_grid(step);
            if is_meas_sep || is_grp_sep {
                let sep_col = if is_meas_sep {
                    HEADER
                } else {
                    Color::Rgb(48, 72, 130)
                };
                spans.push(Span::styled("│", Style::default().fg(sep_col).bg(cell_bg)));
                let one_char: String = cell_str.chars().next().map(|c| c.to_string()).unwrap_or_default();
                spans.push(Span::styled(one_char, Style::default().fg(cell_fg).bg(cell_bg)));
            } else if is_edit_tick {
                // Faint edit-grid tick (lowest priority separator).
                spans.push(Span::styled("┊", Style::default().fg(Color::Rgb(70, 70, 84)).bg(cell_bg)));
                let one_char: String = cell_str.chars().next().map(|c| c.to_string()).unwrap_or_default();
                spans.push(Span::styled(one_char, Style::default().fg(cell_fg).bg(cell_bg)));
            } else {
                spans.push(Span::styled(
                    cell_str.to_string(),
                    Style::default().fg(cell_fg).bg(cell_bg),
                ));
            }
        }

        grid_lines.push(Line::from(spans));
    }

    app.piano_roll_area.set(area);

    // Per-note exact rational readout for the cursor step (Phase 3, item 9):
    // shows Position and Length as both rational and decimal beats when the
    // piano cursor sits on a note.
    let note_readout = {
        let step = app.piano_cursor.1;
        match pat.steps.get(step) {
            Some(n) if !n.is_empty() => {
                let pos = pat.step_start(step);
                let len = pat.step_duration(step);
                format!(
                    " │ pos {}/{} ({:.3})  len {}/{} ({:.3})",
                    pos.num(), pos.den(), pos.to_f64(),
                    len.num(), len.den(), len.to_f64(),
                )
            }
            _ => String::new(),
        }
    };
    let title = format!(
        " PIANO ROLL :: {}{}",
        pat_key,
        if piano_active {
            format!(
                " [L-click=place  L-drag=resize  +/-=dur  Enter=toggle │ GRID {}{}] ",
                app.edit_state.summary(),
                note_readout,
            )
        } else { String::new() }
    );

    let piano_border = if piano_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(BORDER)
    };

    let p = Paragraph::new(grid_lines).block(
        Block::default()
            .title(title)
            .title_style(
                Style::default()
                    .fg(HEADER)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(piano_border)
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);

    // Horizontal scrollbar at bottom — thumb proportional to visible_steps / pat.length.
    if pat.length > visible_steps && visible_steps > 0 {
        let h_sb_area = Rect {
            x: area.x + key_w as u16 + 1,
            y: area.y + area.height.saturating_sub(2),
            width: area.width.saturating_sub(key_w as u16 + 3),
            height: 1,
        };
        let mut h_sb_state = ScrollbarState::new(pat.length)
            .viewport_content_length(visible_steps)
            .position(step_scroll);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
                .begin_symbol(Some("◄"))
                .end_symbol(Some("►"))
                .thumb_symbol("█"),
            h_sb_area,
            &mut h_sb_state,
        );
    }

    // Vertical scrollbar on right — thumb proportional to visible_rows / NOTE_ROWS.len().
    if NOTE_ROWS.len() > visible_rows && visible_rows > 0 {
        let v_sb_area = Rect {
            x: area.x + area.width.saturating_sub(1),
            y: area.y + 2,
            width: 1,
            height: area.height.saturating_sub(3),
        };
        let mut v_sb_state = ScrollbarState::new(NOTE_ROWS.len())
            .viewport_content_length(visible_rows)
            .position(note_scroll);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .thumb_symbol("█"),
            v_sb_area,
            &mut v_sb_state,
        );
    }

}

/// Draw the velocity lane: horizontal bars per step below the note grid.
/// Removed from the piano roll layout; superseded by the velocity display in the
/// aligned TRACK MODULATION panel. Retained (dead) for reference.
#[allow(dead_code)]
fn draw_velocity_lane(
    f: &mut Frame,
    app: &App,
    area: Rect,
    pat: &seqterm_core::Pattern,
    step_scroll: usize,
    visible_steps: usize,
    focused: bool,
) {
    const KEY_W: u16 = 5;
    let step_start_x = area.x + 1 + KEY_W;
    let cell_w: u16 = 2;
    let inner_h = area.height.saturating_sub(1) as usize; // 1 row for label

    app.piano_vel_area.set(area);

    let border_col = if focused { Color::Yellow } else { BORDER };
    let block = Block::default()
        .title(" VEL ")
        .title_style(Style::default().fg(HEADER))
        .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
        .border_style(Style::default().fg(border_col))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 1 || inner.width < 6 { return; }

    let cursor_step = app.piano_cursor.1;

    for i in 0..visible_steps {
        let step = step_scroll + i;
        let note = pat.steps.get(step).cloned().unwrap_or_default();
        let vel = if note.is_empty() { 0u8 } else { note.velocity };
        // Bar height: 0 = empty, inner_h = velocity 127.
        let bar_h = if vel == 0 { 0 } else {
            ((vel as usize * inner_h + 126) / 127).max(1)
        };
        let is_cursor = focused && step == cursor_step;
        let bar_col = if is_cursor {
            Color::Yellow
        } else if vel == 0 {
            BORDER
        } else {
            // Colour by velocity level: low=blue, mid=green, high=red.
            if vel < 64 { Color::Rgb(60, 120, 220) }
            else if vel < 100 { Color::Rgb(80, 200, 80) }
            else { Color::Rgb(220, 80, 60) }
        };

        let x = step_start_x + (i as u16) * cell_w;
        if x + cell_w > area.x + area.width { break; }

        // Draw bar from bottom up.
        for h in 0..inner_h {
            let y = inner.y + (inner_h - 1 - h) as u16;
            if y < inner.y || y >= inner.y + inner.height { continue; }
            let in_bar = h < bar_h;
            let ch = if in_bar { "▐▌" } else if is_cursor && h == 0 { "▁▁" } else { "  " };
            let style = if in_bar {
                Style::default().fg(bar_col).bg(PANEL)
            } else {
                Style::default().fg(BORDER).bg(PANEL)
            };
            f.buffer_mut().set_string(x, y, ch, style);
        }
    }
}

/// Convert MIDI note to piano roll row index.
/// C9=MIDI108=row0, A1=MIDI21=row87. Formula: row = 108 - midi.
pub(crate) fn midi_to_row_idx(midi: u8) -> Option<usize> {
    if midi < 21 || midi > 108 {
        return None;
    }
    let row = (108 - midi) as usize;
    if row < NOTE_ROWS.len() { Some(row) } else { None }
}

// ────────────────────────────────────────────────────── Modulation/Velocity ──

/// Per-step automation parameter names (index matches modulation_cursor 0-7).
const MOD_PARAMS: &[&str] = &["VEL", "GAIN", "PAN", "LP", "HP", "LFO", "SPD", "AMP"];

/// Per-voice colors for polyphonic VEL bars (voice 0 = primary).
#[allow(dead_code)]
const VOICE_COLORS: &[(u8, u8, u8)] = &[
    (50, 200, 80),   // primary: green
    (50, 200, 200),  // voice 2: cyan
    (220, 200, 50),  // voice 3: yellow
    (200, 80, 200),  // voice 4: magenta
    (80, 120, 255),  // voice 5: blue
    (255, 140, 50),  // voice 6: orange
    (180, 80, 255),  // voice 7: purple
    (200, 200, 200), // voice 8+: white
];

/// Extract the 0-127 value for automation parameter `param` from a note.
fn note_param_val(note: &seqterm_core::note::Note, param: usize) -> u8 {
    match param {
        0 => note.velocity,
        1 => note.gain,
        2 => note.pan,
        3 => note.lp,
        4 => note.hp,
        5 => note.lfo,
        6 => note.speed,
        7 => note.amp,
        _ => 0,
    }
}

/// Choose a color for a non-VEL automation parameter bar cell.
fn param_bar_color(param: usize, val: u8) -> Color {
    let v = val as u32;
    match param {
        1 => Color::Rgb((30 + v * 50 / 127) as u8, (100 + v * 80 / 127) as u8, (200) as u8), // GAIN: blue
        2 => {
            // PAN: left=red, center=gray, right=green
            if val < 60 { Color::Rgb(180, 60, 60) }
            else if val > 68 { Color::Rgb(60, 180, 60) }
            else { Color::Rgb(140, 140, 140) }
        }
        3 => Color::Rgb(50, (140 + v * 80 / 127) as u8, 220),          // LP: cyan
        4 => Color::Rgb((140 + v * 80 / 127) as u8, 50, 220),          // HP: purple
        5 => Color::Rgb((200 + v * 50 / 127) as u8, (160 + v * 60 / 127) as u8, 30), // LFO: amber
        6 => Color::Rgb(50, (180 + v * 60 / 127) as u8, (160 + v * 60 / 127) as u8), // SPD: teal
        7 => Color::Rgb((200 + v * 50 / 127) as u8, (110 + v * 80 / 127) as u8, 30), // AMP: orange
        _ => Color::White,
    }
}

fn draw_modulation_panel(f: &mut Frame, app: &App, area: Rect) {
    const EIGHTS: &[&str] = &[" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    const N_CHART: usize = 5;

    let proj = app.project.lock();
    let pat_key = app.tracker_state.pattern_key.as_deref().unwrap_or("KCK01");
    let cursor_step = app.tracker_state.cursor.0;
    let mod_active = app.tracker_section == 3;
    let mc = app.modulation_cursor.min(MOD_PARAMS.len() - 1);

    let pat = match proj.patterns.get(pat_key) {
        Some(p) => p,
        None => {
            f.render_widget(
                Paragraph::new("No pattern").block(
                    Block::default()
                        .title(" TRACK MODULATION ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(BORDER))
                        .style(Style::default().bg(PANEL)),
                ),
                area,
            );
            return;
        }
    };

    let cursor_note = pat.steps.get(cursor_step).cloned().unwrap_or_default();
    let param_name = MOD_PARAMS[mc];
    let cur_val = note_param_val(&cursor_note, mc);

    // ── Automation chart geometry ─────────────────────────────────────────────
    // Aligned with the piano roll: a 5-column left axis (matching the piano-roll
    // key-label width) and 2-column step cells (matching the piano-roll cell
    // width), so each bar position lines up vertically with the note above it.
    // The same `piano_step_scroll` drives both, so the piano-roll horizontal
    // scrollbar scrolls this panel's content in lockstep.
    let axis_w: u16 = 5;
    let cell_w: usize = 2;
    let chart_x = area.x.saturating_add(1 + axis_w);
    let chart_y = area.y + 1;
    // Mirror the piano roll's available-width calc (border + scrollbar margin)
    // so `visible_steps` matches exactly.
    let chart_cols = area.width.saturating_sub(axis_w + 3) as usize;
    let max_vis = chart_cols / cell_w;

    let step_scroll = app.piano_step_scroll;
    let visible_steps = max_vis.min(pat.length.saturating_sub(step_scroll));

    // Publish rect for mouse hit-testing in lib.rs (2 columns per step).
    app.vel_chart_area.set(Rect {
        x: chart_x,
        y: chart_y,
        width: (visible_steps * cell_w) as u16,
        height: N_CHART as u16,
    });

    let axis_style = if mod_active {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT)
    };

    let mut lines: Vec<Line> = Vec::with_capacity(10);

    // ── Automation chart rows ─────────────────────────────────────────────────
    for bar_row in 0..N_CHART {
        let is_top = bar_row == 0;
        let is_bottom = bar_row == N_CHART - 1;

        // Y-axis label (5 columns, aligned with the piano-roll key column).
        let axis_str = if is_top {
            "127 ┤".to_string()
        } else if is_bottom {
            "  0 └".to_string()
        } else {
            "    │".to_string()
        };
        let mut spans = vec![Span::styled(axis_str, axis_style)];

        for i in 0..visible_steps {
            let s = step_scroll + i;
            let step_note = pat.steps.get(s).cloned().unwrap_or_default();
            let is_cur  = s == cursor_step;
            let is_play = app.playing && app.current_step == s;
            let empty   = step_note.is_empty();
            let val     = note_param_val(&step_note, mc);

            // Eighth-block fill character.
            let cell: &str = if empty {
                if is_bottom { "·" } else { " " }
            } else {
                let eighths   = val as usize * N_CHART * 8 / 127;
                let full_rows = eighths / 8;
                let partial   = eighths % 8;
                let row_bot   = N_CHART - 1 - bar_row;
                if row_bot < full_rows              { EIGHTS[8] }
                else if row_bot == full_rows && partial > 0 { EIGHTS[partial] }
                else                                { EIGHTS[0] }
            };

            let fg = if is_cur {
                if empty { Color::Rgb(80, 80, 0) } else { Color::Yellow }
            } else if is_play {
                if empty { Color::Rgb(0, 60, 60) } else { Color::Cyan }
            } else if empty {
                Color::Rgb(40, 40, 40)
            } else if mc == 0 {
                // VEL: velocity colour palette (low=blue, mid=green, high=red).
                if val < 64 { Color::Rgb(60, 120, 220) }
                else if val < 100 { Color::Rgb(80, 200, 80) }
                else { Color::Rgb(220, 80, 60) }
            } else {
                param_bar_color(mc, val)
            };

            // 2-column cell, aligned with the piano-roll step cell above.
            spans.push(Span::styled(format!("{0}{0}", cell), Style::default().fg(fg)));
        }

        // Bottom row: step info appended after all step cells.
        if is_bottom {
            let voice_str = if mc == 0 {
                let vc = cursor_note.voice_count();
                if vc > 1 { format!(" [{} vc]", vc) } else { String::new() }
            } else { String::new() };
            let info = format!(" s:{:03} {}:{:03}{}", cursor_step + 1, param_name, cur_val, voice_str);
            spans.push(Span::styled(info, Style::default().fg(Color::DarkGray)));
        }

        lines.push(Line::from(spans));
    }

    // ── Parameter tabs row ────────────────────────────────────────────────────
    let mut tab_spans = Vec::new();
    tab_spans.push(Span::styled(
        if mod_active { "←→:" } else { "   " },
        Style::default().fg(if mod_active { Color::Yellow } else { Color::DarkGray }),
    ));
    for (i, &name) in MOD_PARAMS.iter().enumerate() {
        let selected = mod_active && i == mc;
        let style = if selected {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(if mod_active { Color::White } else { Color::DarkGray })
        };
        tab_spans.push(Span::styled(format!(" {} ", name), style));
    }
    if mod_active {
        tab_spans.push(Span::styled("  ↑↓=adjust  click/drag=set", Style::default().fg(Color::DarkGray)));
    }
    lines.push(Line::from(tab_spans));

    // ── Hint ──────────────────────────────────────────────────────────────────
    let hint = if mod_active {
        " ←→=param  ↑↓=adjust  click/drag=set  Tab=next"
    } else {
        " Tab=activate track modulation"
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(if mod_active { Color::Yellow } else { Color::DarkGray }),
    )));

    // ── Render ────────────────────────────────────────────────────────────────
    let mod_border = if mod_active { Style::default().fg(Color::Yellow) } else { Style::default().fg(BORDER) };
    let mod_title = if mod_active {
        format!(" TRACK MODULATION :: {} [ACTIVE] ", param_name)
    } else {
        " TRACK MODULATION ".to_string()
    };

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(mod_title)
                .title_style(Style::default().fg(HEADER))
                .borders(Borders::ALL)
                .border_style(mod_border)
                .style(Style::default().bg(PANEL)),
        ),
        area,
    );
}

// ─────────────────────────────────────────────────────────── FX Chain Panel ──

/// Character representation of a rotary knob position (0.0–1.0).
/// Arc sweeps CCW from 7-o'clock (min) to 5-o'clock (max).
pub(crate) fn knob_indicator(val: f32) -> char {
    match (val.clamp(0.0, 1.0) * 7.99) as usize {
        0 => '↙',
        1 => '←',
        2 => '↖',
        3 => '↑',
        4 => '↗',
        5 => '→',
        6 => '↘',
        _ => '↓',
    }
}

/// Arc fill string for a knob value using block characters (width = 8).
pub(crate) fn knob_arc(val: f32, width: usize) -> String {
    let filled = (val.clamp(0.0, 1.0) * width as f32).round() as usize;
    format!("{}{}", "▓".repeat(filled), "░".repeat(width.saturating_sub(filled)))
}

/// Resolve which slot_id the current tracker pattern belongs to (if any).
fn tracker_slot_id(app: &App) -> Option<u32> {
    let (row, col) = app.matrix_state.cursor;
    let row_key = ((b'A' + row as u8) as char).to_string();
    let clip_key = format!("{}{}", row_key, col);
    app.audio_slots.get(&clip_key).copied()
}

/// Uniform knob cell width (columns) shared by all three knob rows so the value
/// area lines up with the arc/label and with the per-parameter mouse hit-rect.
const FX_CELL_W: u16 = 13;

/// Draw a TRANSPORT-style 3-line button box with `label` at (x, y_top).
/// `border` colours the frame; `face` styles the label row. Returns the total
/// box width, or 0 if it would overflow `max_x`. Rows beyond `max_y_excl` are
/// clipped. The caller records the hit-rect.
pub(crate) fn fx_button_box(
    f: &mut Frame, x: u16, y_top: u16, max_x: u16, max_y_excl: u16,
    label: &str, border: Color, face: Style,
) -> u16 {
    let content = format!(" {label} ");
    let w = content.chars().count() as u16;
    let total = w + 2;
    if x + total > max_x { return 0; }
    let bstyle = Style::default().fg(border).bg(PANEL);
    let bg = Style::default().bg(PANEL);
    let bar = "─".repeat(w as usize);
    let mut emit = |yy: u16, line: Line| {
        if yy < max_y_excl {
            f.render_widget(Paragraph::new(line).style(bg), Rect::new(x, yy, total, 1));
        }
    };
    emit(y_top, Line::from(Span::styled(format!("╭{bar}╮"), bstyle)));
    emit(y_top + 1, Line::from(vec![
        Span::styled("│", bstyle),
        Span::styled(content, face),
        Span::styled("│", bstyle),
    ]));
    emit(y_top + 2, Line::from(Span::styled(format!("╰{bar}╯"), bstyle)));
    total
}

pub fn draw_fx_chain_panel(f: &mut Frame, app: &App, area: Rect) {
    let focused   = app.tracker_section == 4;
    let slot_sel  = app.tracker_fx_slot;
    let param_sel = app.tracker_fx_param;
    let learning  = app.tracker_fx_midi_learn;

    let border_style = if focused {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(BORDER)
    };

    let slot_id = tracker_slot_id(app);
    let empty_chain = vec![];
    let chain = slot_id
        .and_then(|sid| app.audio_slot_fx.get(&sid))
        .unwrap_or(&empty_chain);

    // Reset every-frame mouse rects; set below only where a control is drawn.
    app.tracker_fx_param_rects.set([Rect::default(); 8]);
    app.tracker_fx_slot_rects.set([Rect::default(); 5]);
    app.tracker_fx_add_rect.set(Rect::default());
    app.tracker_fx_enable_rect.set(Rect::default());
    app.tracker_fx_delete_rect.set(Rect::default());
    app.tracker_fx_move_prev_rect.set(Rect::default());
    app.tracker_fx_move_next_rect.set(Rect::default());

    // FX applies only to the active pattern's clip (the matrix-cursor cell).
    let (mr, mc) = app.matrix_state.cursor;
    let clip_lbl = format!("{}{}", (b'A' + mr as u8) as char, mc + 1);

    // ── Block + inner ─────────────────────────────────────────────────────────
    let title = if focused {
        format!(" FX CHAIN :: {} [ACTIVE] ", clip_lbl)
    } else {
        format!(" FX CHAIN :: {} ", clip_lbl)
    };
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 { return; }
    let cx = inner.x;
    let cy = inner.y;
    let max_x = cx + inner.width;
    let max_y = cy + inner.height;

    let bg = Style::default().bg(PANEL);
    let put = |f: &mut Frame, line: Line, y: u16| {
        if y < max_y {
            f.render_widget(Paragraph::new(line).style(bg), Rect::new(inner.x, y, inner.width, 1));
        }
    };

    // ── Hint ──────────────────────────────────────────────────────────────────
    let hint = if focused {
        if learning.is_some() {
            "  Move a MIDI CC to bind  |  Esc=cancel".to_string()
        } else {
            format!("  applies to {clip_lbl} only · ←→=fx ↑↓=param wheel=value · click boxes below")
        }
    } else {
        format!("  Tab=enter · FX here affect clip {clip_lbl} only")
    };
    put(f, Line::from(Span::styled(hint, Style::default().fg(BORDER))), cy);
    let mut y = cy + 1;

    // ── Effect selector: one TRANSPORT-style box per FX + a [+ ADD] box ───────
    if slot_id.is_none() {
        put(f, Line::from(Span::styled(
            "  No audio slot — assign SF2 or audio file to this pattern first",
            Style::default().fg(Color::DarkGray))), y);
        return;
    }

    let mut slot_rects = [Rect::default(); 5];
    let mut bx = cx;
    for (i, entry) in chain.iter().enumerate().take(crate::MAX_TRACKER_FX) {
        let is_sel  = i == slot_sel && focused;
        let label   = format!("{}:{}", i + 1, entry.kind.label());
        let border  = if is_sel { Color::Yellow }
                      else if entry.enabled { Color::Rgb(56, 200, 100) }
                      else { Color::Rgb(90, 95, 105) };
        let face = if is_sel {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if entry.enabled {
            Style::default().fg(Color::Rgb(120, 220, 150)).bg(PANEL)
        } else {
            Style::default().fg(Color::Rgb(120, 125, 135)).bg(PANEL).add_modifier(Modifier::CROSSED_OUT)
        };
        let w = fx_button_box(f, bx, y, max_x, max_y, &label, border, face);
        if w == 0 { break; }
        slot_rects[i] = Rect::new(bx, y, w, 3);
        bx += w + 1;
    }
    if chain.len() < crate::MAX_TRACKER_FX {
        let w = fx_button_box(f, bx, y, max_x, max_y, "+ ADD",
            Color::Rgb(100, 160, 220),
            Style::default().fg(Color::Rgb(150, 195, 245)).bg(PANEL).add_modifier(Modifier::BOLD));
        if w > 0 { app.tracker_fx_add_rect.set(Rect::new(bx, y, w, 3)); }
    }
    app.tracker_fx_slot_rects.set(slot_rects);
    y += 3; // box height (no spacer — keep the tab compact)

    // ── ROUTING subsection: signal order IN → … → OUT ─────────────────────────
    let dim = Style::default().fg(Color::Rgb(120, 130, 150));
    let mut rt: Vec<Span> = vec![
        Span::styled(" ROUTING ", Style::default().fg(HEADER).add_modifier(Modifier::BOLD)),
        Span::styled("IN", dim),
    ];
    if chain.is_empty() {
        rt.push(Span::styled(" → OUT (no FX)", dim));
    } else {
        for (i, e) in chain.iter().enumerate() {
            rt.push(Span::styled(" → ", dim));
            let st = if i == slot_sel {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if e.enabled {
                Style::default().fg(Color::Rgb(120, 220, 150))
            } else {
                Style::default().fg(Color::Rgb(110, 115, 125)).add_modifier(Modifier::CROSSED_OUT)
            };
            rt.push(Span::styled(format!("{}:{}", i + 1, e.kind.label()), st));
        }
        rt.push(Span::styled(" → OUT", dim));
    }
    put(f, Line::from(rt), y);
    y += 1;

    // ── Rotary knobs for the selected effect: one row, scrolled horizontally so
    //    the selected parameter stays visible (fits the uniform tab height). ───
    if let Some(entry) = chain.get(slot_sel) {
        let descs = fx_param_descs(entry.kind);
        let n = descs.len();
        let mut param_rects = [Rect::default(); 8];
        let avail   = inner.width.saturating_sub(2) as usize;
        let visible = (avail / FX_CELL_W as usize).max(1);
        let start   = if focused && param_sel >= visible { param_sel + 1 - visible } else { 0 };
        let end     = (start + visible).min(n);
        let top_y   = y;

        let mut top_spans: Vec<Span> = vec![Span::raw("  ")];
        let mut mid_spans: Vec<Span> = vec![Span::raw("  ")];
        let mut lbl_spans: Vec<Span> = vec![Span::raw("  ")];
        for (ci, pi) in (start..end).enumerate() {
            let val   = entry.params.get(pi).copied().unwrap_or(0.0);
            let is_p  = pi == param_sel && focused;
            let learn_this = learning == Some((slot_sel, pi));

            let px = cx + 2 + (ci as u16) * FX_CELL_W;
            if pi < 8 { param_rects[pi] = Rect::new(px, top_y, FX_CELL_W, 3); }

            let col_k = if is_p { Color::Yellow } else { Color::Rgb(100,160,220) };
            top_spans.push(Span::styled(
                format!("{:<width$}", format!("[{}]", knob_arc(val, 8)), width = FX_CELL_W as usize),
                Style::default().fg(col_k)));

            let ind   = knob_indicator(val);
            let cc_s  = entry.cc_bindings.get(pi).copied().flatten()
                .map(|cc| format!("CC{cc:2}"))
                .unwrap_or_else(|| "    ".to_string());
            let col_v = if learn_this { Color::Magenta } else if is_p { Color::Yellow } else { Color::White };
            let ind_str = if learn_this { '◎' } else { ind };
            mid_spans.push(Span::styled(
                format!("{:<width$}", format!(" {ind_str}{:4.2} {cc_s}", val), width = FX_CELL_W as usize),
                Style::default().fg(col_v).add_modifier(if is_p { Modifier::BOLD } else { Modifier::empty() })));

            let name = descs.get(pi).map(|d| d.name).unwrap_or("?");
            lbl_spans.push(Span::styled(
                format!(" {:<width$}", name, width = (FX_CELL_W - 1) as usize),
                Style::default().fg(if is_p { Color::Yellow } else { HEADER })));
        }
        // Overflow markers when more params exist than fit on the row.
        if start > 0 { lbl_spans.insert(1, Span::styled("←", dim)); }
        if end < n   { lbl_spans.push(Span::styled(format!(" +{} →", n - end), dim)); }
        put(f, Line::from(top_spans), top_y);
        put(f, Line::from(mid_spans), top_y + 1);
        put(f, Line::from(lbl_spans), top_y + 2);
        app.tracker_fx_param_rects.set(param_rects);
    } else {
        put(f, Line::from(Span::styled(
            "  No FX — click [+ ADD] (or press a) to insert one",
            Style::default().fg(Color::DarkGray))), y);
    }
    y += 3;

    // ── Controls (TRANSPORT-style boxes): ON/OFF · DELETE · MOVE◀ · MOVE▶ ─────
    if let Some(entry) = chain.get(slot_sel) {
        let mut bx = cx;

        // ON/OFF
        let (en_lbl, en_border, en_face) = if entry.enabled {
            ("● ON", Color::Rgb(56, 200, 100),
             Style::default().fg(Color::Black).bg(Color::Rgb(56, 200, 100)).add_modifier(Modifier::BOLD))
        } else {
            ("○ OFF", Color::Rgb(90, 95, 105),
             Style::default().fg(Color::Rgb(180, 185, 195)).bg(PANEL))
        };
        let w = fx_button_box(f, bx, y, max_x, max_y, en_lbl, en_border, en_face);
        if w > 0 { app.tracker_fx_enable_rect.set(Rect::new(bx, y, w, 3)); bx += w + 1; }

        // DELETE
        let w = fx_button_box(f, bx, y, max_x, max_y, "✖ DEL", Color::Rgb(200, 70, 70),
            Style::default().fg(Color::White).bg(Color::Rgb(170, 50, 50)).add_modifier(Modifier::BOLD));
        if w > 0 { app.tracker_fx_delete_rect.set(Rect::new(bx, y, w, 3)); bx += w + 1; }

        // MOVE◀ (earlier in the chain)
        let can_prev = slot_sel > 0;
        let mv_col = |on: bool| if on { Color::Rgb(100, 160, 220) } else { Color::Rgb(80, 85, 95) };
        let mv_face = |on: bool| if on {
            Style::default().fg(Color::Rgb(150, 195, 245)).bg(PANEL)
        } else {
            Style::default().fg(Color::Rgb(90, 95, 105)).bg(PANEL)
        };
        let w = fx_button_box(f, bx, y, max_x, max_y, "◀ MOVE", mv_col(can_prev), mv_face(can_prev));
        if w > 0 { app.tracker_fx_move_prev_rect.set(Rect::new(bx, y, w, 3)); bx += w + 1; }

        // MOVE▶ (later in the chain)
        let can_next = slot_sel + 1 < chain.len();
        let w = fx_button_box(f, bx, y, max_x, max_y, "MOVE ▶", mv_col(can_next), mv_face(can_next));
        if w > 0 { app.tracker_fx_move_next_rect.set(Rect::new(bx, y, w, 3)); }
    }
}
