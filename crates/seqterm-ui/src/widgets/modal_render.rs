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
/// Full-screen dim backdrop painted behind every modal so the underlying view
/// never bleeds through (modals were appearing semi-transparent otherwise).
const BACKDROP: Color = Color::Rgb(8, 10, 14);
const BORDER:  Color = Color::Rgb(58, 64, 72);
const ACCENT:  Color = Color::Rgb(31, 111, 235);
const HEADER:  Color = Color::Rgb(240, 136, 62);
const OK:      Color = Color::Rgb(56, 200, 100);
const ERR:     Color = Color::Rgb(220, 80, 80);
const SUCCESS: Color = Color::Rgb(56, 200, 100);

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Render two action buttons (OK/Cancel) at the bottom of `area` and store their rects
/// in `app.modal_ok_rect` / `app.modal_cancel_rect` for mouse-click detection.
/// Returns the height consumed (always 2: 1 gap + 1 button row).
fn render_modal_buttons(f: &mut Frame, app: &mut App, area: Rect, ok_label: &str, cancel_label: &str) {
    let ok_w     = ok_label.len() as u16 + 4; // [ label ] padding
    let cancel_w = cancel_label.len() as u16 + 4;
    const GAP: u16 = 3;
    let total_w = ok_w + GAP + cancel_w;
    let btn_y = area.y + area.height.saturating_sub(1);
    let btn_x = area.x + area.width.saturating_sub(total_w) / 2;

    let ok_rect     = Rect::new(btn_x, btn_y, ok_w, 1);
    let cancel_rect = Rect::new(btn_x + ok_w + GAP, btn_y, cancel_w, 1);
    app.modal_ok_rect.set(ok_rect);
    app.modal_cancel_rect.set(cancel_rect);

    f.render_widget(
        Paragraph::new(Span::styled(
            format!("[ {ok_label} ]"),
            Style::default().fg(Color::Black).bg(OK).add_modifier(Modifier::BOLD),
        )),
        ok_rect,
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("[ {cancel_label} ]"),
            Style::default().fg(Color::Black).bg(ERR).add_modifier(Modifier::BOLD),
        )),
        cancel_rect,
    );
}

/// Draw the active modal centred over the terminal.
/// Clears the area under it before drawing.
pub fn draw_modal(f: &mut Frame, app: &mut App, full_area: Rect) {
    // Reset mouse hit-test rects — set below only when a modal is actually drawn.
    app.modal_close_rect.set(Rect::default());
    app.modal_area.set(Rect::default());

    let Some(modal) = &app.active_modal else { return };

    // Full-screen opaque backdrop so the view underneath is fully hidden and the
    // modal does not look transparent. `Clear` resets cells to blank symbols; the
    // dim Block then paints an opaque background over the entire screen. The modal
    // itself is drawn on top by the match below.
    f.render_widget(Clear, full_area);
    f.render_widget(
        Block::default().style(Style::default().bg(BACKDROP)),
        full_area,
    );

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
            draw_confirm(f, app, title.clone(), body.clone(), area);
            render_close_btn(f, app, area);
        }
        Modal::QuitConfirm => {
            let area = centered_rect(60, 35, full_area);
            draw_shadow(f, area, full_area);
            draw_quit_confirm(f, app, area);
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
            use crate::modal::FilePickerTarget;
            let area = centered_rect(80, 82, full_area);
            draw_shadow(f, area, full_area);
            draw_file_picker(f, app, area);
            // Show Accept/Cancel buttons for the MIDI-import pickers (choosing the
            // .mid file, and choosing the SF2 for that import).
            let show_buttons = matches!(
                &app.active_modal,
                Some(Modal::FilePicker(s)) if matches!(
                    s.target,
                    FilePickerTarget::AssignSf2ForMidiImport | FilePickerTarget::ImportMidi
                )
            );
            if show_buttons {
                render_modal_buttons(f, app, area, "Aceptar", "Cancelar");
            }
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
            let area = centered_rect(60, 65, full_area);
            draw_shadow(f, area, full_area);
            draw_audio_settings(f, app, area);
            render_modal_buttons(f, app, area, "Apply", "Cancel");
            render_close_btn(f, app, area);
        }
        Modal::MidiSettings(_) => {
            let area = centered_rect(70, 75, full_area);
            draw_shadow(f, area, full_area);
            draw_midi_settings(f, app, area);
            render_modal_buttons(f, app, area, "OK", "Cancel");
            render_close_btn(f, app, area);
        }
        Modal::CommandPalette(_) => {
            let area = centered_rect(70, 60, full_area);
            draw_shadow(f, area, full_area);
            draw_command_palette(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::MidiImportOptions(_) => {
            let area = centered_rect(60, 50, full_area);
            draw_shadow(f, area, full_area);
            draw_midi_import_options(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::KeybindingsEditor(_) => {
            let area = centered_rect(70, 83, full_area);
            draw_shadow(f, area, full_area);
            draw_keybindings_editor(f, app, area);
            render_modal_buttons(f, app, area, "Save", "Cancel");
            render_close_btn(f, app, area);
        }
        Modal::AudioExportOptions(_) => {
            let area = centered_rect(50, 40, full_area);
            draw_shadow(f, area, full_area);
            draw_audio_export_options(f, app, area);
            render_modal_buttons(f, app, area, "Export", "Cancel");
            render_close_btn(f, app, area);
        }
        Modal::Sf2Browser(_) => {
            let area = centered_rect(60, 73, full_area);
            draw_shadow(f, area, full_area);
            draw_sf2_browser(f, app, area);
            render_modal_buttons(f, app, area, "Select", "Cancel");
            render_close_btn(f, app, area);
        }
        Modal::PluginParams(_) => {
            let area = centered_rect(70, 70, full_area);
            draw_shadow(f, area, full_area);
            draw_plugin_params(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::SourcePicker(_) => {
            let area = centered_rect(55, 58, full_area);
            draw_shadow(f, area, full_area);
            draw_source_picker(f, app, area);
            render_modal_buttons(f, app, area, "Confirm", "Cancel");
            render_close_btn(f, app, area);
        }
        Modal::FxPicker(_) => {
            let area = centered_rect(55, 70, full_area);
            draw_shadow(f, area, full_area);
            draw_fx_picker(f, app, area);
            render_modal_buttons(f, app, area, "Select", "Cancel");
            render_close_btn(f, app, area);
        }
        Modal::PatternPicker(_) => {
            let area = centered_rect(50, 70, full_area);
            draw_shadow(f, area, full_area);
            draw_pattern_picker(f, app, area);
            render_modal_buttons(f, app, area, "Assign", "Cancel");
            render_close_btn(f, app, area);
        }
        Modal::AudioEdit(_) => {
            let area = centered_rect(80, 70, full_area);
            draw_shadow(f, area, full_area);
            draw_audio_edit(f, app, area);
            render_modal_buttons(f, app, area, "Apply", "Cancel");
            render_close_btn(f, app, area);
        }
        Modal::Tutorial(_) => {
            let area = centered_rect(65, 55, full_area);
            draw_shadow(f, area, full_area);
            draw_tutorial(f, app, area);
            render_close_btn(f, app, area);
        }
        Modal::LuaRepl(_) => {
            let area = centered_rect(80, 70, full_area);
            draw_shadow(f, area, full_area);
            draw_lua_repl(f, app, area);
            render_close_btn(f, app, area);
        }
    }

    // ── Opacity pass ────────────────────────────────────────────────────────
    // `Clear` resets cells to `Color::Reset`, which renders *transparent* on
    // terminals with background transparency — making modal panels look see-
    // through and their options unreadable. Replace any leftover Reset background
    // on screen with a solid colour so every modal is fully opaque.
    let buf = f.buffer_mut();
    for y in full_area.top()..full_area.bottom() {
        for x in full_area.left()..full_area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                if cell.bg == Color::Reset {
                    cell.set_bg(BG);
                }
            }
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

fn draw_confirm(f: &mut Frame, app: &mut App, title: String, body: String, area: Rect) {
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
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(inner);

    f.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::White).bg(BG)),
        chunks[0],
    );

    // Render clickable buttons row.
    let btn_area = chunks[1];
    // Center two buttons: "[ ✓  Yes ]" (11 chars) + gap (4) + "[ ✗  Cancel ]" (13 chars) = 28 chars
    const YES_W:    u16 = 11;
    const NO_W:     u16 = 13;
    const GAP:      u16 = 4;
    let total_w = YES_W + GAP + NO_W;
    let btn_y   = btn_area.y + btn_area.height.saturating_sub(2) / 2;
    let btn_x   = btn_area.x + btn_area.width.saturating_sub(total_w) / 2;

    let yes_rect = Rect::new(btn_x, btn_y, YES_W, 1);
    let no_rect  = Rect::new(btn_x + YES_W + GAP, btn_y, NO_W, 1);
    app.confirm_yes_rect.set(yes_rect);
    app.confirm_no_rect.set(no_rect);

    f.render_widget(
        Paragraph::new(Span::styled(
            "[ ✓  Yes  ]",
            Style::default().fg(Color::Black).bg(OK).add_modifier(Modifier::BOLD),
        )),
        yes_rect,
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            "[ ✗  Cancel ]",
            Style::default().fg(Color::Black).bg(ERR).add_modifier(Modifier::BOLD),
        )),
        no_rect,
    );
}

// ─── QuitConfirm ─────────────────────────────────────────────────────────────

fn draw_quit_confirm(f: &mut Frame, app: &mut App, area: Rect) {
    use ratatui::text::Line;
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Exit SeqTerm ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(3)])
        .split(inner);

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "You have unsaved changes.",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(Span::styled(
                "What would you like to do?",
                Style::default().fg(Color::White),
            )),
        ])
        .wrap(Wrap { trim: true })
        .style(Style::default().bg(BG)),
        chunks[0],
    );

    // Three buttons: [ 💾 Save & Exit ] (18) + gap(2) + [ Exit ] (8) + gap(2) + [ Cancel ] (10)
    const SAVE_W:   u16 = 18;
    const EXIT_W:   u16 = 8;
    const CANCEL_W: u16 = 10;
    const GAP:      u16 = 2;
    let total_w = SAVE_W + GAP + EXIT_W + GAP + CANCEL_W;
    let btn_area = chunks[1];
    let btn_y = btn_area.y + btn_area.height.saturating_sub(2) / 2;
    let btn_x = btn_area.x + btn_area.width.saturating_sub(total_w) / 2;

    let save_rect   = Rect::new(btn_x, btn_y, SAVE_W, 1);
    let exit_rect   = Rect::new(btn_x + SAVE_W + GAP, btn_y, EXIT_W, 1);
    let cancel_rect = Rect::new(btn_x + SAVE_W + GAP + EXIT_W + GAP, btn_y, CANCEL_W, 1);
    app.quit_save_rect.set(save_rect);
    app.quit_nosave_rect.set(exit_rect);
    app.quit_cancel_rect.set(cancel_rect);

    f.render_widget(
        Paragraph::new(Span::styled(
            "[ 💾 Save & Exit ]",
            Style::default().fg(Color::Black).bg(OK).add_modifier(Modifier::BOLD),
        )),
        save_rect,
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            "[ Exit ]",
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        exit_rect,
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            "[ Cancel ]",
            Style::default().fg(Color::Black).bg(ERR).add_modifier(Modifier::BOLD),
        )),
        cancel_rect,
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

    // Layout: hint | input box | button row
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3), Constraint::Length(2)])
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

    // Clickable OK / Cancel buttons.
    render_modal_buttons(f, app, chunks[2], "OK", "Cancel");
}

// ─── File picker ──────────────────────────────────────────────────────────────

fn draw_file_picker(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::modal::{SidebarItemKind, Modal as M};

    let Some(M::FilePicker(_)) = &app.active_modal else { return };
    f.render_widget(Clear, area);

    // ── Outer block ───────────────────────────────────────────────────────────
    let title = {
        let Some(M::FilePicker(s)) = &app.active_modal else { return };
        format!(" {} ", s.target.title())
    };
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width < 30 || inner.height < 4 { return; }

    // ── Horizontal split: sidebar | separator | content ───────────────────────
    const SIDEBAR_W: u16 = 22;
    let hchunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(SIDEBAR_W.min(inner.width / 3)),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);
    let sidebar_area  = hchunks[0];
    let sep_area      = hchunks[1];
    let content_area  = hchunks[2];

    app.file_picker_sidebar_area.set(sidebar_area);

    // ── Separator ─────────────────────────────────────────────────────────────
    {
        let Some(M::FilePicker(s)) = &app.active_modal else { return };
        let sep_col = if s.tree_focused { ACCENT } else { BORDER };
        for y in 0..sep_area.height {
            f.render_widget(
                Paragraph::new("│").style(Style::default().fg(sep_col)),
                Rect::new(sep_area.x, sep_area.y + y, 1, 1),
            );
        }
    }

    // ── Sidebar ───────────────────────────────────────────────────────────────
    {
        let Some(M::FilePicker(s)) = &mut app.active_modal else { return };
        let visible_h = sidebar_area.height as usize;
        s.clamp_sidebar_scroll(visible_h);
        let scroll = s.sidebar_scroll;
        let cursor = s.sidebar_cursor;
        let focused = s.tree_focused;

        let items: Vec<ListItem> = s.sidebar.iter()
            .enumerate()
            .skip(scroll)
            .take(visible_h)
            .map(|(abs_i, entry)| {
                match entry.kind {
                    SidebarItemKind::Header => ListItem::new(Line::from(Span::styled(
                        entry.label.clone(),
                        Style::default().fg(Color::Rgb(90, 110, 150)).add_modifier(Modifier::BOLD),
                    ))),
                    _ => {
                        let is_sel = abs_i == cursor;
                        let indent = "  ".repeat(entry.depth.min(5));
                        let (prefix, base_col) = match entry.kind {
                            SidebarItemKind::Bookmark    => (" ", Color::Rgb(180, 200, 230)),
                            SidebarItemKind::Recent      => (" ", Color::Rgb(160, 180, 210)),
                            SidebarItemKind::TreeAncestor => ("▼ ", Color::Rgb(130, 160, 200)),
                            SidebarItemKind::TreeCurrent  => ("● ", Color::Rgb(56, 200, 130)),
                            SidebarItemKind::TreeChild    => ("▶ ", Color::Rgb(180, 200, 230)),
                            SidebarItemKind::Header       => unreachable!(),
                        };
                        // Truncate label to fit sidebar width.
                        let avail = (sidebar_area.width as usize)
                            .saturating_sub(indent.len() + prefix.len() + 1);
                        let label: String = entry.label.chars().take(avail.max(1)).collect();
                        let text = format!("{indent}{prefix}{label}");
                        let style = if is_sel && focused {
                            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
                        } else if is_sel {
                            Style::default().fg(Color::White).bg(Color::Rgb(35, 45, 60))
                        } else if entry.kind == SidebarItemKind::TreeCurrent {
                            Style::default().fg(base_col).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(base_col)
                        };
                        ListItem::new(Line::from(Span::styled(text, style)))
                    }
                }
            })
            .collect();
        f.render_widget(List::new(items).style(Style::default().bg(BG)), sidebar_area);
    }

    // ── Content area: vertical chunks ─────────────────────────────────────────
    let (show_input, show_search, tree_focused, input_focused) = {
        let Some(M::FilePicker(s)) = &app.active_modal else { return };
        let m = s.target.mode();
        (m == FilePickerMode::Save, m == FilePickerMode::Open, s.tree_focused, s.input_focused)
    };

    let content_constraints = if show_input {
        vec![
            Constraint::Length(1),  // dir
            Constraint::Min(4),     // file list
            Constraint::Length(1),  // separator
            Constraint::Length(1),  // filename input
            Constraint::Length(1),  // hint
        ]
    } else if show_search {
        vec![
            Constraint::Length(1),  // dir
            Constraint::Length(1),  // search bar
            Constraint::Min(4),     // file list
            Constraint::Length(1),  // hint
        ]
    } else {
        vec![
            Constraint::Length(1),  // dir
            Constraint::Min(4),     // file list
            Constraint::Length(1),  // hint
        ]
    };

    let cchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(content_constraints)
        .split(content_area);

    // Current directory row.
    {
        let Some(M::FilePicker(s)) = &app.active_modal else { return };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" DIR: ", Style::default().fg(HEADER)),
                Span::styled(
                    truncate_path(&s.current_dir, content_area.width.saturating_sub(7) as usize),
                    Style::default().fg(Color::White),
                ),
            ])),
            cchunks[0],
        );
    }

    // Search bar (Open mode only).
    let list_chunk_idx = if show_search {
        let Some(M::FilePicker(s)) = &app.active_modal else { return };
        let search_active = !s.search_input.is_empty();
        let (label_col, value_col) = if search_active {
            (HEADER, Color::Yellow)
        } else {
            (BORDER, Color::DarkGray)
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" SEARCH: ", Style::default().fg(label_col)),
                Span::styled(
                    format!("{}{}", s.search_input, if search_active { "█" } else { "_" }),
                    Style::default().fg(value_col),
                ),
                Span::styled(
                    if search_active {
                        format!("  ({} matches)", s.visible_entries().len())
                    } else {
                        "  (type to filter)".to_string()
                    },
                    Style::default().fg(BORDER),
                ),
            ])),
            cchunks[1],
        );
        2usize
    } else {
        1usize
    };

    // File list.
    {
        let list_focus_col = if (show_input && input_focused) || tree_focused { BORDER } else { ACCENT };
        let list_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(list_focus_col))
            .style(Style::default().bg(BG));
        let list_inner = list_block.inner(cchunks[list_chunk_idx]);
        app.file_picker_list_area.set(list_inner);
        f.render_widget(list_block, cchunks[list_chunk_idx]);

        let Some(M::FilePicker(s)) = &mut app.active_modal else { return };
        let visible = list_inner.height as usize;
        s.clamp_scroll(visible);
        let visible_entries: Vec<_> = s.visible_entries();
        let items: Vec<ListItem> = visible_entries
            .iter()
            .skip(s.scroll)
            .take(visible)
            .enumerate()
            .map(|(rel_i, entry)| {
                let abs_i = rel_i + s.scroll;
                let is_sel = abs_i == s.cursor;
                let (icon, col) = if entry.is_dir { ("▶ ", Color::Cyan) } else { ("  ", Color::White) };
                let style = if is_sel {
                    Style::default().fg(Color::Black).bg(Color::Rgb(56, 139, 253))
                } else {
                    Style::default().fg(col)
                };
                ListItem::new(Line::from(Span::styled(format!("{icon}{}", entry.name), style)))
            })
            .collect();
        f.render_widget(List::new(items).style(Style::default().bg(BG)), list_inner);
    }

    // Save-mode filename input.
    let hint_idx = if show_input { 4 } else if show_search { 3 } else { 2 };
    if show_input {
        let Some(M::FilePicker(s)) = &app.active_modal else { return };
        let input_focus_col = if s.input_focused { ACCENT } else { BORDER };
        let input_style = if s.input_focused {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                "─".repeat(content_area.width as usize),
                Style::default().fg(input_focus_col),
            )),
            cchunks[2],
        );
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Filename: ", Style::default().fg(if s.input_focused { HEADER } else { BORDER })),
                Span::styled(s.filename_input.clone() + "█", input_style),
            ])),
            cchunks[3],
        );
    }

    // Hint bar.
    let hint = if show_input {
        "  Tab=filename  ↑↓=nav  Enter=save  Backspace=up  Esc=cancel"
    } else {
        "  Tab=sidebar  ↑↓=nav  Enter=open  Backspace=up  Esc=cancel"
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(BORDER))),
        cchunks[hint_idx],
    );
}

fn truncate_path(path: &std::path::Path, max_len: usize) -> String {
    let s = path.display().to_string();
    if s.len() <= max_len { return s; }
    format!("…{}", &s[s.len().saturating_sub(max_len.saturating_sub(1))..])
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

    // When "AUTO", show what the engine will actually use.
    let pw_running = seqterm_audio_engine::pipewire_is_running();
    let detected_backend = if s.backend.to_uppercase() == "AUTO" {
        if pw_running {
            Some("PipeWire-JACK")
        } else if std::process::Command::new("jack_lsp")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            Some("JACK")
        } else {
            Some("ALSA (CPAL)")
        }
    } else {
        None
    };

    // Base rows always shown.
    let backend_display = if let Some(detected) = detected_backend {
        format!("{} → {}", s.backend, detected)
    } else {
        s.backend.clone()
    };
    // Show whether the build can actually run FluidSynth (feature compiled in).
    let sf2_engine_display = {
        let want_fluid = s.sf2_backend.eq_ignore_ascii_case("fluidsynth");
        if want_fluid && !seqterm_audio_engine::fluidsynth_available() {
            "fluidsynth (not built → oxisynth)".to_string()
        } else {
            s.sf2_backend.clone()
        }
    };
    let mut rows: Vec<(&'static str, String, usize)> = vec![
        ("Backend",     backend_display,                     0),
        ("Device",      s.device.clone(),                    1),
        ("Sample rate", format!("{} Hz", s.sample_rate),     2),
        ("Buffer size", format!("{} samples", s.buffer_size), 3),
        ("SF2 engine",  sf2_engine_display,                  4),
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
        "PIPEWIRE" | "AUTO" => {
            let q = if s.pipewire_quantum == 0 { "system".to_string() } else { s.pipewire_quantum.to_string() };
            rows.push(("PW quantum",   q,   usize::MAX));
        }
        "WASAPI" => {
            rows.push(("WASAPI excl.", if s.wasapi_exclusive { "On" } else { "Off" }.to_string(), usize::MAX));
        }
        _ => {}
    }

    let hint = match state.cursor {
        0 => "  ←→ = cycle backend (AUTO/JACK/PIPEWIRE/ALSA)  |  Enter=save  Esc=cancel",
        1 => "  ←→ = cycle device  |  Enter=save  Esc=cancel",
        4 => "  ←→ = SF2 engine (oxisynth/fluidsynth)  |  Enter=save  Esc=cancel",
        _ => "  ↑↓=select  ←→=adjust  |  Enter=save  Esc=cancel",
    };
    let mut lines = vec![Line::from(Span::styled(hint, Style::default().fg(BORDER)))];

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

fn draw_midi_settings(f: &mut Frame, app: &mut App, area: Rect) {
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

    // Record tab click rects — each tab occupies (label.len() + 3) columns: " label │"
    {
        let tab_y  = chunks[0].y;
        let tab_h  = 1u16;
        let mut x  = chunks[0].x;
        let mut rects = [ratatui::layout::Rect::default(); 4];
        for (i, l) in tab_labels.iter().enumerate() {
            let w = (l.len() as u16) + 3; // " label │"
            rects[i] = ratatui::layout::Rect::new(x, tab_y, w, tab_h);
            x += w;
        }
        app.midi_settings_tab_rects.set(rects);
    }
    // Record list area for row clicks.
    app.midi_settings_list_rect.set(chunks[1]);

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
                    "  No bindings yet. l=Vol p=Pan a=SendA b=SendB g=BPM (selected channel).",
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
        3 => "  l=Vol p=Pan a=SendA b=SendB g=BPM  Del=clear  Tab=next  Esc=close",
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

fn draw_midi_import_options(f: &mut Frame, app: &mut App, area: Rect) {
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

    // Layout: options (8 lines) | buttons (2) | track preview (rest).
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Length(2), Constraint::Min(0)])
        .split(inner);

    let o = &state.opts;
    let bars_label = if o.bars_per_pattern == 0 {
        "Full piece".to_string()
    } else {
        format!("{} bars", o.bars_per_pattern)
    };
    let steps_label = match o.steps_per_beat {
        4 => "4 (16th notes)",
        8 => "8 (32nd notes)",
        _ => "?",
    };
    let drums_label = if o.detect_drums { "On" } else { "Off" };
    let sf2_label: String = o.sf2_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(|n| n.chars().take(24).collect())
        .unwrap_or_else(|| "(none — MIDI out)".to_string());

    let mut opt_lines = vec![
        Line::from(Span::styled(
            "  ↑↓=select  ←→=adjust  Enter=browse SF2",
            Style::default().fg(BORDER),
        )),
        Line::from(""),
    ];
    let rows: &[(&str, usize)] = &[
        ("Bars per pattern", 0),
        ("Steps per beat",   1),
        ("Drum detection",   2),
        ("SF2 synth",        3),
    ];
    let values = [bars_label.as_str(), steps_label, drums_label, sf2_label.as_str()];
    let notes  = ["", "", "", "Enter=browse  ←=clear"];

    for (i, (label, idx)) in rows.iter().enumerate() {
        let is_cur = state.cursor == *idx;
        let vstyle = if is_cur {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(if i == 3 && o.sf2_path.is_some() { Color::Rgb(56, 200, 100) } else { Color::White })
        };
        let prefix = if is_cur { " ▶ " } else { "   " };
        let mut spans = vec![
            Span::styled(format!("{prefix}{:<20} ", label), Style::default().fg(HEADER)),
            Span::styled(values[i].to_string(), vstyle),
        ];
        if is_cur && !notes[i].is_empty() {
            spans.push(Span::styled(format!("  {}", notes[i]), Style::default().fg(BORDER)));
        }
        opt_lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(opt_lines).style(Style::default().bg(BG)), sections[0]);

    // Import / Cancel buttons — drop the borrow on `state` first.
    let (track_infos, sf2_set) = {
        let Some(Modal::MidiImportOptions(s)) = &app.active_modal else { return };
        (s.track_infos.clone(), s.opts.sf2_path.is_some())
    };
    render_modal_buttons(f, app, sections[1], "Import", "Cancel");

    // Track preview — shows GM preset when SF2 is selected.
    if !track_infos.is_empty() {
        let track_block = Block::default()
            .title(if sf2_set { " Tracks + GM Assignments " } else { " Detected Tracks " })
            .title_style(Style::default().fg(HEADER))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(BG));
        let track_inner = track_block.inner(sections[2]);
        f.render_widget(track_block, sections[2]);

        let header = Line::from(vec![
            Span::styled(format!("{:<14}", "Name"), Style::default().fg(BORDER).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {:>2}", "Ch"), Style::default().fg(BORDER)),
            Span::styled(format!(" {:>5}", "Notes"), Style::default().fg(BORDER)),
            if sf2_set {
                Span::styled("  GM Preset (auto)", Style::default().fg(BORDER))
            } else {
                Span::styled("  Type", Style::default().fg(BORDER))
            },
        ]);
        let mut items: Vec<ListItem> = vec![ListItem::new(header)];
        for ti in &track_infos {
            let preset_info = if sf2_set {
                let name = seqterm_midi_io::gm_preset_name(ti.channel, ti.program);
                let (bank, preset) = seqterm_midi_io::gm_sf2_preset(ti.channel, ti.program);
                format!("  B{} P{:>3}  {}", bank, preset, name)
            } else {
                format!("  {}", if ti.is_drum { "Drums" } else { "Melodic" })
            };
            let line = Line::from(vec![
                Span::styled(format!("{:<14}", &ti.name[..ti.name.len().min(14)]), Style::default().fg(Color::White)),
                Span::styled(format!(" {:>2}", ti.channel + 1), Style::default().fg(Color::Cyan)),
                Span::styled(format!(" {:>5}", ti.note_count), Style::default().fg(Color::Yellow)),
                Span::styled(preset_info, Style::default().fg(
                    if ti.is_drum { Color::Magenta } else { Color::Rgb(56, 200, 100) }
                )),
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
    const ACCENT:    Color = Color::Rgb(31, 111, 235);
    const HEADER:    Color = Color::Rgb(240, 136, 62);
    const PANEL:     Color = Color::Rgb(22, 27, 34);
    const BORDER:    Color = Color::Rgb(48, 54, 61);
    const ARROW_BG:  Color = Color::Rgb(40, 60, 100);
    const ARROW_ACT: Color = Color::Rgb(80, 130, 220);

    f.render_widget(Clear, area);

    let Some(crate::modal::Modal::Sf2Browser(state)) = &app.active_modal else { return };

    let path_str = state.path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?");
    let title = format!(" SF2: {} ", path_str);

    let outer = Block::default()
        .title(title)
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));

    let inner_area = outer.inner(area);
    f.render_widget(outer, area);

    // Layout: bank bar (3) + separator (1) + preset list (min) + buttons (1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // bank combobox
            Constraint::Length(1), // separator
            Constraint::Min(1),    // preset list
            Constraint::Length(1), // buttons
        ])
        .split(inner_area);

    let bank_area = chunks[0];
    let sep_area  = chunks[1];
    let list_area = chunks[2];
    let btn_area  = chunks[3];

    // ── Bank combobox ─────────────────────────────────────────────────────────
    let n_banks  = state.banks.len();
    let bank_idx = state.bank_cursor;
    let bank_val = state.selected_bank();

    let bank_block = Block::default()
        .title(" BANK ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));
    let bank_inner = bank_block.inner(bank_area);
    f.render_widget(bank_block, bank_area);

    // ◄  Bank NNN  ►   (N / total banks)
    // Store arrow rects for mouse clicks.
    let arrow_l = Rect::new(bank_inner.x, bank_inner.y, 2, 1);
    let arrow_r = Rect::new(bank_inner.x + bank_inner.width.saturating_sub(2), bank_inner.y, 2, 1);
    app.sf2_bank_left_rect.set(arrow_l);
    app.sf2_bank_right_rect.set(arrow_r);

    // "♪ A3" audition button at the right end of the bank row — plays the
    // selected preset's sound at note A3.
    let a3_lbl = " ♪ A3 ";
    let a3_w = a3_lbl.chars().count() as u16;
    if bank_inner.width > a3_w + 14 {
        let a3_x = bank_inner.x + bank_inner.width.saturating_sub(a3_w + 2);
        let a3_rect = Rect::new(a3_x, bank_inner.y, a3_w, 1);
        app.sf2_a3_btn_rect.set(a3_rect);
        let a3_style = if state.preview_slot.is_some() {
            Style::default().fg(Color::Black).bg(OK).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(OK).add_modifier(Modifier::BOLD)
        };
        f.render_widget(Paragraph::new(Span::styled(a3_lbl, a3_style)), a3_rect);
    } else {
        app.sf2_a3_btn_rect.set(Rect::default());
    }

    f.render_widget(
        Paragraph::new(Span::styled("◄ ", Style::default().fg(ARROW_ACT).bg(ARROW_BG).add_modifier(Modifier::BOLD))),
        arrow_l,
    );
    let mid_w = bank_inner.width.saturating_sub(4) as usize;
    let mid_label = format!(
        " Bank {:>3}  ({}/{}) ",
        bank_val,
        if n_banks == 0 { 0 } else { bank_idx + 1 },
        n_banks,
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("{:^width$}", mid_label, width = mid_w),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Rect::new(bank_inner.x + 2, bank_inner.y, bank_inner.width.saturating_sub(4), 1),
    );
    f.render_widget(
        Paragraph::new(Span::styled(" ►", Style::default().fg(ARROW_ACT).bg(ARROW_BG).add_modifier(Modifier::BOLD))),
        arrow_r,
    );

    // ── Separator ────────────────────────────────────────────────────────────
    f.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(inner_area.width as usize),
            Style::default().fg(BORDER),
        )),
        sep_area,
    );

    // ── Filtered preset list ─────────────────────────────────────────────────
    let filtered = state.filtered_presets();
    let viewport = list_area.height as usize;
    let scroll   = state.scroll;

    app.sf2_list_rect.set(list_area);

    let items: Vec<ListItem> = if filtered.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            if state.loaded { "  (no presets in this bank)" } else { "  Loading preset list…" },
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        filtered.iter().enumerate()
            .skip(scroll)
            .take(viewport)
            .map(|(i, (bank, preset, name))| {
                let selected = i == state.cursor;
                let hi = Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD);
                let lo = Style::default().fg(Color::White);
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" P{:>3}  ", preset),
                        if selected { Style::default().fg(Color::Black).bg(Color::Yellow) }
                        else { Style::default().fg(ACCENT) },
                    ),
                    Span::styled(name.clone(), if selected { hi } else { lo }),
                    Span::styled(
                        format!("  B{}", bank),
                        if selected { Style::default().fg(Color::Black).bg(Color::Yellow) }
                        else { Style::default().fg(BORDER) },
                    ),
                ]))
            })
            .collect()
    };

    f.render_widget(List::new(items).style(Style::default().bg(PANEL)), list_area);

    // ── Accept / Cancel buttons ───────────────────────────────────────────────
    let preview_lbl = if state.preview_loaded { " ♪  Accept" }
                      else if state.preview_slot.is_some() { " …  Accept" }
                      else { "Accept" };
    render_modal_buttons(f, app, btn_area, preview_lbl, "Cancel");
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

// ─── Source picker ────────────────────────────────────────────────────────────

fn draw_source_picker(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::modal::{Modal, SourceKind};
    const PANEL: Color = Color::Rgb(18, 22, 30);
    const DIM:   Color = Color::Rgb(55, 65, 81);

    // Extract a snapshot so we can mutably borrow app.active_modal at the end.
    let (row, col, cursor, midi_ports, port_cursor, current_label) = {
        let Some(Modal::SourcePicker(s)) = &app.active_modal else { return };
        (s.row, s.col, s.cursor, s.midi_ports.clone(), s.port_cursor, s.current_source_label.clone())
    };
    let state_cursor   = cursor;
    let state_ports    = &midi_ports;
    let state_port_cur = port_cursor;
    let row_lbl = ((b'A' + row as u8) as char).to_string();
    let pos_lbl = format!("{}{}", row_lbl, col + 1);

    let block = Block::default()
        .title(format!(" SOURCE  ·  {} ", pos_lbl))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Now: ", Style::default().fg(DIM)),
            Span::styled(current_label.as_str(), Style::default().fg(Color::White)),
        ])).style(Style::default().bg(PANEL)),
        vchunks[0],
    );
    f.render_widget(
        Paragraph::new(Span::styled("─".repeat(inner.width as usize), Style::default().fg(DIM))),
        vchunks[1],
    );

    let opt_area = vchunks[2];
    let third_h = (opt_area.height / 3).max(3);
    let opt_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(third_h),
            Constraint::Length(third_h),
            Constraint::Min(2),
        ])
        .split(opt_area);

    let options = [
        (SourceKind::Midi,  "MIDI",  "m", "⇄", "Yoshimi, ZynAddSubFX, hardware synth…"),
        (SourceKind::Sf2,   "SF2",   "f", "♪", "Built-in SoundFont (.sf2) synthesis"),
        (SourceKind::Audio, "AUDIO", "a", "▶", "Sample — WAV, FLAC, MP3, OGG"),
    ];

    let mut new_option_rects = [Rect::default(); 3];
    let mut new_port_rects: Vec<Rect> = Vec::new();

    for (i, (kind, label, key, icon, desc)) in options.iter().enumerate() {
        let selected = state_cursor == *kind;
        let opt_rect = opt_chunks[i];
        new_option_rects[i] = opt_rect;

        let (bg, fg_lbl, fg_desc, border_col) = if selected {
            (Color::Rgb(28, 55, 90), Color::Yellow, Color::Rgb(200, 220, 255), ACCENT)
        } else {
            (PANEL, Color::Rgb(140, 160, 200), DIM, DIM)
        };

        let blk = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(bg));
        let blk_inner = blk.inner(opt_rect);
        f.render_widget(blk, opt_rect);
        if blk_inner.height == 0 { continue; }

        let header = Line::from(vec![
            Span::styled(format!(" {} {} ", icon, label),
                Style::default().fg(fg_lbl).add_modifier(Modifier::BOLD)),
            Span::styled(format!("[{}] ", key),
                Style::default().fg(if selected { Color::Yellow } else { DIM })),
            Span::styled(desc.to_string(), Style::default().fg(fg_desc)),
        ]);
        f.render_widget(Paragraph::new(vec![header]).style(Style::default().bg(bg)), blk_inner);

        // MIDI block: port list inline when selected.
        if selected && *kind == SourceKind::Midi && blk_inner.height > 1 {
            let ports = state_ports;
            let avail_h = blk_inner.height.saturating_sub(1);
            let port_area = Rect {
                x: blk_inner.x + 2, y: blk_inner.y + 1,
                width: blk_inner.width.saturating_sub(3),
                height: avail_h.min(ports.len().max(1) as u16),
            };
            let port_lines: Vec<Line> = if ports.is_empty() {
                vec![Line::from(Span::styled("  (no MIDI ports)", Style::default().fg(DIM)))]
            } else {
                ports.iter().enumerate().map(|(pi, p)| {
                    let pr = Rect { x: port_area.x, y: port_area.y + pi as u16,
                                    width: port_area.width, height: 1 };
                    if pi < avail_h as usize { new_port_rects.push(pr); }
                    let is_sel = pi == state_port_cur;
                    Line::from(Span::styled(
                        format!(" {} {}", if is_sel { "▶" } else { " " },
                            p.chars().take(port_area.width.saturating_sub(3) as usize).collect::<String>()),
                        if is_sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) }
                        else { Style::default().fg(Color::Rgb(160, 180, 220)) },
                    ))
                }).collect()
            };
            f.render_widget(Paragraph::new(port_lines).style(Style::default().bg(bg)), port_area);
        }
    }

    // Persist rects (mutable borrow — safe since no immutable borrow of active_modal alive).
    if let Some(Modal::SourcePicker(s)) = &mut app.active_modal {
        s.option_rects = new_option_rects;
        s.port_rects   = new_port_rects;
    }

    let hint = match state_cursor {
        SourceKind::Midi  => "  ↑↓=option  ←→=port  Enter=confirm  Esc=cancel",
        SourceKind::Sf2   => "  ↑↓=option  Enter=browse SF2  Esc=cancel",
        SourceKind::Audio => "  ↑↓=option  Enter=browse file  Esc=cancel",
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(DIM)))
            .style(Style::default().bg(PANEL)),
        vchunks[3],
    );
}

fn draw_fx_picker(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::modal::Modal;
    const PANEL: Color = Color::Rgb(18, 22, 30);
    const DIM:   Color = Color::Rgb(55, 65, 81);

    let (slot_id, cursor, scroll, labels) = {
        let Some(Modal::FxPicker(s)) = &app.active_modal else { return };
        let labels: Vec<String> = s.entries.iter().map(|e| e.label()).collect();
        (s.slot_id, s.cursor, s.scroll, labels)
    };

    let block = Block::default()
        .title(format!(" FX / PLUGIN  ·  slot {} ", slot_id))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    let list_area = vchunks[0];

    // Keep cursor visible.
    let rows = list_area.height as usize;
    let mut scroll = scroll;
    if cursor < scroll { scroll = cursor; }
    else if rows > 0 && cursor >= scroll + rows { scroll = cursor + 1 - rows; }

    let mut row_rects: Vec<Rect> = Vec::new();
    for (i, label) in labels.iter().enumerate().skip(scroll).take(rows) {
        let y = list_area.y + (i - scroll) as u16;
        let row_rect = Rect { x: list_area.x, y, width: list_area.width, height: 1 };
        row_rects.push(row_rect);
        let selected = i == cursor;
        let (bg, fg) = if selected {
            (Color::Rgb(28, 55, 90), Color::Yellow)
        } else {
            (PANEL, Color::Rgb(170, 185, 215))
        };
        let prefix = if selected { "▶ " } else { "  " };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(if selected { Color::Yellow } else { DIM })),
                Span::styled(label.as_str(),
                    Style::default().fg(fg).add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() })),
            ])).style(Style::default().bg(bg)),
            row_rect,
        );
    }

    if let Some(Modal::FxPicker(s)) = &mut app.active_modal {
        s.scroll = scroll;
        s.row_rects = row_rects;
    }

    f.render_widget(
        Paragraph::new(Span::styled(
            "  ↑↓=move  Enter/2×click=select  Esc=cancel",
            Style::default().fg(DIM),
        )).style(Style::default().bg(PANEL)),
        vchunks[1],
    );
}

fn draw_pattern_picker(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::modal::Modal;
    const PANEL: Color = Color::Rgb(18, 22, 30);
    const DIM:   Color = Color::Rgb(55, 65, 81);

    let (row, col, cursor, scroll, patterns) = {
        let Some(Modal::PatternPicker(s)) = &app.active_modal else { return };
        (s.row, s.col, s.cursor, s.scroll, s.patterns.clone())
    };
    let cell = format!("{}{}", (b'A' + row as u8) as char, col + 1);

    let block = Block::default()
        .title(format!(" PATTERN → CLIP {} ", cell))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    let list_area = vchunks[0];

    let rows = list_area.height as usize;
    let mut scroll = scroll;
    if cursor < scroll { scroll = cursor; }
    else if rows > 0 && cursor >= scroll + rows { scroll = cursor + 1 - rows; }

    let mut row_rects: Vec<Rect> = Vec::new();
    for (i, name) in patterns.iter().enumerate().skip(scroll).take(rows) {
        let y = list_area.y + (i - scroll) as u16;
        let rect = Rect { x: list_area.x, y, width: list_area.width, height: 1 };
        row_rects.push(rect);
        let selected = i == cursor;
        let (bg, fg) = if selected {
            (Color::Rgb(28, 55, 90), Color::Yellow)
        } else {
            (PANEL, Color::Rgb(170, 185, 215))
        };
        let prefix = if selected { "▶ " } else { "  " };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(if selected { Color::Yellow } else { DIM })),
                Span::styled(name.clone(),
                    Style::default().fg(fg).add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() })),
            ])).style(Style::default().bg(bg)),
            rect,
        );
    }

    if let Some(Modal::PatternPicker(s)) = &mut app.active_modal {
        s.scroll = scroll;
        s.row_rects = row_rects;
    }

    f.render_widget(
        Paragraph::new(Span::styled(
            "  ↑↓=move  Enter/2×click=assign  Esc=cancel",
            Style::default().fg(DIM),
        )).style(Style::default().bg(PANEL)),
        vchunks[1],
    );
}

// ─── Utilities ────────────────────────────────────────────────────────────────

fn centered_rect(pct_w: u16, pct_h: u16, area: Rect) -> Rect {
    let w = (area.width * pct_w / 100).min(area.width);
    let h = (area.height * pct_h / 100).min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    Rect::new(x, y, w, h)
}

// ─── Audio Clip Editor ────────────────────────────────────────────────────────

fn draw_audio_edit(f: &mut Frame, app: &mut App, area: Rect) {
    const PANEL: Color = BG;
    use crate::modal::Modal;
    let (path, trim_start, trim_end, gain, fade_in, fade_out, cursor, normalize) = {
        let Some(Modal::AudioEdit(s)) = &app.active_modal else { return };
        (s.path.clone(), s.trim_start, s.trim_end, s.gain,
         s.fade_in, s.fade_out, s.cursor, s.normalize)
    };

    let block = Block::default()
        .title(format!(" AUDIO EDIT — {} ",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("?")))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 6 { return; }

    // Layout: waveform (top) | params (bottom 8 rows).
    let wf_h = inner.height.saturating_sub(8).max(3);
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(wf_h), Constraint::Min(0)])
        .split(inner);

    let wf_area  = v[0];
    let par_area = v[1];

    // ── Waveform display ──────────────────────────────────────────────────────
    {
        let peaks = app.waveform_cache.get(&path).cloned();
        let w = wf_area.width as usize;
        let h = wf_area.height as usize;
        let mut lines: Vec<Line> = Vec::with_capacity(h);

        for row in 0..h {
            let mut spans: Vec<Span> = Vec::with_capacity(w);
            for col in 0..w {
                let frac = col as f32 / w as f32;
                let amp = peaks.as_ref()
                    .and_then(|p| p.get((frac * p.len() as f32) as usize).copied())
                    .unwrap_or(0.0);
                let bar_h = (amp * h as f32 * 0.9) as usize;
                let mid = h / 2;
                let half = bar_h / 2;
                let in_bar = row >= mid.saturating_sub(half) && row <= mid + half;

                // Trim region overlay.
                let in_trim = frac >= trim_start && frac <= trim_end;
                let at_trim_l = (frac - trim_start).abs() < 1.5 / w as f32;
                let at_trim_r = (frac - trim_end).abs()   < 1.5 / w as f32;

                let (ch, style) = if at_trim_l || at_trim_r {
                    ("│", Style::default().fg(Color::Yellow))
                } else if in_bar && in_trim {
                    let d = (amp * 4.0) as usize;
                    let c = match d { 0 => "░", 1 => "▒", 2 => "▓", _ => "█" };
                    (c, Style::default().fg(Color::Rgb(56, 200, 100)))
                } else if in_bar {
                    ("▒", Style::default().fg(Color::Rgb(40, 80, 40)))
                } else {
                    (" ", Style::default().fg(BORDER))
                };
                spans.push(Span::styled(ch, style));
            }
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines).style(Style::default().bg(PANEL)), wf_area);
    }

    // ── Parameter rows ────────────────────────────────────────────────────────
    let params: &[(&str, String)] = &[
        ("Trim Start", format!("{:5.1}%", trim_start * 100.0)),
        ("Trim End",   format!("{:5.1}%", trim_end   * 100.0)),
        ("Gain",       format!("{:5.2}x ({:+.1}dB)", gain, 20.0 * gain.max(1e-6).log10())),
        ("Fade In",    format!("{:5.1}%", fade_in   * 100.0)),
        ("Fade Out",   format!("{:5.1}%", fade_out  * 100.0)),
    ];

    let mut y = par_area.y;
    for (i, (label, val)) in params.iter().enumerate() {
        let sel = cursor == i;
        let style = if sel {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let arrow = if sel { "► " } else { "  " };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{}{:<12}", arrow, label), style),
                Span::styled(val.clone(), style),
            ])).style(Style::default().bg(PANEL)),
            Rect::new(par_area.x, y, par_area.width, 1),
        );
        y += 1;
    }

    // Normalize checkbox.
    let norm_style = if normalize {
        Style::default().fg(Color::Rgb(56, 200, 100)).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Normalize [", Style::default().fg(Color::White)),
            Span::styled(if normalize { "x" } else { " " }, norm_style),
            Span::styled("]  N to toggle", Style::default().fg(BORDER)),
        ])).style(Style::default().bg(PANEL)),
        Rect::new(par_area.x, y, par_area.width, 1),
    );
    y += 1;

    // Hint row.
    f.render_widget(
        Paragraph::new(Span::styled(
            "  ↑↓=select  ←→=adjust  N=normalize  Enter=apply  Esc=cancel",
            Style::default().fg(BORDER),
        )).style(Style::default().bg(PANEL)),
        Rect::new(par_area.x, y, par_area.width, 1),
    );
}

// ─── Tutorial ─────────────────────────────────────────────────────────────────

fn draw_tutorial(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::modal::Modal;
    let (title, body, hint, progress, is_last) = {
        let Some(Modal::Tutorial(s)) = &app.active_modal else { return };
        let step = s.current();
        (step.title, step.body, step.hint, s.progress(), s.is_last())
    };

    let next_label = if is_last { "[ Finish ]" } else { "[ Next → ]" };

    let block = Block::default()
        .title(format!(" SeqTerm Tutorial  {} ", progress))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 5 { return; }

    // Title bar.
    f.render_widget(
        Paragraph::new(Span::styled(
            title,
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )).style(Style::default().bg(BG)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Separator.
    f.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(inner.width as usize),
            Style::default().fg(BORDER),
        )).style(Style::default().bg(BG)),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );

    // Body text.
    let body_h = inner.height.saturating_sub(4);
    f.render_widget(
        Paragraph::new(body)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .style(Style::default().fg(Color::White).bg(BG)),
        Rect::new(inner.x + 1, inner.y + 2, inner.width.saturating_sub(2), body_h),
    );

    // Hint + Next button.
    let bottom_y = inner.y + inner.height - 2;
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(BORDER))).style(Style::default().bg(BG)),
        Rect::new(inner.x + 1, bottom_y, inner.width.saturating_sub(14), 1),
    );
    let btn_x = inner.x + inner.width.saturating_sub(next_label.len() as u16 + 2);
    f.render_widget(
        Paragraph::new(Span::styled(
            next_label,
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD),
        )).style(Style::default().bg(BG)),
        Rect::new(btn_x, bottom_y, next_label.len() as u16 + 2, 1),
    );
}

// ─── Lua REPL ─────────────────────────────────────────────────────────────────

fn draw_lua_repl(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::modal::Modal;
    let Some(Modal::LuaRepl(state)) = &app.active_modal else { return };
    let (history, scroll, input) = (state.history.clone(), state.scroll, state.input.clone());

    let block = Block::default()
        .title(" Lua REPL  (Esc=close  ↑↓=scroll) ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 4 { return; }

    let output_h = inner.height.saturating_sub(2) as usize;
    let total = history.len();
    let start = if total > output_h + scroll {
        total - output_h - scroll
    } else {
        0
    };
    let visible: Vec<Line> = history[start..]
        .iter()
        .take(output_h)
        .map(|(line, is_err)| {
            let color = if *is_err { Color::Red } else { Color::White };
            Line::from(Span::styled(line.as_str(), Style::default().fg(color)))
        })
        .collect();

    f.render_widget(
        Paragraph::new(visible).style(Style::default().bg(BG)),
        Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), output_h as u16),
    );

    // Input line.
    let input_y = inner.y + inner.height - 2;
    f.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(inner.width as usize),
            Style::default().fg(BORDER),
        )).style(Style::default().bg(BG)),
        Rect::new(inner.x, input_y, inner.width, 1),
    );
    let prompt = format!("> {input}▌");
    f.render_widget(
        Paragraph::new(Span::styled(prompt, Style::default().fg(Color::Yellow))).style(Style::default().bg(BG)),
        Rect::new(inner.x + 1, input_y + 1, inner.width.saturating_sub(2), 1),
    );
}
