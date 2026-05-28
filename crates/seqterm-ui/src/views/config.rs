use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use seqterm_core::SyncMode;

use crate::app::App;

const PANEL: Color = Color::Rgb(22, 27, 34);
const BORDER: Color = Color::Rgb(48, 54, 61);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const HEADER: Color = Color::Rgb(240, 136, 62);

pub fn draw_config(f: &mut Frame, app: &App, area: Rect) {
    // Vertical split: top ~40% = MIDI/OSC/Sync, mid ~15% = Audio engine, bottom ~45% = routing.
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Length(8),
            Constraint::Min(0),
        ])
        .split(area);

    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(v_chunks[0]);

    // Cache subsection rects: [midi_in, midi_out, osc, sync].
    app.config_panel_rects.set([h_chunks[0], h_chunks[1], h_chunks[2], h_chunks[3]]);

    draw_midi_inputs(f, app, h_chunks[0]);
    draw_midi_outputs(f, app, h_chunks[1]);
    draw_osc_routes(f, app, h_chunks[2]);
    draw_sync_panel(f, app, h_chunks[3]);

    // Audio engine panel (section 5 = focused).
    let audio_focused = app.config_state.section == 5;
    app.config_audio_panel_rect.set(v_chunks[1]);
    draw_audio_engine_panel(f, app, v_chunks[1], audio_focused);

    // Routing graph in bottom section (section 4 = focused).
    let routing_focused = app.config_state.section == 4;
    super::routing::draw_routing_focused(f, app, v_chunks[2], routing_focused);
}

/// Shared hint footer for all config panels.
fn hint_item(focused: bool) -> ListItem<'static> {
    let text = if focused {
        " ↑↓=select  e=enable/disable  h/l=panel "
    } else {
        " h/l=navigate "
    };
    ListItem::new(Line::from(Span::styled(
        text,
        Style::default().fg(if focused { Color::Yellow } else { BORDER }),
    )))
}

fn port_items<'a>(
    ports: impl Iterator<Item = (usize, &'a seqterm_core::MidiPort)>,
    cursor: usize,
    max_name: usize,
) -> Vec<ListItem<'static>> {
    ports
        .map(|(i, port)| {
            let is_cur = i == cursor;
            let check = if port.enabled { "[x]" } else { "[ ]" };
            let (check_col, name_col) = if is_cur {
                (Color::Yellow, Color::Yellow)
            } else if port.enabled {
                (Color::Green, Color::White)
            } else {
                (Color::DarkGray, Color::DarkGray)
            };
            let style = if is_cur {
                Style::default().fg(name_col).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(name_col)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", check), Style::default().fg(check_col)),
                Span::styled(format!("CH{:02} ", port.channel), Style::default().fg(ACCENT)),
                Span::styled(truncate(&port.name, max_name), style),
            ]))
        })
        .collect()
}

fn draw_midi_inputs(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let focused = app.config_state.section == 0;
    let cursor = if focused { app.config_state.cursor } else { usize::MAX };

    let max_name = area.width.saturating_sub(10) as usize;
    let mut items = vec![hint_item(focused)];
    items.extend(port_items(proj.midi_inputs.iter().enumerate(), cursor, max_name));

    // Show "no ports" message if list is empty.
    if proj.midi_inputs.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  (no MIDI inputs detected)",
            Style::default().fg(Color::DarkGray),
        ))));
    }

    let title = format!(" MIDI IN ({}) ", proj.midi_inputs.len());
    let list = List::new(items).block(
        Block::default()
            .title(title)
            .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(if focused { Style::default().fg(Color::Yellow) } else { Style::default().fg(BORDER) })
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(list, area);
}

fn draw_midi_outputs(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let focused = app.config_state.section == 1;
    let cursor = if focused { app.config_state.cursor } else { usize::MAX };

    let max_name = area.width.saturating_sub(10) as usize;
    let mut items = vec![hint_item(focused)];
    items.extend(port_items(proj.midi_outputs.iter().enumerate(), cursor, max_name));

    if proj.midi_outputs.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  (no MIDI outputs detected)",
            Style::default().fg(Color::DarkGray),
        ))));
    }

    let title = format!(" MIDI OUT ({}) ", proj.midi_outputs.len());
    let list = List::new(items).block(
        Block::default()
            .title(title)
            .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(if focused { Style::default().fg(Color::Yellow) } else { Style::default().fg(BORDER) })
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(list, area);
}

fn draw_osc_routes(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let focused = app.config_state.section == 2;
    let cursor = if focused { app.config_state.cursor } else { usize::MAX };

    let max_w = (area.width.saturating_sub(8) / 2) as usize;

    let mut items: Vec<ListItem> = vec![hint_item(focused)];
    for (i, route) in proj.osc_routes.iter().enumerate() {
        let is_cur = i == cursor;
        let check = if route.enabled { "[x]" } else { "[ ]" };
        let (check_col, text_col) = if is_cur {
            (Color::Yellow, Color::Yellow)
        } else if route.enabled {
            (Color::Cyan, Color::White)
        } else {
            (Color::DarkGray, Color::DarkGray)
        };
        let text_style = if is_cur {
            Style::default().fg(text_col).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(text_col)
        };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!("{} ", check), Style::default().fg(check_col)),
            Span::styled(truncate(&route.address, max_w), text_style),
            Span::styled(" → ", Style::default().fg(ACCENT)),
            Span::styled(truncate(&route.target, max_w), text_style),
        ])));
    }

    if proj.osc_routes.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  (no OSC routes configured)",
            Style::default().fg(Color::DarkGray),
        ))));
    }

    // OSC server status separator.
    items.push(ListItem::new(Line::from(Span::styled(
        "  ──────────────────────",
        Style::default().fg(BORDER),
    ))));
    let osc_running = app.osc_port > 0;
    let (osc_label, osc_col) = if osc_running {
        (format!("UDP :{} ● listening", app.osc_port), Color::Green)
    } else {
        ("off  (o=start :57120)".to_string(), Color::DarkGray)
    };
    items.push(ListItem::new(Line::from(vec![
        Span::styled("  SERVER ", Style::default().fg(HEADER).add_modifier(Modifier::BOLD)),
        Span::styled(osc_label, Style::default().fg(osc_col)),
    ])));

    let title = format!(" OSC ROUTES ({}) ", proj.osc_routes.len());
    let list = List::new(items).block(
        Block::default()
            .title(title)
            .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(if focused { Style::default().fg(Color::Yellow) } else { Style::default().fg(BORDER) })
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(list, area);
}

fn draw_sync_panel(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let focused = app.config_state.section == 3;
    let cursor  = if focused { app.config_state.cursor } else { usize::MAX };

    let modes = [
        SyncMode::Internal,
        SyncMode::Usb,
        SyncMode::Midi,
        SyncMode::Clock,
    ];

    // ── List items (same structure as MIDI panels so click y-math works) ──────
    // items[0] = hint   → inner_y = rect.y+2, skip
    // items[1..4] = modes → item_idx 0-3 map to cursor 0-3
    let mut items: Vec<ListItem> = vec![hint_item(focused)];

    for (i, mode) in modes.iter().enumerate() {
        let is_active = &proj.sync_mode == mode;
        let is_cur    = i == cursor;

        let (marker, check) = if is_active { ("▶", "●") } else { (" ", "○") };

        let row_style = if is_cur && focused {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED)
        } else if is_active {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!(" {} ", marker), row_style),
            Span::styled(
                format!("({}) ", check),
                if is_active {
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(BORDER)
                },
            ),
            Span::styled(mode.label().to_string(), row_style),
        ])));
    }

    // ── Separator + info lines (non-interactive) ──────────────────────────────
    items.push(ListItem::new(Line::from(Span::styled(
        "  ──────────────────────",
        Style::default().fg(BORDER),
    ))));

    let (jack_label, jack_col) = if app.jack_available {
        ("JACK  ● RUNNING", Color::Green)
    } else {
        ("JACK  ○ not found", Color::DarkGray)
    };
    items.push(ListItem::new(Line::from(vec![
        Span::styled("  BACKEND ", Style::default().fg(HEADER).add_modifier(Modifier::BOLD)),
        Span::styled(jack_label, Style::default().fg(jack_col)),
    ])));

    items.push(ListItem::new(Line::from(vec![
        Span::styled("  XRUN    ", Style::default().fg(HEADER)),
        Span::styled(
            format!("{}", proj.xrun),
            if proj.xrun > 0 { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) },
        ),
        Span::styled("   CPU  ", Style::default().fg(HEADER)),
        Span::styled(
            format!("{}%", proj.cpu),
            if proj.cpu > 80 { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) },
        ),
    ])));

    let clock_active = matches!(proj.sync_mode, SyncMode::Clock);
    let (clk_label, clk_col) = if clock_active {
        ("CLK OUT  ● sending", Color::Green)
    } else {
        ("CLK OUT  ○ off (Enter=CLK to enable)", Color::DarkGray)
    };
    items.push(ListItem::new(Line::from(vec![
        Span::styled("  MIDI ", Style::default().fg(HEADER).add_modifier(Modifier::BOLD)),
        Span::styled(clk_label, Style::default().fg(clk_col)),
    ])));

    let list_title = format!(
        " SYNC / CLOCK [{}] ",
        if app.jack_available { "JACK" } else { "ALSA" }
    );
    let list = List::new(items).block(
        Block::default()
            .title(list_title)
            .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(if focused { Style::default().fg(Color::Yellow) } else { Style::default().fg(BORDER) })
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(list, area);
}

fn draw_audio_engine_panel(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    let border_col = if focused { Color::Yellow } else { BORDER };

    let sr    = app.audio_sample_rate;
    let bufsz = app.audio_buffer_size;
    let latency_ms = if sr > 0 { bufsz as f64 * 1000.0 / sr as f64 } else { 0.0 };
    let dsp   = app.audio_dsp_load;
    let xruns = app.audio_xrun_count;
    let running = app.audio_engine_running;

    let (status_label, status_col) = if running {
        ("● RUNNING", Color::Green)
    } else {
        ("○ STOPPED", Color::DarkGray)
    };
    let dsp_col = if dsp > 80.0 { Color::Red } else if dsp > 50.0 { Color::Yellow } else { Color::Green };
    let xrun_col = if xruns > 0 { Color::Red } else { Color::Green };

    let backend_name = if app.settings.audio.backend == "JACK" { "JACK" } else { "CPAL" };
    let backend_col  = if app.settings.audio.backend == "JACK" { Color::Rgb(56, 200, 100) } else { Color::White };

    let lines = vec![
        Line::from(vec![
            Span::styled("  BACKEND ", Style::default().fg(HEADER).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:<6}", backend_name), Style::default().fg(backend_col)),
            Span::styled(status_label, Style::default().fg(status_col)),
            Span::styled("   SAMPLE RATE ", Style::default().fg(HEADER)),
            Span::styled(format!("{} Hz", sr), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  BUFFER  ", Style::default().fg(HEADER)),
            Span::styled(format!("{} frames", bufsz), Style::default().fg(Color::White)),
            Span::styled(format!("  ({:.1}ms latency)", latency_ms), Style::default().fg(Color::DarkGray)),
            Span::styled("   DSP ", Style::default().fg(HEADER)),
            Span::styled(format!("{:.1}%", dsp), Style::default().fg(dsp_col)),
            Span::styled("   XRUN ", Style::default().fg(HEADER)),
            Span::styled(format!("{}", xruns), Style::default().fg(xrun_col)),
        ]),
        Line::from(vec![
            Span::styled(
                if focused { "  ↑↓=buffer  ←→=rate  s=start/stop  J=JACK toggle  h/l=nav" }
                           else { "  h/l=navigate" },
                Style::default().fg(if focused { Color::Yellow } else { BORDER }),
            ),
        ]),
    ];

    let p = Paragraph::new(lines).block(
        Block::default()
            .title(" AUDIO ENGINE ")
            .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
