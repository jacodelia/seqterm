use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Table, TableState, Wrap,
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

    // RHYTHM → FIGURE modal floats over everything when open.
    if app.rhythm_modal.is_some() {
        draw_rhythm_modal(f, app, area);
    } else {
        app.rhythm_modal_rects.set([Rect::default(); 12]);
    }
}

/// Friendly name for a grid note value (resolution denominator).
fn note_value_label(den: i64) -> &'static str {
    match den {
        1 => "whole",
        2 => "half",
        4 => "quarter",
        8 => "8th",
        16 => "16th",
        32 => "32nd",
        64 => "64th",
        _ => "note",
    }
}

/// Centered modal listing irregular-rhythm groupings (2…12) to quantize the
/// selection onto. The base note value is the current grid view, so the result is
/// e.g. a triplet of whole notes or a quintuplet of quarters. Rows are clickable.
fn draw_rhythm_modal(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::widgets::Clear;
    let figs = crate::RHYTHM_FIGURES;
    let cursor = app.rhythm_modal.unwrap_or(0).min(figs.len() - 1);
    let sel_n = app.piano_selection.len() + app.piano_event_selection.len();
    let den = app.edit_state.resolution.den();
    let base_name = note_value_label(den);

    let w: u16 = 44;
    let h: u16 = figs.len() as u16 + 6;
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let modal = Rect::new(x, y, w.min(area.width), h.min(area.height));

    f.render_widget(Clear, modal);
    let block = Block::default()
        .title(" RHYTHM FIGURE ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(170, 80, 200)))
        .style(Style::default().bg(PANEL_DARK));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            format!(" Quantize {sel_n} note(s) · base 1/{den} ({base_name})"),
            Style::default().fg(Color::Rgb(180, 190, 200)),
        )),
    ];
    let mut rects = [Rect::default(); 12];
    for (i, &count) in figs.iter().enumerate() {
        let is_sel = i == cursor;
        let style = if is_sel {
            Style::default().fg(Color::Black).bg(HEADER).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(200, 205, 215)).bg(PANEL_DARK)
        };
        let marker = if is_sel { "▶ " } else { "  " };
        let m = crate::tuplet_partner(count);
        let name = match count {
            3 => "  triplet",
            5 => "  quintuplet",
            6 => "  sextuplet",
            7 => "  septuplet",
            _ => "",
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{count:>2} in {m}  ({count}:{m}){name}"),
            style,
        )));
        let ry = inner.y + 1 + i as u16;
        if ry < inner.y + inner.height {
            rects[i] = Rect::new(inner.x, ry, inner.width, 1);
        }
    }
    let (mode_label, mode_col) = if app.rhythm_modal_add_layer {
        ("[a] mode: ADD LAYER (stack new layer, keep notes)", Color::Rgb(120, 220, 150))
    } else {
        ("[a] mode: REPLACE (retime selection)", Color::Rgb(200, 180, 90))
    };
    lines.push(Line::from(Span::styled(mode_label,
        Style::default().fg(mode_col).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(Span::styled(
        " ↑↓ select · digit=count · a=mode · Enter apply · Esc",
        Style::default().fg(Color::DarkGray),
    )));
    app.rhythm_modal_rects.set(rects);
    f.render_widget(Paragraph::new(lines).style(Style::default().bg(PANEL_DARK)), inner);
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

    // Display zoom: when the piano-roll grid is zoomed finer, each step splits into
    // `pdiv` rows in the tracker too, so off-grid rational events show on decimal
    // sub-lines (000.0, 000.5…). `pdiv == 1` = the classic one-row-per-step table.
    let pdiv = pattern
        .map(|p| display_pdiv(p.step_beats(), app.edit_state.resolution))
        .unwrap_or(1);

    // Visible area height — store the visible STEP count (rows / pdiv) for scroll
    // clamping and mouse hit-testing, which both work in step units.
    let inner_h = area.height.saturating_sub(3) as usize; // borders + header
    let visible_steps = (inner_h / pdiv).max(1);
    app.tracker_view_height.set(visible_steps);
    app.tracker_table_area.set(area);
    let scroll = app.tracker_scroll;

    // When editing, the active column gets bright-yellow; all others get dim gold.
    let edit_col_idx = app.tracker_edit_field + 1; // col index (0=LN, 1=NOTE, ..., 10=PROB)

    // Highest exact-event pitch landing on each DISPLAY cell (step*pdiv + sub), so
    // the tracker NOTE column reflects notes from the piano roll's rational layer —
    // on the main step row (sub 0) and on the decimal sub-lines when zoomed. Uses
    // the same round-to-nearest-cell mapping as the piano roll.
    let cell_top: Vec<Option<u8>> = if let Some(pat) = pattern {
        let step_b = pat.step_beats();
        let total_cells = pat.length * pdiv;
        let mut tops = vec![None; total_cells];
        let sub_b = step_b / pdiv as i64;
        if !sub_b.is_zero() {
            let half = seqterm_core::RationalTime::new(1, 2);
            for ev in &pat.events {
                let c = (ev.start / sub_b + half).floor();
                if c < 0 || c as usize >= total_cells { continue; }
                if let Some(m) = ev.note.to_midi() {
                    let slot = &mut tops[c as usize];
                    *slot = Some(slot.map_or(m, |t: u8| t.max(m)));
                }
            }
        }
        tops
    } else {
        Vec::new()
    };

    let rows: Vec<Row> = if let Some(pat) = pattern {
        (0..pat.length)
            .flat_map(|step| {
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

                // Note display: the TOP note sounding at this step — the step
                // note + its chord voices AND any exact rational events in the
                // step's span. With more than one note we show only the highest
                // (top of the chord), so the tracker mirrors the piano roll.
                let mut voices: Vec<u8> = Vec::new();
                if !note.is_empty() {
                    if let Some(m) = seqterm_core::note::parse_note_name(&note.note) { voices.push(m); }
                    for cn in &note.chord_notes {
                        if let Some(m) = seqterm_core::note::parse_note_name(cn) { voices.push(m); }
                    }
                }
                if let Some(Some(m)) = cell_top.get(step * pdiv) { voices.push(*m); }
                let top_midi = voices.iter().copied().max();
                let is_chord = voices.len() > 1;
                let note_str = match top_midi {
                    None => "···".to_string(),
                    // Keep the stored name when it already is the top single note.
                    Some(m) if !is_chord && seqterm_core::note::parse_note_name(&note.note) == Some(m)
                        => note.note.clone(),
                    Some(m) => seqterm_core::Note::from_midi(m, 100)
                        .map(|n| n.note)
                        .unwrap_or_else(|_| "···".to_string()),
                };
                let note_style = if is_cursor || is_playing {
                    base_style
                } else if top_midi.is_none() {
                    Style::default().fg(Color::DarkGray).bg(beat_bg)
                } else if is_chord {
                    // Chord/poly present: amber to flag "top of stack shown".
                    Style::default().fg(Color::Rgb(255, 190, 90)).bg(beat_bg)
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
                let mut out = vec![Row::new(cells).height(1)];

                // Decimal sub-lines (zoom): one per extra display cell in the step.
                // They surface off-grid rational events at their exact sub-position;
                // the editable cursor stays on the main (.0) row.
                if pdiv > 1 {
                    let sub_bg = Color::Rgb(16, 16, 20);
                    let dim = Style::default().fg(Color::Rgb(85, 85, 100)).bg(sub_bg);
                    for sub in 1..pdiv {
                        // Compact fractional index of the sub-line within the step
                        // (e.g. `.50`, `.33`); the LN column is too narrow for the
                        // full `000.50`, so the fraction reads as the step's decimal.
                        let frac = sub as f64 / pdiv as f64;
                        let ln = format!("{:.2}", frac).trim_start_matches('0').to_string();
                        let top = cell_top.get(step * pdiv + sub).copied().flatten();
                        let nstr = match top {
                            Some(m) => seqterm_core::Note::from_midi(m, 100)
                                .map(|n| n.note)
                                .unwrap_or_else(|_| "···".to_string()),
                            None => "···".to_string(),
                        };
                        let nstyle = if top.is_some() {
                            Style::default().fg(Color::Rgb(90, 160, 210)).bg(sub_bg)
                        } else {
                            dim
                        };
                        out.push(Row::new(vec![
                            Cell::from(ln).style(Style::default().fg(Color::Rgb(110, 110, 130)).bg(sub_bg)),
                            Cell::from(nstr).style(nstyle),
                            Cell::from("··").style(dim),
                            Cell::from("··").style(dim),
                            Cell::from("··").style(dim),
                            Cell::from("··").style(dim),
                            Cell::from("···").style(dim),
                            Cell::from("···").style(dim),
                            Cell::from("···").style(dim),
                            Cell::from("····").style(dim),
                            Cell::from("···").style(dim),
                        ]).height(1));
                    }
                }
                out
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
            format!(" i=ins v=vis Esc=norm │ GRID {} ", app.edit_state.grid_label())
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
    // Offset and selection are in TABLE ROW units (pdiv rows per step); the cursor
    // lands on the step's main (.0) row.
    let cursor_row = app.tracker_state.cursor.0;
    let mut table_state = TableState::default();
    *table_state.offset_mut() = scroll * pdiv;
    table_state.select(Some(cursor_row * pdiv));

    // Render scrollbar on the right.
    let scrollbar_area = Rect {
        x: area.x + area.width.saturating_sub(1),
        y: area.y + 2,
        width: 1,
        height: area.height.saturating_sub(3),
    };

    f.render_stateful_widget(table, area, &mut table_state);

    let total = pat_len;
    if total > visible_steps {
        let mut sb_state = ScrollbarState::new(total)
            .viewport_content_length(visible_steps)
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

    let (swing, random, prob, pat_len, pat_name, euclid_fill, euclid_len, euclid_enabled,
         humanization, evolution, prob_lock, microshift, time_sig_num, time_sig_den,
         beat_groups) = proj
        .patterns
        .get(pat_key)
        .map(|p| (p.swing.saturating_sub(50), p.random, p.prob, p.length, p.name.clone(),
                  p.euclid_fill, p.euclid_len, p.euclid_enabled, p.humanization, p.evolution,
                  p.prob_lock, p.microshift, p.time_sig_num, p.time_sig_den,
                  p.effective_groups()))
        .unwrap_or((0, 0, 0, 16, pat_key.to_string(), 3, 16, false, 0, 0, false, 0, 4, 4,
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
            if euclid_enabled { "  ON " } else { "  off" },
            if euclid_enabled {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
        Span::styled(
            if gen_active && (gc == 8 || gc == 9) { "  ←→=adjust Enter=on/off" } else { "" },
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
const TRACKER_TAB_PANEL_H: u16 = 16;

/// Width (cols) of the vertical RHYTHM tab on the right edge of the piano roll.
const RHYTHM_STRIP_W: u16 = 14;

fn draw_piano_roll_panel(f: &mut Frame, app: &App, area: Rect) {
    // Right column: [piano roll | RHYTHM strip] (top) | tab bar (1 row) | panel.
    let tab = app.tracker_tab.min(3);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(7),
            Constraint::Length(1),
            Constraint::Length(TRACKER_TAB_PANEL_H),
        ])
        .split(area);

    // Split the piano-roll row horizontally: roll on the left, RHYTHM tab on the right.
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(RHYTHM_STRIP_W)])
        .split(chunks[0]);

    let mut tr = app.tracker_panel_rects.get();
    tr[1] = top[0];
    app.tracker_panel_rects.set(tr);

    draw_piano_roll(f, app, top[0]);
    draw_rhythm_strip(f, app, top[1]);
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
    // Render in the user's customised display order; `rects[slot]` hit-tests to the
    // logical tab id at that slot (`tracker_tab_order[slot]`).
    for slot in 0..4 {
        let id = app.tracker_tab_order[slot] as usize;
        let label = TRACKER_TAB_LABELS[id];
        let is_sel     = app.tracker_tab == id;
        let is_focused = is_sel && app.tracker_section == TRACKER_TAB_SECTIONS[id];
        let is_fav     = app.settings.pattern_fav_tab.min(3) == id;
        let text = if is_fav { format!(" ★{} ", label) } else { format!(" {} ", label) };
        let w = text.chars().count() as u16;
        if x + w <= area.x + area.width {
            rects[slot] = Rect::new(x, area.y, w, 1);
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

/// RHYTHM vertical tab — sits on the right edge of the piano roll. Shows the
/// grid-only readout (`GRID 1/16 · snap grid`, **never** the per-selection tuplet)
/// and three stacked TRANSPORT-style clickable boxes, mirroring the keyboard path:
///   ZOOM− / ZOOM+  — coarser/finer displayed grid (down to 1/64) = `Ctrl+-`/`Ctrl+=`
///   FIGURE         — with a selection, open the irregular-rhythm modal to quantise
///                    the selected notes into a grouping (2…12); else drop one = `g`
/// Active when the piano roll (section 1) or step table (section 0) has focus.
fn draw_rhythm_strip(f: &mut Frame, app: &App, area: Rect) {
    let active = matches!(app.tracker_section, 0 | 1);
    let border_col = if active { Color::Yellow } else { BORDER };
    let block = Block::default()
        .title(" RHYTHM ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_col))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut rects = [Rect::default(); 5];
    if inner.height == 0 || inner.width == 0 {
        app.tracker_rhythm_btn_rects.set(rects);
        return;
    }

    // Grid-only readout (resolution + snap; the tuplet is local to a selection and
    // must never appear here as if it changed the global grid).
    let grid_txt = format!("GRID\n{}", app.edit_state.grid_label());
    let grid_h = 4.min(inner.height);
    f.render_widget(
        Paragraph::new(grid_txt)
            .style(Style::default().fg(HEADER).bg(PANEL))
            .wrap(Wrap { trim: true }),
        Rect::new(inner.x, inner.y, inner.width, grid_h),
    );

    // Stacked buttons below the readout.
    let buttons: [(&str, Color); 3] = [
        ("ZOOM −", Color::Rgb(70, 110, 150)),
        ("ZOOM +", Color::Rgb(70, 130, 110)),
        ("FIGURE", Color::Rgb(190, 150, 70)),
    ];

    let max_x = inner.x + inner.width;
    let max_y = inner.y + inner.height;
    let mut y = inner.y + grid_h + 1;
    for (i, (label, col)) in buttons.iter().enumerate() {
        let face = Style::default().fg(Color::White).bg(PANEL).add_modifier(Modifier::BOLD);
        let w = fx_button_box(f, inner.x, y, max_x, max_y, label, *col, face);
        if w == 0 {
            break; // ran out of vertical room
        }
        rects[i] = Rect::new(inner.x, y, w, 3.min(max_y.saturating_sub(y)));
        y += 4; // 3 box rows + 1 gap
    }
    app.tracker_rhythm_btn_rects.set(rects);
}

/// Sub-cells per pattern step at the current edit resolution (display zoom).
/// `1` = the edit grid matches the step grid (no extra subdivision); higher =
/// each step is split into finer cells so corcheas…semifusas are visible/placeable
/// within a beat WITHOUT changing the pattern length. Clamped to 8 for legibility.
pub(crate) fn display_pdiv(
    step_beats: seqterm_core::RationalTime,
    res: seqterm_core::Resolution,
) -> usize {
    let cell = res.step_beats();
    if cell.is_zero() || step_beats.is_zero() {
        return 1;
    }
    let q = step_beats / cell;
    ((q + seqterm_core::RationalTime::new(1, 2)).floor().max(1) as usize).clamp(1, 8)
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

    // ── Display zoom (Phase 6) ────────────────────────────────────────────────
    // The edit resolution subdivides each pattern step into `pdiv` sub-cells so we
    // can SHOW (and place) finer rhythms within a beat WITHOUT changing the
    // pattern's length. `pdiv == 1` when the edit grid matches the step grid (the
    // legacy behaviour). Each sub-cell spans `sub_b` beats exactly.
    let step_b = pat.step_beats();
    let pdiv = display_pdiv(step_b, app.edit_state.resolution);

    // Non-uniform grid: straight `pdiv` cells per step, but tuplet regions subdivide
    // by their own count (a quintuplet → 5·pdiv cells), so irregular rhythms draw at
    // their real subdivision. Cells have uniform visual width (2 columns each); the
    // grid maps every column to an exact beat. Single source of truth shared with the
    // mouse hit-test, modulation chart and delete (see `Pattern::piano_grid`).
    let grid = pat.piano_grid(pdiv);
    let total_cells = grid.total_cells().max(1);

    // Columns available for cells (subtract key area, borders, scrollbar).
    let step_display_w = area.width.saturating_sub(key_w as u16 + 3) as usize;
    // Each cell is 2 columns wide.
    let max_visible_cells = (step_display_w / 2).max(1);
    let step_scroll = app.piano_step_scroll.min(pat.length.saturating_sub(1));
    // First visible cell follows the step-based scroll position.
    let first_cell = grid.nearest_cell(step_b * step_scroll as i64).min(total_cells - 1);
    let visible_cells = max_visible_cells.min(total_cells - first_cell);
    let last_cell_excl = first_cell + visible_cells;
    // Distinct steps spanned by the visible cells — published so process_events can
    // clamp horizontal scroll (still step-granular).
    let visible_steps = {
        let last_beat = grid.cell_start(last_cell_excl.saturating_sub(1));
        let last_step = (last_beat / step_b).floor() as usize;
        (last_step + 1).saturating_sub(step_scroll).max(1)
    };
    app.piano_visible_steps.set(visible_steps);
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
    let edit_grid = app.edit_state.display_grid_beats();
    let pat_step_beats = pat.step_beats();
    let on_edit_grid = |step: usize| -> bool {
        if edit_grid.is_zero() {
            return false;
        }
        let abs = pat_step_beats * step as i64;
        (abs / edit_grid).frac().is_zero()
    };

    // Build step header: beat numbers 01..N on each step's first cell (2 cols),
    // remaining cells of the step padded blank. Colored by grouping.
    let mut hdr_spans: Vec<Span> =
        vec![Span::styled(format!("{:<5}", " "), Style::default())];
    for cell in first_cell..last_cell_excl {
        let cell_beat = grid.cell_start(cell);
        let at_step_start = (cell_beat / step_b).frac().is_zero();
        if at_step_start {
            let step = (cell_beat / step_b).floor() as usize;
            let beat = step % time_sig_num; // 0-based position within measure
            let style = if beat == 0 {
                Style::default().fg(HEADER).add_modifier(Modifier::BOLD)
            } else if group_starts.contains(&beat) {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            hdr_spans.push(Span::styled(format!("{:02}", beat + 1), style));
        } else {
            hdr_spans.push(Span::styled("  ".to_string(), Style::default()));
        }
    }
    let hdr_line = Line::from(hdr_spans);

    // Visible note rows (scroll-adjusted; published for scrollbar thumb sizing).
    // Note rows start immediately below the header — figure brackets are now drawn
    // ON the note grid (see `bracket_overlays` below), not on a separate row.
    let visible_rows = inner_h.min(NOTE_ROWS.len() - note_scroll);
    app.piano_visible_rows.set(visible_rows.max(1));

    // Build note grid lines.
    let mut grid_lines: Vec<Line> = Vec::with_capacity(visible_rows + 2);
    grid_lines.push(hdr_line);

    // Polyphonic note map at CELL granularity: cell → Vec<(row_idx, gate_cells)>.
    // Cell indices come from the non-uniform `grid`, so step notes and tuplet events
    // land on the same column the mouse/delete resolve to. Gate width is in cells.
    let mut note_map: Vec<Vec<(usize, usize)>> = vec![vec![]; total_cells];
    let cells_between = |a: seqterm_core::RationalTime, b: seqterm_core::RationalTime| -> usize {
        (grid.nearest_cell(b).saturating_sub(grid.nearest_cell(a))).max(1)
    };
    for (step, note) in pat.steps.iter().enumerate() {
        if note.is_empty() { continue; }
        let start_b = step_b * step as i64;
        let cell = grid.nearest_cell(start_b);
        let gate_steps = ((note.gate as usize + 99) / 100).max(1);
        let gate_cells = cells_between(start_b, start_b + step_b * gate_steps as i64);
        // Primary note voice.
        if let Some(midi) = seqterm_core::note::parse_note_name(&note.note) {
            if let Some(row_idx) = midi_to_row_idx(midi) {
                note_map[cell].push((row_idx, gate_cells));
            }
        }
        // Chord voices.
        for chord_name in &note.chord_notes {
            if let Some(midi) = seqterm_core::note::parse_note_name(chord_name) {
                if let Some(row_idx) = midi_to_row_idx(midi) {
                    note_map[cell].push((row_idx, gate_cells));
                }
            }
        }
    }

    // Overlay the exact rational-note layer (Phase 6): map each event onto its grid
    // cell. Inside a tuplet region the grid has a cell per tuplet note, so each event
    // gets its own column; it always plays via `to_events` regardless of zoom.
    for ev in &pat.events {
        let c = grid.nearest_cell(ev.start);
        if c >= total_cells { continue; }
        let gate_cells = cells_between(ev.start, ev.start + ev.duration);
        if let Some(midi) = ev.note.to_midi() {
            if let Some(row_idx) = midi_to_row_idx(midi) {
                note_map[c].push((row_idx, gate_cells));
            }
        }
    }

    // ── Figure (tuplet) overlays — drawn ON the note grid ─────────────────────
    // Each `TupletMark` is local to its own region, so several different figures
    // (3, 5, 7…) coexist independently. Instead of a separate bracket row, we draw
    // each mark's `⌐──N──¬` bracket on the grid row directly above its top note,
    // and tint the grouped notes. `bracket_overlays[row]` holds one char per grid
    // column for that row; `mark_spans` lists each mark's global cell range.
    let total_cols = key_w + visible_cells * 2;
    let last_visible_cell = last_cell_excl;
    let mut bracket_overlays: std::collections::HashMap<usize, Vec<char>> =
        std::collections::HashMap::new();
    let mut mark_spans: Vec<(usize, usize)> = Vec::new(); // (sc, ec) inclusive global cells
    if total_cols > key_w {
        for mk in &pat.tuplet_marks {
            let c0 = grid.nearest_cell(mk.start);
            let c1 = grid.nearest_cell(mk.start + mk.duration);
            if c1 <= c0 {
                continue;
            }
            let (sc, ec) = (c0, c1.saturating_sub(1));
            mark_spans.push((sc, ec));
            // Skip the bracket if the mark is entirely outside the viewport.
            if ec < first_cell || sc >= last_visible_cell {
                continue;
            }
            // Top note row (smallest row_idx = highest pitch) within the span.
            let mut top_row: Option<usize> = None;
            for gc in sc..=ec.min(total_cells.saturating_sub(1)) {
                if let Some(voices) = note_map.get(gc) {
                    for (ri, _) in voices {
                        top_row = Some(top_row.map_or(*ri, |t: usize| t.min(*ri)));
                    }
                }
            }
            // Bracket sits on the row just above the top note (or on it at the top).
            // Nested figures stack: a mark that contains other marks is pushed one
            // extra row up per level of nesting, so a child figure inside a parent
            // gets its own visible bracket+digit instead of being hidden under it.
            let Some(tr) = top_row else { continue };
            let nest_depth = pat.tuplet_marks.iter().filter(|o| {
                let o_end = o.start + o.duration;
                o.start >= mk.start && o_end <= mk.start + mk.duration
                    && !(o.start == mk.start && o_end == mk.start + mk.duration)
            }).count();
            let bracket_row = tr.saturating_sub(1 + nest_depth);
            let chars = bracket_overlays
                .entry(bracket_row)
                .or_insert_with(|| vec![' '; total_cols]);
            let vsc = sc.max(first_cell);
            let vec_ = ec.min(last_visible_cell.saturating_sub(1));
            let start_col = key_w + (vsc - first_cell) * 2;
            let end_col = key_w + (vec_ - first_cell) * 2;
            let last = (end_col + 1).min(total_cols - 1);
            for c in start_col..=last {
                if chars[c] == ' ' {
                    chars[c] = '─';
                }
            }
            chars[start_col] = '⌐';
            chars[last] = '¬';
            // Grouping digit(s) over the bracket centre.
            let label = mk.count.to_string();
            let mid = start_col + (last - start_col) / 2;
            for (k, ch) in label.chars().enumerate() {
                let pos = mid + k;
                if pos <= last {
                    chars[pos] = ch;
                }
            }
        }
    }

    let piano_active = app.tracker_section == 1;
    let piano_cursor_step = app.piano_cursor.1;
    let piano_cursor_row = app.piano_cursor.0;
    // Fine (exact) cursor cell — where `\` / a sub-cell click drops an event.
    let fine_cell: i64 = grid.nearest_cell(app.piano_fine_beat) as i64;
    // In-progress rubber-band rectangle (global-cell / row corners, zoom-aware) →
    // draw its border. Corners are sub-cell indices so the marquee is exact at any
    // zoom (cell == step*pdiv + sub in the loop below).
    let marquee: Option<(usize, usize, usize, usize)> =
        match (app.piano_select_anchor, app.piano_select_cur) {
            (Some((a_c, a_r)), Some((c_c, c_r))) => {
                Some((a_c.min(c_c), a_c.max(c_c), a_r.min(c_r), a_r.max(c_r)))
            }
            _ => None,
        };

    // Cells holding a CURRENTLY-SELECTED note (step notes + exact events), keyed by
    // (global_cell, row_idx), so selected notes are clearly recoloured after the
    // rubber-band is released — for both layers.
    let selected_cells: std::collections::HashSet<(usize, usize)> = {
        let mut set = std::collections::HashSet::new();
        for &s in &app.piano_selection {
            let cell = grid.nearest_cell(step_b * s as i64);
            if let Some(note) = pat.steps.get(s) {
                for name in std::iter::once(note.note.as_str())
                    .chain(note.chord_notes.iter().map(|x| x.as_str()))
                {
                    if let Some(ri) = seqterm_core::note::parse_note_name(name).and_then(midi_to_row_idx) {
                        set.insert((cell, ri));
                    }
                }
            }
        }
        for &i in &app.piano_event_selection {
            if let Some(ev) = pat.events.get(i) {
                let c = grid.nearest_cell(ev.start);
                if let Some(ri) = ev.note.to_midi().and_then(midi_to_row_idx) {
                    set.insert((c, ri));
                }
            }
        }
        set
    };

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

          {
            for cell in first_cell..last_cell_excl {
            let vis = cell - first_cell;
            let cell_beat = grid.cell_start(cell);
            let step = (cell_beat / step_b).floor() as usize;
            let at_step_start = (cell_beat / step_b).frac().is_zero();
            // First of this cell's two grid columns (after the 5-col key label).
            let col_base = key_w + vis * 2;
            // When piano is active, the cursor column is piano_cursor_step (its
            // first sub-cell); otherwise it follows the tracker row cursor.
            let is_cursor_col = at_step_start && if piano_active {
                piano_cursor_step == step
            } else {
                app.tracker_state.cursor.0 == step
            };
            let is_piano_cursor = piano_active && piano_cursor_row == row_idx
                && piano_cursor_step == step && at_step_start;
            // Fine cursor crosshair: the exact sub-cell where `\` drops a note.
            let is_fine_cursor = piano_active && piano_cursor_row == row_idx
                && fine_cell >= 0 && cell as i64 == fine_cell && !is_piano_cursor;
            let is_piano_cursor_row_cell = piano_active && piano_cursor_row == row_idx;
            let is_playing = app.playing && app.current_step == step && at_step_start;

            // Note start (any polyphonic voice) on this exact cell.
            let has_start = note_map
                .get(cell)
                .map(|voices| voices.iter().any(|(nr, _)| *nr == row_idx))
                .unwrap_or(false);

            // Gate continuation of a note started earlier (in cells).
            let has_continuation = {
                let mut cont = false;
                let max_back = (16 * pdiv).max(16);
                for back in 1..=max_back {
                    if cell < back { break; }
                    if let Some(voices) = note_map.get(cell - back) {
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
            } else if is_fine_cursor {
                // Fine (exact) placement cursor: cyan diamond.
                if has_start {
                    ("██", Color::Black, Color::Rgb(80, 200, 220))
                } else {
                    ("◇◇", Color::Rgb(120, 230, 240), Color::Rgb(0, 40, 48))
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

            // Tuplet-group tint: note starts inside a figure region read as a
            // violet group, so each complex rhythm is visible at a glance. Applied
            // below selection/marquee so those still win.
            let (cell_str, cell_fg, cell_bg) =
                if has_start && mark_spans.iter().any(|&(s, e)| cell >= s && cell <= e) {
                    ("██", Color::Rgb(210, 160, 255), Color::Rgb(48, 26, 72))
                } else {
                    (cell_str, cell_fg, cell_bg)
                };

            // Selection highlight: any selected note (step note OR exact event)
            // gets a bright magenta wash so the marked set is unmistakable.
            let (cell_str, cell_fg, cell_bg) =
                if piano_active && has_start && selected_cells.contains(&(cell, row_idx)) {
                    ("██", Color::Rgb(255, 120, 235), Color::Rgb(90, 20, 100))
                } else {
                    (cell_str, cell_fg, cell_bg)
                };

            // Live rubber-band marquee: fill the dragged rectangle and brighten
            // its border so the selection bounds are visible while dragging.
            let (cell_str, cell_fg, cell_bg) = match marquee {
                Some((c0, c1, r0, r1)) if cell >= c0 && cell <= c1
                    && row_idx >= r0 && row_idx <= r1 =>
                {
                    let on_edge = cell == c0 || cell == c1
                        || row_idx == r0 || row_idx == r1;
                    if on_edge {
                        (cell_str, Color::White, Color::Rgb(170, 80, 200))
                    } else {
                        (cell_str, cell_fg, Color::Rgb(55, 22, 70))
                    }
                }
                _ => (cell_str, cell_fg, cell_bg),
            };

            // Figure-bracket overlay: on an empty cell of a bracket row, draw the
            // `⌐──N──¬` glyphs (violet) over the cell. Notes are never covered. The
            // overlay owns both columns (it replaces any separator/tick there), so
            // the bracket line stays continuous.
            let mut disp = cell_str.to_string();
            let mut disp_fg = cell_fg;
            let mut has_overlay = false;
            if !has_start && !has_continuation {
                if let Some(v) = bracket_overlays.get(&row_idx) {
                    let a = v.get(col_base).copied().unwrap_or(' ');
                    let b = v.get(col_base + 1).copied().unwrap_or(' ');
                    if a != ' ' || b != ' ' {
                        disp = format!("{a}{b}");
                        disp_fg = Color::Rgb(220, 170, 255);
                        has_overlay = true;
                    }
                }
            }
            if has_overlay {
                spans.push(Span::styled(disp, Style::default().fg(disp_fg).bg(cell_bg)));
                continue;
            }

            // Separators. At a step boundary (sub 0): measure start = amber │,
            // sub-group start = dim blue │. Between sub-cells: faint edit tick ┊.
            let beat = step % time_sig_num;
            let is_meas_sep = at_step_start && vis > 0 && beat == 0;
            let is_grp_sep  = at_step_start && vis > 0 && !is_meas_sep && group_starts.contains(&beat);
            let is_step_tick = at_step_start && vis > 0 && !is_meas_sep && !is_grp_sep && on_edit_grid(step);
            let is_sub_tick = !at_step_start; // every sub-cell starts on an edit tick
            if is_meas_sep || is_grp_sep {
                let sep_col = if is_meas_sep { HEADER } else { Color::Rgb(48, 72, 130) };
                spans.push(Span::styled("│", Style::default().fg(sep_col).bg(cell_bg)));
                let one_char: String = disp.chars().next().map(|c| c.to_string()).unwrap_or_default();
                spans.push(Span::styled(one_char, Style::default().fg(disp_fg).bg(cell_bg)));
            } else if is_step_tick || is_sub_tick {
                // Faint edit-grid tick (lowest priority separator).
                spans.push(Span::styled("┊", Style::default().fg(Color::Rgb(70, 70, 84)).bg(cell_bg)));
                let one_char: String = disp.chars().next().map(|c| c.to_string()).unwrap_or_default();
                spans.push(Span::styled(one_char, Style::default().fg(disp_fg).bg(cell_bg)));
            } else {
                spans.push(Span::styled(
                    disp,
                    Style::default().fg(disp_fg).bg(cell_bg),
                ));
            }
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
            let sel = !(app.piano_selection.is_empty() && app.piano_event_selection.is_empty());
            let sel_hint = if sel { "↑↓=transpose  g=figure(nest)  " } else { "" };
            format!(
                " [{}L-drag=select  Alt+drag=move  Scroll-btn=insert  R-click=erase  Ctrl+scroll=zoom │ GRID {}{}] ",
                sel_hint,
                app.edit_state.grid_label(),
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

/// Height (rows) of each TRACK MODULATION bar chart. Taller = finer vertical
/// resolution. Shared so the tab-row hit-test offset in `lib.rs` stays in sync.
pub(crate) const MOD_CHART_ROWS: usize = 8;

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

/// The integer (MIDI/audio) part of automation parameter `param`.
fn note_param_coarse(note: &seqterm_core::note::Note, param: usize) -> u8 {
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

/// Effective fractional value (0.0..127.0) for automation parameter `param`:
/// the integer field plus its `mod_fine` refinement, for sub-decimal graph
/// resolution. MIDI/audio note-on still use the coarse `u8`.
pub(crate) fn note_param_val(note: &seqterm_core::note::Note, param: usize) -> f32 {
    let coarse = note_param_coarse(note, param) as f32;
    let fine = note.mod_fine.get(param).copied().unwrap_or(0) as f32 / 256.0;
    (coarse + fine).clamp(0.0, 127.0)
}

/// Write a fractional value (0.0..127.0) into automation parameter `param`: the
/// integer part goes to the matching `u8` field, the fraction to `mod_fine[param]`.
pub(crate) fn note_param_set(note: &mut seqterm_core::note::Note, param: usize, val: f32) {
    let v = val.clamp(0.0, 127.0);
    let coarse = v.floor() as u8;
    let frac = (((v - coarse as f32) * 256.0).round() as i32).clamp(0, 255) as u8;
    match param {
        0 => note.velocity = coarse,
        1 => note.gain = coarse,
        2 => note.pan = coarse,
        3 => note.lp = coarse,
        4 => note.hp = coarse,
        5 => note.lfo = coarse,
        6 => note.speed = coarse,
        7 => note.amp = coarse,
        _ => {}
    }
    if let Some(slot) = note.mod_fine.get_mut(param) {
        *slot = frac;
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
    const N_CHART: usize = MOD_CHART_ROWS;

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

    // When irregular-rhythm events are selected in the piano roll, the panel
    // edits and displays those events (their notes live in the `events` layer,
    // off the step grid). Otherwise it tracks the cursor step.
    let ev_sel_count = app.piano_event_selection.len();
    let cursor_note = if ev_sel_count > 0 {
        app.piano_event_selection.iter().min().copied()
            .and_then(|i| pat.events.get(i))
            .map(|ev| ev.note.clone())
            .unwrap_or_default()
    } else {
        pat.steps.get(cursor_step).cloned().unwrap_or_default()
    };
    let param_name = MOD_PARAMS[mc];
    let cur_val = note_param_val(&cursor_note, mc);

    // ── Automation chart geometry ─────────────────────────────────────────────
    // Aligned with the piano roll: a 5-column left axis (matching the piano-roll
    // key-label width) and 2-column step cells (matching the piano-roll cell
    // width), so each bar position lines up vertically with the note above it.
    // The same `piano_step_scroll` drives both, so the piano-roll horizontal
    // scrollbar scrolls this panel's content in lockstep.
    let axis_w: u16 = 5;
    let chart_x = area.x.saturating_add(1 + axis_w);
    let chart_y = area.y + 1;

    // Mirror the piano roll's non-uniform grid so bars line up with the notes above
    // — including tuplet regions, where each tuplet note gets its own cell.
    let step_b = pat.step_beats();
    let pdiv = display_pdiv(step_b, app.edit_state.resolution);
    let grid = pat.piano_grid(pdiv);
    let total_cells = grid.total_cells().max(1);
    let chart_cols = area.width.saturating_sub(axis_w + 3) as usize;
    let step_scroll = app.piano_step_scroll;
    let max_visible_cells = (chart_cols / 2).max(1);
    let first_cell = grid.nearest_cell(step_b * step_scroll as i64).min(total_cells - 1);
    let visible_cells = max_visible_cells.min(total_cells - first_cell);

    // Publish rect for mouse hit-testing in lib.rs (clicks resolve via the grid).
    app.vel_chart_area.set(Rect {
        x: chart_x,
        y: chart_y,
        width: (visible_cells * 2) as u16,
        height: N_CHART as u16,
    });

    // Precompute each visible cell once (event scan is O(events) per cell).
    struct ModCell { val: f32, empty: bool, is_event: bool, sel: bool, is_cur: bool, is_play: bool }
    let mut cells: Vec<ModCell> = Vec::with_capacity(visible_cells);
    for c in 0..visible_cells {
        let global_cell = first_cell + c;
        let cell_beat = grid.cell_start(global_cell);
        let step = (cell_beat / step_b).floor() as usize;
        let at_step_start = (cell_beat / step_b).frac().is_zero();
        // Topmost (highest value) event rounding onto this cell, if any.
        let mut ev_val: Option<f32> = None;
        let mut ev_sel = false;
        for (idx, ev) in pat.events.iter().enumerate() {
            if grid.nearest_cell(ev.start) == global_cell {
                let v = note_param_val(&ev.note, mc);
                if ev_val.map_or(true, |cur| v > cur) { ev_val = Some(v); }
                if app.piano_event_selection.contains(&idx) { ev_sel = true; }
            }
        }
        let (val, empty, is_event) = if let Some(v) = ev_val {
            (v, false, true)
        } else if at_step_start {
            let n = pat.steps.get(step).cloned().unwrap_or_default();
            let e = n.is_empty();
            (if e { 0.0 } else { note_param_val(&n, mc) }, e, false)
        } else {
            (0.0, true, false)
        };
        cells.push(ModCell {
            val, empty, is_event, sel: ev_sel,
            is_cur:  at_step_start && step == cursor_step && !is_event,
            is_play: app.playing && app.current_step == step && at_step_start,
        });
    }

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

        for cell in &cells {
            let val = cell.val;
            let empty = cell.empty;

            // Eighth-block fill character.
            let chr: &str = if empty {
                if is_bottom { "·" } else { " " }
            } else {
                let eighths   = (val * (N_CHART * 8) as f32 / 127.0).round() as usize;
                let full_rows = eighths / 8;
                let partial   = eighths % 8;
                let row_bot   = N_CHART - 1 - bar_row;
                if row_bot < full_rows              { EIGHTS[8] }
                else if row_bot == full_rows && partial > 0 { EIGHTS[partial] }
                else                                { EIGHTS[0] }
            };

            let fg = if cell.sel {
                Color::Rgb(255, 120, 235)               // selected irregular-rhythm event
            } else if cell.is_cur {
                if empty { Color::Rgb(80, 80, 0) } else { Color::Yellow }
            } else if cell.is_play {
                if empty { Color::Rgb(0, 60, 60) } else { Color::Cyan }
            } else if empty {
                Color::Rgb(40, 40, 40)
            } else if cell.is_event {
                Color::Rgb(200, 150, 255)               // irregular-rhythm event (violet)
            } else if mc == 0 {
                // VEL: velocity colour palette (low=blue, mid=green, high=red).
                if val < 64.0 { Color::Rgb(60, 120, 220) }
                else if val < 100.0 { Color::Rgb(80, 200, 80) }
                else { Color::Rgb(220, 80, 60) }
            } else {
                param_bar_color(mc, val.round() as u8)
            };

            // 2-column cell, aligned with the piano-roll step cell above.
            spans.push(Span::styled(format!("{0}{0}", chr), Style::default().fg(fg)));
        }

        // Bottom row: step info appended after all step cells.
        if is_bottom {
            let voice_str = if mc == 0 {
                let vc = cursor_note.voice_count();
                if vc > 1 { format!(" [{} vc]", vc) } else { String::new() }
            } else { String::new() };
            let info = if ev_sel_count > 0 {
                format!(" {} ev {}:{:06.2}{}", ev_sel_count, param_name, cur_val, voice_str)
            } else {
                format!(" s:{:03} {}:{:06.2}{}", cursor_step + 1, param_name, cur_val, voice_str)
            };
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
    let hint = if !mod_active {
        " Tab=activate track modulation"
    } else if ev_sel_count > 0 {
        " editing selected rhythm events · ←→=param ↑↓=±1 scroll=fine"
    } else {
        " ←→=param  ↑↓=±1  scroll=±0.1 fine  click/drag=set  Tab=next"
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(if mod_active { Color::Yellow } else { Color::DarkGray }),
    )));

    // ── Render ────────────────────────────────────────────────────────────────
    let mod_border = if mod_active { Style::default().fg(Color::Yellow) } else { Style::default().fg(BORDER) };
    // Value monitor: the cursor step's note pitch + the selected param's exact
    // (fractional) value, so fine edits are visible numerically.
    let mon_note = if cursor_note.is_empty() {
        "—".to_string()
    } else {
        cursor_note.note.clone()
    };
    let mod_title = if !mod_active {
        " TRACK MODULATION ".to_string()
    } else if ev_sel_count > 0 {
        format!(" TRACK MODULATION :: {} {:.2}  {} EVENT(S) {} ", param_name, cur_val, ev_sel_count, mon_note)
    } else {
        format!(" TRACK MODULATION :: {} {:.2}  NOTE {} ", param_name, cur_val, mon_note)
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

/// One-row buffer scope for the Z5 Texture effect. With a live `meter` (shared
/// from the audio thread) it draws the **real** recorded buffer as a sparkline
/// with the actual write `▼` and scrub `▲` heads + `❄` when frozen. Without a
/// meter (e.g. the effect is disabled) it falls back to a param-derived activity
/// view: a faint BufLen window, density dots, and animated heads.
pub(crate) fn draw_z5_buffer_viz(
    f: &mut Frame, app: &App, p: &[f32],
    meter: Option<&std::sync::Arc<seqterm_audio_engine::Z5Meter>>,
    x0: u16, y: u16, w: u16,
) {
    let w = w as usize;
    if w < 6 { return; }
    const SPARK: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    let mut cells: Vec<(char, Color)>;
    let (write_n, scrub_n, frozen);

    if let Some(m) = meter {
        // Real buffer waveform → sparkline.
        let mut wave = [0u8; seqterm_audio_engine::Z5_WAVE_BINS];
        m.waveform(&mut wave);
        let bins = wave.len();
        cells = (0..w).map(|i| {
            let mag = wave[(i * bins) / w] as usize;
            let lvl = (mag * 8 / 255).min(8);
            let g = 90 + (mag as u16 * 120 / 255) as u8;
            (SPARK[lvl], Color::Rgb(60, g, 110))
        }).collect();
        write_n = m.write_pos();
        scrub_n = m.scrub_pos();
        frozen  = m.is_frozen();
    } else {
        // Fallback: param-derived activity view.
        let g = |i: usize| p.get(i).copied().unwrap_or(0.0);
        let density = g(1);
        let buflen  = g(14).clamp(0.05, 1.0);
        let stretch = g(10);
        let position = g(11);
        frozen = g(8) > 0.5;
        let win = ((buflen * w as f32) as usize).clamp(2, w);
        cells = (0..w).map(|i| if i < win { ('·', Color::Rgb(55, 64, 76)) } else { (' ', PANEL) }).collect();
        let ndots = ((density * win as f32 * 0.4) as usize).clamp(0, win.saturating_sub(1));
        for k in 0..ndots {
            let xi = (k * win) / ndots.max(1);
            if xi < win { cells[xi] = ('•', Color::Rgb(80, 170, 120)); }
        }
        let drift = (stretch - 0.5) * 2.0;
        // Only animate while playing — STOP keeps the bar static.
        scrub_n = if !app.playing || drift.abs() < 0.02 { position }
                  else { (position + app.frame_count as f32 * 0.01 * drift).rem_euclid(1.0) };
        write_n = if frozen || !app.playing { -1.0 } else { (app.frame_count as f32 * 0.013).fract() };
    }

    // Overlay heads.
    let si = ((scrub_n.clamp(0.0, 1.0) * (w as f32 - 1.0)) as usize).min(w - 1);
    cells[si] = ('▲', Color::Rgb(90, 200, 230));
    if frozen {
        for (k, ch) in "❄FRZ".chars().enumerate() { if k < w { cells[k] = (ch, Color::Rgb(120, 200, 245)); } }
    } else if write_n >= 0.0 {
        let wi = ((write_n.clamp(0.0, 1.0) * (w as f32 - 1.0)) as usize).min(w - 1);
        cells[wi] = ('▼', Color::Rgb(245, 210, 70));
    }

    let spans: Vec<Span> = cells.into_iter()
        .map(|(c, col)| Span::styled(c.to_string(), Style::default().fg(col).bg(PANEL)))
        .collect();
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(PANEL)),
        Rect::new(x0, y, w as u16, 1));
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
    app.tracker_fx_param_rects.set([Rect::default(); crate::app::FX_MAX_PARAMS]);
    app.tracker_fx_slot_rects.set([Rect::default(); 5]);
    app.tracker_fx_add_rect.set(Rect::default());
    app.tracker_fx_enable_rect.set(Rect::default());
    app.tracker_fx_delete_rect.set(Rect::default());
    app.tracker_fx_move_prev_rect.set(Rect::default());
    app.tracker_fx_move_next_rect.set(Rect::default());
    app.tracker_fx_param_prev_rect.set(Rect::default());
    app.tracker_fx_param_next_rect.set(Rect::default());
    app.tracker_fx_param_prev_target.set(usize::MAX);
    app.tracker_fx_param_next_target.set(usize::MAX);
    app.tracker_fx_cat_prev_rect.set(Rect::default());
    app.tracker_fx_cat_next_rect.set(Rect::default());
    app.tracker_fx_preset_prev_rect.set(Rect::default());
    app.tracker_fx_preset_next_rect.set(Rect::default());

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

    let box_y = y;
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
        if w > 0 { app.tracker_fx_add_rect.set(Rect::new(bx, y, w, 3)); bx += w + 1; }
    }
    app.tracker_fx_slot_rects.set(slot_rects);

    // ── ROUTING: rendered to the RIGHT of the selector boxes (same row, middle
    //    line) so it no longer steals a vertical row that would clip the control
    //    buttons below. ─────────────────────────────────────────────────────────
    let dim = Style::default().fg(Color::Rgb(120, 130, 150));
    let mut rt: Vec<Span> = vec![
        Span::styled(" IN", dim),
    ];
    if chain.is_empty() {
        rt.push(Span::styled(" → OUT", dim));
    } else {
        for (i, e) in chain.iter().enumerate() {
            rt.push(Span::styled("→", dim));
            let st = if i == slot_sel {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if e.enabled {
                Style::default().fg(Color::Rgb(120, 220, 150))
            } else {
                Style::default().fg(Color::Rgb(110, 115, 125)).add_modifier(Modifier::CROSSED_OUT)
            };
            rt.push(Span::styled(format!("{}", i + 1), st));
        }
        rt.push(Span::styled("→OUT", dim));
    }
    let route_x = bx + 1;
    if route_x < max_x {
        f.render_widget(
            Paragraph::new(Line::from(rt)).style(Style::default().bg(PANEL)),
            Rect::new(route_x, box_y + 1, max_x.saturating_sub(route_x), 1));
    }
    y += 3; // box height (no spacer — keep the tab compact)

    // ── Rotary knobs for the selected effect: one row, scrolled horizontally so
    //    the selected parameter stays visible (fits the uniform tab height). ───
    if let Some(entry) = chain.get(slot_sel) {
        let descs = fx_param_descs(entry.kind);
        let n = descs.len();
        let mut param_rects = [Rect::default(); crate::app::FX_MAX_PARAMS];
        let avail   = inner.width.saturating_sub(2) as usize;
        let visible = (avail / FX_CELL_W as usize).max(1);

        // Parameter category window: effects with many knobs (e.g. Z5Texture) show
        // ~8 at a time via a clickable combobox, so a MIDI surface maps cleanly.
        // The PRESET combobox sits on the same row to the right.
        let cats    = crate::app::fx_param_categories(entry.kind);
        let presets = crate::app::fx_presets(entry.kind);
        let (lo, hi) = if cats.is_empty() {
            (0usize, n)
        } else {
            let c = cats[app.tracker_fx_category.min(cats.len() - 1)];
            (c.start.min(n), (c.start + c.len).min(n))
        };
        if !cats.is_empty() || !presets.is_empty() {
            let hdr = Style::default().fg(HEADER).bg(PANEL).add_modifier(Modifier::BOLD);
            let bg  = Style::default().bg(PANEL);
            let mut rx = cx + 2;
            if !cats.is_empty() {
                let ci = app.tracker_fx_category.min(cats.len() - 1);
                let cb = format!("◀ {} ({}/{}) ▶", cats[ci].name, ci + 1, cats.len());
                let cols = cb.chars().count() as u16;
                f.render_widget(Paragraph::new(Span::styled(cb, hdr)).style(bg),
                    Rect::new(rx, y, (max_x.saturating_sub(rx)).min(cols + 1), 1));
                app.tracker_fx_cat_prev_rect.set(Rect::new(rx, y, 2, 1));
                app.tracker_fx_cat_next_rect.set(Rect::new((rx + cols.saturating_sub(2)).min(max_x.saturating_sub(2)), y, 2, 1));
                rx += cols + 3;
            }
            if !presets.is_empty() && rx < max_x {
                let pi = app.tracker_fx_preset.min(presets.len() - 1);
                let pstyle = Style::default().fg(Color::Rgb(150, 195, 245)).bg(PANEL).add_modifier(Modifier::BOLD);
                let pb = format!("PRESET ◀ {} ▶", presets[pi].0);
                let cols = pb.chars().count() as u16;
                f.render_widget(Paragraph::new(Span::styled(pb, pstyle)).style(bg),
                    Rect::new(rx, y, (max_x.saturating_sub(rx)).min(cols + 1), 1));
                app.tracker_fx_preset_prev_rect.set(Rect::new((rx + 7).min(max_x.saturating_sub(1)), y, 2, 1));
                app.tracker_fx_preset_next_rect.set(Rect::new((rx + cols.saturating_sub(1)).min(max_x.saturating_sub(1)), y, 2, 1));
            }
            y += 1;
        }
        // Keep the displayed selection inside the active category window.
        let param_sel = param_sel.clamp(lo, hi.saturating_sub(1).max(lo));

        let start = if focused && (param_sel.saturating_sub(lo)) >= visible {
            (param_sel + 1 - visible).max(lo)
        } else { lo };
        let end   = (start + visible).min(hi);
        let top_y = y;

        let mut top_spans: Vec<Span> = vec![Span::raw("  ")];
        let mut mid_spans: Vec<Span> = vec![Span::raw("  ")];
        let mut lbl_spans: Vec<Span> = vec![Span::raw("  ")];
        for (ci, pi) in (start..end).enumerate() {
            let val   = entry.params.get(pi).copied().unwrap_or(0.0);
            let is_p  = pi == param_sel && focused;
            let learn_this = learning == Some((slot_sel, pi));

            let px = cx + 2 + (ci as u16) * FX_CELL_W;
            if pi < crate::app::FX_MAX_PARAMS { param_rects[pi] = Rect::new(px, top_y, FX_CELL_W, 3); }

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
        put(f, Line::from(top_spans), top_y);
        put(f, Line::from(mid_spans), top_y + 1);
        put(f, Line::from(lbl_spans), top_y + 2);
        app.tracker_fx_param_rects.set(param_rects);

        // Overflow markers — clickable so the mouse can page within the category.
        let marker = Style::default().fg(Color::Rgb(150, 195, 245)).bg(PANEL).add_modifier(Modifier::BOLD);
        if start > lo {
            let r = Rect::new(cx, top_y, 4, 3);
            f.render_widget(Paragraph::new("◀").style(marker), Rect::new(cx, top_y + 1, 4, 1));
            app.tracker_fx_param_prev_rect.set(r);
            app.tracker_fx_param_prev_target.set(start - 1);
        }
        if end < hi {
            let mx = cx + 2 + ((end - start) as u16) * FX_CELL_W;
            if mx < max_x {
                let lbl = format!("+{}▶", hi - end);
                let w = (lbl.chars().count() as u16).min(max_x - mx);
                f.render_widget(Paragraph::new(lbl).style(marker), Rect::new(mx, top_y + 1, w, 1));
                app.tracker_fx_param_next_rect.set(Rect::new(mx, top_y, w.max(3), 3));
                app.tracker_fx_param_next_target.set(end);
            }
        }
    } else {
        put(f, Line::from(Span::styled(
            "  No FX — click [+ ADD] (or press a) to insert one",
            Style::default().fg(Color::DarkGray))), y);
    }
    y += 3;

    // ── Live buffer activity strip (Z5 Texture): write/read heads, grain density
    //    and Freeze, derived from the effect's control state. ───────────────────
    if let Some(entry) = chain.get(slot_sel) {
        if entry.kind == crate::app::AudioFxKind::Z5Texture && y + 1 < max_y {
            let meter = slot_id
                .and_then(|sid| app.z5_meters.get(&sid))
                .and_then(|v| v.iter().find(|(i, _)| *i == slot_sel).map(|(_, m)| m));
            draw_z5_buffer_viz(f, app, &entry.params, meter, cx + 1, y, inner.width.saturating_sub(2));
            y += 1;
        }
    }

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
