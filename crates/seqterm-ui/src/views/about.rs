use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;

const BG: Color     = Color::Rgb(22, 27, 34);
const BORDER: Color = Color::Rgb(48, 54, 61);
const HEADER: Color = Color::Rgb(240, 136, 62);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const GREEN: Color  = Color::Rgb(56, 200, 100);

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn draw_about(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" ABOUT SEQTERM-RS ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),  // logo + version
            Constraint::Length(1),  // separator
            Constraint::Length(8),  // build info
            Constraint::Length(1),  // separator
            Constraint::Min(4),     // runtime diagnostics
        ])
        .split(inner);

    // ── Logo ─────────────────────────────────────────────────────────────
    let logo = vec![
        Line::from(Span::styled(
            "  ╔═══════════════════════╗",
            Style::default().fg(ACCENT),
        )),
        Line::from(Span::styled(
            "  ║  S E Q T E R M - r s  ║",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ╚═══════════════════════╝",
            Style::default().fg(ACCENT),
        )),
        Line::from(Span::styled(
            format!("       Terminal DAW  v{VERSION}"),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "     MIT / Apache-2.0 License",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    f.render_widget(
        Paragraph::new(logo).alignment(Alignment::Left),
        chunks[0],
    );

    // ── Separator ─────────────────────────────────────────────────────────
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(inner.width as usize),
            Style::default().fg(BORDER),
        ))),
        chunks[1],
    );

    // ── Build info ────────────────────────────────────────────────────────
    let proj = app.project.lock();
    let build_lines = vec![
        row("Version",   VERSION,               Color::White),
        row("Built",     "2026-05-24",          Color::White),
        row("Rust",      "1.87.0",              Color::White),
        row("Ratatui",   "0.29",                Color::White),
        row("midir",     "0.10.4",              Color::White),
        row("Backend",   if app.jack_available { "JACK/PipeWire" } else { "ALSA" },
                        if app.jack_available { GREEN } else { Color::DarkGray }),
        row("License",   "MIT / Apache-2.0",    Color::White),
        row("Author",    "SeqTerm Contributors", Color::White),
    ];
    drop(proj);
    f.render_widget(
        Paragraph::new(build_lines).style(Style::default().bg(BG)),
        chunks[2],
    );

    // ── Separator ─────────────────────────────────────────────────────────
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(inner.width as usize),
            Style::default().fg(BORDER),
        ))),
        chunks[3],
    );

    // ── Runtime diagnostics ───────────────────────────────────────────────
    let proj = app.project.lock();
    let n_patterns  = proj.patterns.len();
    let n_midi_out  = proj.midi_outputs.len();
    let xrun        = proj.xrun;
    let cpu         = proj.cpu;
    drop(proj);

    let runtime_lines = vec![
        row_head("RUNTIME DIAGNOSTICS"),
        row("BPM",          &format!("{:.1}", app.bpm),         ACCENT),
        row("Patterns",     &n_patterns.to_string(),            Color::White),
        row("MIDI outputs", &n_midi_out.to_string(),            Color::White),
        row("XRun count",   &xrun.to_string(),
            if xrun > 0 { Color::Red } else { GREEN }),
        row("CPU load",     &format!("{}%", cpu),
            if cpu > 80 { Color::Red } else { GREEN }),
        row("Playback",     if app.playing { "PLAYING" } else { "STOPPED" },
            if app.playing { GREEN } else { Color::DarkGray }),
        Line::from(Span::styled(
            "  Press Esc or Enter to close",
            Style::default().fg(BORDER),
        )),
    ];
    f.render_widget(
        Paragraph::new(runtime_lines).style(Style::default().bg(BG)),
        chunks[4],
    );
}

fn row(label: &str, value: &str, value_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {:>18}  ", label), Style::default().fg(HEADER)),
        Span::styled(value.to_string(), Style::default().fg(value_color)),
    ])
}

fn row_head(label: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {label}"),
        Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
    ))
}
