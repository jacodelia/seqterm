use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::app::{App, SplashState};

// ─── Modal palette (matches modal_render.rs exactly) ─────────────────────────
const BACKDROP: Color = Color::Rgb(8,  10, 14);
const BG:       Color = Color::Rgb(22, 27, 34);
const BORDER:   Color = Color::Rgb(58, 64, 72);
const HEADER:   Color = Color::Rgb(240, 136, 62);
const ACCENT:   Color = Color::Rgb(31, 111, 235);
const OK:       Color = Color::Rgb(56, 200, 100);
const SHADOW:   Color = Color::Rgb(10, 12, 16);
const DIM:      Color = Color::Rgb(80, 90, 110);

// ─── Metallic gradient across logo columns ────────────────────────────────────
const GRADIENT: &[Color] = &[
    Color::Rgb(160, 160, 180),
    Color::Rgb(190, 190, 210),
    Color::Rgb(215, 210, 190),
    Color::Rgb(235, 225, 175),
    Color::Rgb(250, 240, 155),
    Color::Rgb(235, 225, 175),
    Color::Rgb(215, 210, 190),
    Color::Rgb(190, 190, 210),
    Color::Rgb(160, 160, 180),
    Color::Rgb(140, 140, 165),
    Color::Rgb(160, 160, 180),
    Color::Rgb(190, 190, 210),
];

const SPINNERS: &[char] = &['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

// SeqTerm logo — 6-row block font
const LOGO: &[&str] = &[
    "███████╗███████╗ ██████╗ ████████╗███████╗██████╗ ███╗   ███╗",
    "██╔════╝██╔════╝██╔═══██╗╚══██╔══╝██╔════╝██╔══██╗████╗ ████║",
    "███████╗█████╗  ██║   ██║   ██║   █████╗  ██████╔╝██╔████╔██║",
    "╚════██║██╔══╝  ██║▄▄ ██║   ██║   ██╔══╝  ██╔══██╗██║╚██╔╝██║",
    "███████║███████╗╚██████╔╝   ██║   ███████╗██║  ██║██║ ╚═╝ ██║",
    "╚══════╝╚══════╝ ╚══▀▀═╝    ╚═╝   ╚══════╝╚═╝  ╚═╝╚═╝     ╚═╝",
];

// ─── Public entry point ───────────────────────────────────────────────────────

pub fn draw_splash(f: &mut Frame, app: &mut App, area: Rect) {
    let tick = app.splash_state.tick;

    // 1. Full-screen opaque backdrop (same as modals).
    f.render_widget(Clear, area);
    f.render_widget(Block::default().style(Style::default().bg(BACKDROP)), area);

    // 2. Centered modal rect — wide enough for the logo (~68 chars + 4 border).
    let modal = centered_fixed(74, 24, area);

    // 3. Drop shadow (1 right, 1 down).
    draw_shadow(f, modal, area);

    // 4. Clear + draw the modal window.
    f.render_widget(Clear, modal);

    let border_col = if app.splash_state.ready { OK } else { BORDER };
    let block = Block::default()
        .title(" SeqTerm ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_col))
        .style(Style::default().bg(BG));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    // 5. Inner layout.
    draw_splash_content(f, &app.splash_state, inner, tick);
}

// ─── Inner content ────────────────────────────────────────────────────────────

fn draw_splash_content(f: &mut Frame, splash: &SplashState, area: Rect, tick: u64) {
    let logo_h = LOGO.len() as u16;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(logo_h), // logo
            Constraint::Length(1),      // subtitle
            Constraint::Length(1),      // divider
            Constraint::Length(1),      // spinner / ready label
            Constraint::Length(1),      // progress bar
            Constraint::Length(1),      // percent + stage
            Constraint::Length(1),      // spacing
            Constraint::Length(1),      // plugin stats
            Constraint::Length(1),      // current plugin
            Constraint::Min(0),         // flex
        ])
        .split(area);

    // Logo
    draw_logo(f, chunks[0], tick);

    // Subtitle
    f.render_widget(
        Paragraph::new("Terminal Digital Audio Workstation")
            .style(Style::default().fg(DIM).add_modifier(Modifier::ITALIC))
            .alignment(Alignment::Center),
        chunks[1],
    );

    // Divider
    let divider_w = area.width.saturating_sub(4) as usize;
    f.render_widget(
        Paragraph::new(format!(" {}", "─".repeat(divider_w)))
            .style(Style::default().fg(BORDER)),
        chunks[2],
    );

    if splash.ready {
        draw_ready(f, chunks[3], chunks[4], chunks[5], tick);
        return;
    }

    // Spinner + stage label
    let spinner = SPINNERS[tick as usize % SPINNERS.len()];
    let stage   = splash.current_stage_label();
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {spinner} "), Style::default().fg(ACCENT)),
            Span::styled(stage, Style::default().fg(Color::Rgb(180, 195, 215))),
        ]))
        .alignment(Alignment::Center),
        chunks[3],
    );

    // Progress bar
    let progress = splash.overall_progress().min(1.0);
    draw_progress_bar(f, chunks[4], progress, tick);

    // Percent + completed stages summary
    let pct = (progress * 100.0) as u32;
    let summary = completed_stages_summary(splash);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {pct:3}%  "), Style::default().fg(DIM)),
            Span::styled(summary, Style::default().fg(Color::Rgb(100, 115, 135))),
        ]))
        .alignment(Alignment::Center),
        chunks[5],
    );

    // Plugin scan details
    if splash.plugin_scan_started {
        let stats = format!(
            "Plugins: {}  VST3: {}  CLAP: {}",
            splash.plugins_found, splash.vst3_count, splash.clap_count,
        );
        f.render_widget(
            Paragraph::new(stats)
                .style(Style::default().fg(Color::Rgb(90, 105, 125)))
                .alignment(Alignment::Center),
            chunks[7],
        );
    }

    // Skip hint (loading): tell the user they don't have to wait.
    f.render_widget(
        Paragraph::new("press any key to skip")
            .style(Style::default().fg(Color::Rgb(70, 80, 100)).add_modifier(Modifier::ITALIC))
            .alignment(Alignment::Center),
        chunks[8],
    );
}

fn draw_ready(f: &mut Frame, label_area: Rect, bar_area: Rect, hint_area: Rect, tick: u64) {
    let pulse = if (tick / 5) % 2 == 0 {
        Color::Rgb(70, 210, 110)
    } else {
        Color::Rgb(40, 160, 75)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ✓ ", Style::default().fg(pulse).add_modifier(Modifier::BOLD)),
            Span::styled("SEQTERM READY", Style::default().fg(pulse).add_modifier(Modifier::BOLD)),
        ]))
        .alignment(Alignment::Center),
        label_area,
    );

    draw_progress_bar(f, bar_area, 1.0, tick);

    f.render_widget(
        Paragraph::new("Audio Ready  ·  MIDI Ready  ·  Plugins Loaded  ·  Press any key")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center),
        hint_area,
    );
}

// ─── Logo ─────────────────────────────────────────────────────────────────────

fn draw_logo(f: &mut Frame, area: Rect, tick: u64) {
    let pulse: u8 = if (tick / 8) % 2 == 0 { 15 } else { 0 };

    for (row, text) in LOGO.iter().enumerate() {
        let y = area.y + row as u16;
        if y >= area.y + area.height { break; }

        let chars: Vec<char> = text.chars().collect();
        let logo_w = chars.len() as u16;
        let x_off  = area.x + area.width.saturating_sub(logo_w) / 2;

        let spans: Vec<Span> = chars
            .iter()
            .enumerate()
            .map(|(col, &ch)| {
                let base  = GRADIENT[(col / 3 + (tick / 3) as usize) % GRADIENT.len()];
                let color = pulse_color(base, pulse);
                Span::styled(ch.to_string(), Style::default().fg(color).add_modifier(Modifier::BOLD))
            })
            .collect();

        f.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect { x: x_off, y, width: logo_w.min(area.width), height: 1 },
        );
    }
}

// ─── Progress bar ─────────────────────────────────────────────────────────────

fn draw_progress_bar(f: &mut Frame, area: Rect, progress: f32, tick: u64) {
    let bar_w   = area.width.saturating_sub(4) as usize;
    let filled  = (progress * bar_w as f32) as usize;
    let empty   = bar_w.saturating_sub(filled);
    let wave    = (tick / 2) as usize;

    let mut spans = vec![Span::styled(" [", Style::default().fg(BORDER))];
    for i in 0..filled {
        let color = GRADIENT[(i + wave) % GRADIENT.len()];
        spans.push(Span::styled("█", Style::default().fg(color)));
    }
    if filled < bar_w {
        spans.push(Span::styled("▌", Style::default().fg(BORDER)));
        for _ in 0..empty.saturating_sub(1) {
            spans.push(Span::styled("░", Style::default().fg(Color::Rgb(35, 42, 55))));
        }
    }
    spans.push(Span::styled("]", Style::default().fg(BORDER)));

    f.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn completed_stages_summary(splash: &SplashState) -> String {
    for s in splash.stages.iter().rev() {
        if !s.done {
            return s.label.clone();
        }
    }
    "Finalizing...".to_string()
}

fn pulse_color(c: Color, add: u8) -> Color {
    match c {
        Color::Rgb(r, g, b) => Color::Rgb(
            r.saturating_add(add),
            g.saturating_add(add),
            b.saturating_add(add),
        ),
        other => other,
    }
}

fn draw_shadow(f: &mut Frame, area: Rect, full: Rect) {
    let sx = (area.x + 1).min(full.x + full.width.saturating_sub(1));
    let sy = (area.y + 1).min(full.y + full.height.saturating_sub(1));
    let sw = area.width.min(full.width.saturating_sub(sx.saturating_sub(full.x)));
    let sh = area.height.min(full.height.saturating_sub(sy.saturating_sub(full.y)));
    if sw > 0 && sh > 0 {
        f.render_widget(
            Block::default().style(Style::default().bg(SHADOW)),
            Rect::new(sx, sy, sw, sh),
        );
    }
}

/// Center a rect of exact character dimensions inside `area`.
fn centered_fixed(w: u16, h: u16, area: Rect) -> Rect {
    let cw = w.min(area.width);
    let ch = h.min(area.height);
    let x  = area.x + area.width.saturating_sub(cw) / 2;
    let y  = area.y + area.height.saturating_sub(ch) / 2;
    Rect::new(x, y, cw, ch)
}
