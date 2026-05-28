use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use seqterm_core::PatternSource;

use crate::app::App;

const PANEL: Color = Color::Rgb(22, 27, 34);
const BORDER: Color = Color::Rgb(48, 54, 61);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const HEADER: Color = Color::Rgb(240, 136, 62);

// ─── Color palette from the reference matrix project (Indexed 256-color) ────
const C_CURRENT:    Color = Color::Indexed(15);  // white        – playing + note hit
const C_ACTIVE:     Color = Color::Indexed(250); // light gray   – cursor  + enabled
const C_ENABLED:    Color = Color::Indexed(18);  // dark blue    – enabled, idle
const C_DISABLED:   Color = Color::Indexed(237); // dark gray    – disabled (same structure, gray)
const C_DIS_CURSOR: Color = Color::Indexed(243); // medium gray  – cursor on disabled
const C_INACTIVE:   Color = Color::Indexed(238); // medium gray  – cursor, empty slot
const C_INACT_DIM:  Color = Color::Indexed(234); // very dark    – empty, not cursor
const C_TEXT_DARK:  Color = Color::Indexed(232); // near-black text (on bright bg)
const C_TEXT_BLUE:  Color = Color::Indexed(252); // light text on dark blue
const C_TEXT_GRAY:  Color = Color::Indexed(246); // light-gray text on dark gray (disabled)
const C_TEXT_MED:   Color = Color::Indexed(242); // medium-gray text (empty slots)
const C_ROUTE_FAIL: Color = Color::Indexed(130); // dark amber  – MIDI destination unavailable
const C_ROUTE_CUR:  Color = Color::Indexed(172); // bright amber – cursor on route-fail cell

// ─── 3×3 micro-font (same glyphs as the reference project) ──────────────────

fn glyph(c: char) -> [&'static str; 3] {
    match c.to_ascii_uppercase() {
        '0' => ["█▀█", "█ █", "█▄█"],
        '1' => ["▀█ ", " █ ", "▄█▄"],
        '2' => ["█▀█", "▄▀▀", "█▄▄"],
        '3' => ["█▀█", " ▀█", "█▄█"],
        '4' => ["█ █", "▀▀█", "  █"],
        '5' => ["█▀▀", "▀▀█", "▄▄█"],
        '6' => ["█▀▀", "█▀█", "█▄█"],
        '7' => ["▀▀█", "  █", "  █"],
        '8' => ["█▀█", "█▀█", "█▄█"],
        '9' => ["█▀█", "▀▀█", "  █"],
        'A' => ["█▀█", "█▀█", "█ █"],
        'B' => ["█▀█", "█▀▄", "█▄█"],
        'C' => ["█▀█", "█  ", "█▄█"],
        'D' => ["█▀▄", "█ █", "█▄▀"],
        'E' => ["█▀▀", "█▀ ", "█▄▄"],
        'F' => ["█▀▀", "█▀ ", "█  "],
        'G' => ["█▀▀", "█ █", "█▄█"],
        'H' => ["█ █", "███", "█ █"],
        'I' => ["▀█▀", " █ ", "▄█▄"],
        'J' => ["  █", "  █", "▄▄█"],
        'K' => ["█ █", "██ ", "█ █"],
        'L' => ["█  ", "█  ", "█▄▄"],
        'M' => ["█▄█", "█ █", "█ █"],
        'N' => ["█▀▄", "█ █", "█ █"],
        'O' => ["█▀█", "█ █", "█▄█"],
        'P' => ["█▀█", "█▀▀", "█  "],
        'Q' => ["█▀█", "█▀▄", "▀▄▀"],
        'R' => ["█▀█", "█▀▄", "█ █"],
        'S' => ["█▀█", "▀▀▄", "█▄█"],
        'T' => ["▀█▀", " █ ", " █ "],
        'U' => ["█ █", "█ █", "█▄█"],
        'V' => ["█ █", "█ █", " ▀ "],
        'W' => ["█ █", "█ █", "▀▄▀"],
        'X' => ["█ █", " ▀ ", "█ █"],
        'Y' => ["█ █", " ▀ ", " █ "],
        'Z' => ["▀▀█", " ▀ ", "█▄▄"],
        '-' => ["   ", "▄▄▄", "   "],
        _   => ["   ", "   ", "   "],
    }
}

/// Render `text` (up to 3 chars) as 3 rows of the micro-font,
/// each padded/centred to exactly `cell_w` visible columns.
fn ascii_rows(text: &str, cell_w: usize) -> [String; 3] {
    let mut rows = [String::new(), String::new(), String::new()];
    let mut n = 0usize;
    for c in text.chars() {
        let g = glyph(c);
        for r in 0..3 {
            rows[r].push_str(g[r]); // 3 visible cols
            rows[r].push(' ');      // 1 space separator
        }
        n += 1;
    }
    // content is n*4 visible columns wide; centre inside cell_w
    let content = n * 4;
    let pad_l = cell_w.saturating_sub(content) / 2;
    let pad_r = cell_w.saturating_sub(content).saturating_sub(pad_l);
    for r in 0..3 {
        let inner = std::mem::take(&mut rows[r]);
        rows[r] = format!("{}{}{}", " ".repeat(pad_l), inner, " ".repeat(pad_r));
    }
    rows
}

/// Render amplitude peaks as a row of block characters, padded to `width` columns.
fn waveform_bar(peaks: &[f32], width: usize) -> String {
    const BLOCKS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if peaks.is_empty() || width == 0 { return " ".repeat(width); }
    (0..width).map(|i| {
        let idx = i * peaks.len() / width;
        let peak = peaks[idx.min(peaks.len() - 1)];
        BLOCKS[(peak * 8.0).round() as usize % 9]
    }).collect()
}

pub fn draw_matrix(f: &mut Frame, app: &App, area: Rect) {
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(60), Constraint::Length(36)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(20), Constraint::Length(9)])
        .split(h_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(h_chunks[1]);

    // Cache panel areas for mouse-hover section switching.
    app.matrix_panel_rects.set([left_chunks[0], left_chunks[1], right_chunks[0], right_chunks[1]]);

    draw_clip_grid(f, app, left_chunks[0]);
    draw_transport_buttons(f, app, left_chunks[1]);
    draw_polymeter(f, app, right_chunks[0]);
    draw_routing_panel(f, app, right_chunks[1]);
}

fn draw_clip_grid(f: &mut Frame, app: &App, area: Rect) {
    const ROW_LBL: usize = 3;

    let proj = app.project.lock();
    let (cursor_row, cursor_col) = app.matrix_state.cursor;
    let n_rows = app.matrix_rows;
    let n_cols = app.matrix_cols;
    let grid_active = app.matrix_section == 0;
    let tracker_key = app.tracker_state.pattern_key.as_deref();

    // Responsive square cells.
    // Strategy: consume all available height first, then derive width so that
    // cell_w ≈ 2 × cell_h (monospace chars are ~2× taller than wide in pixels).
    let available_w = (area.width as usize).saturating_sub(2);
    let available_h = (area.height as usize).saturating_sub(2);
    // Max cell_h from vertical space: total = 3 + n_rows*(1+cell_h) ≤ available_h
    let max_cell_h = if n_rows == 0 { 10 } else {
        (available_h.saturating_sub(3) / n_rows).saturating_sub(1).max(1)
    };
    let cell_h = max_cell_h;
    // Max cell_w from horizontal space: total = 4 + n_cols*(1+cell_w) ≤ available_w
    let max_cell_w = if n_cols == 0 { 4 } else {
        (available_w.saturating_sub(4) / n_cols).saturating_sub(1).max(4)
    };
    // Ideal width for a square cell; cap at what actually fits.
    let cell_w = (cell_h * 2).min(max_cell_w).max(4);
    // each micro-font glyph is 3 cols + 1 space = 4 cols per char
    let n_font_chars = (cell_w / 4).min(5);
    // font rows that fit below the label (micro-font is always 3 rows tall)
    let n_font_rows = cell_h.saturating_sub(1).min(3);

    let mut lines: Vec<Line> = Vec::new();

    // ── Column header ─────────────────────────────────────────────────────────
    {
        let mut hdr: Vec<Span> = vec![Span::raw(" ".repeat(ROW_LBL))];
        for col in 0..n_cols {
            let label = format!("{:^width$}", format!("{:02}", col + 1), width = cell_w);
            hdr.push(Span::raw(" ")); // align with │ border
            hdr.push(Span::styled(
                label,
                Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
            ));
        }
        hdr.push(Span::raw(" ")); // align with trailing │
        lines.push(Line::from(hdr));
    }

    // ── Matrix rows ───────────────────────────────────────────────────────────
    let bdr_default = Color::Indexed(240);
    let hovered = app.hovered_matrix_cell.get();

    // Returns border color for a horizontal segment: yellow if adjacent to hovered cell.
    let h_color = |loop_row: usize, c: usize| -> Color {
        if hovered == Some((loop_row, c)) || hovered.map(|(hr, hc)| hr + 1 == loop_row && hc == c).unwrap_or(false) {
            Color::Yellow
        } else {
            bdr_default
        }
    };
    // Returns border color for a vertical bar: yellow if left or right edge of hovered cell.
    let v_color = |loop_row: usize, c: usize| -> Color {
        if hovered.map(|(hr, hc)| hr == loop_row && (hc == c || hc + 1 == c)).unwrap_or(false) {
            Color::Yellow
        } else {
            bdr_default
        }
    };

    let h_seg = "─".repeat(cell_w);

    for row in 0..n_rows {
        let row_label = (b'A' + row as u8) as char;
        let row_key   = row_label.to_string();
        let is_row_cursor = cursor_row == row;

        // Pre-compute display data for every cell in this row.
        // Each element: (bg, fg, cell_h content lines with vertical centering)
        let grabbed = app.matrix_state.grabbed_clip;
        let cell_data: Vec<(Color, Color, Vec<String>)> = (0..n_cols).map(|col| {
            let is_cursor = is_row_cursor && cursor_col == col;
            let is_grabbed_src = grabbed.map(|(gr, gc)| gr == row && gc == col).unwrap_or(false);
            let is_drop_target = grabbed.is_some() && is_cursor;
            let clip = proj.matrix.get(&row_key)
                .and_then(|r| r.get(col))
                .and_then(|c| c.as_ref());

            let has_clip   = clip.is_some();
            let is_enabled = clip.map(|c| c.enabled).unwrap_or(false);
            let is_disabled = has_clip && !is_enabled;
            let pat_key    = clip.and_then(|c| c.pattern_key.as_deref()).unwrap_or("");
            let source     = clip.map(|c| &c.source);

            // True when this clip has a configured MIDI output that is no longer available.
            let is_route_fail = is_enabled && clip
                .and_then(|c| c.midi_out.as_deref())
                .map(|out| app.unavailable_midi_routes.contains(out))
                .unwrap_or(false);

            let is_tracker_active = !pat_key.is_empty()
                && tracker_key.map(|k| k == pat_key).unwrap_or(false);

            let is_hit = app.playing && is_enabled && !pat_key.is_empty() && {
                proj.patterns.get(pat_key).map(|p| {
                    let pos = app.current_step % p.length.max(1);
                    p.steps.get(pos).map(|n| !n.is_empty()).unwrap_or(false)
                }).unwrap_or(false)
            };

            let bg = if is_grabbed_src {
                Color::Rgb(180, 90, 20)   // orange  – this clip is held for move
            } else if is_drop_target {
                Color::Rgb(20, 150, 100)  // teal    – valid drop zone (cursor)
            } else if is_enabled && is_hit {
                C_CURRENT           // white       – note fires while playing
            } else if is_route_fail && is_cursor {
                C_ROUTE_CUR         // bright amber – cursor on route-fail clip
            } else if is_route_fail {
                C_ROUTE_FAIL        // dark amber  – MIDI destination gone
            } else if is_enabled && is_cursor {
                C_ACTIVE            // light gray  – cursor on enabled clip
            } else if is_enabled && is_tracker_active {
                Color::Indexed(19)  // medium blue – open in tracker
            } else if is_enabled {
                C_ENABLED           // dark blue   – clip present, idle
            } else if is_disabled && is_cursor {
                C_DIS_CURSOR        // medium gray – cursor on disabled clip
            } else if is_disabled {
                C_DISABLED          // dark gray   – disabled clip, idle
            } else if is_cursor {
                C_INACTIVE          // medium gray – cursor on empty slot
            } else {
                C_INACT_DIM         // very dark   – empty, not selected
            };

            let fg = if is_hit || (is_cursor && is_enabled && !is_route_fail) {
                C_TEXT_DARK         // near-black on white/light bg
            } else if is_route_fail {
                C_TEXT_DARK         // near-black on amber bg
            } else if is_enabled {
                C_TEXT_BLUE         // light text on dark blue bg
            } else if is_disabled {
                C_TEXT_GRAY         // light-gray text on dark gray
            } else {
                C_TEXT_MED          // dim text for empty cells
            };

            // Label line (line 0): position reference + source icon + tracker marker
            let label_line = {
                let pos = format!("{}{:02}", row_label, col + 1);
                let marker = if is_grabbed_src { "↑" }
                             else if is_drop_target { "↓" }
                             else if is_tracker_active { "▸" }
                             else { " " };
                let src_icon = match source {
                    Some(PatternSource::Sf2 { .. }) => "♪",
                    Some(PatternSource::AudioFile { .. }) => "▶",
                    _ => " ",
                };
                format!("{:<width$}", format!("{}{}{}", marker, src_icon, pos), width = cell_w)
            };

            // ASCII font rows (lines 1-3): SF2/AudioFile clips show their file name instead.
            let font = if has_clip {
                let display_key = match source {
                    Some(PatternSource::Sf2 { preset_name, .. }) if !preset_name.is_empty() => {
                        preset_name.chars().take(n_font_chars).collect::<String>().to_uppercase()
                    }
                    Some(PatternSource::AudioFile { path, .. }) => {
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("AUDIO")
                            .chars().take(n_font_chars).collect::<String>().to_uppercase()
                    }
                    _ => pat_key.chars().take(n_font_chars).collect::<String>().to_uppercase(),
                };
                ascii_rows(&display_key, cell_w)
            } else {
                [" ".repeat(cell_w), " ".repeat(cell_w), " ".repeat(cell_w)]
            };

            // Top-align: label first, font rows below, optional waveform, empty padding.
            let content_h = 1 + n_font_rows;
            let pad_bot = cell_h.saturating_sub(content_h);
            let empty = " ".repeat(cell_w);
            let mut content: Vec<String> = Vec::with_capacity(cell_h);
            content.push(label_line);
            for i in 0..n_font_rows { content.push(font[i].clone()); }
            // For AudioFile clips with spare rows: show amplitude waveform.
            if pad_bot > 0 {
                let wave_row = if let Some(PatternSource::AudioFile { path, .. }) = source {
                    if let Some(peaks) = app.waveform_cache.get(path) {
                        waveform_bar(peaks, cell_w)
                    } else {
                        // Pending scan: show a dim scanning indicator.
                        "·".repeat(cell_w)
                    }
                } else {
                    empty.clone()
                };
                content.push(wave_row);
                for _ in 0..(pad_bot - 1) { content.push(empty.clone()); }
            }

            (bg, fg, content)
        }).collect();

        // ── Separator / top border ────────────────────────────────────────────
        {
            let (l, m, r) = if row == 0 { ("┌", "┬", "┐") } else { ("├", "┼", "┤") };
            let lbl_style = Style::default()
                .fg(if is_row_cursor { Color::Yellow } else { HEADER })
                .add_modifier(Modifier::BOLD);
            let mut sep: Vec<Span> = vec![
                Span::styled(format!("{} ", row_label), lbl_style),
                Span::raw(" "),
                Span::styled(l.to_string(), Style::default().fg(h_color(row, 0))),
            ];
            for col in 0..n_cols {
                sep.push(Span::styled(h_seg.clone(), Style::default().fg(h_color(row, col))));
                if col < n_cols - 1 {
                    // Junction: yellow if adjacent to either the left (col) or right (col+1) hovered column.
                    let jc = if h_color(row, col) == Color::Yellow || h_color(row, col + 1) == Color::Yellow {
                        Color::Yellow
                    } else {
                        bdr_default
                    };
                    sep.push(Span::styled(m.to_string(), Style::default().fg(jc)));
                }
            }
            sep.push(Span::styled(r.to_string(), Style::default().fg(h_color(row, n_cols.saturating_sub(1)))));
            lines.push(Line::from(sep));
        }

        // ── Content lines ─────────────────────────────────────────────────────
        for h in 0..cell_h {
            let mut spans: Vec<Span> = vec![Span::raw(" ".repeat(ROW_LBL))];
            for col in 0..n_cols {
                let (bg, fg, ref content) = cell_data[col];
                spans.push(Span::styled("│", Style::default().fg(v_color(row, col))));
                spans.push(Span::styled(
                    content[h].clone(),
                    Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
                ));
            }
            spans.push(Span::styled("│", Style::default().fg(v_color(row, n_cols))));
            lines.push(Line::from(spans));
        }
    }

    // ── Bottom border ─────────────────────────────────────────────────────────
    {
        // Bottom border is the "bottom edge" of matrix row n_rows-1.
        let bot_row = n_rows;
        let mut bot: Vec<Span> = vec![Span::raw(" ".repeat(ROW_LBL))];
        bot.push(Span::styled("└".to_string(), Style::default().fg(h_color(bot_row, 0))));
        for col in 0..n_cols {
            bot.push(Span::styled(h_seg.clone(), Style::default().fg(h_color(bot_row, col))));
            if col < n_cols - 1 {
                let jc = if h_color(bot_row, col) == Color::Yellow || h_color(bot_row, col + 1) == Color::Yellow {
                    Color::Yellow
                } else {
                    bdr_default
                };
                bot.push(Span::styled("┴".to_string(), Style::default().fg(jc)));
            }
        }
        bot.push(Span::styled("┘".to_string(), Style::default().fg(h_color(bot_row, n_cols.saturating_sub(1)))));
        lines.push(Line::from(bot));
    }

    // ── Hint row ──────────────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        if grid_active {
            "  e=enable  Enter=open  Del=remove  hjkl=navigate"
        } else {
            "  SPACE=play  s=stop  r=rec  Tab=transport"
        },
        Style::default().fg(if grid_active { Color::Yellow } else { Color::DarkGray }),
    )));

    // ── Render ────────────────────────────────────────────────────────────────
    let title = format!(
        " MATRIX {}×{} :: BPM {} :: {} ",
        n_rows,
        n_cols,
        app.bpm as u32,
        if app.playing { "▶ PLAYING" } else { "■ STOPPED" }
    );
    let border_col = if grid_active { Color::Yellow } else { BORDER };

    // Publish cell geometry so the click handler can map pixel→cell without recalculating.
    app.matrix_cell_size.set((cell_w, cell_h));

    let p = Paragraph::new(lines).block(
        Block::default()
            .title(title)
            .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}

fn draw_transport_buttons(f: &mut Frame, app: &App, area: Rect) {
    let ta = app.matrix_section == 1; // transport active
    let tc = app.transport_cursor;

    let tap_recently = !app.tap_times.is_empty();

    // Each button has its own color identity: bright when active, dim when not.
    let play_col = if app.playing    { Color::Green }              else { Color::Rgb(20, 80, 30)  };
    let stop_col = Color::Rgb(80, 80, 95);
    let rec_col  = if app.recording  { Color::Red   }              else { Color::Rgb(100, 25, 25) };
    let tap_col  = if tap_recently   { Color::White }              else { Color::Rgb(80, 80, 90)  };

    // Content style (text inside box): always uses button color.
    let play_state = Style::default().fg(play_col).add_modifier(if app.playing   { Modifier::BOLD } else { Modifier::empty() });
    let stop_state = Style::default().fg(stop_col);
    let rec_state  = Style::default().fg(rec_col ).add_modifier(if app.recording { Modifier::BOLD } else { Modifier::empty() });
    let tap_state  = Style::default().fg(tap_col ).add_modifier(if tap_recently  { Modifier::BOLD } else { Modifier::empty() });

    // Border style: yellow when cursor is on this button, cyan on hover, otherwise button color.
    let border_s = |idx: usize, col: Color, bold: bool| -> Style {
        if ta && tc == idx {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if app.hovered_transport_btn == Some(idx as u8) {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if bold {
            Style::default().fg(col).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(col)
        }
    };

    let play_border = border_s(0, play_col, app.playing);
    let stop_border = border_s(1, stop_col, false);
    let rec_border  = border_s(2, rec_col,  app.recording);
    let tap_border  = border_s(3, tap_col,  tap_recently);

    // BPM box: highlighted when tc=4 or hovered.
    let bpm_col = if ta && tc == 4 { Color::Yellow }
        else if app.hovered_transport_btn == Some(4) { Color::Cyan }
        else { ACCENT };
    let bpm_val = if ta && tc == 4 {
        Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    };

    // Matrix size row: tc=5=ROWS, tc=6=COLS.
    let rows = app.matrix_rows;
    let cols = app.matrix_cols;
    let hov = app.hovered_transport_btn;
    let lbl_s = |idx: usize| -> Style {
        if ta && tc == idx { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) }
        else if hov == Some(idx as u8) { Style::default().fg(Color::Cyan) }
        else { Style::default().fg(ACCENT) }
    };
    let val_s = |idx: usize| -> Style {
        if ta && tc == idx { Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD) }
        else if hov == Some(idx as u8) { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) }
        else { Style::default().fg(Color::White) }
    };

    let hint = if ta {
        match tc {
            0..=3 => "  Enter=trigger  ←→=navigate  Tab=back to grid",
            4     => "  ↑↓=BPM  ←→=navigate  Tab=back to grid",
            5     => "  ↑↓=ROWS  ←→=navigate  Tab=back to grid",
            _     => "  ↑↓=COLS  ←→=navigate  Tab=back to grid",
        }
    } else {
        "  SPACE=play  s=stop  r=rec  hjkl=navigate  Enter=open  e=enable  Tab=transport"
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("╭──────╮", play_border),
            Span::raw(" "),
            Span::styled("╭──────╮", stop_border),
            Span::raw(" "),
            Span::styled("╭──────╮", rec_border),
            Span::raw(" "),
            Span::styled("╭──────╮", tap_border),
            Span::raw(" "),
            Span::styled("╭─────────╮", Style::default().fg(bpm_col)),
        ]),
        Line::from(vec![
            Span::styled(if app.playing { "│▶ PLAY│" } else { "│■ PLAY│" }, play_state),
            Span::raw(" "),
            Span::styled("│■ STOP│", stop_state),
            Span::raw(" "),
            Span::styled(if app.recording { "│● REC │" } else { "│  REC │" }, rec_state),
            Span::raw(" "),
            Span::styled("│  TAP │", tap_state),
            Span::raw(" "),
            Span::styled("│BPM:", Style::default().fg(bpm_col)),
            Span::styled(format!("{:>4}│", app.bpm as u32), bpm_val),
        ]),
        Line::from(vec![
            Span::styled("╰──────╯", play_border),
            Span::raw(" "),
            Span::styled("╰──────╯", stop_border),
            Span::raw(" "),
            Span::styled("╰──────╯", rec_border),
            Span::raw(" "),
            Span::styled("╰──────╯", tap_border),
            Span::raw(" "),
            Span::styled("╰─────────╯", Style::default().fg(bpm_col)),
        ]),
        Line::from(vec![
            Span::styled("MATRIX SIZE : ", lbl_s(5)),
            Span::styled(format!("{:>3}", rows), val_s(5)),
            Span::styled(" × ", Style::default().fg(ACCENT)),
            Span::styled(format!("{:>3}", cols), val_s(6)),
            Span::styled(
                format!("  = {} slots", rows * cols),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(Span::styled(
            hint,
            Style::default().fg(if ta { Color::Yellow } else { Color::DarkGray }),
        )),
    ];

    let border_col = if ta { Color::Yellow } else { BORDER };
    let p = Paragraph::new(lines).block(
        Block::default()
            .title(if ta { " TRANSPORT [ACTIVE] " } else { " TRANSPORT " })
            .title_style(Style::default().fg(HEADER))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}

fn draw_polymeter(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let step = app.current_step;
    let poly_active = app.matrix_section == 2;

    // Label prefix: "► KEY   LEN " = 1+5+3+1 = 10 chars, plus borders = 12 total overhead.
    let bar_w = area.width.saturating_sub(12) as usize;

    // Collect only patterns that have a matrix cell assigned, in grid order (A01, A02, ...).
    let n_rows = app.matrix_rows;
    let n_cols = app.matrix_cols;
    let mut seen = std::collections::HashSet::new();
    let mut pat_list: Vec<(&String, &seqterm_core::Pattern)> = Vec::new();
    for row in 0..n_rows {
        let row_key = ((b'A' + row as u8) as char).to_string();
        if let Some(slots) = proj.matrix.get(&row_key) {
            for col in 0..n_cols.min(slots.len()) {
                if let Some(Some(clip)) = slots.get(col) {
                    if let Some(pk) = &clip.pattern_key {
                        if !seen.contains(pk) {
                            if let Some(pat) = proj.patterns.get(pk) {
                                seen.insert(pk.clone());
                                pat_list.push((pk, pat));
                            }
                        }
                    }
                }
            }
        }
    }

    // Visible rows available for patterns (subtract 2: info line + hint).
    let inner_h = area.height.saturating_sub(2) as usize;
    let reserved = 2usize;
    let visible_rows = inner_h.saturating_sub(reserved);

    // Clamp pat_scroll so cursor is always visible.
    let cursor = app.polymeter_cursor.min(pat_list.len().saturating_sub(1));
    let mut pat_scroll = app.polymeter_pat_scroll;
    if cursor < pat_scroll { pat_scroll = cursor; }
    if cursor >= pat_scroll + visible_rows.max(1) {
        pat_scroll = cursor + 1 - visible_rows.max(1);
    }

    let mut lines: Vec<Line> = Vec::new();

    // Pattern rows — each bar is compressed to show the full pattern length.
    let visible_pats = pat_list.iter().enumerate().skip(pat_scroll).take(visible_rows);
    for (abs_idx, (key, pat)) in visible_pats {
        let is_cursor = poly_active && abs_idx == cursor;
        let cur_pos = if pat.length > 0 { step % pat.length } else { 0 };

        // Compress full pattern into bar_w chars.
        // Each char i represents steps [ i*L/W .. (i+1)*L/W ).
        let bar: String = if bar_w == 0 || pat.length == 0 {
            String::new()
        } else {
            (0..bar_w).map(|i| {
                let s0 = i * pat.length / bar_w;
                let s1 = ((i + 1) * pat.length / bar_w).max(s0 + 1).min(pat.length);
                // Playhead lands in this bucket?
                if cur_pos >= s0 && cur_pos < s1 {
                    return '►';
                }
                // Any note in this bucket?
                let has_note = (s0..s1)
                    .any(|s| pat.steps.get(s).map(|n| !n.is_empty()).unwrap_or(false));
                if has_note { '█' } else { '·' }
            }).collect()
        };

        let on_beat = pat.steps.get(cur_pos).map(|n| !n.is_empty()).unwrap_or(false);
        let bar_col = if on_beat { Color::Green } else { Color::Rgb(80, 100, 130) };

        let (lbl_style, bar_style) = if is_cursor {
            (
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                Style::default().fg(Color::Yellow),
            )
        } else {
            (
                Style::default().fg(Color::White),
                Style::default().fg(bar_col),
            )
        };

        let prefix = if is_cursor { "►" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}{:<5}{:>3} ", prefix, &key[..key.len().min(5)], pat.length),
                lbl_style,
            ),
            Span::styled(bar, bar_style),
        ]));
    }

    // Info line: global step + bar + pattern count.
    lines.push(Line::from(vec![
        Span::styled(
            format!("step:{:>4}  bar:{:>3}  ", step % 16 + 1, app.current_bar + 1),
            Style::default().fg(ACCENT),
        ),
        Span::styled(
            if app.playing { "▶ PLAY" } else { "■ STOP" },
            Style::default().fg(if app.playing { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            format!("  [{} pats]", pat_list.len()),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // Hint line.
    lines.push(Line::from(Span::styled(
        if poly_active {
            "  ↑↓=select  Tab=next"
        } else {
            "  Tab=focus"
        },
        Style::default().fg(if poly_active { Color::Yellow } else { Color::DarkGray }),
    )));

    let border_col = if poly_active { Color::Yellow } else { BORDER };
    let p = Paragraph::new(lines).block(
        Block::default()
            .title(" POLYMETER VISUALIZER ")
            .title_style(Style::default().fg(HEADER))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}

fn draw_routing_panel(f: &mut Frame, app: &App, area: Rect) {
    let routing_active = app.matrix_section == 3;
    let proj = app.project.lock();
    let (row, col) = app.matrix_state.cursor;
    let row_key = ((b'A' + row as u8) as char).to_string();

    let selected_clip = proj
        .matrix
        .get(&row_key)
        .and_then(|r| r.get(col))
        .and_then(|c| c.as_ref());

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "CLIP ROUTING",
            Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    // ── Clip info ─────────────────────────────────────────────────────────────
    if let Some(clip) = selected_clip {
        lines.push(Line::from(vec![
            Span::styled("Pattern: ", Style::default().fg(ACCENT)),
            Span::styled(
                clip.pattern_key.clone().unwrap_or_else(|| "---".to_string()),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Pos:     ", Style::default().fg(ACCENT)),
            Span::styled(format!("{}{}", row_key, col + 1), Style::default().fg(Color::Yellow)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Status:  ", Style::default().fg(ACCENT)),
            if clip.playing {
                Span::styled("▶ PLAYING", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("■ STOPPED", Style::default().fg(Color::DarkGray))
            },
        ]));
        if let Some(pat_key) = &clip.pattern_key {
            if let Some(pat) = proj.patterns.get(pat_key) {
                lines.push(Line::from(vec![
                    Span::styled("Steps:   ", Style::default().fg(ACCENT)),
                    Span::styled(pat.length.to_string(), Style::default().fg(Color::White)),
                ]));
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            format!("(empty)  {}{}", row_key, col + 1),
            Style::default().fg(Color::DarkGray),
        )));
    }

    // ── Tab header: MIDI OUT | SOURCE ────────────────────────────────────────
    lines.push(Line::from(""));
    {
        let tab0_style = if routing_active && app.routing_tab == 0 {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if app.routing_tab == 0 {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(BORDER)
        };
        let tab1_style = if routing_active && app.routing_tab == 1 {
            Style::default().fg(Color::Black).bg(Color::Rgb(56, 200, 100)).add_modifier(Modifier::BOLD)
        } else if app.routing_tab == 1 {
            Style::default().fg(Color::Rgb(56, 200, 100))
        } else {
            Style::default().fg(BORDER)
        };
        lines.push(Line::from(vec![
            Span::styled(" MIDI OUT ", tab0_style),
            Span::styled(" | ", Style::default().fg(BORDER)),
            Span::styled(" SOURCE ", tab1_style),
            Span::styled(
                if routing_active { "  Tab=switch" } else { "" },
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    lines.push(Line::from(""));

    if app.routing_tab == 1 {
        // ── SOURCE BROWSER tab ────────────────────────────────────────────────
        // Collect unique SF2/AudioFile sources from the project.
        let mut sources: Vec<PatternSource> = Vec::new();
        for slots in proj.matrix.values() {
            for opt in slots {
                let Some(clip) = opt else { continue };
                let src = &clip.source;
                let is_dup = sources.iter().any(|s| match (s, src) {
                    (PatternSource::Sf2  { path: p1, bank: b1, preset: pr1, .. },
                     PatternSource::Sf2  { path: p2, bank: b2, preset: pr2, .. }) => p1 == p2 && b1 == b2 && pr1 == pr2,
                    (PatternSource::AudioFile { path: p1, .. },
                     PatternSource::AudioFile { path: p2, .. }) => p1 == p2,
                    _ => false,
                });
                if !is_dup && !matches!(src, PatternSource::Midi) {
                    sources.push(src.clone());
                }
            }
        }

        // Show current clip's source.
        {
            let label = match selected_clip.map(|c| &c.source) {
                Some(PatternSource::Sf2  { preset_name, bank, preset, .. }) =>
                    format!("Now: SF2 B{}/P{} {}", bank, preset, preset_name.chars().take(12).collect::<String>()),
                Some(PatternSource::AudioFile { path, looping, .. }) => {
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                    format!("Now: {} {}", if *looping { "↻" } else { "▶" }, &stem[..stem.len().min(16)])
                }
                Some(PatternSource::Midi) | None =>
                    "Now: MIDI (unset)".to_string(),
            };
            lines.push(Line::from(Span::styled(label, Style::default().fg(ACCENT))));
            lines.push(Line::from(""));
        }

        if sources.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no SF2/audio sources in project)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            let src_cursor = app.routing_source_cursor.min(sources.len().saturating_sub(1));
            for (i, src) in sources.iter().enumerate() {
                let is_sel = routing_active && i == src_cursor;
                let label = match src {
                    PatternSource::Sf2 { preset_name, bank, preset, path } => {
                        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                        format!(" B{:03}/P{:03}  {:<14} {}", bank, preset,
                            preset_name.chars().take(14).collect::<String>(),
                            &fname[..fname.len().min(10)])
                    }
                    PatternSource::AudioFile { path, looping, .. } => {
                        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                        format!(" {} {}", if *looping { "↻" } else { "▶" }, &stem[..stem.len().min(22)])
                    }
                    PatternSource::Midi => continue,
                };
                let style = if is_sel {
                    Style::default().fg(Color::Black).bg(Color::Rgb(56, 200, 100)).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(Span::styled(label, style)));
            }
        }

        // Hint.
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            if routing_active { "  ↑↓=browse  Enter=assign  f=SF2  F=audio  x=clear" } else { "  Tab=focus routing" },
            Style::default().fg(if routing_active { Color::Rgb(56, 200, 100) } else { Color::DarkGray }),
        )));
    } else {
        // ── MIDI OUTPUT tab ───────────────────────────────────────────────────
        lines.push(Line::from(Span::styled(
            "MIDI OUTPUT",
            Style::default()
                .fg(if routing_active { Color::Yellow } else { HEADER })
                .add_modifier(Modifier::BOLD),
        )));

    let assigned_out: Option<&str> = selected_clip.and_then(|c| c.midi_out.as_deref());
    let cursor = app.routing_cursor;

    // Record the absolute Y of the first routing list item for mouse hit-testing.
    app.routing_list_item_y.set(area.y + 1 + lines.len() as u16);

    // Item 0: (none / unrouted)
    {
        let is_cursor = routing_active && cursor == 0;
        let is_assigned = assigned_out.is_none();
        let check = if is_assigned { "✓" } else { " " };
        let row_style = if is_cursor {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if is_assigned {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(Span::styled(
            format!(" [{}] (none)", check),
            row_style,
        )));
    }

    // If a port is assigned but not in the current list, show it as unavailable.
    if let Some(out) = assigned_out {
        if app.unavailable_midi_routes.contains(out) {
            lines.push(Line::from(vec![
                Span::styled(" [✓] ", Style::default().fg(Color::Indexed(172))),
                Span::styled(
                    format!("{:<20}", out.chars().take(20).collect::<String>()),
                    Style::default().fg(Color::Indexed(172)),
                ),
                Span::styled(" ! UNAVAILABLE", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ]));
        }
    }

    // Items 1..n: currently available midi_outputs
    for (i, port) in proj.midi_outputs.iter().enumerate() {
        let list_idx = i + 1;
        let is_cursor = routing_active && cursor == list_idx;
        let is_assigned = assigned_out == Some(port.name.as_str());
        let check = if is_assigned { "✓" } else { " " };
        let name: String = port.name.chars().take(24).collect();
        let row_style = if is_cursor {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if is_assigned {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(
            format!(" [{}] {}", check, name),
            row_style,
        )));
    }

    // ── MIDI channel ──────────────────────────────────────────────────────────
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "MIDI CHANNEL",
        Style::default()
            .fg(if routing_active { Color::Yellow } else { HEADER })
            .add_modifier(Modifier::BOLD),
    )));
    {
        let ch = selected_clip.map(|c| c.midi_channel).unwrap_or(1);
        let (l_col, r_col, ch_col) = if routing_active {
            (Color::Yellow, Color::Yellow, Color::White)
        } else {
            (Color::DarkGray, Color::DarkGray, Color::White)
        };
        // Publish Y so mouse clicks on ◄/► can be resolved without re-deriving layout.
        app.routing_channel_y.set(area.y + 1 + lines.len() as u16);
        lines.push(Line::from(vec![
            Span::styled("  ◄ ", Style::default().fg(l_col)),
            Span::styled(
                format!("CH {:02}", ch),
                Style::default().fg(ch_col).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ►", Style::default().fg(r_col)),
        ]));
    }

    // ── Scenes ────────────────────────────────────────────────────────────────
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "SCENES",
        Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
    )));
    for scene in proj.scenes.iter().take(4) {
        let active = scene.active_clips.iter().filter(|c| c.is_some()).count();
        lines.push(Line::from(vec![
            Span::styled("  [", Style::default().fg(BORDER)),
            Span::styled(format!("{:<8}", &scene.name), Style::default().fg(Color::White)),
            Span::styled("]", Style::default().fg(BORDER)),
            Span::styled(format!(" {:>2}ch", active), Style::default().fg(ACCENT)),
        ]));
    }

    // ── Hint ──────────────────────────────────────────────────────────────────
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        if routing_active { "  ↑↓=MIDI out  Enter=assign  ←→=channel  R=refresh" } else { "  Tab=focus routing" },
        Style::default().fg(if routing_active { Color::Yellow } else { Color::DarkGray }),
    )));
    } // end of MIDI OUTPUT tab else-branch

    let border_col = if routing_active { Color::Yellow } else { BORDER };
    let p = Paragraph::new(lines).block(
        Block::default()
            .title(" ROUTING ")
            .title_style(Style::default().fg(HEADER))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}
