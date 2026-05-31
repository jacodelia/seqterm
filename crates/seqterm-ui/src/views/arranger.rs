use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;

const PANEL: Color = Color::Rgb(22, 27, 34);
const BORDER: Color = Color::Rgb(48, 54, 61);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const HEADER: Color = Color::Rgb(240, 136, 62);

pub fn draw_arranger(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // bar ruler
            Constraint::Min(8),     // track lanes (matrix patterns)
            Constraint::Length(9),  // automation
            Constraint::Length(9),  // song transport
        ])
        .split(area);

    // Cache subsection rects: [tracks, automation, song_transport].
    // chunks[0] is the bar ruler — not an interactive section.
    app.arranger_panel_rects.set([chunks[1], chunks[2], chunks[3]]);

    draw_bar_ruler(f, app, chunks[0]);
    draw_track_lanes(f, app, chunks[1]);
    draw_automation_lanes(f, app, chunks[2]);

    // Song transport area: left = controls, right = chain editor.
    let transport_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(0)])
        .split(chunks[3]);
    draw_song_transport(f, app, transport_cols[0]);
    draw_chain_editor(f, app, transport_cols[1]);
}

// ──────────────────────────────────────────────────────────────── Bar ruler ──

fn draw_bar_ruler(f: &mut Frame, app: &App, area: Rect) {
    let offset = app.arranger_state.bar_offset;
    let track_name_w: u32 = 14;
    let avail_w = (area.width as u32).saturating_sub(track_name_w);
    let visible = avail_w / 4;

    let mut spans: Vec<Span> = vec![Span::styled(
        format!("{:<14}", " BAR"),
        Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
    )];

    for bar in offset..offset + visible {
        let is_current = app.song_playing && bar == app.current_bar as u32;
        let label = if bar % 4 == 0 {
            format!("{:02}──", bar + 1)
        } else {
            "────".to_string()
        };
        let style = if is_current {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else if bar % 4 == 0 {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(BORDER)
        };
        spans.push(Span::styled(label, style));
    }

    let ruler = Paragraph::new(Line::from(spans)).style(Style::default().bg(PANEL));
    f.render_widget(ruler, area);
}

// ────────────────────────────────────────────── Track lanes (matrix clips) ──

fn draw_track_lanes(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let offset = app.arranger_state.bar_offset;
    let track_name_w: u16 = 14;
    let avail_bars = ((area.width as u32).saturating_sub(track_name_w as u32) / 4).max(1) as usize;

    let n_rows = app.matrix_rows;
    let n_cols = app.matrix_cols;

    // Compute accumulated bar start for each column (synchronized across all rows).
    // col_starts[col] = bar at which column `col` begins.
    let mut col_starts: Vec<u32> = vec![0u32; n_cols + 1];
    for col in 0..n_cols {
        let max_bars = (0..n_rows)
            .filter_map(|row| {
                let rk = ((b'A' + row as u8) as char).to_string();
                proj.matrix.get(&rk)
                    ?.get(col)?
                    .as_ref()
                    .and_then(|c| c.pattern_key.as_ref())
                    .and_then(|k| proj.patterns.get(k))
                    .map(|p| {
                        let tsn = p.time_sig_num.max(1) as usize;
                        ((p.length + tsn - 1) / tsn).max(1) as u32
                    })
            })
            .max()
            .unwrap_or(2); // empty column = 2 bars wide
        col_starts[col + 1] = col_starts[col] + max_bars;
    }

    let mut lines: Vec<Line> = Vec::new();

    // Header row with column bar positions.
    let mut hdr_spans: Vec<Span> = vec![Span::styled(
        format!("{:<14}", " TRACK"),
        Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
    )];
    for col in 0..n_cols {
        let bar = col_starts[col];
        if bar >= offset as u32 && (bar - offset as u32) < avail_bars as u32 {
            let pos = ((bar - offset as u32) * 4) as usize;
            let label = format!("C{:02}", col + 1);
            hdr_spans.push(Span::styled(
                format!("{:<4}", label),
                Style::default().fg(ACCENT),
            ));
            let _ = pos; // used for alignment below
        }
    }
    lines.push(Line::from(hdr_spans));

    // One track lane per matrix row (A, B, C ...).
    for row in 0..n_rows {
        let row_label = (b'A' + row as u8) as char;
        let row_key = row_label.to_string();
        let is_selected = app.arranger_state.selected_track == row
            && app.arranger_state.section == 0;

        // Build a flat bar map for this row: bar_pos → (label, bars_wide, col_idx).
        let clips_in_row: Vec<(u32, u32, String)> = (0..n_cols)
            .filter_map(|col| {
                let clip = proj
                    .matrix
                    .get(&row_key)
                    .and_then(|r| r.get(col))
                    .and_then(|c| c.as_ref())?;
                let pat_key = clip.pattern_key.as_ref()?;
                let pat = proj.patterns.get(pat_key)?;
                let tsn = pat.time_sig_num.max(1) as usize;
                let bars = ((pat.length + tsn - 1) / tsn).max(1) as u32;
                let bar_start = col_starts[col];
                Some((bar_start, bars, pat_key.clone()))
            })
            .collect();

        // Track name: custom name from track_names or row letter.
        let custom_name = proj.track_names.get(&row_key).cloned().unwrap_or_default();
        let display_name = if is_selected && app.arranger_track_name_editing {
            format!("{} {}_", row_label, app.arranger_track_name_buffer)
        } else if !custom_name.is_empty() {
            format!("{} {}", row_label, custom_name)
        } else {
            format!("{}  {:>2} clips", row_label, clips_in_row.len())
        };

        let name_style = if is_selected && app.arranger_track_name_editing {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED)
        } else if clips_in_row.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(
            format!(" {:<13}", display_name),
            name_style,
        )));

        // Clip content row.
        let mut block_spans: Vec<Span> = vec![
            Span::styled(" ".repeat(14), Style::default()),
        ];

        let visible_end = offset as u32 + avail_bars as u32;
        let cur_bar = app.current_bar as u32;

        let mut bar = offset as u32;
        while bar < visible_end {
            let occupied = clips_in_row.iter().find(|(start, len, _)| {
                bar >= *start && bar < start + len
            });
            match occupied {
                Some((start, len, label)) if bar == *start => {
                    let end = (start + len).min(visible_end);
                    let block_w = ((end - bar) * 4) as usize;
                    let is_playing = app.song_playing
                        && bar <= cur_bar
                        && cur_bar < (start + len);
                    let inner = format!("▸{}", &label[..label.len().min(block_w.saturating_sub(2))]);
                    let display = format!("{:<width$}", inner, width = block_w);
                    let style = if is_playing {
                        Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
                    } else if is_selected {
                        Style::default().fg(Color::Yellow).bg(Color::Rgb(30, 55, 120)).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White).bg(Color::Rgb(20, 45, 90))
                    };
                    block_spans.push(Span::styled(display, style));
                    bar = end;
                }
                Some((start, len, _)) => {
                    // Continuation of a block that started before visible area.
                    let end = (start + len).min(visible_end);
                    let cont_w = ((end - bar) * 4) as usize;
                    let is_playing = app.song_playing && bar <= cur_bar;
                    let style = if is_playing {
                        Style::default().fg(Color::Rgb(50, 200, 80)).bg(Color::Rgb(10, 40, 20))
                    } else {
                        Style::default().fg(Color::Rgb(40, 80, 160)).bg(Color::Rgb(15, 30, 60))
                    };
                    block_spans.push(Span::styled("━".repeat(cont_w), style));
                    bar = end;
                }
                None => {
                    let is_current = app.song_playing && bar == app.current_bar as u32;
                    let style = if is_current {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(BORDER)
                    };
                    block_spans.push(Span::styled("····", style));
                    bar += 1;
                }
            }
        }

        lines.push(Line::from(block_spans));
        lines.push(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(BORDER),
        )));
    }

    // Hint.
    if app.arranger_state.section == 0 {
        let hint = if app.arranger_track_name_editing {
            "  TYPE=track name  Enter=confirm  Esc=cancel"
        } else {
            "  ↑↓=track  ←→=scroll  Enter=rename  Tab=transport"
        };
        lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::Yellow),
        )));
    }

    let tracks_focused = app.arranger_state.section == 0;
    let tracks_title = format!(" TRACKS [{} rows] ", n_rows);
    let p = Paragraph::new(lines).block(
        Block::default()
            .title(tracks_title)
            .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(if tracks_focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(BORDER)
            })
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}

// ───────────────────────────────────────────────────────── Automation lanes ──

fn draw_automation_lanes(f: &mut Frame, app: &App, area: Rect) {
    const EIGHTS: &[&str] = &[" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    const N_CHART: usize = 5;

    let proj = app.project.lock();
    let offset = app.arranger_state.bar_offset;
    let auto_focused = app.arranger_state.section == 1;
    let selected_lane = app.arranger_state.automation_lane;
    let auto_cursor = app.arranger_state.automation_cursor as u32;

    // Axis label column: "FIL 127 ┤" = 9 chars (same width as track modulation).
    let axis_w: u16 = 9;
    let chart_w = area.width.saturating_sub(axis_w + 2) as usize;
    let avail_bars = chart_w.max(1);

    let axis_style = if auto_focused {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT)
    };

    let mut lines: Vec<Line> = Vec::new();

    if let Some(lane) = proj.automation.get(selected_lane) {
        let pn_short = &lane.name[..lane.name.len().min(3)];

        // Chart rows (top=127, bottom=0).
        for bar_row in 0..N_CHART {
            let is_top    = bar_row == 0;
            let is_bottom = bar_row == N_CHART - 1;

            let axis_str = if is_top {
                format!("{:<3} 127 ┤", pn_short)
            } else if is_bottom {
                "      0 └".to_string()
            } else {
                "        │".to_string()
            };
            let mut spans = vec![Span::styled(axis_str, axis_style)];

            for i in 0..avail_bars {
                let bar = offset + i as u32;
                let is_cursor  = auto_focused && bar == auto_cursor;
                let is_playing = app.song_playing && bar == app.current_bar as u32;

                let val = lane.points.iter()
                    .filter(|(b, _)| *b <= bar)
                    .last()
                    .map(|(_, v)| *v)
                    .unwrap_or(0);

                let cell: &str = {
                    let eighths   = val as usize * N_CHART * 8 / 127;
                    let full_rows = eighths / 8;
                    let partial   = eighths % 8;
                    let row_bot   = N_CHART - 1 - bar_row;
                    if row_bot < full_rows                          { EIGHTS[8] }
                    else if row_bot == full_rows && partial > 0     { EIGHTS[partial] }
                    else                                            { EIGHTS[0] }
                };

                let fg = if is_cursor {
                    Color::Yellow
                } else if is_playing {
                    Color::Green
                } else {
                    Color::Rgb(60, 100, 180)
                };

                let bg = if is_cursor {
                    Color::Rgb(50, 40, 0)
                } else {
                    Color::Reset
                };

                spans.push(Span::styled(cell.to_string(), Style::default().fg(fg).bg(bg)));
            }

            // Bottom row: show cursor bar info.
            if is_bottom {
                let cur_val = lane.points.iter()
                    .filter(|(b, _)| *b <= auto_cursor)
                    .last()
                    .map(|(_, v)| *v)
                    .unwrap_or(0);
                let info = format!(" bar:{:03} val:{:03}", auto_cursor + 1, cur_val);
                spans.push(Span::styled(info, Style::default().fg(Color::DarkGray)));
            }

            lines.push(Line::from(spans));
        }

        // Lane tabs row.
        let mut tab_spans = vec![Span::styled(
            if auto_focused { "←→:" } else { "   " },
            Style::default().fg(if auto_focused { Color::Yellow } else { Color::DarkGray }),
        )];
        for (i, al) in proj.automation.iter().enumerate() {
            let is_sel = auto_focused && i == selected_lane;
            tab_spans.push(Span::styled(
                format!(" {} ", &al.name),
                if is_sel {
                    Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(if auto_focused { Color::White } else { Color::DarkGray })
                },
            ));
        }
        if auto_focused {
            tab_spans.push(Span::styled(
                "  ↑↓=value  a=add/remove",
                Style::default().fg(Color::DarkGray),
            ));
        }
        lines.push(Line::from(tab_spans));
    } else {
        lines.push(Line::from(Span::styled(
            "  No automation lanes",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Hint row.
    let hint = if auto_focused {
        " ←→=lane/bar  ↑↓=value  a=point  Tab=next"
    } else {
        " Tab=activate automation"
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(if auto_focused { Color::Yellow } else { Color::DarkGray }),
    )));

    let auto_border = if auto_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(BORDER)
    };

    let auto_title = if auto_focused {
        let name = proj.automation.get(selected_lane)
            .map(|l| l.name.as_str())
            .unwrap_or("—");
        format!(" AUTOMATION :: {} [ACTIVE] ", name)
    } else {
        " AUTOMATION ".to_string()
    };

    let p = Paragraph::new(lines).block(
        Block::default()
            .title(auto_title)
            .title_style(Style::default().fg(HEADER))
            .borders(Borders::ALL)
            .border_style(auto_border)
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}

// ───────────────────────────────────────────────────────── Song transport ──

fn draw_chain_editor(f: &mut Frame, app: &App, area: Rect) {
    let (chain, scene_names) = {
        let proj = app.project.lock();
        let chain = proj.chain.clone();
        let names: Vec<String> = proj.scenes.iter().map(|s| s.name.clone()).collect();
        (chain, names)
    };

    let chain_on = app.chain_mode;
    let cur_pos  = app.chain_pos;

    let block = Block::default()
        .title(if chain_on { " CHAIN [ON] " } else { " CHAIN " })
        .title_style(Style::default()
            .fg(if chain_on { Color::Green } else { HEADER })
            .add_modifier(if chain_on { Modifier::BOLD } else { Modifier::empty() }))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if chain_on { Color::Green } else { BORDER }))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let w = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    if chain.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (empty — 'a' to add scene entry)",
            Style::default().fg(BORDER),
        )));
    } else {
        for (i, entry) in chain.iter().enumerate() {
            let is_cur = chain_on && i == cur_pos;
            let scene_name = scene_names.get(entry.scene_idx)
                .map(|n| n.chars().take(8).collect::<String>())
                .unwrap_or_else(|| format!("#{}", entry.scene_idx));
            let label = format!(" {:>2}. S{} {:>8}  x{} bars",
                i + 1, entry.scene_idx + 1, scene_name, entry.bars);
            let padded = format!("{:<w$}", label, w = w.saturating_sub(1));
            let style = if is_cur {
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(if entry.scene_idx < scene_names.len() {
                    Color::White
                } else {
                    Color::DarkGray
                })
            };
            lines.push(Line::from(Span::styled(padded, style)));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  C=toggle  a=add  Del=rm  ←→=seek  ↑↓=bars",
        Style::default().fg(BORDER),
    )));

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(PANEL)),
        inner,
    );
}

fn draw_song_transport(f: &mut Frame, app: &App, area: Rect) {
    let ta = app.arranger_state.section == 2;
    let tc = app.arranger_state.song_transport_cursor;
    let sp = app.song_playing;

    let play_col = if sp { Color::Green } else { Color::Rgb(20, 80, 30) };
    let stop_col = Color::Rgb(80, 80, 95);
    let rec_col  = if app.recording { Color::Red } else { Color::Rgb(100, 25, 25) };
    let bpm_col  = if ta && tc == 3 { Color::Yellow } else { ACCENT };

    let play_state = Style::default().fg(play_col).add_modifier(if sp { Modifier::BOLD } else { Modifier::empty() });
    let stop_state = Style::default().fg(stop_col);
    let rec_state  = Style::default().fg(rec_col).add_modifier(if app.recording { Modifier::BOLD } else { Modifier::empty() });

    let border_s = |idx: usize, col: Color, bold: bool| -> Style {
        if ta && tc == idx {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if bold {
            Style::default().fg(col).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(col)
        }
    };

    let play_border = border_s(0, play_col, sp);
    let stop_border = border_s(1, stop_col, false);
    let rec_border  = border_s(2, rec_col, app.recording);

    let bpm_val = if ta && tc == 3 {
        Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    };

    let hint = if ta {
        match tc {
            0..=2 => "  Enter=trigger  ←→=navigate  Tab=back to tracks",
            _     => "  ↑↓=BPM  ←→=navigate  Tab=back to tracks",
        }
    } else {
        "  SPACE=play/stop  s=stop  r=rec  Tab=activate song transport"
    };

    // Song position display.
    let song_pos_style = Style::default().fg(if sp { Color::Green } else { Color::DarkGray }).add_modifier(Modifier::BOLD);
    let song_pos_label = format!("BAR {:>4}", app.current_bar + 1);

    let lines = vec![
        Line::from(vec![
            Span::styled("╭──────╮", play_border),
            Span::raw(" "),
            Span::styled("╭──────╮", stop_border),
            Span::raw(" "),
            Span::styled("╭──────╮", rec_border),
            Span::raw("   "),
            Span::styled("╭─────────╮", Style::default().fg(bpm_col)),
            Span::raw("  "),
            Span::styled("╭──────────╮", Style::default().fg(if sp { Color::Green } else { BORDER })),
        ]),
        Line::from(vec![
            Span::styled(if sp { "│▶ PLAY│" } else { "│■ PLAY│" }, play_state),
            Span::raw(" "),
            Span::styled("│■ STOP│", stop_state),
            Span::raw(" "),
            Span::styled(if app.recording { "│● REC │" } else { "│  REC │" }, rec_state),
            Span::raw("   "),
            Span::styled("│BPM:", Style::default().fg(bpm_col)),
            Span::styled(format!("{:>4}│", app.bpm as u32), bpm_val),
            Span::raw("  "),
            Span::styled(format!("│{}│", song_pos_label), song_pos_style),
        ]),
        Line::from(vec![
            Span::styled("╰──────╯", play_border),
            Span::raw(" "),
            Span::styled("╰──────╯", stop_border),
            Span::raw(" "),
            Span::styled("╰──────╯", rec_border),
            Span::raw("   "),
            Span::styled("╰─────────╯", Style::default().fg(bpm_col)),
            Span::raw("  "),
            Span::styled("╰──────────╯", Style::default().fg(if sp { Color::Green } else { BORDER })),
        ]),
        Line::from(vec![
            Span::styled(
                if sp { "  SONG MODE  ▶ PLAYING" } else { "  SONG MODE  ■ STOPPED" },
                Style::default()
                    .fg(if sp { Color::Green } else { Color::DarkGray })
                    .add_modifier(if sp { Modifier::BOLD } else { Modifier::empty() }),
            ),
        ]),
        Line::from(Span::styled(
            hint,
            Style::default().fg(if ta { Color::Yellow } else { Color::DarkGray }),
        )),
    ];

    let border_col = if ta { Color::Yellow } else { BORDER };
    let title = if ta { " SONG TRANSPORT [ACTIVE] " } else { " SONG TRANSPORT " };
    let p = Paragraph::new(lines).block(
        Block::default()
            .title(title)
            .title_style(Style::default().fg(HEADER))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}
