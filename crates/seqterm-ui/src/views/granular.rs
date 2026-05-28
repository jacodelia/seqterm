//! Granular engine view — waveform display, param editor, live grain meter.
//!
//! Layout:
//!   ┌─ waveform ──────────────────────────────────┐
//!   │ ░░▒▓█▓▒░░ position marker, spray range       │
//!   ├─ PARAMS ────────────────┬─ ZONE ─────────────┤
//!   │ size_ms   80ms   ─ ─ ─  │ pos    0.00   ─ ─  │
//!   │ density   10     ─ ─ ─  │ range  1.00   ─ ─  │
//!   │ ...                     │ ...                 │
//!   └─────────────────────────┴─────────────────────┘
//!   hint line

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use seqterm_core::{GrainDirection, GrainEnvelope, GrainParams, ScanMode};

use crate::app::App;

const BG:     Color = Color::Rgb(13, 17, 23);
const PANEL:  Color = Color::Rgb(18, 24, 32);
const BORDER: Color = Color::Rgb(48, 54, 61);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const OK:     Color = Color::Rgb(56, 200, 100);
const WARM:   Color = Color::Rgb(240, 136, 62);
const DIM:    Color = Color::Rgb(60, 70, 85);

pub fn draw_granular(f: &mut Frame, app: &App, area: Rect) {
    let state = &app.granular_state;

    // Vertical split: waveform (top) + editor (bottom) + hint (1 line).
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),  // waveform strip
            Constraint::Min(10),    // param editor
            Constraint::Length(1),  // hint
        ])
        .split(area);

    draw_waveform(f, app, vchunks[0]);
    draw_param_editor(f, app, vchunks[1]);

    // Hint line.
    let hint = if state.pad.is_some() {
        "  ↑↓=param  ←→=adjust  g=back  f=freeze  F=unfreeze"
    } else {
        "  No pad selected — open granular from Sampler view (g key on a pad)"
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(BORDER))),
        vchunks[2],
    );
}

fn draw_waveform(f: &mut Frame, app: &App, area: Rect) {
    let state = &app.granular_state;

    let block = Block::default()
        .title(" WAVEFORM ")
        .title_style(Style::default().fg(WARM).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 { return; }

    let w = inner.width as usize;
    let h = inner.height as usize;
    let pos = state.zone.position;
    let range = state.zone.range.clamp(0.0, 1.0);
    let spray = state.params.spray;

    // Look up waveform from cache if a pad is loaded.
    let waveform: Option<Vec<f32>> = state.pad.and_then(|(bank, pad)| {
        let proj = app.project.lock();
        let path = proj.sampler.banks.get(bank)
            .and_then(|b| b.slots[pad].as_ref())
            .map(|s| s.path.clone())?;
        drop(proj);
        app.waveform_cache.get(&path).cloned()
    });

    let mut lines: Vec<Line> = Vec::with_capacity(h);

    for row in 0..h {
        let mut chars: Vec<char> = vec![' '; w];

        // Waveform bars.
        if let Some(ref peaks) = waveform {
            let n = peaks.len();
            for col in 0..w {
                let frac = col as f32 / w as f32;
                let peak_idx = (frac * n as f32) as usize;
                let amp = peaks.get(peak_idx).copied().unwrap_or(0.0);
                let bar_h = (amp * h as f32) as usize;
                // Row 0 = top; bar grows from middle for stereo.
                let mid = h / 2;
                let half_bar = bar_h / 2;
                if row >= mid.saturating_sub(half_bar) && row <= mid + half_bar {
                    let density = (amp * 4.0) as usize;
                    chars[col] = match density {
                        0 => '░',
                        1 => '▒',
                        2 => '▓',
                        _ => '█',
                    };
                }
            }
        } else {
            // No waveform — draw empty placeholder in top row.
            if row == 0 {
                let msg = " no source ";
                let start = w.saturating_sub(msg.len()) / 2;
                for (i, c) in msg.chars().enumerate() {
                    if start + i < w { chars[start + i] = c; }
                }
            }
        }

        // Draw zone range overlay on the middle row.
        if row == h / 2 {
            let zone_l = ((pos - range / 2.0).max(0.0) * w as f32) as usize;
            let zone_r = ((pos + range / 2.0).min(1.0) * w as f32) as usize;
            for col in zone_l..zone_r.min(w) {
                if chars[col] == ' ' { chars[col] = '·'; }
            }

            // Spray range.
            let spray_half = (spray * range * w as f32 / 2.0) as usize;
            let pos_x = (pos * w as f32) as usize;
            for col in pos_x.saturating_sub(spray_half)..(pos_x + spray_half).min(w) {
                if chars[col] == '·' { chars[col] = '~'; }
            }

            // Position marker.
            if pos_x < w { chars[pos_x] = '▼'; }
        }

        let s: String = chars.into_iter().collect();
        let style = if row == h / 2 {
            Style::default().fg(ACCENT).bg(PANEL)
        } else {
            Style::default().fg(DIM).bg(PANEL)
        };
        lines.push(Line::from(Span::styled(s, style)));
    }

    f.render_widget(Paragraph::new(lines).style(Style::default().bg(PANEL)), inner);

    // Frozen indicator overlay.
    if state.zone.frozen {
        let msg = " ❄ FROZEN ";
        let x = inner.x + (inner.width.saturating_sub(msg.len() as u16)) / 2;
        let y = inner.y;
        let r = Rect { x, y, width: msg.len() as u16, height: 1 };
        f.render_widget(
            Paragraph::new(Span::styled(msg, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                .style(Style::default().bg(Color::Rgb(20, 50, 70))),
            r,
        );
    }
}

fn draw_param_editor(f: &mut Frame, app: &App, area: Rect) {
    let state = &app.granular_state;
    let cursor = state.cursor;

    // Horizontal split: grain params (left 60%) | zone params (right 40%).
    let hchunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(3, 5), Constraint::Ratio(2, 5)])
        .split(area);

    // ── Grain params ─────────────────────────────────────────────────────────
    let gblock = Block::default()
        .title(" GRAIN PARAMS ")
        .title_style(Style::default().fg(WARM).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if state.pad.is_some() { BORDER } else { DIM }))
        .style(Style::default().bg(BG));
    let ginner = gblock.inner(hchunks[0]);
    f.render_widget(gblock, hchunks[0]);

    let p = &state.params;
    let grain_rows: Vec<(&str, String)> = vec![
        ("size_ms",  format!("{:.0} ms", p.size_ms)),
        ("density",  format!("{:.1} /s", p.density)),
        ("spray",    format!("{:.3}", p.spray)),
        ("overlap",  format!("{:.2}", p.overlap)),
        ("pitch",    format!("{:+.1} st", p.pitch_st)),
        ("direction",format!("{}", dir_label(p.direction))),
        ("pan",      format!("{:+.2}", p.pan)),
        ("gain",     format!("{:.2}", p.gain)),
        ("jitter",   format!("{:.3}", p.jitter)),
        ("spread",   format!("{:.2}", p.stereo_spread)),
        ("envelope", format!("{}", p.envelope.label())),
        ("voices",   format!("{}", p.max_voices)),
    ];

    let glines: Vec<Line> = grain_rows.iter().enumerate().map(|(i, (lbl, val))| {
        let is_sel = cursor == i && state.pad.is_some();
        let (bg, fg_lbl, fg_val) = if is_sel {
            (ACCENT, Color::Black, Color::Black)
        } else {
            (BG, Color::Rgb(140, 160, 200), Color::White)
        };
        let bar = param_bar(i, p, ginner.width as usize);
        Line::from(vec![
            Span::styled(format!(" {:<10}", lbl), Style::default().fg(fg_lbl).bg(bg)),
            Span::styled(format!("{:<10}", val), Style::default().fg(fg_val).bg(bg).add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() })),
            Span::styled(bar, Style::default().fg(if is_sel { Color::White } else { DIM }).bg(bg)),
        ])
    }).collect();

    f.render_widget(Paragraph::new(glines).style(Style::default().bg(BG)), ginner);

    // ── Zone params ───────────────────────────────────────────────────────────
    let zblock = Block::default()
        .title(" ZONE ")
        .title_style(Style::default().fg(OK).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if state.pad.is_some() { BORDER } else { DIM }))
        .style(Style::default().bg(BG));
    let zinner = zblock.inner(hchunks[1]);
    f.render_widget(zblock, hchunks[1]);

    let z = &state.zone;
    let zone_rows: Vec<(&str, String)> = vec![
        ("position",   format!("{:.3}", z.position)),
        ("range",      format!("{:.3}", z.range)),
        ("scan_speed", format!("{:.2}", z.scan_speed)),
        ("scan_mode",  format!("{}", scan_mode_label(z.scan_mode))),
        ("frozen",     format!("{}", if z.frozen { "YES" } else { "no" })),
    ];

    let zlines: Vec<Line> = zone_rows.iter().enumerate().map(|(i, (lbl, val))| {
        let zone_cursor = 12 + i;
        let is_sel = cursor == zone_cursor && state.pad.is_some();
        let (bg, fg_lbl, fg_val) = if is_sel {
            (ACCENT, Color::Black, Color::Black)
        } else {
            (BG, Color::Rgb(140, 200, 140), Color::White)
        };
        Line::from(vec![
            Span::styled(format!(" {:<12}", lbl), Style::default().fg(fg_lbl).bg(bg)),
            Span::styled(format!("{:<10}", val), Style::default().fg(fg_val).bg(bg).add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() })),
        ])
    }).collect();

    f.render_widget(Paragraph::new(zlines).style(Style::default().bg(BG)), zinner);
}

fn dir_label(d: GrainDirection) -> &'static str {
    match d {
        GrainDirection::Forward  => "Forward",
        GrainDirection::Backward => "Backward",
        GrainDirection::Random   => "Random",
    }
}

fn scan_mode_label(m: ScanMode) -> &'static str {
    match m {
        ScanMode::Linear     => "Linear",
        ScanMode::RandomWalk => "RandomWalk",
        ScanMode::Freeze     => "Freeze",
    }
}

/// Build a small ASCII bar for normalised param values.
fn param_bar(param_idx: usize, p: &GrainParams, area_w: usize) -> String {
    let bar_w = area_w.saturating_sub(22).max(4).min(12);
    let frac: f32 = match param_idx {
        0  => (p.size_ms - 1.0) / 499.0,
        1  => (p.density - 1.0) / 199.0,
        2  => p.spray,
        3  => p.overlap,
        4  => (p.pitch_st + 24.0) / 48.0,
        5  => match p.direction { GrainDirection::Forward => 0.0, GrainDirection::Backward => 0.5, GrainDirection::Random => 1.0 },
        6  => (p.pan + 1.0) / 2.0,
        7  => p.gain / 2.0,
        8  => p.jitter,
        9  => p.stereo_spread,
        10 => match p.envelope { GrainEnvelope::Hann => 0.0, GrainEnvelope::Gaussian => 0.33, GrainEnvelope::Triangle => 0.67, GrainEnvelope::Exponential => 1.0 },
        11 => (p.max_voices as f32 - 1.0) / 31.0,
        _  => 0.0,
    };
    let filled = (frac.clamp(0.0, 1.0) * bar_w as f32) as usize;
    let mut s = String::new();
    s.push('[');
    for i in 0..bar_w {
        s.push(if i < filled { '█' } else { '─' });
    }
    s.push(']');
    s
}
