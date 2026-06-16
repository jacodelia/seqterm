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
            Constraint::Length(3),  // bar ruler + marker row
            Constraint::Min(8),     // track lanes (matrix patterns)
            Constraint::Length(9),  // automation
            Constraint::Length(9),  // song transport
        ])
        .split(area);

    // Cache subsection rects: [tracks, automation, song_transport].
    // chunks[0] is the bar ruler — not an interactive section.
    app.arranger_panel_rects.set([chunks[1], chunks[2], chunks[3]]);

    draw_bar_ruler(f, app, chunks[0]);
    if app.arranger_state.arrangement_mode {
        draw_arrangement_timeline(f, app, chunks[1]);
    } else {
        draw_track_lanes(f, app, chunks[1]);
    }
    draw_automation_lanes(f, app, chunks[2]);

    // Song transport area: left = controls, right = chain editor.
    let transport_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(0)])
        .split(chunks[3]);
    draw_song_transport(f, app, transport_cols[0]);
    draw_chain_editor(f, app, transport_cols[1]);
}

/// 8-color track palette (indices 0-7).
const TRACK_PALETTE: [Color; 8] = [
    Color::Rgb(31, 111, 235),  // 0 blue (default)
    Color::Rgb(180,  60,  60), // 1 red
    Color::Rgb( 60, 160,  60), // 2 green
    Color::Rgb(200, 130,  20), // 3 amber
    Color::Rgb(130,  60, 200), // 4 purple
    Color::Rgb( 20, 170, 160), // 5 teal
    Color::Rgb(200, 100, 160), // 6 pink
    Color::Rgb(150, 150, 150), // 7 grey
];

// ──────────────────────────────────────────────────────────────── Bar ruler ──

fn draw_bar_ruler(f: &mut Frame, app: &App, area: Rect) {
    let offset = app.arranger_state.bar_offset;
    let bw = app.arranger_state.bar_width.max(2) as u32;
    let track_name_w: u32 = 18;
    let avail_w = (area.width as u32).saturating_sub(track_name_w);
    let visible = (avail_w / bw).max(1);

    let (markers, loop_region) = {
        let proj = app.project.lock();
        (proj.markers.clone(), proj.loop_region)
    };

    // Split the 3-row area: bar ruler (row 0), beat dots (row 1), markers + loop (row 2).
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    let bw_str = bw as usize;

    // Row 0: bar numbers.
    let mut bar_spans: Vec<Span> = vec![Span::styled(
        format!("{:<18}", " BAR"),
        Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
    )];
    for bar in offset..offset + visible {
        let is_current = app.song_playing && bar == app.current_bar as u32;
        let label = if bar % 4 == 0 {
            format!("{:<w$}", bar + 1, w = bw_str)
        } else {
            " ".repeat(bw_str)
        };
        let style = if is_current {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else if bar % 4 == 0 { Style::default().fg(ACCENT) } else { Style::default().fg(BORDER) };
        bar_spans.push(Span::styled(label, style));
    }
    f.render_widget(Paragraph::new(Line::from(bar_spans)).style(Style::default().bg(PANEL)), rows[0]);

    // Row 1: beat subdivisions within each bar + loop region tint.
    let mut beat_spans: Vec<Span> = vec![Span::styled(format!("{:<18}", ""), Style::default())];
    for bar in offset..offset + visible {
        let is_current = app.song_playing && bar == app.current_bar as u32;
        let in_loop = loop_region.map(|(lo, hi)| bar >= lo && bar < hi).unwrap_or(false);
        let bg = if in_loop { Color::Rgb(20, 60, 20) } else { Color::Reset };
        let label = if is_current {
            format!("{:<w$}", "◀", w = bw_str)
        } else {
            let dot = if bw_str >= 2 { "·" } else { "·" };
            format!("{}{}", dot, " ".repeat(bw_str.saturating_sub(1)))
        };
        let fg = if is_current { Color::Green } else { Color::Rgb(60, 80, 60) };
        beat_spans.push(Span::styled(label, Style::default().fg(fg).bg(bg)));
    }
    f.render_widget(Paragraph::new(Line::from(beat_spans)).style(Style::default().bg(PANEL)), rows[1]);

    // Row 2: markers + loop boundaries.
    let mut mk_spans: Vec<Span> = vec![Span::styled(
        format!("{:<18}", " MARKERS"),
        Style::default().fg(Color::DarkGray),
    )];
    for bar in offset..offset + visible {
        let marker = markers.iter().find(|(b, _)| *b == bar);
        let is_loop_in  = loop_region.map(|(lo, _)| lo == bar).unwrap_or(false);
        let is_loop_out = loop_region.map(|(_, hi)| hi == bar).unwrap_or(false);
        let (label, style) = if let Some((_, name)) = marker {
            let trunc = &name[..name.len().min(bw_str.saturating_sub(1))];
            (format!("▼{:<w$}", trunc, w = bw_str.saturating_sub(1)),
             Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        } else if is_loop_in {
            (format!("{:<w$}", "[I", w = bw_str), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        } else if is_loop_out {
            (format!("{:<w$}", "O]", w = bw_str), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        } else {
            (" ".repeat(bw_str), Style::default().fg(BORDER))
        };
        mk_spans.push(Span::styled(label, style));
    }
    f.render_widget(Paragraph::new(Line::from(mk_spans)).style(Style::default().bg(PANEL)), rows[2]);
}

// ────────────────────────────────────────────── Track lanes (matrix clips) ──

fn draw_track_lanes(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let offset = app.arranger_state.bar_offset;
    let bw = app.arranger_state.bar_width.max(2) as u32;
    let track_name_w: u16 = 18;
    let avail_bars = ((area.width as u32).saturating_sub(track_name_w as u32) / bw).max(1) as usize;

    let n_rows = app.matrix_rows;
    let n_cols = app.matrix_cols;

    // ── Virtualized col_starts: stop computing once we are past the right edge.
    // For 10,000+ clips this avoids O(n_clips) work every frame; only O(visible_cols)
    // clip data is fetched.
    let visible_end_bar = offset as u32 + avail_bars as u32;
    let mut col_starts: Vec<u32> = vec![0u32; n_cols + 1];
    let mut first_visible_col = n_cols; // leftmost col whose right edge > offset
    let mut last_visible_col  = 0usize;  // rightmost col whose left edge < visible_end
    for col in 0..n_cols {
        let col_start = col_starts[col];
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
            .unwrap_or(2);
        col_starts[col + 1] = col_start + max_bars;
        let col_end = col_start + max_bars;
        // Track viewport intersection for visible range culling.
        if col_end > offset as u32 { first_visible_col = first_visible_col.min(col); }
        if col_start < visible_end_bar { last_visible_col = col; }
        // Early-out: once the column starts past the right edge, remaining cols
        // are invisible — no need to fetch their clip data.
        if col_start >= visible_end_bar { break; }
    }

    let tracks_focused = app.arranger_state.section == 0;
    let sel_track  = app.arranger_state.selected_track;
    let sel_col    = app.arranger_state.selected_col;
    let track_scroll = app.arranger_state.track_scroll;

    // Vertical viewport: how many track rows fit in the available height.
    // Each track uses at least 2 lines (name + clip row) plus 1 separator,
    // so estimate max visible tracks = area.height / 3.
    let max_visible_tracks = ((area.height as usize).saturating_sub(2) / 3).max(1);
    // Clamp first visible row to keep selected track in view.
    let first_row = track_scroll.min(n_rows.saturating_sub(1));
    let last_row  = (first_row + max_visible_tracks).min(n_rows);

    let mut lines: Vec<Line> = Vec::new();

    // Header row: show snap mode + column start bars.
    let snap_label = app.arranger_state.snap.label();
    let mut hdr_spans: Vec<Span> = vec![Span::styled(
        format!("{:<14}SNAP:{:<4}", " TRACKS", snap_label),
        Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
    )];
    for col in 0..n_cols {
        let bar = col_starts[col];
        if bar >= offset as u32 && (bar - offset as u32) < avail_bars as u32 {
            let is_cur_col = tracks_focused && col == sel_col;
            hdr_spans.push(Span::styled(
                format!("C{:02} ", col + 1),
                if is_cur_col {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(ACCENT)
                },
            ));
        }
    }
    lines.push(Line::from(hdr_spans));

    for row in first_row..last_row {
        let row_label = (b'A' + row as u8) as char;
        let row_key   = row_label.to_string();

        // Skip hidden tracks — but show a collapsed stub when focused.
        let is_hidden  = proj.track_hidden.contains(&row_key);
        let is_selected = tracks_focused && sel_track == row;

        if is_hidden && !is_selected {
            lines.push(Line::from(Span::styled(
                format!(" {} [HIDDEN]", row_label),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        let track_color = TRACK_PALETTE[
            proj.track_colors.get(&row_key).copied().unwrap_or(0) as usize % TRACK_PALETTE.len()
        ];
        let track_kind = proj.track_types.get(&row_key).copied()
            .unwrap_or_default();

        // Build clip list for this row — only for columns that overlap the viewport.
        // This is the core virtualization: skip O(off-screen clips) every frame.
        let vis_range = first_visible_col..=last_visible_col.min(n_cols.saturating_sub(1));
        let clips_in_row: Vec<(u32, u32, String, usize)> = vis_range
            .filter_map(|col| {
                let clip_start = col_starts[col];
                // Viewport cull at the element level.
                if clip_start >= visible_end_bar { return None; }
                let clip = proj
                    .matrix
                    .get(&row_key)
                    .and_then(|r| r.get(col))
                    .and_then(|c| c.as_ref())?;
                let pat_key = clip.pattern_key.as_ref()?;
                let pat = proj.patterns.get(pat_key)?;
                let tsn = pat.time_sig_num.max(1) as usize;
                let bars = ((pat.length + tsn - 1) / tsn).max(1) as u32;
                // Skip clips fully to the left of the viewport.
                if clip_start + bars <= offset as u32 { return None; }
                Some((clip_start, bars, pat_key.clone(), col))
            })
            .collect();

        // Track name header: ■ TYPE row_label [name | clip count].
        let custom_name = proj.track_names.get(&row_key).cloned().unwrap_or_default();
        let display_name = if is_selected && app.arranger_track_name_editing {
            format!("{}_", app.arranger_track_name_buffer)
        } else if !custom_name.is_empty() {
            custom_name.chars().take(8).collect()
        } else {
            format!("{:>2}cl", clips_in_row.len())
        };

        let name_bg = if is_selected && app.arranger_track_name_editing {
            Color::Rgb(60, 50, 0)
        } else if is_selected {
            Color::Rgb(30, 30, 50)
        } else {
            PANEL
        };

        let name_fg = if is_selected {
            Color::Yellow
        } else if is_hidden {
            Color::DarkGray
        } else {
            Color::White
        };

        // "■ MIDI A name    " = 18 chars
        let kind_badge = track_kind.short_label();
        let is_frozen = proj.channels.iter()
            .find(|c| c.midi_port.as_deref() == Some(row_key.as_str()))
            .map(|c| c.frozen)
            .unwrap_or(false);
        let freeze_icon = if is_frozen { "❄" } else { "■" };
        let freeze_style = if is_frozen {
            Style::default().fg(Color::Rgb(80, 180, 220)) // ice-blue for frozen
        } else {
            Style::default().fg(track_color)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", freeze_icon), freeze_style),
            Span::styled(
                format!("{} {} {:<8}", kind_badge, row_label, display_name),
                Style::default().fg(name_fg).bg(name_bg),
            ),
        ]));

        let row_height = proj.track_heights.get(&row_key).copied()
            .unwrap_or(2).clamp(2, 6) as usize;

        let visible_end = offset as u32 + avail_bars as u32;
        let cur_bar = app.current_bar as u32;

        // Build a closure that renders one row of clip blocks.
        // `top_row` = true for the first row (shows label); false for body rows.
        let build_clip_row = |top_row: bool| -> Line<'static> {
            let mut spans: Vec<Span<'static>> = vec![
                Span::raw(" ".repeat(track_name_w as usize)),
            ];
            let mut bar = offset as u32;
            while bar < visible_end {
                let occupied = clips_in_row.iter().find(|(start, len, _, _)| {
                    bar >= *start && bar < start + len
                });
                match occupied {
                    Some((start, len, label, col_idx)) if top_row && bar == *start => {
                        let end = (start + len).min(visible_end);
                        let block_w = ((end - bar) * bw) as usize;
                        let is_playing = app.song_playing
                            && bar <= cur_bar && cur_bar < (start + len);
                        let is_clip_sel = tracks_focused && sel_track == row && sel_col == *col_idx;
                        let is_multi = app.arranger_state.multi_select.contains(&(row, *col_idx));
                        let inner = format!("▸{}", &label[..label.len().min(block_w.saturating_sub(2))]);
                        let display = format!("{:<width$}", inner, width = block_w);
                        let style = if is_playing {
                            Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
                        } else if is_multi {
                            Style::default().fg(Color::Black).bg(Color::Rgb(180, 100, 0)).add_modifier(Modifier::BOLD)
                        } else if is_clip_sel {
                            Style::default().fg(Color::Yellow).bg(Color::Rgb(50, 70, 160)).add_modifier(Modifier::BOLD)
                        } else if is_selected {
                            Style::default().fg(track_color).bg(Color::Rgb(20, 35, 80)).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(track_color).bg(Color::Rgb(15, 25, 55))
                        };
                        spans.push(Span::styled(display, style));
                        bar = end;
                    }
                    Some((start, len, _, col_idx)) => {
                        let end = (start + len).min(visible_end);
                        let w = ((end - bar) * bw) as usize;
                        let is_clip_sel = tracks_focused && sel_track == row && sel_col == *col_idx;
                        let is_playing = app.song_playing && bar <= cur_bar && cur_bar < (start + len);
                        let style = if is_playing {
                            Style::default().fg(Color::Rgb(50, 200, 80)).bg(Color::Rgb(10, 40, 20))
                        } else if is_clip_sel {
                            Style::default().fg(Color::Yellow).bg(Color::Rgb(40, 55, 120))
                        } else if is_selected {
                            Style::default().fg(track_color).bg(Color::Rgb(20, 35, 80))
                        } else {
                            Style::default().fg(track_color).bg(Color::Rgb(12, 20, 45))
                        };
                        let fill = if top_row { "━".repeat(w) } else { " ".repeat(w) };
                        spans.push(Span::styled(fill, style));
                        bar = end;
                    }
                    None => {
                        let is_current = app.song_playing && bar == cur_bar;
                        let style = if is_current {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default().fg(BORDER)
                        };
                        spans.push(Span::styled("·".repeat(bw as usize), style));
                        bar += 1;
                    }
                }
            }
            Line::from(spans)
        };

        // First clip row: shows labels.
        lines.push(build_clip_row(true));

        // Extra body rows for height > 2.
        for _ in 1..row_height.saturating_sub(1) {
            lines.push(build_clip_row(false));
        }

        // Separator between tracks.
        lines.push(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(BORDER),
        )));
    }

    // Hint.
    if tracks_focused {
        let hint = if app.arranger_track_name_editing {
            "  TYPE=name  Enter=confirm  Esc=cancel"
        } else if app.arranger_state.resize_mode {
            "  RESIZE MODE — [=shrink  ]=grow  r/Esc=exit"
        } else {
            "  ↑↓=track  ←→=scroll  []=clip  d=dup  Del=rm  x=split  g=glue  r=resize  H=hide  t=type  c=color  S=snap"
        };
        lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::Yellow),
        )));
    }

    // Scroll indicator when more tracks than visible.
    if last_row < n_rows {
        lines.push(Line::from(Span::styled(
            format!("  … {} more tracks below (Ctrl+↓ to scroll)", n_rows - last_row),
            Style::default().fg(Color::DarkGray),
        )));
    }
    if first_row > 0 {
        lines.push(Line::from(Span::styled(
            format!("  … {} tracks above (Ctrl+↑ to scroll)", first_row),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let tool_label = app.arranger_state.tool.label();
    let tracks_title = format!(" TRACKS [{}/{}]  T=tool:[{}]  F=freeze  B=bounce ", last_row - first_row, n_rows, tool_label);
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

// ──────────────────────────────────────────────── Rational arrangement (Phase 4) ──

/// Beats per bar used to map the rational timeline onto the bar grid. Matches the
/// `migrate_legacy_arrangement` convention (4/4); per-pattern meters are honored
/// inside clips, not at the arrangement-bar level yet.
const ARR_BEATS_PER_BAR: f64 = 4.0;

/// Amplitude levels for the audio-clip waveform preview.
const WAVE_BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The chars rendered inside a clip after its leading kind glyph: an audio
/// waveform, a MIDI/pattern note-density preview, or the bare name.
fn clip_body(
    clip: &seqterm_core::ArrangementClip,
    w: usize,
    app: &App,
    proj: &seqterm_core::Project,
) -> Vec<char> {
    use seqterm_core::ClipKind;
    if w == 0 {
        return Vec::new();
    }
    match &clip.kind {
        ClipKind::Audio { path, .. } => {
            if let Some(peaks) = app.waveform_cache.get(path) {
                if !peaks.is_empty() {
                    return (0..w)
                        .map(|i| {
                            let pi = (i * peaks.len() / w).min(peaks.len() - 1);
                            let lvl = (peaks[pi].clamp(0.0, 1.0) * 8.0).round() as usize;
                            WAVE_BLOCKS[lvl.min(8)]
                        })
                        .collect();
                }
            }
            // Not cached yet → show the name while the background scan runs.
            name_body(&clip.name, w)
        }
        ClipKind::Pattern { pattern_key } => density_body(&clip.name, Some(pattern_key.as_str()), proj, w),
        ClipKind::Midi { pattern_key } => density_body(&clip.name, pattern_key.as_deref(), proj, w),
    }
}

/// Clip name left-aligned and padded to `w` chars.
fn name_body(name: &str, w: usize) -> Vec<char> {
    let mut out: Vec<char> = name.chars().take(w).collect();
    out.resize(w, ' ');
    out
}

/// Clip name followed by a note-density preview: every event in the referenced
/// pattern marks the column its (looped) start falls in (`•` = note onset).
fn density_body(name: &str, key: Option<&str>, proj: &seqterm_core::Project, w: usize) -> Vec<char> {
    let mut out = name_body(name, w);
    let Some(pat) = key.and_then(|k| proj.patterns.get(k)) else {
        return out;
    };
    let region_start = (name.chars().count() + 1).min(w);
    let region_w = w.saturating_sub(region_start);
    if region_w == 0 {
        return out;
    }
    let len = pat.length_beats().to_f64();
    if len <= 0.0 {
        return out;
    }
    for ev in pat.to_events() {
        let f = ev.start.to_f64().rem_euclid(len) / len;
        let col = region_start + ((f * region_w as f64) as usize).min(region_w - 1);
        if col < w {
            out[col] = '•';
        }
    }
    out
}

/// Overview minimap coverage (Phase 5, Fase 10): the whole arrangement compressed
/// into `width` columns, each holding the count of clips overlapping that column's
/// beat slice. Independent of timeline zoom/scroll — it always spans `[0, total)`.
fn overview_coverage(arr: &seqterm_core::Arrangement, total: f64, width: usize) -> Vec<u8> {
    let mut cells = vec![0u8; width];
    if width == 0 || total <= 0.0 {
        return cells;
    }
    let per_col = total / width as f64;
    for t in &arr.tracks {
        for l in &t.lanes {
            for c in &l.clips {
                let c0 = (c.start.to_f64() / per_col).floor().max(0.0) as usize;
                let c1 = ((c.end().to_f64() / per_col).ceil() as usize).min(width);
                for cell in cells.iter_mut().take(c1).skip(c0) {
                    *cell = cell.saturating_add(1);
                }
            }
        }
    }
    cells
}

/// Render the rational `Arrangement` (Phase 4) in the track-lanes panel. Each
/// track is one row; clips are colored bars positioned by their exact rational
/// `[start, end)` mapped onto the bar grid. The cursor clip is highlighted.
fn draw_arrangement_timeline(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let arr = &proj.arrangement;
    let focused = app.arranger_state.section == 0;
    let sel_track = app.arranger_state.selected_track;
    let cursor_clip = app.arranger_state.arr_cursor_clip;

    let offset = app.arranger_state.bar_offset;
    let bw = app.arranger_state.bar_width.max(2) as f64;
    let name_w: usize = 18;
    let avail = (area.width as usize).saturating_sub(name_w);
    // Beats covered by the visible width, and the leftmost visible beat.
    let start_beat = offset as f64 * ARR_BEATS_PER_BAR;
    let beats_per_col = ARR_BEATS_PER_BAR / bw;

    // beat → column (0-based within the lane area); clamps to the visible range.
    let beat_to_col = |beat: f64| -> isize { ((beat - start_beat) / beats_per_col).round() as isize };

    let mut lines: Vec<Line> = Vec::new();

    // Header.
    lines.push(Line::from(vec![
        Span::styled(
            format!("{:<width$}", " ARRANGEMENT", width = name_w),
            Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "rational timeline — g: back to matrix",
            Style::default().fg(BORDER),
        ),
    ]));

    if arr.tracks.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("{:<width$}(no arrangement tracks — legacy projects migrate on load)", "", width = name_w),
            Style::default().fg(Color::DarkGray),
        )));
    }

    // ── Marker ruler (Phase 5, Fase 8): `▼name` at each marker column. ──
    if !arr.markers.is_empty() {
        let mut mcells: Vec<(char, Style)> = vec![(' ', Style::default().bg(PANEL)); avail];
        let mstyle = Style::default().fg(Color::Rgb(230, 180, 80)).bg(PANEL);
        for m in &arr.markers {
            let c0 = beat_to_col(m.beat.to_f64());
            if c0 < 0 || c0 >= avail as isize {
                continue;
            }
            let label: String = std::iter::once('▼').chain(m.name.chars()).collect();
            for (k, ch) in label.chars().enumerate() {
                let idx = c0 as usize + k;
                if idx < avail {
                    mcells[idx] = (ch, mstyle);
                }
            }
        }
        let mut spans = vec![Span::styled(
            format!("{:<width$}", " MARKERS", width = name_w),
            Style::default().fg(Color::Rgb(230, 180, 80)),
        )];
        let mut run = String::new();
        let mut run_style = mcells.first().map(|c| c.1).unwrap_or_default();
        for (ch, st) in &mcells {
            if *st != run_style && !run.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run), run_style));
            }
            run_style = *st;
            run.push(*ch);
        }
        if !run.is_empty() {
            spans.push(Span::styled(run, run_style));
        }
        lines.push(Line::from(spans));
    }

    // ── Region band (Phase 5, Fase 8): `[name…]` bars; cycle span reversed. ──
    if !arr.regions.is_empty() || arr.cycle.is_some() {
        let mut rcells: Vec<(char, Style)> = vec![(' ', Style::default().bg(PANEL)); avail];
        for (ri, region) in arr.regions.iter().enumerate() {
            let c0 = beat_to_col(region.start.to_f64()).max(0);
            let c1 = beat_to_col(region.end.to_f64()).min(avail as isize);
            if c1 <= c0 {
                continue;
            }
            let base = TRACK_PALETTE[(region.color as usize).wrapping_add(ri) % TRACK_PALETTE.len()];
            let style = Style::default().fg(Color::Black).bg(base);
            let span_w = (c1 - c0) as usize;
            // Bracketed label clipped to the region width: "[name…]".
            let inner: String = region.name.chars().take(span_w.saturating_sub(2)).collect();
            let label: String = format!("[{inner}]");
            for col in c0..c1 {
                let li = (col - c0) as usize;
                let ch = label.chars().nth(li).unwrap_or('─');
                rcells[col as usize] = (ch, style);
            }
        }
        // Overlay the cycle span with a reversed loop style on top of any region.
        if let Some((cs, ce)) = arr.cycle {
            let c0 = beat_to_col(cs.to_f64()).max(0);
            let c1 = beat_to_col(ce.to_f64()).min(avail as isize);
            let cyc = Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(90, 200, 220))
                .add_modifier(Modifier::BOLD);
            for col in c0..c1 {
                let li = (col - c0) as usize;
                let ch = if li == 0 { '↺' } else { rcells[col as usize].0 };
                rcells[col as usize] = (ch, cyc);
            }
        }
        let mut spans = vec![Span::styled(
            format!("{:<width$}", " REGIONS", width = name_w),
            Style::default().fg(Color::Rgb(160, 200, 130)),
        )];
        let mut run = String::new();
        let mut run_style = rcells.first().map(|c| c.1).unwrap_or_default();
        for (ch, st) in &rcells {
            if *st != run_style && !run.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run), run_style));
            }
            run_style = *st;
            run.push(*ch);
        }
        if !run.is_empty() {
            spans.push(Span::styled(run, run_style));
        }
        lines.push(Line::from(spans));
    }

    // ── Section band (Phase 5, Fase 10): `◖name──◗` blocks across their span. ──
    if !arr.sections.is_empty() {
        let mut scells: Vec<(char, Style)> = vec![(' ', Style::default().bg(PANEL)); avail];
        for (si, section) in arr.sections.iter().enumerate() {
            let c0 = beat_to_col(section.start.to_f64()).max(0);
            let c1 = beat_to_col(section.end.to_f64()).min(avail as isize);
            if c1 <= c0 {
                continue;
            }
            let base = TRACK_PALETTE[(section.color as usize).wrapping_add(si) % TRACK_PALETTE.len()];
            let style = Style::default().fg(Color::White).bg(base).add_modifier(Modifier::BOLD);
            let span_w = (c1 - c0) as usize;
            let inner: String = section.name.chars().take(span_w.saturating_sub(2)).collect();
            let label: String = format!("◖{inner}◗");
            for col in c0..c1 {
                let li = (col - c0) as usize;
                let ch = label.chars().nth(li).unwrap_or('━');
                scells[col as usize] = (ch, style);
            }
        }
        let mut spans = vec![Span::styled(
            format!("{:<width$}", " SECTIONS", width = name_w),
            Style::default().fg(Color::Rgb(200, 160, 220)),
        )];
        let mut run = String::new();
        let mut run_style = scells.first().map(|c| c.1).unwrap_or_default();
        for (ch, st) in &scells {
            if *st != run_style && !run.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run), run_style));
            }
            run_style = *st;
            run.push(*ch);
        }
        if !run.is_empty() {
            spans.push(Span::styled(run, run_style));
        }
        lines.push(Line::from(spans));
    }

    // Beat-cursor (insertion point) column, for the playhead-style marker.
    let cursor_beat = app.arranger_state.arr_cursor_beat;
    let cursor_col = beat_to_col(cursor_beat.to_f64());

    let max_rows = (area.height as usize).saturating_sub(1).max(1);
    for (ti, track) in arr.tracks.iter().enumerate().take(max_rows) {
        let is_sel = focused && ti == sel_track;
        let name_bg = if is_sel { Color::Rgb(30, 30, 50) } else { PANEL };

        // ── Track inspector cell (mixer-free): badge + arm/solo/mute/monitor + name. ──
        // Unrouted tracks (no instrument) are dimmed — they are silent on playback.
        let name_fg = if is_sel {
            Color::Yellow
        } else if track.mute || track.source_row.is_none() {
            Color::DarkGray
        } else {
            Color::White
        };
        // The instrument route shows in place of the kind badge: "→A" / "-- ".
        let badge = match &track.source_row {
            Some(r) => format!("→{:<2}", r),
            None => "-- ".to_string(),
        };
        let badge = badge.as_str();
        let name: String = track.name.chars().take(6).collect();
        let flag = |on: bool, ch: char, color: Color| {
            if on {
                Span::styled(ch.to_string(), Style::default().fg(color).bg(name_bg).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("·".to_string(), Style::default().fg(BORDER).bg(name_bg))
            }
        };
        let mut spans = vec![
            Span::styled(
                format!(" {}{:<4} ", if is_sel { "▶" } else { " " }, badge),
                Style::default().fg(name_fg).bg(name_bg),
            ),
            flag(track.arm, 'A', Color::Rgb(220, 70, 70)),
            flag(track.solo, 'S', Color::Rgb(220, 200, 60)),
            flag(track.mute, 'M', Color::Rgb(150, 150, 150)),
            flag(track.monitor, 'I', Color::Rgb(80, 180, 220)),
            Span::styled(
                format!(" {:<6}", name),
                Style::default().fg(name_fg).bg(name_bg),
            ),
        ];

        // ── Lane cell: paint clips into a per-column buffer. ──
        let mut cells: Vec<(char, Style)> = vec![(' ', Style::default().bg(PANEL)); avail];
        for lane in &track.lanes {
            for clip in &lane.clips {
                let c0 = beat_to_col(clip.start.to_f64());
                let c1 = beat_to_col(clip.end().to_f64()).max(c0 + 1);
                let is_cursor = Some(clip.id) == cursor_clip;
                let is_selected = app.arr_selection.contains(&clip.id);
                let clip_color = TRACK_PALETTE[clip.color as usize % TRACK_PALETTE.len()];
                let base = if clip.muted { Color::Rgb(50, 50, 50) } else { clip_color };
                let style = if is_cursor {
                    Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else if is_selected {
                    // Multi-selected: bright magenta bg so it reads as part of the set.
                    Style::default().fg(Color::Black).bg(Color::Rgb(200, 120, 220)).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White).bg(base)
                };
                // Distinct leading glyph by clip kind.
                let kind_glyph = match clip.kind {
                    seqterm_core::ClipKind::Pattern { .. } => '▏',
                    seqterm_core::ClipKind::Audio { .. } => '≈',
                    seqterm_core::ClipKind::Midi { .. } => '♪',
                };
                // Clip body (the chars after the leading glyph): an audio waveform,
                // a MIDI/pattern note-density preview, or the bare name.
                let width = (c1 - c0).max(1) as usize;
                let body_w = width.saturating_sub(1);
                let body = clip_body(clip, body_w, app, &proj);
                for col in c0.max(0)..c1.min(avail as isize) {
                    let idx = col as usize;
                    let li = (col - c0) as usize;
                    let ch = if li == 0 {
                        kind_glyph
                    } else {
                        body.get(li - 1).copied().unwrap_or(' ')
                    };
                    cells[idx] = (ch, style);
                }
            }
        }
        // Overlay the beat-cursor marker (skips clip cells, which already read as
        // the selection when the cursor sits on a clip).
        if (0..avail as isize).contains(&cursor_col) {
            let idx = cursor_col as usize;
            if cells[idx].0 == ' ' {
                cells[idx] = ('╎', Style::default().fg(Color::Rgb(90, 200, 220)).bg(PANEL));
            }
        }
        // Coalesce adjacent same-style cells into spans.
        let mut run = String::new();
        let mut run_style = cells.first().map(|c| c.1).unwrap_or_default();
        for (ch, st) in &cells {
            if *st != run_style && !run.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run), run_style));
            }
            run_style = *st;
            run.push(*ch);
        }
        if !run.is_empty() {
            spans.push(Span::styled(run, run_style));
        }
        lines.push(Line::from(spans));

        // ── Automation sub-lane (Milestone F): an 8-level breakpoint curve under
        // the focused track when edit mode is on. ──
        if is_sel && app.arranger_state.arr_auto_edit {
            let dest = app.arranger_state.arr_auto_dest.as_str();
            let lane = track.automation_lane(dest);
            let mut auto_spans = vec![Span::styled(
                format!("  ⌁ {:<width$}", dest, width = name_w.saturating_sub(4)),
                Style::default().fg(Color::Rgb(120, 200, 160)).bg(name_bg),
            )];
            let mut acells: Vec<(char, Style)> = vec![(' ', Style::default().bg(PANEL)); avail];
            if let Some(lane) = lane {
                for (col, acell) in acells.iter_mut().enumerate() {
                    let beat = start_beat + col as f64 * beats_per_col;
                    if let Some(v) = lane.value_at(beat) {
                        let lvl = (v.clamp(0.0, 1.0) * 8.0).round() as usize;
                        *acell = (
                            WAVE_BLOCKS[lvl.min(8)],
                            Style::default().fg(Color::Rgb(120, 200, 160)).bg(PANEL),
                        );
                    }
                }
                // Emphasise breakpoint columns.
                for p in &lane.points {
                    let pc = beat_to_col(p.beat);
                    if (0..avail as isize).contains(&pc) {
                        let lvl = (p.value.clamp(0.0, 1.0) * 8.0).round() as usize;
                        acells[pc as usize] = (
                            WAVE_BLOCKS[lvl.min(8)],
                            Style::default().fg(Color::Black).bg(Color::Rgb(120, 200, 160)),
                        );
                    }
                }
            }
            // Value-cursor marker at the beat cursor.
            if (0..avail as isize).contains(&cursor_col) {
                let vlvl = (app.arranger_state.arr_auto_value.clamp(0.0, 1.0) * 8.0).round() as usize;
                acells[cursor_col as usize] = (
                    WAVE_BLOCKS[vlvl.min(8)],
                    Style::default().fg(Color::Rgb(90, 200, 220)).bg(Color::Rgb(40, 40, 60)),
                );
            }
            let mut run = String::new();
            let mut run_style = acells.first().map(|c| c.1).unwrap_or_default();
            for (ch, st) in &acells {
                if *st != run_style && !run.is_empty() {
                    auto_spans.push(Span::styled(std::mem::take(&mut run), run_style));
                }
                run_style = *st;
                run.push(*ch);
            }
            if !run.is_empty() {
                auto_spans.push(Span::styled(run, run_style));
            }
            lines.push(Line::from(auto_spans));
        }
    }

    // ── Overview minimap (Phase 5, Fase 10): the whole arrangement compressed to
    // one strip, independent of zoom/scroll; section tints, marker ticks, a window
    // bracket for the visible range, and the cursor position. ──
    let total = arr.length_beats().to_f64().max(1.0);
    if !arr.tracks.is_empty() {
        let cov = overview_coverage(arr, total, avail);
        let to_mini = |beat: f64| -> isize { ((beat / total) * avail as f64) as isize };
        let mut mcells: Vec<(char, Style)> = Vec::with_capacity(avail);
        for &count in &cov {
            let ch = match count {
                0 => '·',
                1 => '▃',
                2 => '▆',
                _ => '█',
            };
            let fg = if count == 0 { Color::Rgb(60, 60, 70) } else { Color::Rgb(120, 170, 210) };
            mcells.push((ch, Style::default().fg(fg).bg(PANEL)));
        }
        // Section tints (background under the strip).
        for (si, section) in arr.sections.iter().enumerate() {
            let c0 = to_mini(section.start.to_f64()).max(0);
            let c1 = to_mini(section.end.to_f64()).min(avail as isize);
            let bg = TRACK_PALETTE[si % TRACK_PALETTE.len()];
            for col in c0..c1 {
                let cell = &mut mcells[col as usize];
                cell.1 = cell.1.bg(bg).fg(Color::Black);
            }
        }
        // Visible-window bracket (where the zoomed timeline currently looks).
        let win0 = to_mini(start_beat).clamp(0, avail as isize - 1);
        let win1 = to_mini(start_beat + avail as f64 * beats_per_col).clamp(0, avail as isize - 1);
        for &(wc, glyph) in &[(win0, '▕'), (win1, '▏')] {
            mcells[wc as usize] = (glyph, Style::default().fg(Color::Gray).bg(PANEL));
        }
        // Marker ticks.
        for m in &arr.markers {
            let c = to_mini(m.beat.to_f64());
            if (0..avail as isize).contains(&c) {
                mcells[c as usize].0 = '│';
                mcells[c as usize].1 = mcells[c as usize].1.fg(Color::Rgb(230, 180, 80));
            }
        }
        // Cursor position (on top of everything).
        let cur = to_mini(cursor_beat.to_f64());
        if (0..avail as isize).contains(&cur) {
            mcells[cur as usize] = (
                '▮',
                Style::default().fg(Color::Rgb(90, 200, 220)).bg(PANEL).add_modifier(Modifier::BOLD),
            );
        }
        let mut spans = vec![Span::styled(
            format!("{:<width$}", " OVERVIEW", width = name_w),
            Style::default().fg(Color::Rgb(120, 170, 210)),
        )];
        let mut run = String::new();
        let mut run_style = mcells.first().map(|c| c.1).unwrap_or_default();
        for (ch, st) in &mcells {
            if *st != run_style && !run.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run), run_style));
            }
            run_style = *st;
            run.push(*ch);
        }
        if !run.is_empty() {
            spans.push(Span::styled(run, run_style));
        }
        // Record the minimap's screen rect (lane portion) for click-to-navigate.
        // Block has Borders::TOP only → content begins at area.y + 1.
        let row_y = area.y + 1 + lines.len() as u16;
        app.arr_overview_rect.set(Rect {
            x: area.x + name_w as u16,
            y: row_y,
            width: avail as u16,
            height: 1,
        });
        lines.push(Line::from(spans));
    }

    let play = if app.arranger_state.arr_playback { "▶PLAY" } else { "■off" };
    let title = match cursor_clip.and_then(|id| arr.clip(id)) {
        Some(c) => format!(
            " SONG · TIMELINE [{}] │ clip '{}' {} start {} len {}",
            play,
            c.name,
            c.kind.label(),
            c.start.to_f64(),
            c.length.to_f64(),
        ),
        None => format!(" SONG · TIMELINE [{}] │ (no clip selected)", play),
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .title(title)
        .border_style(Style::default().fg(if focused { ACCENT } else { BORDER }))
        .style(Style::default().bg(PANEL));
    f.render_widget(Paragraph::new(lines).block(block), area);
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

#[cfg(test)]
mod tests {
    use super::{density_body, name_body, overview_coverage};

    #[test]
    fn overview_coverage_counts_overlaps() {
        use seqterm_core::{Arrangement, ArrangementClip, ArrangementTrack, ClipKind, RationalTime, TrackKind};
        let r = |n: i64| RationalTime::whole(n);
        let mut arr = Arrangement::default();
        let mut t = ArrangementTrack::new("T", TrackKind::Midi);
        // A clip covering [0,4) and another [2,8) → overlap on [2,4).
        t.primary_lane_mut().clips.push(ArrangementClip::new(
            0, "a", ClipKind::Pattern { pattern_key: "P".into() }, r(0), r(4)));
        t.primary_lane_mut().clips.push(ArrangementClip::new(
            1, "b", ClipKind::Pattern { pattern_key: "P".into() }, r(2), r(6)));
        arr.tracks.push(t);

        // Total 8 beats over 8 columns ⇒ 1 beat/col.
        let cov = overview_coverage(&arr, 8.0, 8);
        assert_eq!(cov.len(), 8);
        assert_eq!(cov[0], 1, "only clip a at beat 0");
        assert_eq!(cov[3], 2, "both clips overlap at beat 3");
        assert_eq!(cov[6], 1, "only clip b at beat 6");
        // Degenerate inputs are safe.
        assert!(overview_coverage(&arr, 0.0, 8).iter().all(|&c| c == 0));
        assert!(overview_coverage(&arr, 8.0, 0).is_empty());
    }

    #[test]
    fn name_body_truncates_and_pads() {
        assert_eq!(name_body("hello", 3), vec!['h', 'e', 'l']);
        assert_eq!(name_body("hi", 5), vec!['h', 'i', ' ', ' ', ' ']);
        assert!(name_body("x", 0).is_empty());
    }

    #[test]
    fn density_body_marks_note_onsets() {
        use seqterm_core::{Note, Pattern, Project};
        let mut proj = Project::blank("t");
        let mut pat = Pattern::new("P", 4);
        pat.set_step(0, Note::from_midi(60, 100).unwrap());
        proj.patterns.insert("P".into(), pat);

        // Name "AB" (2 chars) + 1 gap → density region starts at col 3.
        let body = density_body("AB", Some("P"), &proj, 12);
        assert_eq!(&body[0..2], &['A', 'B']);
        // The single onset at beat 0 maps to the first density column.
        assert_eq!(body[3], '•', "onset marked at region start; got {body:?}");
        // Unknown pattern → just the padded name, no markers.
        let plain = density_body("AB", Some("missing"), &proj, 8);
        assert!(!plain.contains(&'•'));
    }
}
