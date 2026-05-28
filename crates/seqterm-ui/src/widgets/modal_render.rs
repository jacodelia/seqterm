use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::App,
    modal::{AlertKind, CommandPaletteState, FilePickerMode, Modal, EXPORT_SAMPLE_RATES, EXPORT_BIT_DEPTHS},
    views::{draw_about, draw_help},
};
const BG:      Color = Color::Rgb(22, 27, 34);
const BORDER:  Color = Color::Rgb(58, 64, 72);
const ACCENT:  Color = Color::Rgb(31, 111, 235);
const HEADER:  Color = Color::Rgb(240, 136, 62);
const OK:      Color = Color::Rgb(56, 200, 100);
const ERR:     Color = Color::Rgb(220, 80, 80);
const SUCCESS: Color = Color::Rgb(56, 200, 100);

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Draw the active modal centred over the terminal.
/// Clears the area under it before drawing.
pub fn draw_modal(f: &mut Frame, app: &mut App, full_area: Rect) {
    // Reset mouse hit-test rects — set below only when a modal is actually drawn.
    app.modal_close_rect.set(Rect::default());
    app.modal_area.set(Rect::default());

    let Some(modal) = &app.active_modal else { return };

    match modal {
        Modal::Alert { title, message, kind } => {
            let area = centered_rect(60, 30, full_area);
            let border_col = match kind {
                AlertKind::Info    => ACCENT,
                AlertKind::Success => SUCCESS,
                AlertKind::Error   => ERR,
            };
            draw_shadow(f, area, full_area);
            draw_alert(f, title.clone(), message.clone(), area, border_col);
            render_close_btn(f, app, area);
        }
        Modal::Confirm { title, body, .. } => {
            let area = centered_rect(60, 30, full_area);
            draw_shadow(f, area, full_area);
            draw_confirm(f, title.clone(), body.clone(), area);
            render_close_btn(f, app, area);
        }
        Modal::Progress { title, message, progress, cancelable } => {
            let area = centered_rect(60, 20, full_area);
            draw_shadow(f, area, full_area);
            draw_progress(f, title.clone(), message.clone(), *progress, app.frame_count, area);
            if *cancelable {
                render_close_btn(f, app, area);
            }
        }
        Modal::Input(_) => {
            let area = centered_rect(60, 25, full_area);
            draw_shadow(f, area, full_area);
            draw_input_dialog(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::FilePicker(_) => {
            let area = centered_rect(80, 80, full_area);
            draw_shadow(f, area, full_area);
            draw_file_picker(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::About => {
            let area = centered_rect(70, 85, full_area);
            draw_shadow(f, area, full_area);
            f.render_widget(Clear, area);
            draw_about(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::Help(_) => {
            let area = centered_rect(90, 90, full_area);
            draw_shadow(f, area, full_area);
            f.render_widget(Clear, area);
            // draw_help needs mutable access to state.scroll; work around immutable borrow.
            draw_help_from_app(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::AudioSettings(_) => {
            let area = centered_rect(60, 60, full_area);
            draw_shadow(f, area, full_area);
            draw_audio_settings(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::MidiSettings(_) => {
            let area = centered_rect(70, 70, full_area);
            draw_shadow(f, area, full_area);
            draw_midi_settings(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::CommandPalette(_) => {
            let area = centered_rect(70, 60, full_area);
            draw_shadow(f, area, full_area);
            draw_command_palette(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::MidiImportOptions(_) => {
            let area = centered_rect(60, 45, full_area);
            draw_shadow(f, area, full_area);
            draw_midi_import_options(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::KeybindingsEditor(_) => {
            let area = centered_rect(70, 80, full_area);
            draw_shadow(f, area, full_area);
            draw_keybindings_editor(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::AudioExportOptions(_) => {
            let area = centered_rect(50, 35, full_area);
            draw_shadow(f, area, full_area);
            draw_audio_export_options(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::Sf2Browser(_) => {
            let area = centered_rect(60, 70, full_area);
            draw_shadow(f, area, full_area);
            draw_sf2_browser(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::PluginParams(_) => {
            let area = centered_rect(70, 70, full_area);
            draw_shadow(f, area, full_area);
            draw_plugin_params(f, app, area);
            render_close_btn(f, app, area);
        }
    }
}

/// Render `[×]` in the top-right corner of the modal border and store the
/// click rect so the mouse handler can close the modal.
fn render_close_btn(f: &mut Frame, app: &mut App, area: Rect) {
    if area.width < 6 { return; }
    let close_rect = Rect {
        x: area.x + area.width.saturating_sub(4),
        y: area.y,
        width: 3,
        height: 1,
    };
    app.modal_close_rect.set(close_rect);
    app.modal_area.set(area);
    f.render_widget(
        Paragraph::new(Span::styled(
            "[×]",
            Style::default().fg(ERR).add_modifier(Modifier::BOLD),
        )),
        close_rect,
    );
}

/// Render a 1-char drop-shadow offset 1 right and 1 down from `area`.
fn draw_shadow(f: &mut Frame, area: Rect, full_area: Rect) {
    const SHADOW: Color = Color::Rgb(10, 12, 16);
    let sx = (area.x + 1).min(full_area.x + full_area.width.saturating_sub(1));
    let sy = (area.y + 1).min(full_area.y + full_area.height.saturating_sub(1));
    let sw = area.width.min(full_area.width.saturating_sub(sx.saturating_sub(full_area.x)));
    let sh = area.height.min(full_area.height.saturating_sub(sy.saturating_sub(full_area.y)));
    if sw > 0 && sh > 0 {
        f.render_widget(
            Block::default().style(Style::default().bg(SHADOW)),
            Rect::new(sx, sy, sw, sh),
        );
    }
}

// ─── Alert ────────────────────────────────────────────────────────────────────

fn draw_alert(f: &mut Frame, title: String, message: String, area: Rect, border: Color) {
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(format!(" {title} "))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(
        Paragraph::new(message).wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::White).bg(BG)),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  Enter / Esc = close",
            Style::default().fg(BORDER),
        ))),
        chunks[1],
    );
}

// ─── Confirm ─────────────────────────────────────────────────────────────────

fn draw_confirm(f: &mut Frame, title: String, body: String, area: Rect) {
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(format!(" {title} "))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::White).bg(BG)),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Enter", Style::default().fg(OK).add_modifier(Modifier::BOLD)),
            Span::styled(" = Yes  ", Style::default().fg(Color::White)),
            Span::styled("Esc", Style::default().fg(ERR).add_modifier(Modifier::BOLD)),
            Span::styled(" = Cancel", Style::default().fg(Color::White)),
        ])),
        chunks[1],
    );
}

// ─── Progress ────────────────────────────────────────────────────────────────

fn draw_progress(f: &mut Frame, title: String, message: String, progress: f32, frame: u64, area: Rect) {
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(format!(" {title} "))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(message).style(Style::default().fg(Color::White).bg(BG)),
        chunks[0],
    );

    // Progress bar.
    let bar_width = chunks[1].width as usize;
    let filled = ((progress.clamp(0.0, 1.0) * bar_width as f32) as usize).min(bar_width);
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(bar_width - filled));
    f.render_widget(
        Paragraph::new(Span::styled(bar, Style::default().fg(ACCENT))),
        chunks[1],
    );

    // Animated spinner + percentage.
    let spinner = SPINNER_FRAMES[(frame as usize / 3) % SPINNER_FRAMES.len()];
    let pct = (progress * 100.0) as u32;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("  {spinner} "), Style::default().fg(ACCENT)),
            Span::styled(format!("{pct}%  Esc=cancel"), Style::default().fg(Color::DarkGray)),
        ])),
        chunks[2],
    );
}

// ─── Input dialog ─────────────────────────────────────────────────────────────

fn draw_input_dialog(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(crate::modal::Modal::Input(state)) = &app.active_modal else { return };
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" {} ", state.title))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3), Constraint::Length(1)])
        .split(inner);

    // Placeholder hint.
    f.render_widget(
        Paragraph::new(format!("  {}", state.placeholder))
            .style(Style::default().fg(Color::DarkGray).bg(BG)),
        chunks[0],
    );

    // Text input box.
    let display = if state.value.is_empty() {
        Span::styled("▏", Style::default().fg(Color::DarkGray))
    } else {
        Span::styled(format!("{}_", state.value), Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
    };
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let input_inner = input_block.inner(chunks[1]);
    f.render_widget(input_block, chunks[1]);
    f.render_widget(Paragraph::new(Line::from(display)), input_inner);

    // Footer hint.
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Enter", Style::default().fg(OK).add_modifier(Modifier::BOLD)),
            Span::styled(" = confirm  ", Style::default().fg(Color::White)),
            Span::styled("Esc", Style::default().fg(ERR).add_modifier(Modifier::BOLD)),
            Span::styled(" = cancel", Style::default().fg(Color::White)),
        ])),
        chunks[2],
    );
}

// ─── File picker ──────────────────────────────────────────────────────────────

fn draw_file_picker(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(Modal::FilePicker(state)) = &mut app.active_modal else { return };
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" {} ", state.target.title()))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mode = state.target.mode();
    let show_input = mode == FilePickerMode::Save;
    let show_search = mode == FilePickerMode::Open;

    let constraints = if show_input {
        vec![
            Constraint::Length(1),  // current dir
            Constraint::Min(4),     // file list
            Constraint::Length(1),  // separator
            Constraint::Length(1),  // filename input
            Constraint::Length(1),  // hint
        ]
    } else if show_search {
        vec![
            Constraint::Length(1),  // current dir
            Constraint::Length(1),  // search bar
            Constraint::Min(4),     // file list
            Constraint::Length(1),  // hint
        ]
    } else {
        vec![
            Constraint::Length(1),  // current dir
            Constraint::Min(4),     // file list
            Constraint::Length(1),  // hint
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // Current directory.
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  DIR: ", Style::default().fg(HEADER)),
            Span::styled(
                state.current_dir.display().to_string(),
                Style::default().fg(Color::White),
            ),
        ])),
        chunks[0],
    );

    // Search bar (Open mode only).
    let list_chunk_idx = if show_search {
        // Render search bar in chunks[1].
        let search_active = !state.search_input.is_empty();
        let (label_col, value_col) = if search_active {
            (HEADER, Color::Yellow)
        } else {
            (BORDER, Color::DarkGray)
        };
        let cursor_str = if search_active { "█" } else { "_" };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  SEARCH: ", Style::default().fg(label_col)),
                Span::styled(
                    format!("{}{}", state.search_input, cursor_str),
                    Style::default().fg(value_col),
                ),
                Span::styled(
                    if search_active {
                        format!("  ({} matches)", state.visible_entries().len())
                    } else {
                        "  (type to filter)".to_string()
                    },
                    Style::default().fg(BORDER),
                ),
            ])),
            chunks[1],
        );
        2usize // file list is in chunks[2]
    } else {
        1usize // file list is in chunks[1]
    };

    // File list — focus ring shows ACCENT border when list has focus.
    let list_focus_col = if show_input && state.input_focused { BORDER } else { ACCENT };
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(list_focus_col))
        .style(Style::default().bg(BG));
    let list_inner = list_block.inner(chunks[list_chunk_idx]);
    app.file_picker_list_area.set(list_inner);
    f.render_widget(list_block, chunks[list_chunk_idx]);
    let visible = list_inner.height as usize;
    state.clamp_scroll(visible);
    let visible_entries: Vec<_> = state.visible_entries();
    let items: Vec<ListItem> = visible_entries
        .iter()
        .skip(state.scroll)
        .take(visible)
        .enumerate()
        .map(|(rel_i, entry)| {
            let abs_i = rel_i + state.scroll;
            let is_sel = abs_i == state.cursor;
            let (icon, col) = if entry.is_dir {
                ("▶ ", Color::Cyan)
            } else {
                ("  ", Color::White)
            };
            let style = if is_sel {
                Style::default().fg(Color::Black).bg(Color::Rgb(56, 139, 253))
            } else {
                Style::default().fg(col)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{icon}{}", entry.name),
                style,
            )))
        })
        .collect();

    f.render_widget(
        List::new(items).style(Style::default().bg(BG)),
        list_inner,
    );

    let hint_idx = if show_input { 4 } else if show_search { 3 } else { 2 };
    if show_input {
        // Focus color: ACCENT on the focused section, BORDER on the inactive one.
        let input_focus_col = if state.input_focused { ACCENT } else { BORDER };
        let input_style = if state.input_focused {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        // Separator — color shifts to ACCENT when filename input has focus.
        f.render_widget(
            Paragraph::new(Span::styled(
                "─".repeat(inner.width as usize),
                Style::default().fg(input_focus_col),
            )),
            chunks[2],
        );
        // Filename input.
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  Filename: ", Style::default().fg(if state.input_focused { HEADER } else { BORDER })),
                Span::styled(state.filename_input.clone() + "█", input_style),
            ])),
            chunks[3],
        );
    }

    // Hint.
    let recent_hint = if !state.recent_dirs.is_empty() { "  r=recent  H=home" } else { "" };
    let hint = if show_input {
        format!("  Tab=toggle focus  Enter=save  Backspace=up  Esc=cancel{recent_hint}")
    } else if show_search {
        format!("  type=filter  ↑↓=navigate  Enter=open  Del=clear  Backspace=up dir  Esc=cancel{recent_hint}")
    } else {
        format!("  ↑↓=navigate  Enter=open  Backspace=up  Esc=cancel{recent_hint}")
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(BORDER))),
        chunks[hint_idx],
    );

    // Recent-dirs overlay: drawn over the file list when show_recent is true.
    if state.show_recent && !state.recent_dirs.is_empty() {
        let list_rect = chunks[1];
        let overlay_h = (state.recent_dirs.len() as u16 + 2).min(list_rect.height);
        let overlay = Rect::new(
            list_rect.x,
            list_rect.y,
            list_rect.width,
            overlay_h,
        );
        f.render_widget(Clear, overlay);
        let block = Block::default()
            .title(" RECENT DIRECTORIES ")
            .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .style(Style::default().bg(BG));
        let inner_r = block.inner(overlay);
        f.render_widget(block, overlay);
        let items: Vec<ListItem> = state.recent_dirs.iter().enumerate().map(|(i, dir)| {
            let is_sel = i == state.recent_cursor;
            let style = if is_sel {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(
                format!("  {}", dir.display()),
                style,
            )))
        }).collect();
        f.render_widget(List::new(items).style(Style::default().bg(BG)), inner_r);
    }
}

// ─── Audio settings ───────────────────────────────────────────────────────────

fn draw_audio_settings(f: &mut Frame, app: &App, area: Rect) {
    let Some(Modal::AudioSettings(state)) = &app.active_modal else { return };
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" AUDIO SETTINGS ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let s = &app.settings.audio;
    let lat_ms = s.buffer_size as f32 / s.sample_rate as f32 * 1000.0;

    // Base rows always shown.
    let mut rows: Vec<(&'static str, String, usize)> = vec![
        ("Backend",     s.backend.clone(),                   0),
        ("Device",      s.device.clone(),                    1),
        ("Sample rate", format!("{} Hz", s.sample_rate),     2),
        ("Buffer size", format!("{} samples", s.buffer_size), 3),
        ("Latency",     format!("{:.1} ms", lat_ms),         usize::MAX),
    ];

    // Backend-specific rows (read-only display; editing via config file for now).
    match s.backend.to_uppercase().as_str() {
        "ALSA" => {
            let hw = if s.alsa_hw_device.is_empty() { "(default)".to_string() } else { s.alsa_hw_device.clone() };
            rows.push(("ALSA hw dev",  hw, usize::MAX));
        }
        "JACK" => {
            let srv = if s.jack_server_name.is_empty() { "(default)".to_string() } else { s.jack_server_name.clone() };
            rows.push(("JACK server",  srv, usize::MAX));
        }
        "PIPEWIRE" => {
            let q = if s.pipewire_quantum == 0 { "system".to_string() } else { s.pipewire_quantum.to_string() };
            rows.push(("PW quantum",   q,   usize::MAX));
        }
        "WASAPI" => {
            rows.push(("WASAPI excl.", if s.wasapi_exclusive { "On" } else { "Off" }.to_string(), usize::MAX));
        }
        _ => {}
    }

    let mut lines = vec![Line::from(Span::styled(
        "  Use ↑↓ to select, ←→ to adjust  |  Esc=close",
        Style::default().fg(BORDER),
    ))];

    for (label, value, idx) in &rows {
        let is_cur = state.cursor == *idx && *idx != usize::MAX;
        let style = if is_cur {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED)
        } else if *idx == usize::MAX {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {:>14}  ", label), Style::default().fg(HEADER)),
            Span::styled(value.clone(), style),
        ]));
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(BG)),
        inner,
    );
}

// ─── MIDI settings ────────────────────────────────────────────────────────────

fn draw_midi_settings(f: &mut Frame, app: &App, area: Rect) {
    let Some(Modal::MidiSettings(state)) = &app.active_modal else { return };
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" MIDI SETTINGS ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let tab_labels = ["Inputs", "Outputs", "Sync", "Learn"];
    let tab_bar: Vec<Span> = tab_labels
        .iter()
        .enumerate()
        .flat_map(|(i, l)| {
            let s = if i == state.tab {
                Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            vec![
                Span::styled(format!(" {l} "), s),
                Span::styled("│", Style::default().fg(BORDER)),
            ]
        })
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(Paragraph::new(Line::from(tab_bar)), chunks[0]);

    let proj = app.project.lock();
    let items: Vec<ListItem> = match state.tab {
        0 => proj.midi_inputs.iter().enumerate().map(|(i, p)| {
            let sel = i == state.cursor;
            let check = if p.enabled { "[x]" } else { "[ ]" };
            let style = if sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED) }
                        else   { Style::default().fg(if p.enabled { Color::White } else { Color::DarkGray }) };
            ListItem::new(Line::from(Span::styled(
                format!(" {check} CH{:02} {}", p.channel, p.name), style,
            )))
        }).collect(),
        1 => proj.midi_outputs.iter().enumerate().map(|(i, p)| {
            let sel = i == state.cursor;
            let check = if p.enabled { "[x]" } else { "[ ]" };
            let style = if sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED) }
                        else   { Style::default().fg(if p.enabled { Color::White } else { Color::DarkGray }) };
            ListItem::new(Line::from(Span::styled(
                format!(" {check} CH{:02} {}", p.channel, p.name), style,
            )))
        }).collect(),
        2 => {
            use seqterm_core::SyncMode;
            [SyncMode::Internal, SyncMode::Usb, SyncMode::Midi, SyncMode::Clock]
                .iter().enumerate().map(|(i, m)| {
                    let active = &proj.sync_mode == m;
                    let sel = i == state.cursor;
                    let marker = if active { "▶" } else { " " };
                    let style = if sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED) }
                                else if active { Style::default().fg(Color::Green) }
                                else { Style::default().fg(Color::White) };
                    ListItem::new(Line::from(Span::styled(
                        format!(" {} {} {}", marker, if active {"(●)"} else {"( )"}, m.label()),
                        style,
                    )))
                }).collect()
        }
        _ => {
            // MIDI Learn tab — show existing bindings.
            drop(proj);
            let learn_active = app.midi_learn.is_some();
            let mut learn_items: Vec<ListItem> = Vec::new();
            if learn_active {
                learn_items.push(ListItem::new(Line::from(Span::styled(
                    "  ● Waiting for CC… (send a CC on any channel)",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ))));
            }
            if app.settings.midi_learn_bindings.is_empty() && !learn_active {
                learn_items.push(ListItem::new(Line::from(Span::styled(
                    "  No bindings yet. Press 'l' on a channel strip to learn.",
                    Style::default().fg(Color::DarkGray),
                ))));
            }
            for b in &app.settings.midi_learn_bindings {
                learn_items.push(ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  CC{:>3} (ch{:>2}) → ", b.cc, b.midi_ch + 1),
                        Style::default().fg(ACCENT),
                    ),
                    Span::styled(b.target.label(), Style::default().fg(Color::White)),
                ])));
            }
            learn_items
        }
    };
    let hint = match state.tab {
        3 => "  l=learn  Del=clear  Tab=next tab  Esc=close",
        _ => "  ↑↓=select  e=toggle  Tab=next tab  Esc=close",
    };

    f.render_widget(List::new(items).style(Style::default().bg(BG)), chunks[1]);
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(BORDER))),
        chunks[2],
    );
}

// ─── Help wrapper ─────────────────────────────────────────────────────────────

fn draw_help_from_app(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some(Modal::Help(state)) = &mut app.active_modal {
        draw_help(f, state, area);
    }
}

// ─── Floating menu panel ──────────────────────────────────────────────────────

/// Draw the open dropdown menu below its label in the menu bar.
pub fn draw_menu_dropdown(
    f: &mut Frame,
    kind: crate::menu::MenuKind,
    cursor: usize,
    bar_x: u16,
    bar_y: u16,
    full_area: Rect,
) {
    let items = kind.items();

    // Measure panel width.
    let panel_w = (items
        .iter()
        .map(|i| i.label.len() + i.shortcut.len() + 6)
        .max()
        .unwrap_or(20) as u16)
        .max(20)
        .min(full_area.width - bar_x);

    let panel_h = (items.len() as u16 + 2).min(full_area.height - bar_y - 1);

    let area = Rect::new(
        bar_x.min(full_area.width.saturating_sub(panel_w)),
        bar_y + 1,
        panel_w,
        panel_h,
    );

    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut sel_idx = 0usize;
    let items_list: Vec<ListItem> = items.iter().map(|item| {
        if item.separator {
            return ListItem::new(Line::from(Span::styled(
                "─".repeat(inner.width as usize),
                Style::default().fg(BORDER),
            )));
        }
        let is_sel = sel_idx == cursor;
        if !item.disabled { sel_idx += 1; }
        let style = if is_sel && !item.disabled {
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else if item.disabled {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let label_w = (inner.width as usize).saturating_sub(item.shortcut.len() + 2);
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {:<width$}", item.label, width = label_w.saturating_sub(1)), style),
            Span::styled(
                format!("{} ", item.shortcut),
                if is_sel && !item.disabled { style } else { Style::default().fg(Color::DarkGray) },
            ),
        ]))
    }).collect();

    f.render_widget(List::new(items_list).style(Style::default().bg(BG)), inner);
}

// ─── Command Palette ──────────────────────────────────────────────────────────

fn draw_command_palette(f: &mut Frame, app: &mut App, area: Rect) {
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" COMMAND PALETTE  (Ctrl+P) ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Layout: search box (3 rows) | results list (rest)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    let Some(Modal::CommandPalette(state)) = &app.active_modal else { return };

    // Search input.
    let query_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let query_inner = query_block.inner(chunks[0]);
    f.render_widget(query_block, chunks[0]);
    let cursor_char = if (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_millis()) < 500 { "▋" } else { " " };
    f.render_widget(
        Paragraph::new(format!("{}{}", state.query, cursor_char))
            .style(Style::default().fg(Color::White).bg(BG)),
        query_inner,
    );

    // Results list.
    let visible_h = chunks[1].height as usize;
    let cursor = state.cursor;
    let scroll = cursor.saturating_sub(visible_h.saturating_sub(1));

    let items: Vec<ratatui::widgets::ListItem> = state.results.iter().enumerate()
        .skip(scroll)
        .take(visible_h)
        .map(|(i, entry)| {
            let is_sel = i == cursor;
            let style = if is_sel {
                Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let sc_style = if is_sel {
                Style::default().fg(Color::Black).bg(ACCENT)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let label_w = (chunks[1].width as usize).saturating_sub(entry.shortcut.len() + 4);
            ratatui::widgets::ListItem::new(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(
                    format!("  {:<width$}", entry.label, width = label_w),
                    style,
                ),
                ratatui::text::Span::styled(
                    format!("{} ", entry.shortcut),
                    sc_style,
                ),
            ]))
        })
        .collect();

    f.render_widget(
        ratatui::widgets::List::new(items).style(Style::default().bg(BG)),
        chunks[1],
    );

    // Hint if no results.
    if state.results.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  No matching commands",
                Style::default().fg(Color::DarkGray),
            )),
            chunks[1],
        );
    }

    // Suppress "unused import" for CommandPaletteState.
    let _ = CommandPaletteState::new;
}

// ─── MIDI import options ──────────────────────────────────────────────────────

fn draw_midi_import_options(f: &mut Frame, app: &App, area: Rect) {
    use crate::modal::Modal;
    let Some(Modal::MidiImportOptions(state)) = &app.active_modal else { return };
    f.render_widget(Clear, area);

    let file_name = state.path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let block = Block::default()
        .title(format!(" MIDI IMPORT — {file_name} "))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner area: top = options, bottom = track preview.
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(0)])
        .split(inner);

    let o = &state.opts;
    let bars_label  = format!("{} bars",  o.bars_per_pattern);
    let steps_label = match o.steps_per_beat {
        4 => "4 (16th notes)",
        8 => "8 (32nd notes)",
        _ => "?",
    };
    let drums_label = if o.detect_drums { "On" } else { "Off" };

    let option_rows: &[(&str, &str, usize)] = &[
        ("Bars per pattern", &bars_label,  0),
        ("Steps per beat",   steps_label,  1),
        ("Drum detection",   drums_label,  2),
    ];

    let mut opt_lines = vec![
        Line::from(Span::styled(
            "  ↑↓ select  ←→ adjust  Enter=import  Esc=cancel",
            Style::default().fg(BORDER),
        )),
        Line::from(""),
    ];
    for (label, value, idx) in option_rows {
        let is_cur = state.cursor == *idx;
        let vstyle = if is_cur {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_cur { " ▶ " } else { "   " };
        opt_lines.push(Line::from(vec![
            Span::styled(format!("{prefix}{:<20} ", label), Style::default().fg(HEADER)),
            Span::styled(value.to_string(), vstyle),
        ]));
    }
    f.render_widget(Paragraph::new(opt_lines).style(Style::default().bg(BG)), sections[0]);

    // Track preview list.
    if !state.track_infos.is_empty() {
        let track_block = Block::default()
            .title(" Detected Tracks ")
            .title_style(Style::default().fg(HEADER))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(BG));
        let track_inner = track_block.inner(sections[1]);
        f.render_widget(track_block, sections[1]);

        let header = Line::from(vec![
            Span::styled(format!("{:<16}", "Name"), Style::default().fg(BORDER).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {:>3}", "Ch"), Style::default().fg(BORDER)),
            Span::styled(format!(" {:>6}", "Notes"), Style::default().fg(BORDER)),
            Span::styled(" Type", Style::default().fg(BORDER)),
        ]);
        let mut items: Vec<ListItem> = vec![ListItem::new(header)];
        for ti in &state.track_infos {
            let kind = if ti.is_drum { "Drums" } else { "Melodic" };
            let line = Line::from(vec![
                Span::styled(format!("{:<16}", &ti.name[..ti.name.len().min(16)]), Style::default().fg(Color::White)),
                Span::styled(format!(" {:>3}", ti.channel + 1), Style::default().fg(Color::Cyan)),
                Span::styled(format!(" {:>6}", ti.note_count), Style::default().fg(Color::Yellow)),
                Span::styled(format!(" {kind}"), Style::default().fg(if ti.is_drum { Color::Magenta } else { Color::Green })),
            ]);
            items.push(ListItem::new(line));
        }
        f.render_widget(List::new(items).style(Style::default().bg(BG)), track_inner);
    }
}

// ─── Audio export options ─────────────────────────────────────────────────────

fn draw_audio_export_options(f: &mut Frame, app: &App, area: Rect) {
    let Some(Modal::AudioExportOptions(state)) = &app.active_modal else { return };

    f.render_widget(Clear, area);
    let outer = Block::default()
        .title(" Export Audio — Options ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    f.render_widget(outer, area);

    let inner = Rect {
        x: area.x + 2,
        y: area.y + 2,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(4),
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // sample rate label
            Constraint::Length(1), // sample rate values
            Constraint::Length(1), // spacer
            Constraint::Length(1), // bit depth label
            Constraint::Length(1), // bit depth values
            Constraint::Length(1), // spacer
            Constraint::Length(1), // mode label
            Constraint::Length(1), // mode values
            Constraint::Length(1), // spacer
            Constraint::Length(1), // note
            Constraint::Min(0),
            Constraint::Length(1), // hint
        ])
        .split(inner);

    // ── Sample rate ──────────────────────────────────────────────────────────
    let sr_label_style = if state.cursor == 0 {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    f.render_widget(
        Paragraph::new(if state.cursor == 0 { "▶ Sample Rate" } else { "  Sample Rate" })
            .style(sr_label_style),
        rows[0],
    );

    let sr_spans: Vec<Span> = EXPORT_SAMPLE_RATES.iter().enumerate().map(|(i, &r)| {
        let label = format!("  {:>6} Hz  ", r);
        if i == state.sample_rate_idx {
            Span::styled(label, Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(label, Style::default().fg(Color::White))
        }
    }).collect();
    f.render_widget(Paragraph::new(Line::from(sr_spans)), rows[1]);

    // ── Bit depth ────────────────────────────────────────────────────────────
    let bd_label_style = if state.cursor == 1 {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    f.render_widget(
        Paragraph::new(if state.cursor == 1 { "▶ Bit Depth" } else { "  Bit Depth" })
            .style(bd_label_style),
        rows[3],
    );

    let bd_spans: Vec<Span> = EXPORT_BIT_DEPTHS.iter().enumerate().map(|(i, &d)| {
        let label = format!("  {d}-bit    ");
        if i == state.bit_depth_idx {
            Span::styled(label, Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(label, Style::default().fg(Color::White))
        }
    }).collect();
    f.render_widget(Paragraph::new(Line::from(bd_spans)), rows[4]);

    // ── Mode ─────────────────────────────────────────────────────────────────
    let mode_label_style = if state.cursor == 2 {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    f.render_widget(
        Paragraph::new(if state.cursor == 2 { "▶ Mode" } else { "  Mode" })
            .style(mode_label_style),
        rows[6],
    );

    let mode_spans = vec![
        if !state.stems {
            Span::styled("  Full Mix  ", Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD))
        } else {
            Span::styled("  Full Mix  ", Style::default().fg(Color::White))
        },
        if state.stems {
            Span::styled("  Stems  ", Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD))
        } else {
            Span::styled("  Stems  ", Style::default().fg(Color::White))
        },
    ];
    f.render_widget(Paragraph::new(Line::from(mode_spans)), rows[7]);

    // ── Note: stub output ────────────────────────────────────────────────────
    f.render_widget(
        Paragraph::new("  Format: WAV  (offline synth: placeholder only)")
            .style(Style::default().fg(Color::DarkGray)),
        rows[9],
    );

    // ── Hint bar ─────────────────────────────────────────────────────────────
    f.render_widget(
        Paragraph::new("  ↑↓ row   ←→/Space change   Enter confirm   Esc cancel")
            .style(Style::default().fg(Color::DarkGray)),
        rows[11],
    );
}

// ─── Keybindings editor ───────────────────────────────────────────────────────

fn draw_keybindings_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(Modal::KeybindingsEditor(state)) = &mut app.active_modal else { return };
    f.render_widget(Clear, area);

    let title = if state.rebinding.is_some() {
        " KEYBINDINGS — press new key combo (Esc=cancel) "
    } else {
        " KEYBINDINGS  (↑↓=nav  Enter=rebind  Del=clear  r=reset  e=export  i=import  Esc=save) "
    };
    let border_col = if state.rebinding.is_some() { Color::Yellow } else { ACCENT };
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_col))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let conflicts = state.conflicts();
    let col_action_w = (inner.width as usize / 2).max(16);

    // Build items with group headers interleaved; track the visual row of the cursor.
    let mut items: Vec<ListItem> = Vec::new();
    let mut visual_cursor = 0usize;
    let mut current_group = String::new();

    for (i, b) in state.bindings.iter().enumerate() {
        if b.group != current_group {
            current_group = b.group.clone();
            let sep = "─".repeat(inner.width.saturating_sub(4) as usize);
            items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:<width$}─{sep}", b.group, width = b.group.len()),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ])));
        }

        if i == state.cursor {
            visual_cursor = items.len();
        }

        let is_cur = i == state.cursor;
        let is_conflict = conflicts.contains(&i);
        let action_style = if is_cur {
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(HEADER)
        };
        let key_str = if state.rebinding.is_some() && is_cur {
            "…waiting…".to_string()
        } else if b.key.is_empty() {
            "(unbound)".to_string()
        } else {
            b.display()
        };
        let key_style = if is_conflict {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else if is_cur {
            Style::default().fg(Color::Black).bg(ACCENT)
        } else {
            Style::default().fg(Color::White)
        };

        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!("  {:<width$}", b.action, width = col_action_w.saturating_sub(2)),
                action_style,
            ),
            Span::styled(key_str, key_style),
        ])));
    }

    let visible_h = inner.height as usize;
    let scroll = visual_cursor.saturating_sub(visible_h.saturating_sub(1));

    f.render_widget(
        List::new(items.into_iter().skip(scroll).take(visible_h).collect::<Vec<_>>())
            .style(Style::default().bg(BG)),
        inner,
    );
}

// ─── SF2 preset browser ───────────────────────────────────────────────────────

fn draw_sf2_browser(f: &mut Frame, app: &mut App, area: Rect) {
    const ACCENT: Color = Color::Rgb(31, 111, 235);
    const HEADER: Color = Color::Rgb(240, 136, 62);
    const PANEL: Color = Color::Rgb(22, 27, 34);
    const BORDER: Color = Color::Rgb(48, 54, 61);

    f.render_widget(Clear, area);

    let Some(crate::modal::Modal::Sf2Browser(state)) = &app.active_modal else { return };

    let path_str = state.path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?");
    let title = format!(" SF2: {} — Bank {} / Preset {} ", path_str, state.bank, state.preset);

    let inner = Block::default()
        .title(title.clone())
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));

    let inner_area = inner.inner(area);
    f.render_widget(inner, area);

    // Split: preset list on top, hint bar at bottom.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner_area);

    let list_area = chunks[0];
    let hint_area = chunks[1];

    let viewport = list_area.height as usize;
    let scroll = state.scroll;

    let items: Vec<ListItem> = if state.presets.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            if state.loaded {
                "  (no presets found)"
            } else {
                "  Loading preset list…"
            },
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        state.presets.iter().enumerate()
            .skip(scroll)
            .take(viewport)
            .map(|(i, (bank, preset, name))| {
                let selected = i == state.cursor;
                let style = if selected {
                    Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {:3} / {:3}  ", bank, preset), Style::default().fg(ACCENT)),
                    Span::styled(name.clone(), style),
                ]))
            })
            .collect()
    };

    let list = List::new(items)
        .style(Style::default().bg(PANEL));
    f.render_widget(list, list_area);

    // Hint bar.
    let preview_indicator = if state.preview_loaded { " ♪" } else if state.preview_slot.is_some() { " …" } else { "" };
    let hint_text = format!("↑↓=select  Space=preview{}  Enter=confirm  Esc=cancel", preview_indicator);
    let hint = Paragraph::new(hint_text)
        .style(Style::default().fg(BORDER));
    f.render_widget(hint, hint_area);
}

// ─── Plugin parameter browser ─────────────────────────────────────────────────

fn draw_plugin_params(f: &mut Frame, app: &mut App, area: Rect) {
    const PANEL: Color = Color::Rgb(22, 27, 34);
    let Some(crate::modal::Modal::PluginParams(state)) = &app.active_modal else { return };

    let title = format!(" Plugin Params: {} ", state.plugin_name);
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner_area);

    let list_area = chunks[0];
    let hint_area = chunks[1];

    let viewport = list_area.height as usize;
    let scroll    = state.scroll;

    let items: Vec<ListItem> = if state.params.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  (no parameters)",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        // Column widths: value bar (10), name (~rest), display+label
        state.params.iter().enumerate()
            .skip(scroll)
            .take(viewport)
            .map(|(i, p)| {
                let selected = i == state.cursor;
                let bar_filled = (p.value * 10.0).round() as usize;
                let bar: String = (0..10).map(|b| if b < bar_filled { '█' } else { '░' }).collect();
                let name_style = if selected {
                    Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let val_text = if p.label.is_empty() {
                    p.display.clone()
                } else {
                    format!("{} {}", p.display, p.label)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {:2} ", p.id), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{:<28}", &p.name), name_style),
                    Span::styled(format!(" [{}] ", bar), Style::default().fg(ACCENT)),
                    Span::styled(val_text, Style::default().fg(OK)),
                ]))
            })
            .collect()
    };

    let list = List::new(items).style(Style::default().bg(PANEL));
    f.render_widget(list, list_area);

    let hint = Paragraph::new("↑↓=select  ←→=nudge±1%  r=refresh  Esc=close")
        .style(Style::default().fg(BORDER));
    f.render_widget(hint, hint_area);
}

// ─── Utilities ────────────────────────────────────────────────────────────────

fn centered_rect(pct_w: u16, pct_h: u16, area: Rect) -> Rect {
    let w = (area.width * pct_w / 100).min(area.width);
    let h = (area.height * pct_h / 100).min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    Rect::new(x, y, w, h)
}
