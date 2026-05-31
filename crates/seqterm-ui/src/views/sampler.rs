//! SP-404-style 4×4 pad grid view.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;

const PAD_COLS: usize = 4;
const PAD_ROWS: usize = 4;

const BG:      Color = Color::Rgb(13, 17, 23);
const ACCENT:  Color = Color::Rgb(99, 179, 237);
const DIM:     Color = Color::Rgb(55, 65, 81);
const LOADED:  Color = Color::Rgb(72, 187, 120);
const CURSOR:  Color = Color::Rgb(246, 173, 85);
const LABEL:   Color = Color::Rgb(160, 174, 192);

pub fn draw_sampler(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let active_bank = proj.sampler.active_bank;
    let bank_count  = proj.sampler.banks.len();

    // ── Layout: bank tabs on top, pad grid below ──────────────────────────────
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let tab_area  = chunks[0];
    let grid_area = chunks[1];

    // ── Bank tab bar ─────────────────────────────────────────────────────────
    let tab_spans: Vec<Span> = (0..bank_count)
        .flat_map(|i| {
            let label = format!(" {} ", (b'A' + i as u8) as char);
            let style = if i == active_bank {
                Style::default().fg(BG).bg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(LABEL).bg(DIM)
            };
            vec![Span::styled(label, style), Span::raw(" ")]
        })
        .collect();

    let tab_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(" SAMPLER ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(BG));

    let tab_inner = tab_block.inner(tab_area);
    f.render_widget(tab_block, tab_area);
    f.render_widget(
        Paragraph::new(Line::from(tab_spans)).style(Style::default().bg(BG)),
        tab_inner,
    );

    // ── 4×4 Pad grid ─────────────────────────────────────────────────────────
    let row_constraints: Vec<Constraint> = (0..PAD_ROWS)
        .map(|_| Constraint::Ratio(1, PAD_ROWS as u32))
        .collect();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(grid_area);

    let col_constraints: Vec<Constraint> = (0..PAD_COLS)
        .map(|_| Constraint::Ratio(1, PAD_COLS as u32))
        .collect();

    let bank = proj.sampler.banks.get(active_bank);
    let (cur_row, cur_col) = app.sampler_state.cursor;

    for row in 0..PAD_ROWS {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints.clone())
            .split(rows[row]);

        for col in 0..PAD_COLS {
            let pad_idx = row * PAD_COLS + col;
            let slot    = bank.and_then(|b| b.slots[pad_idx].as_ref());
            let key     = (active_bank, pad_idx);
            let loaded  = app.sampler_slots.contains_key(&key);
            let is_cur  = row == cur_row && col == cur_col;

            let border_style = if is_cur {
                Style::default().fg(CURSOR)
            } else if loaded {
                Style::default().fg(LOADED)
            } else if slot.is_some() {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(DIM)
            };

            let pad_label = format!("{}{}", (b'A' + active_bank as u8) as char, pad_idx + 1);

            let (name_line, info_line, waveform_line) = if let Some(s) = slot {
                let fname = s.path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?");
                let truncated = if fname.len() > 10 { &fname[..10] } else { fname };
                let mode = match s.trigger {
                    seqterm_core::TriggerMode::OneShot  => "1SH",
                    seqterm_core::TriggerMode::Loop     => "LP",
                    seqterm_core::TriggerMode::Gate     => "GT",
                    seqterm_core::TriggerMode::Retrigger => "RT",
                };
                let info = format!("{mode} {:.0}dB", 20.0 * s.gain.max(1e-6).log10());
                let wave = app.waveform_cache.get(&s.path)
                    .map(|peaks| mini_waveform(peaks, 12))
                    .unwrap_or_default();
                (truncated.to_string(), info, wave)
            } else {
                ("--".to_string(), String::new(), String::new())
            };

            let status_style = if loaded {
                Style::default().fg(LOADED)
            } else if slot.is_some() {
                Style::default().fg(LABEL)
            } else {
                Style::default().fg(DIM)
            };

            let wave_style = if is_cur {
                Style::default().fg(CURSOR)
            } else if loaded {
                Style::default().fg(LOADED)
            } else {
                Style::default().fg(ACCENT)
            };

            let mut text = vec![
                Line::from(Span::styled(
                    pad_label,
                    Style::default().fg(if is_cur { CURSOR } else { LABEL }).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(name_line, status_style)),
                Line::from(Span::styled(info_line, Style::default().fg(DIM))),
            ];
            if !waveform_line.is_empty() {
                text.push(Line::from(Span::styled(waveform_line, wave_style)));
            }

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .style(Style::default().bg(BG));

            let inner = block.inner(cols[col]);
            f.render_widget(block, cols[col]);
            f.render_widget(Paragraph::new(text).style(Style::default().bg(BG)), inner);
        }
    }

    drop(proj);

    // ── Help line at bottom ───────────────────────────────────────────────────
    // (rendered into the last pixel row of the last pad row — cheap overlay)
    let help_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };
    let help = Paragraph::new(Line::from(vec![
        Span::styled(" Space", Style::default().fg(CURSOR)),
        Span::styled("=play  ", Style::default().fg(LABEL)),
        Span::styled("s", Style::default().fg(CURSOR)),
        Span::styled("=stop  ", Style::default().fg(LABEL)),
        Span::styled("a", Style::default().fg(CURSOR)),
        Span::styled("=assign  ", Style::default().fg(LABEL)),
        Span::styled("d", Style::default().fg(CURSOR)),
        Span::styled("=clear  ", Style::default().fg(LABEL)),
        Span::styled("←→↑↓", Style::default().fg(CURSOR)),
        Span::styled("=nav  ", Style::default().fg(LABEL)),
        Span::styled("[/]", Style::default().fg(CURSOR)),
        Span::styled("=bank", Style::default().fg(LABEL)),
    ]))
    .style(Style::default().bg(BG));
    f.render_widget(help, help_area);
}

/// Build a `width`-char waveform preview from amplitude peaks using ▁▂▃▄▅▆▇█.
fn mini_waveform(peaks: &[f32], width: usize) -> String {
    const BLOCKS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let n = peaks.len();
    if n == 0 || width == 0 { return String::new(); }
    (0..width).map(|col| {
        let idx = (col * n / width).min(n - 1);
        let amp = peaks[idx].clamp(0.0, 1.0);
        BLOCKS[(amp * (BLOCKS.len() - 1) as f32).round() as usize]
    }).collect()
}
