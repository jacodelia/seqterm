pub mod app;
pub mod error;
pub mod menu;
pub mod modal;
pub mod views;
pub mod widgets;

use std::{path::PathBuf, time::Duration};

use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind,
};

use seqterm_command::{AppCommand, HelpTopic};
use seqterm_history as hist;
use menu::MenuKind;
use modal::{FilePickerState, FilePickerTarget, HelpState, Modal,
            AudioSettingsState, MidiSettingsState, MidiImportOptionsState,
            KeybindingsEditorState, SidebarItemKind};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    Frame,
};

use app::{App, AudioExportMsg, ViewKind};
use views::{draw_arranger, draw_config, draw_granular, draw_matrix, draw_mixer, draw_tracker};
use widgets::transport::TransportBar;
use widgets::{draw_menu_dropdown, draw_modal};

const BG: Color = Color::Rgb(13, 17, 23);

/// View labels shown in the transport tab bar.
const VIEW_LABELS: &[&str] = &[
    "MATRIX",
    "TRACKER/P.ROLL",
    "ARRANGER",
    "MIXER",
    "CONFIG",
    "GRANULAR",
];

/// Main ratatui event loop.
pub fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        // ratatui needs a closure; we need &mut App inside draw for modal rendering.
        // Use a raw pointer workaround to satisfy the borrow checker.
        let app_ptr = app as *mut App;
        terminal.draw(|f| ui(f, unsafe { &mut *app_ptr }))?;

        if event::poll(Duration::from_millis(40))? {
            match event::read()? {
                Event::Key(key) => handle_key(app, key),
                Event::Mouse(mouse_event) => handle_mouse(app, mouse_event),
                _ => {}
            }
        }

        app.process_events();

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();

    f.render_widget(
        ratatui::widgets::Block::default().style(Style::default().bg(BG)),
        area,
    );

    let has_tabs = app.tab_count() > 1;

    // Layout: menu bar (1) | [tab bar (1)] | content | transport bar (4).
    let constraints = if has_tabs {
        vec![Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(4)]
    } else {
        vec![Constraint::Length(1), Constraint::Min(1), Constraint::Length(4)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let (menu_area, tab_area, content_area, transport_area) = if has_tabs {
        (chunks[0], Some(chunks[1]), chunks[2], chunks[3])
    } else {
        (chunks[0], None, chunks[1], chunks[2])
    };
    app.transport_area.set(transport_area);

    // Draw menu bar.
    draw_menu_bar(f, app, menu_area);

    // Draw tab bar (only when >1 tab open).
    if let Some(tab_area) = tab_area {
        draw_tab_bar(f, app, tab_area);
    }

    // Draw current view.
    match app.current_view {
        ViewKind::Matrix   => draw_matrix(f, app, content_area),
        ViewKind::Tracker  => draw_tracker(f, app, content_area),
        ViewKind::Arranger => draw_arranger(f, app, content_area),
        ViewKind::Mixer    => draw_mixer(f, app, content_area),
        ViewKind::Config   => draw_config(f, app, content_area),
        ViewKind::Granular => draw_granular(f, app, content_area),
    }

    // Draw transport bar.
    {
        let proj = app.project.lock();
        let dirty_marker = if app.project_dirty { "*" } else { "" };
        let status = if app.project_dirty {
            format!("{} [unsaved]", app.status_msg)
        } else {
            app.status_msg.clone()
        };
        let _ = dirty_marker; // used implicitly in status
        let transport = TransportBar {
            status_msg: &status,
            view_labels: VIEW_LABELS,
            current_view: app.current_view.index(),
            xrun: proj.xrun,
            cpu: proj.cpu,
            capturing: app.capturing,
            midi_clock_sync: app.midi_clock_sync,
        };
        drop(proj);
        f.render_widget(transport, transport_area);
    }

    // Draw open menu dropdown (overlays content).
    if let Some(kind) = app.menu_open {
        // Find x offset of this menu in the bar.
        let mut x = 0u16;
        for k in MenuKind::ALL {
            if *k == kind { break; }
            x += k.label().len() as u16;
        }
        draw_menu_dropdown(f, kind, app.menu_cursor, x, menu_area.y, area);
    }

    // Draw active modal (topmost layer).
    draw_modal(f, app, area);
}

fn draw_menu_bar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    use ratatui::{style::{Color, Modifier, Style}, text::{Line, Span}, widgets::Paragraph};

    const MENU_BG:  Color = Color::Rgb(30, 35, 42);
    const MENU_FG:  Color = Color::Rgb(200, 200, 200);
    const MENU_SEL: Color = Color::Rgb(31, 111, 235);
    const DIRTY:    Color = Color::Rgb(240, 136, 62);

    let mut spans: Vec<Span> = Vec::new();

    for kind in MenuKind::ALL {
        let is_open = app.menu_open == Some(*kind);
        let style = if is_open {
            Style::default().fg(Color::Black).bg(MENU_SEL).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MENU_FG).bg(MENU_BG)
        };
        spans.push(Span::styled(kind.label(), style));
    }

    // Dirty indicator on the right.
    if app.project_dirty {
        let label_w: usize = MenuKind::ALL.iter().map(|k| k.label().len()).sum();
        let pad = (area.width as usize).saturating_sub(label_w + 14);
        spans.push(Span::styled(" ".repeat(pad), Style::default().bg(MENU_BG)));
        spans.push(Span::styled(
            "● unsaved ",
            Style::default().fg(DIRTY).bg(MENU_BG).add_modifier(Modifier::BOLD),
        ));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .style(Style::default().bg(MENU_BG)),
        area,
    );
}

fn draw_tab_bar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    use ratatui::{style::{Color, Modifier, Style}, text::{Line, Span}, widgets::Paragraph};

    const TAB_BG:     Color = Color::Rgb(22, 28, 36);
    const TAB_FG:     Color = Color::Rgb(150, 150, 150);
    const TAB_ACT_BG: Color = Color::Rgb(40, 50, 64);
    const TAB_ACT_FG: Color = Color::Rgb(230, 230, 230);
    const TAB_DIRTY:  Color = Color::Rgb(240, 136, 62);

    let mut spans: Vec<Span> = Vec::new();
    let total = app.tab_count();
    for i in 0..total {
        let name = app.tab_name(i);
        let is_active = i == app.active_tab;
        // Determine dirty state.
        let dirty = if is_active {
            app.project_dirty
        } else {
            let stored = if i < app.active_tab { i } else { i - 1 };
            app.tabs.get(stored).map(|t| t.project_dirty).unwrap_or(false)
        };
        let dirty_mark = if dirty { "●" } else { " " };
        let label = format!(" {dirty_mark} {name} [{i_disp}] ", i_disp = i + 1);
        let style = if is_active {
            Style::default().fg(TAB_ACT_FG).bg(TAB_ACT_BG).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TAB_FG).bg(TAB_BG)
        };
        if dirty && !is_active {
            let dirty_style = style.fg(TAB_DIRTY);
            spans.push(Span::styled(label, dirty_style));
        } else {
            spans.push(Span::styled(label, style));
        }
    }
    spans.push(Span::styled(
        " Ctrl+T=new  Ctrl+W=close  Alt+1-9=switch ",
        Style::default().fg(Color::Rgb(80, 80, 80)).bg(TAB_BG),
    ));

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(TAB_BG)),
        area,
    );
}

// ─── Command dispatcher ───────────────────────────────────────────────────────

pub fn dispatch_command(app: &mut App, cmd: AppCommand) {
    match cmd {
        // ── File ──────────────────────────────────────────────────────────
        AppCommand::NewProject => {
            if app.project_dirty {
                app.active_modal = Some(Modal::confirm(
                    "New Project",
                    "Unsaved changes will be lost. Create new project?",
                    AppCommand::NewProjectConfirmed,
                ));
            } else {
                app.active_modal = Some(Modal::input(
                    "New Project",
                    "BPM (20-300, default 128)",
                    bpm_dialog_to_command,
                ));
            }
        }
        AppCommand::NewProjectConfirmed => {
            app.active_modal = Some(Modal::input(
                "New Project",
                "BPM (20-300, default 128)",
                bpm_dialog_to_command,
            ));
        }
        AppCommand::NewProjectWithBpm(bpm) => do_new_project(app, bpm as f64),

        AppCommand::OpenProject => {
            if app.project_dirty {
                app.active_modal = Some(Modal::confirm(
                    "Open Project",
                    "Unsaved changes will be lost. Open another project?",
                    AppCommand::OpenProject,
                ));
            } else {
                app.active_modal = Some(Modal::FilePicker(
                    FilePickerState::new(FilePickerTarget::OpenProject)
                        .with_recent_dirs(&app.recent_projects),
                ));
            }
        }
        AppCommand::OpenProjectPath(path) => {
            do_open_project(app, path);
        }

        AppCommand::SaveProject => {
            if let Some(path) = app.project_path.clone() {
                do_save_project(app, &path);
            } else {
                dispatch_command(app, AppCommand::SaveProjectAs);
            }
        }
        AppCommand::SaveProjectAs => {
            app.active_modal = Some(Modal::FilePicker(
                FilePickerState::new(FilePickerTarget::SaveProject)
                    .with_recent_dirs(&app.recent_projects),
            ));
        }
        AppCommand::SaveProjectToPath(path) => {
            do_save_project(app, &path);
        }

        AppCommand::ImportMidi => {
            app.active_modal = Some(Modal::FilePicker(
                FilePickerState::new(FilePickerTarget::ImportMidi)
                    .with_recent_dirs(&app.recent_midi_imports),
            ));
        }
        AppCommand::ImportMidiFromPath(path) => {
            do_import_midi(app, path);
        }
        AppCommand::ImportMidiShowOptions(path) => {
            app.active_modal = Some(Modal::MidiImportOptions(MidiImportOptionsState::new(path)));
        }
        AppCommand::ImportMidiWithOptions(path, opts) => {
            do_import_midi_run(app, path, opts);
        }
        AppCommand::SetMidiImportSf2(sf2_path) => {
            // Restore MIDI import options modal with the newly chosen SF2 path.
            if let Some((midi_path, mut opts)) = app.pending_midi_import.take() {
                opts.sf2_path = Some(sf2_path);
                let track_infos = seqterm_midi_io::probe_midi(&midi_path).unwrap_or_default();
                app.active_modal = Some(Modal::MidiImportOptions(
                    crate::modal::MidiImportOptionsState { path: midi_path, opts, cursor: 3, track_infos }
                ));
            }
        }

        AppCommand::ExportMidi => {
            app.active_modal = Some(Modal::FilePicker(
                FilePickerState::new(FilePickerTarget::ExportMidi),
            ));
        }
        AppCommand::ExportMidiToPath(path) => {
            do_export_midi(app, &path);
        }
        AppCommand::ExportMidiActiveOnly => {
            app.active_modal = Some(Modal::FilePicker(
                FilePickerState::new(FilePickerTarget::ExportMidiActiveOnly),
            ));
        }
        AppCommand::ExportMidiActiveOnlyToPath(path) => {
            do_export_midi_active_only(app, &path);
        }

        AppCommand::ExportMuseScore => {
            app.active_modal = Some(Modal::FilePicker(
                FilePickerState::new(FilePickerTarget::ExportMuseScore),
            ));
        }
        AppCommand::ExportMuseScoreToPath(path) => {
            let proj = app.project.lock().clone();
            match seqterm_midi_io::export_musicxml(&proj, &path) {
                Ok(()) => {
                    app.active_modal = None;
                    app.set_timed_status(format!("MusicXML exported: {}", path.display()), 3);
                }
                Err(e) => {
                    app.active_modal = Some(Modal::alert("Export Failed", format!("{e}")));
                }
            }
        }

        AppCommand::ExportAudio => {
            let state = modal::AudioExportOptionsState::new(&app.audio_export_opts);
            app.active_modal = Some(Modal::AudioExportOptions(state));
        }
        AppCommand::ExportAudioToPath(path) => {
            do_export_audio(app, &path);
        }

        AppCommand::ExportKeybindings => {
            app.active_modal = Some(Modal::FilePicker(
                FilePickerState::new(FilePickerTarget::ExportKeybindings),
            ));
        }
        AppCommand::ExportKeybindingsToPath(path) => {
            match seqterm_persistence::export_keybindings(&app.settings.keybindings, &path) {
                Ok(()) => {
                    app.active_modal = None;
                    app.status_msg = format!("Keybindings exported: {}", path.display());
                }
                Err(e) => {
                    app.active_modal = Some(Modal::error("Export Failed", format!("{e}")));
                }
            }
        }
        AppCommand::ImportKeybindings => {
            app.active_modal = Some(Modal::FilePicker(
                FilePickerState::new(FilePickerTarget::ImportKeybindings),
            ));
        }
        AppCommand::ImportKeybindingsFromPath(path) => {
            match seqterm_persistence::import_keybindings(&path) {
                Ok(bindings) => {
                    app.settings.keybindings = bindings;
                    let _ = seqterm_persistence::save_settings(&app.settings);
                    app.active_modal = None;
                    app.status_msg = format!("Keybindings imported from {}", path.display());
                }
                Err(e) => {
                    app.active_modal = Some(Modal::error("Import Failed", format!("{e}")));
                }
            }
        }

        AppCommand::RecentProject(idx) => {
            if let Some(path) = app.recent_projects.get(idx).cloned() {
                dispatch_command(app, AppCommand::OpenProjectPath(path));
            }
        }

        AppCommand::Exit => {
            if app.project_dirty {
                app.active_modal = Some(Modal::QuitConfirm);
            } else {
                app.should_quit = true;
            }
        }
        AppCommand::ExitConfirmed => {
            app.engine.stop();
            app.silence_all_audio();
            app.should_quit = true;
        }
        AppCommand::SaveAndExit => {
            dispatch_command(app, AppCommand::SaveProject);
            app.engine.stop();
            app.silence_all_audio();
            app.should_quit = true;
        }

        // ── Edit ──────────────────────────────────────────────────────────
        AppCommand::Undo => {
            let mut proj = app.project.lock();
            if let Some(desc) = app.history.undo(&mut proj) {
                app.status_msg = format!("Undo: {desc}");
            } else {
                app.status_msg = "Nothing to undo".to_string();
            }
        }
        AppCommand::Redo => {
            let mut proj = app.project.lock();
            if let Some(desc) = app.history.redo(&mut proj) {
                app.status_msg = format!("Redo: {desc}");
            } else {
                app.status_msg = "Nothing to redo".to_string();
            }
        }

        AppCommand::ShowRoutingConfig => {
            app.switch_view(ViewKind::Config);
            app.config_state.section = 4;
            app.status_msg = "ROUTING: hjkl=navigate  Tab=panel  Enter=toggle edge  a=sync nodes".to_string();
        }
        AppCommand::ShowAudioSettings => {
            let state = AudioSettingsState::with_snapshot(
                app.settings.audio.backend.clone(),
                app.settings.audio.sample_rate,
            );
            app.active_modal = Some(Modal::AudioSettings(state));
        }
        AppCommand::ShowMidiSettings => {
            app.active_modal = Some(Modal::MidiSettings(MidiSettingsState::new()));
        }
        AppCommand::ShowKeybindings => {
            let bindings = app.settings.keybindings.clone();
            app.active_modal = Some(Modal::KeybindingsEditor(
                KeybindingsEditorState::new(bindings),
            ));
        }

        // ── About / Help ──────────────────────────────────────────────────
        AppCommand::ShowAbout => {
            app.active_modal = Some(Modal::About);
        }
        AppCommand::ShowHelp(topic) => {
            app.active_modal = Some(Modal::Help(HelpState::new(topic)));
        }
        AppCommand::ShowCommandPalette => {
            app.active_modal = Some(Modal::CommandPalette(modal::CommandPaletteState::new()));
        }

        AppCommand::CloseModal => {
            app.active_modal = None;
        }

        AppCommand::StartOscServer(port) => {
            app.osc_rx = None; // drop old server if any
            match seqterm_midi_io::OscServer::start(port) {
                Ok(rx) => {
                    app.osc_rx = Some(rx);
                    app.osc_port = port;
                    app.set_timed_status(format!("OSC server started on UDP :{port}"), 4);
                }
                Err(e) => {
                    app.set_timed_status(format!("OSC start failed: {e}"), 5);
                }
            }
        }
        AppCommand::StopOscServer => {
            app.osc_rx = None;
            app.osc_port = 0;
            app.set_timed_status("OSC server stopped", 3);
        }

        AppCommand::ToggleCapture => {
            if app.capturing {
                if let Some(ae) = &mut app.audio_engine {
                    ae.stop_capture();
                }
            } else if app.audio_engine_running {
                // Timestamp-based filename in the project dir (or cwd if unsaved).
                let base_dir = app.project_path
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let fname = format!("seqterm_capture_{ts}.wav");
                let path = base_dir.join(fname);
                if let Some(ae) = &mut app.audio_engine {
                    ae.start_capture(path);
                }
            } else {
                app.set_timed_status("Audio engine not running — start it first", 4);
            }
        }

        AppCommand::ToggleChainMode => {
            app.chain_mode = !app.chain_mode;
            app.engine.set_chain_mode(app.chain_mode);
            if !app.chain_mode { app.chain_pos = 0; }
            app.set_timed_status(
                if app.chain_mode { "Chain mode: ON — playing scene chain" }
                else { "Chain mode: OFF" },
                2,
            );
        }
        AppCommand::AddChainEntry { scene_idx, bars } => {
            app.project.lock().chain.push(seqterm_core::ChainEntry::new(scene_idx, bars));
            app.project_dirty = true;
            app.set_timed_status(format!("Chain: added scene {} ({} bars)", scene_idx + 1, bars), 2);
        }
        AppCommand::RemoveChainEntry { pos } => {
            let mut proj = app.project.lock();
            if pos < proj.chain.len() {
                proj.chain.remove(pos);
                app.project_dirty = true;
            }
        }
        AppCommand::SeekChain { pos } => {
            app.chain_pos = pos;
            app.engine.seek_chain(pos);
        }

        AppCommand::ToggleMidiClockSync => {
            app.midi_clock_sync = !app.midi_clock_sync;
            if !app.midi_clock_sync {
                app.midi_clock_last_pulse = None;
                app.midi_clock_intervals.clear();
            }
            app.set_timed_status(
                if app.midi_clock_sync { "MIDI Clock sync: ON — BPM from external source" }
                else { "MIDI Clock sync: OFF" },
                3,
            );
        }

        AppCommand::MidiLearn(target) => {
            app.midi_learn = Some(target);
            app.set_timed_status("MIDI Learn: send a CC…", 10);
        }
        AppCommand::CancelMidiLearn => {
            app.midi_learn = None;
        }

        AppCommand::RecentMidiImport(idx) => {
            if let Some(path) = app.recent_midi_imports.get(idx).cloned() {
                dispatch_command(app, AppCommand::ImportMidiFromPath(path));
            }
        }

        // ── Audio source assignment ────────────────────────────────────────
        AppCommand::AssignSf2ToClip { row, col } => {
            let state = FilePickerState::new(FilePickerTarget::AssignSf2 { row, col });
            app.active_modal = Some(Modal::FilePicker(state));
        }
        AppCommand::OpenSf2Browser { row, col, path } => {
            use modal::Sf2BrowserState;
            let state = Sf2BrowserState::new(path.clone(), row, col);
            app.active_modal = Some(Modal::Sf2Browser(state));
            // Start background scan — result arrives via sf2_presets_rx.
            let (tx, rx) = flume::bounded(1);
            app.sf2_presets_rx = Some(rx);
            std::thread::spawn(move || {
                let presets = seqterm_audio_engine::enumerate_sf2_presets(&path);
                let _ = tx.send(presets);
            });
        }
        AppCommand::ReopenSf2Browser { row, col } => {
            use modal::Sf2BrowserState;
            use seqterm_core::PatternSource;
            let row_key = ((b'A' + row as u8) as char).to_string();
            let sf2_path = {
                let proj = app.project.lock();
                proj.matrix
                    .get(&row_key)
                    .and_then(|r| r.get(col))
                    .and_then(|s| s.as_ref())
                    .and_then(|c| if let PatternSource::Sf2 { path, bank, preset, .. } = &c.source {
                        Some((path.clone(), *bank, *preset))
                    } else {
                        None
                    })
            };
            if let Some((path, cur_bank, cur_preset)) = sf2_path {
                let mut state = Sf2BrowserState::new(path.clone(), row, col);
                // Pre-select current bank/preset so the user sees their current choice.
                state.bank   = cur_bank;
                state.preset = cur_preset;
                app.active_modal = Some(Modal::Sf2Browser(state));
                let (tx, rx) = flume::bounded(1);
                app.sf2_presets_rx = Some(rx);
                std::thread::spawn(move || {
                    let presets = seqterm_audio_engine::enumerate_sf2_presets(&path);
                    let _ = tx.send(presets);
                });
            }
        }
        AppCommand::AssignAudioFileToClip { row, col } => {
            let state = FilePickerState::new(FilePickerTarget::AssignAudioFile { row, col });
            app.active_modal = Some(Modal::FilePicker(state));
        }
        AppCommand::ConfirmAudioFileAssignment { row, col, path } => {
            use seqterm_core::PatternSource;
            let row_key = ((b'A' + row as u8) as char).to_string();
            let new_source = PatternSource::AudioFile {
                path: path.clone(),
                looping: false,
                original_bpm: 0.0,
                gain: 1.0,
            };
            {
                let old_source = app.project.lock()
                    .matrix.get(&row_key)
                    .and_then(|s| s.get(col))
                    .and_then(|s| s.as_ref())
                    .map(|c| c.source.clone())
                    .unwrap_or(PatternSource::Midi);
                let mut proj = app.project.lock();
                app.history.push(Box::new(hist::SetClipSource {
                    row_key,
                    col,
                    old: old_source,
                    new: new_source,
                }), &mut proj);
            }
            app.project_dirty = true;
            app.active_modal = None;
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            app.status_msg = format!("Audio: {} → {}{}", fname, (b'A' + row as u8) as char, col + 1);
            if let Some(ae) = &mut app.audio_engine {
                let slot_id = ae.load_audio_file(path, false, 0.0);
                let clip_key = format!("{}{}", (b'A' + row as u8) as char, col);
                app.audio_slots.insert(clip_key, slot_id);
                app.engine.set_audio_slots(app.audio_slots.clone());
            }
        }
        AppCommand::ConfirmSf2Assignment { row, col, path, bank, preset } => {
            use seqterm_core::PatternSource;
            let row_key = ((b'A' + row as u8) as char).to_string();
            let new_source = PatternSource::Sf2 {
                path: path.clone(),
                bank,
                preset,
                preset_name: format!("Bank:{bank} Prog:{preset}"),
            };
            {
                let old_source = app.project.lock()
                    .matrix.get(&row_key)
                    .and_then(|s| s.get(col))
                    .and_then(|s| s.as_ref())
                    .map(|c| c.source.clone())
                    .unwrap_or(PatternSource::Midi);
                let mut proj = app.project.lock();
                app.history.push(Box::new(hist::SetClipSource {
                    row_key,
                    col,
                    old: old_source,
                    new: new_source,
                }), &mut proj);
            }
            app.project_dirty = true;
            app.active_modal = None;
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            app.status_msg = format!("SF2: {} B{bank}P{preset} → {}{}", fname, (b'A' + row as u8) as char, col + 1);
            if let Some(ae) = &mut app.audio_engine {
                let slot_id = ae.load_sf2(path, bank, preset);
                let clip_key = format!("{}{}", (b'A' + row as u8) as char, col);
                app.audio_slots.insert(clip_key, slot_id);
                app.sf2_slots.insert(slot_id);
                app.engine.set_audio_slots(app.audio_slots.clone());
            }
        }
        AppCommand::ClearClipSource { row, col } => {
            use seqterm_core::PatternSource;
            let row_key = ((b'A' + row as u8) as char).to_string();
            let old_source = app.project.lock()
                .matrix.get(&row_key)
                .and_then(|s| s.get(col))
                .and_then(|s| s.as_ref())
                .map(|c| c.source.clone())
                .unwrap_or(PatternSource::Midi);
            {
                let mut proj = app.project.lock();
                app.history.push(Box::new(hist::SetClipSource {
                    row_key,
                    col,
                    old: old_source,
                    new: PatternSource::Midi,
                }), &mut proj);
            }
            app.project_dirty = true;
            app.status_msg = format!("Source cleared → MIDI: {}{}", (b'A' + row as u8) as char, col + 1);
        }

        AppCommand::AssignMidiPort { row, col, port } => {
            use seqterm_core::PatternSource;
            let row_key = ((b'A' + row as u8) as char).to_string();
            let old_source = app.project.lock()
                .matrix.get(&row_key).and_then(|s| s.get(col))
                .and_then(|s| s.as_ref()).map(|c| c.source.clone())
                .unwrap_or(PatternSource::Midi);
            {
                let mut proj = app.project.lock();
                // Set source to Midi and assign the port.
                if proj.matrix
                    .get_mut(&row_key).and_then(|r| r.get_mut(col))
                    .and_then(|c| c.as_mut()).is_some()
                {
                    app.history.push(Box::new(hist::SetClipSource {
                        row_key: row_key.clone(),
                        col,
                        old: old_source,
                        new: PatternSource::Midi,
                    }), &mut proj);
                    // Also update midi_out directly.
                    if let Some(clip2) = proj.matrix
                        .get_mut(&row_key).and_then(|r| r.get_mut(col))
                        .and_then(|c| c.as_mut())
                    {
                        clip2.midi_out = if port.is_empty() { None } else { Some(port.clone()) };
                    }
                }
            }
            app.project_dirty = true;
            app.active_modal = None;
            app.set_timed_status(format!("MIDI → {} : {}{}", port, (b'A' + row as u8) as char, col + 1), 2);
            // Open the ALSA MIDI connection to the newly assigned port.
            rebuild_midi_ports(app);
        }

        AppCommand::OpenSourcePicker { row, col } => {
            let (midi_ports, current_label) = {
                let proj = app.project.lock();
                let ports: Vec<String> = proj.midi_outputs.iter().map(|p| p.name.clone()).collect();
                let label = proj.matrix.get(&((b'A' + row as u8) as char).to_string())
                    .and_then(|r| r.get(col)).and_then(|c| c.as_ref())
                    .map(|c| match &c.source {
                        seqterm_core::PatternSource::Midi => format!("MIDI → {}", c.midi_out.as_deref().unwrap_or("(none)")),
                        seqterm_core::PatternSource::Sf2  { preset_name, path, .. } =>
                            format!("SF2: {} [{}]", preset_name, path.file_name().and_then(|n| n.to_str()).unwrap_or("?")),
                        seqterm_core::PatternSource::AudioFile { path, .. } =>
                            format!("AUDIO: {}", path.file_name().and_then(|n| n.to_str()).unwrap_or("?")),
                    })
                    .unwrap_or_else(|| "(empty slot)".to_string());
                (ports, label)
            };
            app.active_modal = Some(modal::Modal::SourcePicker(
                modal::SourcePickerState::new(row, col, midi_ports, current_label)
            ));
        }

        AppCommand::MoveClip { from_row, from_col, to_row, to_col } => {
            if from_row == to_row && from_col == to_col {
                app.matrix_state.grabbed_clip = None;
                return;
            }
            let from_key = ((b'A' + from_row as u8) as char).to_string();
            let to_key   = ((b'A' + to_row   as u8) as char).to_string();
            {
                let mut proj = app.project.lock();
                app.history.push(Box::new(hist::SwapClips {
                    from_key: from_key.clone(),
                    from_col,
                    to_key: to_key.clone(),
                    to_col,
                }), &mut proj);
            }
            app.matrix_state.grabbed_clip = None;
            app.project_dirty = true;
            let from_label = format!("{}{}", (b'A' + from_row as u8) as char, from_col + 1);
            let to_label   = format!("{}{}", (b'A' + to_row   as u8) as char, to_col   + 1);
            app.set_timed_status(format!("Moved clip {} → {}", from_label, to_label), 2);
        }

        // ── Plugin system ─────────────────────────────────────────────────
        AppCommand::OpenPluginParams { registry_id } => {
            let plugin_name = app.plugin_registry.instances()
                .find(|i| i.registry_id == registry_id)
                .map(|i| i.descriptor.name.clone())
                .unwrap_or_else(|| format!("Plugin {registry_id}"));
            let mut state = modal::PluginParamBrowserState::new(registry_id, plugin_name);
            state.refresh(&app.plugin_registry);
            app.active_modal = Some(modal::Modal::PluginParams(state));
        }
        AppCommand::ScanPlugins { dir } => {
            let found = app.plugin_registry.scan(&dir);
            app.set_timed_status(format!("Scanned: {} plugin(s) found", found.len()), 3);
        }
        AppCommand::LoadPlugin { plugin_id } => {
            // Use a default sample rate / block size; the audio engine may not be running yet.
            let (sr, bs) = app.audio_engine.as_ref()
                .map(|_| (48000u32, 256u32))
                .unwrap_or((48000, 256));
            match app.plugin_registry.instantiate(&plugin_id, sr, bs) {
                Ok(registry_id) => {
                    app.set_timed_status(format!("Loaded plugin '{plugin_id}' (id {registry_id})"), 3);
                }
                Err(e) => {
                    app.set_timed_status(format!("Load plugin failed: {e}"), 5);
                }
            }
        }
        AppCommand::UnloadPlugin { registry_id } => {
            app.plugin_registry.destroy(registry_id);
            if let Some(modal::Modal::PluginParams(s)) = &app.active_modal {
                if s.registry_id == registry_id {
                    app.active_modal = None;
                }
            }
            app.set_timed_status(format!("Unloaded plugin {registry_id}"), 2);
        }

        // ── Sampler / SP-404 pad system ───────────────────────────────────
        AppCommand::TriggerPad { bank, pad, velocity } => {
            use seqterm_core::{ChokeGroup, MuteGroup};
            use seqterm_audio_engine::AudioCommand;

            let slot_info = {
                let proj = app.project.lock();
                proj.sampler.banks.get(bank)
                    .and_then(|b| b.slots[pad].as_ref())
                    .map(|s| (
                        s.path.clone(), s.trigger, s.mute_group, s.choke_group,
                        s.gain, s.vel_to_vol, s.loop_start, s.loop_end,
                        s.reverse, s.pitch_st, s.trim_start, s.trim_end, s.normalize,
                        s.retrigger,
                    ))
            };

            let Some((path, trigger, mute_group, choke_group, gain, vel_to_vol,
                       loop_start, loop_end, reverse, pitch_st, trim_start, trim_end, normalize,
                       retrigger)) = slot_info else {
                return;
            };

            if let Some(ae) = app.audio_engine.as_mut() {
                // Enforce choke group — instant silence for all pads in same group.
                if choke_group != ChokeGroup(0) {
                    let choke_pads: Vec<usize> = {
                        let proj = app.project.lock();
                        proj.sampler.banks.get(bank)
                            .map(|b| b.slots.iter().enumerate()
                                .filter(|(i, s)| *i != pad && s.as_ref().map_or(false, |p| p.choke_group == choke_group))
                                .map(|(i, _)| i)
                                .collect())
                            .unwrap_or_default()
                    };
                    for other_pad in choke_pads {
                        if let Some(&sid) = app.sampler_slots.get(&(bank, other_pad)) {
                            ae.send(AudioCommand::StopAudioClip { slot_id: sid });
                        }
                    }
                }

                // Enforce mute group — fade-out all other pads in same group.
                if mute_group != MuteGroup(0) {
                    let mute_pads: Vec<usize> = {
                        let proj = app.project.lock();
                        proj.sampler.pads_in_mute_group(bank, mute_group)
                    };
                    for other_pad in mute_pads {
                        if other_pad != pad {
                            if let Some(&sid) = app.sampler_slots.get(&(bank, other_pad)) {
                                ae.send(AudioCommand::StopAudioClip { slot_id: sid });
                            }
                        }
                    }
                }

                // Apply velocity scaling to slot volume.
                let vol = if vel_to_vol > 0.0 {
                    gain * (1.0 - vel_to_vol + vel_to_vol * (velocity as f32 / 127.0))
                } else {
                    gain
                };

                let is_loop = trigger == seqterm_core::TriggerMode::Loop;
                let key = (bank, pad);
                if let Some(&slot_id) = app.sampler_slots.get(&key) {
                    // Already loaded — retrigger.
                    ae.send(AudioCommand::SetSlotVolume { slot_id, volume: vol });
                    ae.send(AudioCommand::SetPlaybackRange {
                        slot_id, start_frac: trim_start, end_frac: trim_end,
                    });
                    if is_loop {
                        ae.send(AudioCommand::SetLoopPoints {
                            slot_id, start_frac: loop_start, end_frac: loop_end,
                        });
                    }
                    ae.send(AudioCommand::SetReverse { slot_id, reverse });
                    ae.send(AudioCommand::SetPitchSt { slot_id, semitones: pitch_st });
                    ae.send(AudioCommand::PlayAudioClip { slot_id });
                } else {
                    // First trigger — load the file, queue play for when loaded.
                    let slot_id = ae.load_audio_file_ex(path, is_loop, normalize);
                    ae.send(AudioCommand::SetSlotVolume { slot_id, volume: vol });
                    ae.send(AudioCommand::SetPlaybackRange {
                        slot_id, start_frac: trim_start, end_frac: trim_end,
                    });
                    if is_loop {
                        ae.send(AudioCommand::SetLoopPoints {
                            slot_id, start_frac: loop_start, end_frac: loop_end,
                        });
                    }
                    ae.send(AudioCommand::SetReverse { slot_id, reverse });
                    ae.send(AudioCommand::SetPitchSt { slot_id, semitones: pitch_st });
                    app.sampler_slots.insert(key, slot_id);
                    app.pending_plays.insert(slot_id);
                }
            }

            // Retrigger: schedule additional plays via timed background thread → retrigger_tx.
            let n = retrigger.clamp(1, 8) as usize;
            if n > 1 {
                if let Some(&slot_id) = app.sampler_slots.get(&(bank, pad)) {
                    let step_ms = 60_000.0 / (app.bpm * 4.0);
                    let interval_ms = (step_ms / n as f64) as u64;
                    let tx = app.retrigger_tx.clone();
                    std::thread::spawn(move || {
                        for _ in 1..n {
                            std::thread::sleep(std::time::Duration::from_millis(interval_ms));
                            let _ = tx.send(slot_id);
                        }
                    });
                }
            }

            app.set_timed_status(
                format!("Pad {}{} vel={velocity}", (b'A' + bank as u8) as char, pad + 1), 1,
            );
        }
        AppCommand::StopPad { bank, pad } => {
            if let Some(&slot_id) = app.sampler_slots.get(&(bank, pad)) {
                if let Some(ae) = app.audio_engine.as_mut() {
                    ae.send(seqterm_audio_engine::AudioCommand::StopAudioClip { slot_id });
                }
            }
        }
        AppCommand::SelectPadBank(bank) => {
            {
                let mut proj = app.project.lock();
                proj.sampler.active_bank = bank.min(proj.sampler.banks.len().saturating_sub(1));
            }
            app.set_timed_status(format!("Bank {}", (b'A' + bank as u8) as char), 1);
        }
        AppCommand::AssignSampleToPad { bank, pad } => {
            use modal::{FilePickerState, FilePickerTarget};
            let state = FilePickerState::new(FilePickerTarget::AssignSampleToPad { bank, pad });
            app.active_modal = Some(modal::Modal::FilePicker(state));
        }
        AppCommand::ConfirmSampleAssignment { bank, pad, path } => {
            use seqterm_core::PadSlot;
            let slot = PadSlot::new(path.clone());
            {
                let mut proj = app.project.lock();
                if let Some(b) = proj.sampler.banks.get_mut(bank) {
                    b.assign(pad, slot);
                }
            }
            app.project_dirty = true;
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            app.set_timed_status(format!("Assigned {} → {}{}", fname, (b'A' + bank as u8) as char, pad + 1), 3);
        }
        AppCommand::ClearPad { bank, pad } => {
            let mut proj = app.project.lock();
            if let Some(b) = proj.sampler.banks.get_mut(bank) {
                b.clear(pad);
            }
            app.project_dirty = true;
        }
        AppCommand::CaptureSkipBackToPad { bank, pad } => {
            let sb_arc = app.audio_engine.as_ref().and_then(|ae| ae.skip_back());
            let Some(sb_arc) = sb_arc else {
                app.set_timed_status("Skip-back: audio engine not running", 3);
                return;
            };
            let sr = app.audio_sample_rate;
            let secs = { app.project.lock().sampler.skip_back_secs };
            let frames = (sr as usize).saturating_mul(secs as usize);

            let captured = { sb_arc.read().capture(frames) };
            if captured.is_empty() {
                app.set_timed_status("Skip-back: buffer empty", 3);
                return;
            }

            // Determine output directory: <project_dir>/samples/ or /tmp/seqterm-samples/.
            let samples_dir = if let Some(ref proj_path) = app.project_path {
                proj_path.parent().unwrap_or(std::path::Path::new(".")).join("samples")
            } else {
                std::path::PathBuf::from("/tmp/seqterm-samples")
            };
            if let Err(e) = std::fs::create_dir_all(&samples_dir) {
                app.set_timed_status(format!("Skip-back: mkdir failed: {e}"), 5);
                return;
            }

            let fname = format!("{}{}_skipback.wav", (b'A' + bank as u8) as char, pad + 1);
            let out_path = samples_dir.join(&fname);

            // Write WAV on background thread so we don't block the UI loop.
            let out_path2 = out_path.clone();
            std::thread::spawn(move || {
                let spec = hound::WavSpec {
                    channels: 2,
                    sample_rate: sr,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                };
                match hound::WavWriter::create(&out_path2, spec) {
                    Ok(mut writer) => {
                        for &s in &captured {
                            let _ = writer.write_sample(s);
                        }
                        let _ = writer.finalize();
                    }
                    Err(_e) => {}
                }
            });

            // Assign the captured file to the pad slot.
            {
                use seqterm_core::PadSlot;
                let mut proj = app.project.lock();
                if let Some(b) = proj.sampler.banks.get_mut(bank) {
                    b.assign(pad, PadSlot::new(out_path.clone()));
                }
            }
            app.project_dirty = true;

            // Evict stale slot_id so next TriggerPad reloads from the new file.
            app.sampler_slots.remove(&(bank, pad));

            app.set_timed_status(
                format!("Skip-back → {}{}: {fname}", (b'A' + bank as u8) as char, pad + 1), 3,
            );
        }
        AppCommand::BouncePatternToPad { pattern_key, bank, pad } => {
            // Find the matrix row containing this pattern key.
            let row_key = {
                let proj = app.project.lock();
                let mut found = None;
                'outer: for (rk, slots) in &proj.matrix {
                    for slot in slots {
                        if let Some(clip) = slot {
                            if clip.pattern_key.as_deref() == Some(pattern_key.as_str()) {
                                found = Some(rk.clone());
                                break 'outer;
                            }
                        }
                    }
                }
                found
            };
            let Some(row_key) = row_key else {
                app.set_timed_status(format!("Bounce: pattern '{}' not in matrix", pattern_key), 4);
                return;
            };

            let project_snap = app.project.lock().clone();
            let sample_rate = app.audio_sample_rate;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs()).unwrap_or(0);
            let out_path = std::env::temp_dir().join(format!("seqterm_bounce_{ts}.wav"));
            let event_tx = app.audio_engine
                .as_ref()
                .map(|ae| ae.handle().event_tx.clone());

            app.set_timed_status(format!("Bouncing row {}…", row_key), 2);

            // Background render thread.
            let path_clone = out_path.clone();
            let row_clone  = row_key.clone();
            std::thread::Builder::new()
                .name("seqterm-bounce".into())
                .spawn(move || {
                    let result = seqterm_audio_engine::render_offline_stem(
                        project_snap, &row_clone, &path_clone, sample_rate, 16, |_, _| {},
                    );
                    if let Some(tx) = event_tx {
                        let ev = match result {
                            Ok(()) => seqterm_audio_engine::AudioEngineEvent::AudioFileLoaded {
                                slot_id: u32::MAX, // sentinel — handled below
                                duration_secs: 0.0,
                                sample_rate,
                            },
                            Err(e) => seqterm_audio_engine::AudioEngineEvent::Error(e.to_string()),
                        };
                        let _ = tx.send(ev);
                    }
                })
                .expect("spawn bounce thread");

            // Assign result path to the pad slot immediately so it loads when done.
            {
                let mut proj = app.project.lock();
                let banks = &mut proj.sampler.banks;
                while banks.len() <= bank { banks.push(seqterm_core::PadBank::default()); }
                let pad_idx = pad;
                let slot = &mut banks[bank].slots[pad_idx];
                *slot = Some(seqterm_core::PadSlot::new(out_path.clone()));
            }
            app.project_dirty = true;
            // Load the audio file once the render completes (next time pad is triggered).
            app.set_timed_status(
                format!("Bounce started → pad {}{}", (b'A' + bank as u8) as char, pad + 1),
                3,
            );
        }

        // ── Granular engine ───────────────────────────────────────────────
        AppCommand::OpenGranularView { bank, pad } => {
            let key = (bank, pad);
            app.granular_state.pad = Some(key);
            app.granular_state.cursor = 0;
            // Load default params for this pad if not already cached.
            // (GrainParams::default is used until the user or engine updates them.)
            app.switch_view(ViewKind::Granular);
            app.set_timed_status(
                format!("Granular: {}{}", (b'A' + bank as u8) as char, pad + 1), 2,
            );
        }
        AppCommand::GranularFreeze { bank, pad } => {
            if let Some(&slot_id) = app.sampler_slots.get(&(bank, pad)) {
                if let Some(ae) = app.audio_engine.as_mut() {
                    ae.send(seqterm_audio_engine::AudioCommand::FreezeGranular { slot_id });
                }
                app.set_timed_status(
                    format!("Granular freeze: {}{}", (b'A' + bank as u8) as char, pad + 1), 2,
                );
            }
        }
        AppCommand::GranularUnfreeze { bank, pad } => {
            if let Some(&slot_id) = app.sampler_slots.get(&(bank, pad)) {
                if let Some(ae) = app.audio_engine.as_mut() {
                    ae.send(seqterm_audio_engine::AudioCommand::UnfreezeGranular { slot_id });
                }
                app.set_timed_status(
                    format!("Granular unfreeze: {}{}", (b'A' + bank as u8) as char, pad + 1), 2,
                );
            }
        }
        AppCommand::SetGranularParam { param, value, .. } => {
            app.set_timed_status(format!("Granular {param}={value:.2}"), 1);
        }

        AppCommand::SetGranularModSlot { slot_idx, enabled, shape_idx, rate_hz, depth, target_idx } => {
            use seqterm_core::{LfoShape, ModTarget};
            let shape = match shape_idx { 1 => LfoShape::Triangle, 2 => LfoShape::Square, 3 => LfoShape::SampleHold, _ => LfoShape::Sine };
            let target = match target_idx { 1 => ModTarget::Density, 2 => ModTarget::PitchSt, 3 => ModTarget::Pan, 4 => ModTarget::GrainSize, 5 => ModTarget::Overlap, 6 => ModTarget::Jitter, _ => ModTarget::Spray };
            if let Some(s) = app.granular_mod.slots.get_mut(slot_idx) {
                s.enabled = enabled;
                s.shape   = shape;
                s.rate_hz = rate_hz;
                s.depth   = depth;
                s.target  = target;
            }
            if let Some((bank, pad)) = app.granular_state.pad {
                if let Some(&slot_id) = app.sampler_slots.get(&(bank, pad)) {
                    if let Some(ae) = app.audio_engine.as_mut() {
                        ae.send(seqterm_audio_engine::AudioCommand::SetGranularMod {
                            slot_id,
                            mod_matrix: app.granular_mod.clone(),
                        });
                    }
                }
            }
        }

        AppCommand::SaveGranularScene { slot, name } => {
            let preset = seqterm_core::GranularPreset {
                name: if name.is_empty() { format!("Scene {}", slot + 1) } else { name.clone() },
                params: app.granular_state.params.clone(),
                zone:   app.granular_state.zone.clone(),
            };
            let mut proj = app.project.lock();
            if slot >= proj.granular_scenes.len() {
                proj.granular_scenes.resize_with(slot + 1, Default::default);
            }
            proj.granular_scenes[slot] = preset;
            app.project_dirty = true;
            drop(proj);
            app.set_timed_status(format!("Scene {} saved: \"{}\"", slot + 1, name), 2);
        }

        AppCommand::RecallGranularScene { slot } => {
            let preset = {
                let proj = app.project.lock();
                proj.granular_scenes.get(slot).cloned()
            };
            if let Some(preset) = preset {
                app.granular_state.params = preset.params.clone();
                app.granular_state.zone   = preset.zone.clone();
                app.granular_mod = seqterm_core::GranularMod::default();
                // Push the new params/zone to the audio engine if a pad is loaded.
                if let Some((bank, pad)) = app.granular_state.pad {
                    if let Some(&slot_id) = app.sampler_slots.get(&(bank, pad)) {
                        if let Some(ae) = app.audio_engine.as_mut() {
                            ae.send(seqterm_audio_engine::AudioCommand::SetGranularParams {
                                slot_id,
                                params: preset.params,
                            });
                            ae.send(seqterm_audio_engine::AudioCommand::SetGranularZone {
                                slot_id,
                                zone: preset.zone,
                            });
                        }
                    }
                }
                app.set_timed_status(format!("Scene {} recalled: \"{}\"", slot + 1, preset.name), 2);
            } else {
                app.set_timed_status(format!("Scene slot {} is empty", slot + 1), 2);
            }
        }

        AppCommand::SetGranularLiveSource { bank, pad, source_slot_id } => {
            let gran_slot_id = app.sampler_slots.get(&(bank, pad)).copied();
            if let (Some(gran_id), Some(ae)) = (gran_slot_id, app.audio_engine.as_mut()) {
                ae.send(seqterm_audio_engine::AudioCommand::SetGranularLiveSource {
                    granular_slot_id: gran_id,
                    source_slot_id,
                });
            }
            app.granular_live_source = source_slot_id;
            let msg = match source_slot_id {
                Some(sid) => format!("Granular live input: slot {} → pad {}{}", sid, (b'A' + bank as u8) as char, pad + 1),
                None      => "Granular live input: OFF".to_string(),
            };
            app.set_timed_status(msg, 3);
        }

        AppCommand::CaptureGranularToPad { bank, pad } => {
            // Start a 4-second audio capture, assign result to pad when done.
            if !app.audio_engine_running {
                app.set_timed_status("Audio engine not running", 3);
                return;
            }
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs()).unwrap_or(0);
            let capture_path = std::env::temp_dir().join(format!("seqterm_texture_{ts}.wav"));

            if let Some(ae) = &mut app.audio_engine {
                ae.start_capture(capture_path.clone());
            }
            // Assign the capture path to the pad slot immediately.
            {
                let mut proj = app.project.lock();
                let banks = &mut proj.sampler.banks;
                while banks.len() <= bank { banks.push(seqterm_core::PadBank::default()); }
                banks[bank].slots[pad] = Some(seqterm_core::PadSlot::new(capture_path));
            }
            app.project_dirty = true;
            app.set_timed_status(
                format!("Capturing texture → pad {}{}  (Ctrl+R to stop)",
                    (b'A' + bank as u8) as char, pad + 1), 4,
            );
        }

        AppCommand::MorphGranularScene { to_slot, beats } => {
            let to_preset = {
                let proj = app.project.lock();
                proj.granular_scenes.get(to_slot).cloned()
            };
            if let Some(to_preset) = to_preset {
                let beats_secs = beats as f64 * 60.0 / app.bpm;
                let step = (1.0 / (beats_secs * 60.0)) as f32;
                let from = seqterm_core::GranularPreset {
                    name: String::new(),
                    params: app.granular_state.params.clone(),
                    zone:   app.granular_state.zone.clone(),
                };
                app.granular_morph = Some(crate::app::GranularMorph { from, to: to_preset, progress: 0.0, step });
                app.set_timed_status(format!("Morphing → scene {} over {} beat(s)", to_slot + 1, beats), 2);
            } else {
                app.set_timed_status(format!("Scene slot {} is empty", to_slot + 1), 2);
            }
        }

        AppCommand::RandomiseGranularPreset => {
            // LCG seeded from wall-clock nanoseconds — no rand crate needed.
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(12345) as u64;
            let mut lcg = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let mut rng = move || -> f32 {
                lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((lcg >> 33) as f32) / (u32::MAX as f32)
            };

            use seqterm_core::{GrainEnvelope, GrainDirection};
            let p = &mut app.granular_state.params;
            p.size_ms        = 10.0 + rng() * 490.0;
            p.density        = 1.0  + rng() * 49.0;
            p.spray          = rng() * 0.8;
            p.overlap        = 0.1  + rng() * 0.8;
            p.pitch_st       = (rng() * 24.0) - 12.0;
            p.jitter         = rng() * 0.5;
            p.stereo_spread  = rng();
            p.envelope       = match (rng() * 4.0) as u8 {
                0 => GrainEnvelope::Hann,
                1 => GrainEnvelope::Gaussian,
                2 => GrainEnvelope::Triangle,
                _ => GrainEnvelope::Exponential,
            };
            p.direction      = match (rng() * 3.0) as u8 {
                0 => GrainDirection::Forward,
                1 => GrainDirection::Backward,
                _ => GrainDirection::Random,
            };

            if let Some((bank, pad)) = app.granular_state.pad {
                if let Some(&slot_id) = app.sampler_slots.get(&(bank, pad)) {
                    if let Some(ae) = app.audio_engine.as_mut() {
                        ae.send(seqterm_audio_engine::AudioCommand::SetGranularParams {
                            slot_id,
                            params: app.granular_state.params.clone(),
                        });
                    }
                }
            }
            app.set_timed_status(
                format!("Happy accident! sz={:.0}ms d={:.1} sp={:.2} p={:+.1}st",
                    app.granular_state.params.size_ms,
                    app.granular_state.params.density,
                    app.granular_state.params.spray,
                    app.granular_state.params.pitch_st,
                ), 3,
            );
        }

        AppCommand::DeleteGranularScene { slot } => {
            let existed = {
                let mut proj = app.project.lock();
                if let Some(s) = proj.granular_scenes.get_mut(slot) {
                    *s = seqterm_core::GranularPreset::default();
                    true
                } else {
                    false
                }
            };
            if existed {
                app.project_dirty = true;
                app.set_timed_status(format!("Scene {} cleared", slot + 1), 2);
            }
        }
    }
}

// ─── InputDialog callbacks (must be fn pointers, not closures) ───────────────

fn bpm_dialog_to_command(s: String) -> AppCommand {
    let bpm = s.trim().parse::<f64>().unwrap_or(128.0).clamp(20.0, 300.0).round() as u32;
    AppCommand::NewProjectWithBpm(bpm)
}

// ─── File operation helpers ───────────────────────────────────────────────────

fn do_new_project(app: &mut App, bpm: f64) {
    let bpm = bpm.clamp(20.0, 300.0);
    app.engine.stop();
    app.playing = false;
    {
        let mut proj = app.project.lock();
        *proj = seqterm_core::Project::blank("Untitled");
        proj.bpm = bpm;
        for r in 0..8u8 {
            let key = ((b'A' + r) as char).to_string();
            proj.matrix.insert(key, vec![None; 8]);
        }
    }
    app.bpm = bpm;
    app.engine.set_bpm(bpm);
    app.current_step = 0;
    app.current_bar  = 0;
    app.matrix_rows  = 8;
    app.matrix_cols  = 8;
    app.matrix_state.cursor = (0, 0);
    app.tracker_state.pattern_key = None;
    app.project_path  = None;
    app.project_dirty = false;
    app.history.clear();
    app.active_modal  = None;
    app.ensure_matrix_size();
    app.set_timed_status(format!("New project @ {bpm:.0} BPM"), 2);
}

/// Collect all unique `midi_out` destinations from the project and open direct
/// ALSA connections to each one.  The returned map (dest_name → sender) is
/// what the engine uses to route MIDI per-slot independently of pattern key.
fn rebuild_midi_ports(app: &mut App) {
    let destinations: Vec<String> = {
        let proj = app.project.lock();
        let mut seen = std::collections::HashSet::new();
        proj.matrix.values()
            .flat_map(|slots| slots.iter().flatten())
            .filter_map(|clip| clip.midi_out.clone())
            .filter(|d| seen.insert(d.clone()))
            .collect()
    };
    let ports = seqterm_midi::open_output_connections(&destinations);
    app.engine.set_midi_ports(ports);
}

fn do_open_project(app: &mut App, path: PathBuf) {
    match seqterm_persistence::load_project_auto(&path) {
        Ok(proj) => {
            app.engine.stop();
            app.playing = false;
            let bpm = proj.bpm;

            *app.project.lock() = proj;
            app.bpm = bpm;
            app.engine.set_bpm(bpm);
            app.project_path  = Some(path.clone());
            app.project_dirty = false;
            app.history = seqterm_history::load_history(&path);

            // Open direct ALSA output connections for all routed destinations.
            rebuild_midi_ports(app);

            // Reload all SF2 / AudioFile sources into the audio engine.
            rebuild_audio_slots(app);

            seqterm_persistence::push_recent_project(&path);
            app.recent_projects = seqterm_persistence::load_recent_projects();
            app.active_modal = None;
            app.status_msg = format!("Opened: {}", path.display());
        }
        Err(e) => {
            app.active_modal = Some(Modal::alert("Open Failed", format!("{e}")));
        }
    }
}

/// Reload all SF2 / AudioFile clip sources into the audio engine and
/// push the updated slot map to the scheduler.
pub fn rebuild_audio_slots(app: &mut App) {
    if app.audio_engine.is_none() { return; }

    // Release all currently allocated audio slots before rebuilding.
    // This frees SF2 sample memory and audio clip PCM from the mixer.
    {
        let ae = app.audio_engine.as_mut().unwrap();
        let old_slots: Vec<u32> = app.audio_slots.values().copied().collect();
        for slot_id in old_slots {
            ae.release_slot(slot_id);
        }
    }
    app.audio_slots.clear();
    app.sf2_slots.clear();

    // Collect all clips that need audio engine slots.
    // SF2 clips: (row, col, path, midi_channel_0based, bank, preset)
    // Audio clips: (row, col, path, looping, original_bpm)
    use seqterm_core::PatternSource;
    use std::collections::HashMap as StdMap;

    struct Sf2Entry { row: usize, col: usize, ch: u8, bank: u8, preset: u8 }
    struct AudioEntry { row: usize, col: usize, path: PathBuf, looping: bool, bpm: f64 }

    let (sf2_by_path, audio_clips): (StdMap<PathBuf, Vec<Sf2Entry>>, Vec<AudioEntry>) = {
        let proj = app.project.lock();
        let mut sf2: StdMap<PathBuf, Vec<Sf2Entry>> = StdMap::new();
        let mut audio: Vec<AudioEntry> = Vec::new();

        for (row_label, slots) in &proj.matrix {
            let row_char = match row_label.chars().next() {
                Some(c) if c >= 'A' && c <= 'P' => c,
                _ => continue,
            };
            let row = (row_char as u8 - b'A') as usize;
            for (col, slot) in slots.iter().enumerate() {
                let clip = match slot { Some(c) => c, None => continue };
                match &clip.source {
                    PatternSource::Sf2 { path, bank, preset, .. } => {
                        // midi_channel is 1-based; scheduler uses (midi_channel - 1) & 0x0F
                        let ch = clip.midi_channel.saturating_sub(1) & 0x0F;
                        sf2.entry(path.clone()).or_default().push(Sf2Entry { row, col, ch, bank: *bank, preset: *preset });
                    }
                    PatternSource::AudioFile { path, looping, original_bpm, .. } => {
                        audio.push(AudioEntry { row, col, path: path.clone(), looping: *looping, bpm: *original_bpm });
                    }
                    _ => {}
                }
            }
        }
        (sf2, audio)
    };

    // One synth per unique SF2 path: load the file once, configure all channels.
    let sf2_count = sf2_by_path.len();
    for (path, entries) in sf2_by_path {
        let channels: Vec<(u8, u8, u8)> = entries.iter()
            .map(|e| (e.ch, e.bank, e.preset))
            .collect();
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
        tracing::info!(
            "rebuild_audio_slots: loading SF2 '{}' with {} ch: {:?}",
            fname, channels.len(), channels
        );
        let ae = app.audio_engine.as_mut().unwrap();
        let slot_id = ae.load_sf2_multi(path, channels);
        app.sf2_slots.insert(slot_id);
        for e in &entries {
            let clip_key = format!("{}{}", (b'A' + e.row as u8) as char, e.col);
            app.audio_slots.insert(clip_key, slot_id);
        }
    }
    if sf2_count > 0 {
        app.set_status(format!("Loading {} SF2 file(s)… press play when 'SF2 ready' appears", sf2_count));
    }

    // One slot per audio clip (files can share PCM via AssetCache dedup).
    for e in audio_clips {
        let ae = app.audio_engine.as_mut().unwrap();
        let slot_id = ae.load_audio_file(e.path, e.looping, e.bpm);
        let clip_key = format!("{}{}", (b'A' + e.row as u8) as char, e.col);
        app.audio_slots.insert(clip_key, slot_id);
    }

    app.engine.set_audio_slots(app.audio_slots.clone());

    // Sync per-slot send levels and bus volumes with the project's channel data.
    sync_audio_sends(app);
}

/// Propagate channel send_a/send_b → audio engine slot sends, and bus volumes/mutes.
fn sync_audio_sends(app: &mut App) {
    let ae = match app.audio_engine.as_mut() { Some(e) => e, None => return };

    // Build slot send levels from audio_slots (clip_key → slot_id) × channels (by row index).
    let sends: Vec<(u32, f32, f32)> = {
        let proj = app.project.lock();
        app.audio_slots
            .iter()
            .filter_map(|(clip_key, &slot_id)| {
                // clip_key format: "A0", "B3", etc.
                let row_char = clip_key.chars().next()?;
                if row_char < 'A' || row_char > 'P' { return None; }
                let row = (row_char as u8 - b'A') as usize;
                let ch = proj.channels.get(row)?;
                let sa = ch.send_a as f32 / 127.0;
                let sb = ch.send_b as f32 / 127.0;
                Some((slot_id, sa, sb))
            })
            .collect()
    };
    for (slot_id, sa, sb) in sends {
        ae.send(seqterm_audio_engine::AudioCommand::SetSlotSends { slot_id, send_a: sa, send_b: sb });
    }

    // Sync bus return volumes and mute flags.
    let buses: Vec<(f32, bool)> = {
        let proj = app.project.lock();
        proj.buses.iter().map(|b| {
            let vol = 10.0_f32.powf(b.volume / 20.0); // dBFS → linear
            (vol, b.muted)
        }).collect()
    };
    for (idx, (vol, muted)) in buses.into_iter().enumerate() {
        ae.send(seqterm_audio_engine::AudioCommand::SetBusVolume { bus_idx: idx, volume: vol });
        ae.send(seqterm_audio_engine::AudioCommand::SetBusMuted  { bus_idx: idx, muted });
    }
}

/// Open ALSA connections to all enabled MIDI outputs and route them to the
/// scheduler's clock output. Called whenever sync mode changes.
fn rebuild_clock_ports(app: &mut App, enabled: bool) {
    app.engine.set_midi_clock_out(enabled);
    if !enabled {
        app.engine.set_clock_ports(Vec::new());
        return;
    }
    let dest_names: Vec<String> = {
        let proj = app.project.lock();
        proj.midi_outputs
            .iter()
            .filter(|p| p.enabled)
            .map(|p| p.name.clone())
            .collect()
    };
    let port_map = seqterm_midi::open_output_connections(&dest_names);
    let senders: Vec<flume::Sender<Vec<u8>>> = port_map.into_values().collect();
    let count = senders.len();
    app.engine.set_clock_ports(senders);
    app.set_timed_status(
        format!("MIDI clock OUT {} — {} port(s)", if enabled { "enabled" } else { "disabled" }, count),
        3,
    );
}

fn do_save_project(app: &mut App, path: &std::path::Path) {
    let proj = app.project.lock().clone();
    match seqterm_persistence::save_project_auto(&proj, path) {
        Ok(()) => {
            app.project_path  = Some(path.to_path_buf());
            app.project_dirty = false;
            seqterm_persistence::push_recent_project(path);
            app.recent_projects = seqterm_persistence::load_recent_projects();
            app.active_modal = None;
            // Create sibling assets directory silently.
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let assets_dir = path.with_file_name(format!("{stem}_assets"));
                let _ = std::fs::create_dir_all(&assets_dir);
            }
            // Project versioning: write a numbered snapshot alongside the main file.
            if app.settings.project_versioning {
                if let Some(ver_path) = seqterm_persistence::next_versioned_path(path) {
                    let _ = seqterm_persistence::save_project_auto(&proj, &ver_path);
                }
            }
            // Save undo history alongside the project (best-effort, silent on failure).
            let _ = seqterm_history::save_history(&app.history, path);
            app.set_timed_status(format!("Saved: {}", path.display()), 2);
        }
        Err(e) => {
            app.active_modal = Some(Modal::alert("Save Failed", format!("{e}")));
        }
    }
}

fn do_import_midi(app: &mut App, path: PathBuf) {
    let last_sf2 = app.settings.last_sf2_path.clone();
    app.active_modal = Some(Modal::MidiImportOptions(
        MidiImportOptionsState::with_last_sf2(path, last_sf2),
    ));
}

fn do_import_midi_run(app: &mut App, path: PathBuf, opts: seqterm_midi_io::MidiImportOptions) {
    // Persist the SF2 choice so the next import dialog pre-fills it.
    if let Some(sf2) = &opts.sf2_path {
        app.settings.last_sf2_path = Some(sf2.clone());
        let _ = seqterm_persistence::save_settings(&app.settings);
    }
    let (tx, rx) = flume::bounded(1);
    app.midi_import_rx = Some(rx);
    app.active_modal = Some(Modal::progress("Importing MIDI", "Parsing…"));
    let path2 = path.clone();
    match std::thread::Builder::new()
        .name("midi-import".to_string())
        .spawn(move || {
            // catch_unwind converts panics into Err so the UI shows an error modal
            // instead of crashing the thread silently.
            let result = std::panic::catch_unwind(|| {
                seqterm_midi_io::import_midi(&path2, &opts)
            })
            .unwrap_or_else(|e| {
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "internal error during MIDI import".to_string()
                };
                tracing::error!("MIDI import panic: {msg}");
                Err(anyhow::anyhow!("Import crashed: {msg}"))
            })
            .map_err(|e| e.to_string());
            let _ = tx.send(result);
        })
    {
        Ok(_) => {
            seqterm_persistence::push_recent_midi_import(&path);
            app.recent_midi_imports = seqterm_persistence::load_recent_midi_imports();
        }
        Err(e) => {
            app.midi_import_rx = None;
            app.active_modal = Some(Modal::error("Import Error", format!("Could not start import thread: {e}")));
        }
    }
}

fn do_export_midi(app: &mut App, path: &std::path::Path) {
    let proj = app.project.lock().clone();
    match seqterm_midi_io::export_midi(&proj, path) {
        Ok(()) => {
            app.active_modal = None;
            app.status_msg = format!("MIDI exported: {}", path.display());
        }
        Err(e) => {
            app.active_modal = Some(Modal::alert("Export Failed", format!("{e}")));
        }
    }
}

fn do_export_midi_active_only(app: &mut App, path: &std::path::Path) {
    let proj = app.project.lock().clone();
    match seqterm_midi_io::export_midi_active_only(&proj, path) {
        Ok(()) => {
            app.active_modal = None;
            app.status_msg = format!("MIDI exported (active rows): {}", path.display());
        }
        Err(e) => {
            app.active_modal = Some(Modal::alert("Export Failed", format!("{e}")));
        }
    }
}

fn do_export_audio(app: &mut App, path: &std::path::Path) {
    use seqterm_audio_export::ExportMode;
    use seqterm_audio_engine::{render_offline_mixdown, render_offline_stem};

    let opts = app.audio_export_opts.clone();
    let proj = app.project.lock().clone();
    let path = path.to_path_buf();
    let sr = opts.sample_rate;
    let bd = opts.bit_depth;

    let (tx, rx) = flume::unbounded::<AudioExportMsg>();

    std::thread::Builder::new()
        .name("audio-export".to_string())
        .spawn(move || {
            match opts.mode {
                ExportMode::Stems => {
                    let active_rows: Vec<String> = (0u8..8)
                        .filter_map(|row| {
                            let key = ((b'A' + row) as char).to_string();
                            let has = proj.matrix.get(&key)
                                .map(|s| s.iter().any(|c| c.as_ref().map_or(false, |c| c.enabled)))
                                .unwrap_or(false);
                            if has { Some(key) } else { None }
                        })
                        .collect();

                    let total = active_rows.len().max(1);
                    let dir = path.parent().unwrap_or(std::path::Path::new("."));
                    let base = path.file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "export".to_string());

                    let mut written = 0usize;
                    for (i, row_key) in active_rows.iter().enumerate() {
                        let stem_path = dir.join(format!("{base}_{row_key}.wav"));
                        let stem_frac_base = i as f32 / total as f32;
                        let stem_frac_range = 1.0 / total as f32;
                        let tx2 = tx.clone();
                        let res = render_offline_stem(
                            proj.clone(), row_key, &stem_path, sr, bd,
                            |frac, msg| {
                                let _ = tx2.send(AudioExportMsg::Update {
                                    fraction: stem_frac_base + frac * stem_frac_range,
                                    message: format!("Stem {row_key} ({}/{total}): {msg}", i + 1),
                                });
                            },
                        );
                        match res {
                            Ok(_) => written += 1,
                            Err(e) => {
                                let _ = tx.send(AudioExportMsg::Error(format!("{e}")));
                                return;
                            }
                        }
                    }
                    let _ = tx.send(AudioExportMsg::Done(format!(
                        "Stems exported: {written} files ({sr} Hz / {bd}-bit)"
                    )));
                }
                ExportMode::Mixdown => {
                    let _ = tx.send(AudioExportMsg::Update {
                        fraction: 0.0,
                        message: "Starting offline render…".to_string(),
                    });
                    let tx2 = tx.clone();
                    let res = render_offline_mixdown(proj, &path, sr, bd, |frac, msg| {
                        let _ = tx2.send(AudioExportMsg::Update {
                            fraction: frac,
                            message: msg.to_string(),
                        });
                    });
                    match res {
                        Ok(_) => {
                            let _ = tx.send(AudioExportMsg::Done(format!(
                                "Audio exported: {} ({sr} Hz / {bd}-bit)",
                                path.display()
                            )));
                        }
                        Err(e) => {
                            let _ = tx.send(AudioExportMsg::Error(format!("{e}")));
                        }
                    }
                }
            }
        })
        .expect("audio-export thread");

    app.audio_export_rx = Some(rx);
    app.active_modal = Some(Modal::progress("Exporting Audio", "Starting…"));
}

// ─── Modal keyboard handler ───────────────────────────────────────────────────

fn handle_modal_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    let Some(modal) = &app.active_modal else { return false; };

    match modal {
        Modal::Alert { .. } => {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => { app.active_modal = None; }
                _ => {}
            }
            return true;
        }
        Modal::Confirm { on_confirm, .. } => {
            let cmd = on_confirm.clone();
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    app.active_modal = None;
                    dispatch_command(app, cmd);
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    app.active_modal = None;
                }
                _ => {}
            }
            return true;
        }
        Modal::QuitConfirm => {
            match key.code {
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    app.active_modal = None;
                    dispatch_command(app, AppCommand::SaveAndExit);
                }
                KeyCode::Enter | KeyCode::Char('x') | KeyCode::Char('X') => {
                    app.active_modal = None;
                    dispatch_command(app, AppCommand::ExitConfirmed);
                }
                KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C') => {
                    app.active_modal = None;
                }
                _ => {}
            }
            return true;
        }
        Modal::Progress { cancelable, .. } => {
            if *cancelable && key.code == KeyCode::Esc {
                app.active_modal = None;
                app.midi_import_rx = None;
                app.audio_export_rx = None;
            }
            return true;
        }
        Modal::FilePicker(_) => {
            handle_file_picker_key(app, key);
            return true;
        }
        Modal::About => {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    app.active_modal = None;
                }
                _ => {}
            }
            return true;
        }
        Modal::Help(_) => {
            handle_help_key(app, key);
            return true;
        }
        Modal::AudioSettings(_) => {
            handle_audio_settings_key(app, key);
            return true;
        }
        Modal::MidiSettings(_) => {
            handle_midi_settings_key(app, key);
            return true;
        }
        Modal::CommandPalette(_) => {
            handle_command_palette_key(app, key);
            return true;
        }
        Modal::Input(_) => {
            handle_input_dialog_key(app, key);
            return true;
        }
        Modal::MidiImportOptions(_) => {
            handle_midi_import_options_key(app, key);
            return true;
        }
        Modal::KeybindingsEditor(_) => {
            handle_keybindings_editor_key(app, key);
            return true;
        }
        Modal::AudioExportOptions(_) => {
            handle_audio_export_options_key(app, key);
            return true;
        }
        Modal::Sf2Browser(_) => {
            handle_sf2_browser_key(app, key);
            return true;
        }
        Modal::PluginParams(_) => {
            handle_plugin_params_key(app, key);
            return true;
        }
        Modal::SourcePicker(_) => {
            handle_source_picker_key(app, key);
            return true;
        }
    }
}

fn sf2_preview_stop(app: &mut App) {
    let slot = if let Some(modal::Modal::Sf2Browser(s)) = &app.active_modal {
        s.preview_slot
    } else {
        None
    };
    if let Some(slot_id) = slot {
        if let Some(ae) = app.audio_engine.as_mut() {
            ae.send(seqterm_audio_engine::AudioCommand::NoteOff { slot_id, channel: 0, note: 60 });
            ae.send(seqterm_audio_engine::AudioCommand::UnloadSlot { slot_id });
        }
        if let Some(modal::Modal::Sf2Browser(s)) = &mut app.active_modal {
            s.preview_slot   = None;
            s.preview_loaded = false;
        }
    }
}

fn handle_sf2_browser_key(app: &mut App, key: crossterm::event::KeyEvent) {
    let total = if let Some(modal::Modal::Sf2Browser(s)) = &app.active_modal {
        s.filtered_presets().len()
    } else {
        return
    };

    match key.code {
        KeyCode::Esc => {
            sf2_preview_stop(app);
            app.active_modal = None;
        }
        // ← / → cycle through banks in the combobox.
        KeyCode::Left | KeyCode::Char('h') => {
            if let Some(modal::Modal::Sf2Browser(state)) = &mut app.active_modal {
                state.shift_bank(-1);
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if let Some(modal::Modal::Sf2Browser(state)) = &mut app.active_modal {
                state.shift_bank(1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(modal::Modal::Sf2Browser(state)) = &mut app.active_modal {
                if state.cursor > 0 { state.cursor -= 1; }
                state.clamp_scroll(18);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(modal::Modal::Sf2Browser(state)) = &mut app.active_modal {
                if total > 0 && state.cursor < total - 1 { state.cursor += 1; }
                state.clamp_scroll(18);
            }
        }
        KeyCode::Enter => {
            let data = if let Some(modal::Modal::Sf2Browser(s)) = &app.active_modal {
                s.selected().map(|(b, p, _)| (s.path.clone(), s.row, s.col, b, p))
            } else {
                None
            };
            if let Some((path, row, col, bank, preset)) = data {
                sf2_preview_stop(app);
                app.active_modal = None;
                dispatch_command(app, AppCommand::ConfirmSf2Assignment { row, col, path, bank, preset });
            }
        }
        // Space: trigger a preview note for the currently selected preset.
        KeyCode::Char(' ') => {
            let data = if let Some(modal::Modal::Sf2Browser(s)) = &app.active_modal {
                s.selected().map(|(b, p, _)| (s.path.clone(), b, p, s.preview_slot))
            } else {
                None
            };
            if let Some((path, bank, preset, old_slot)) = data {
                if let Some(old_id) = old_slot {
                    if let Some(ae) = app.audio_engine.as_mut() {
                        ae.send(seqterm_audio_engine::AudioCommand::NoteOff { slot_id: old_id, channel: 0, note: 60 });
                        ae.send(seqterm_audio_engine::AudioCommand::UnloadSlot { slot_id: old_id });
                    }
                }
                let new_slot = if let Some(ae) = app.audio_engine.as_mut() {
                    Some(ae.load_sf2(path, bank, preset))
                } else {
                    None
                };
                if let Some(modal::Modal::Sf2Browser(s)) = &mut app.active_modal {
                    s.preview_slot   = new_slot;
                    s.preview_loaded = false;
                }
            }
        }
        _ => {}
    }
}

fn handle_plugin_params_key(app: &mut App, key: crossterm::event::KeyEvent) {
    use modal::Modal;
    let total = if let Some(Modal::PluginParams(s)) = &app.active_modal { s.params.len() } else { return };
    const VIEWPORT: usize = 20;

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.active_modal = None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(Modal::PluginParams(s)) = &mut app.active_modal {
                if s.cursor > 0 { s.cursor -= 1; }
                s.clamp_scroll(VIEWPORT);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(Modal::PluginParams(s)) = &mut app.active_modal {
                if total > 0 && s.cursor < total - 1 { s.cursor += 1; }
                s.clamp_scroll(VIEWPORT);
            }
        }
        // Left/Right: nudge selected parameter value by ±1%
        KeyCode::Left | KeyCode::Char('h') => {
            let data = if let Some(Modal::PluginParams(s)) = &app.active_modal {
                s.params.get(s.cursor).map(|p| (s.registry_id, p.id, (p.value - 0.01).clamp(0.0, 1.0)))
            } else { None };
            if let Some((rid, pid, new_val)) = data {
                app.plugin_registry.set_param(rid, pid, new_val);
                if let Some(Modal::PluginParams(s)) = &mut app.active_modal {
                    s.refresh(&app.plugin_registry);
                }
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            let data = if let Some(Modal::PluginParams(s)) = &app.active_modal {
                s.params.get(s.cursor).map(|p| (s.registry_id, p.id, (p.value + 0.01).clamp(0.0, 1.0)))
            } else { None };
            if let Some((rid, pid, new_val)) = data {
                app.plugin_registry.set_param(rid, pid, new_val);
                if let Some(Modal::PluginParams(s)) = &mut app.active_modal {
                    s.refresh(&app.plugin_registry);
                }
            }
        }
        // r: refresh parameter list from plugin
        KeyCode::Char('r') => {
            if let Some(Modal::PluginParams(s)) = &mut app.active_modal {
                s.refresh(&app.plugin_registry);
            }
        }
        _ => {}
    }
}

fn handle_source_picker_key(app: &mut App, key: crossterm::event::KeyEvent) {
    use modal::{Modal, SourceKind};
    use modal::{FilePickerState, FilePickerTarget};

    let (row, col, cursor, port_cursor, port_count) = if let Some(Modal::SourcePicker(s)) = &app.active_modal {
        (s.row, s.col, s.cursor, s.port_cursor, s.midi_ports.len())
    } else { return };

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => { app.active_modal = None; }

        // ↑↓ navigate between the 3 source type options.
        KeyCode::Up   | KeyCode::Char('k') => {
            if let Some(Modal::SourcePicker(s)) = &mut app.active_modal { s.prev(); }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(Modal::SourcePicker(s)) = &mut app.active_modal { s.next(); }
        }

        // ←→ navigate MIDI port list when MIDI option is selected.
        KeyCode::Left  | KeyCode::Char('h') => {
            if cursor == SourceKind::Midi {
                if let Some(Modal::SourcePicker(s)) = &mut app.active_modal {
                    if s.port_cursor > 0 { s.port_cursor -= 1; }
                }
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if cursor == SourceKind::Midi && port_count > 0 {
                if let Some(Modal::SourcePicker(s)) = &mut app.active_modal {
                    if port_cursor + 1 < port_count { s.port_cursor += 1; }
                }
            }
        }

        // Quick-select by letter.
        KeyCode::Char('m') => { if let Some(Modal::SourcePicker(s)) = &mut app.active_modal { s.cursor = SourceKind::Midi; } }
        KeyCode::Char('f') => { if let Some(Modal::SourcePicker(s)) = &mut app.active_modal { s.cursor = SourceKind::Sf2; } }
        KeyCode::Char('a') => { if let Some(Modal::SourcePicker(s)) = &mut app.active_modal { s.cursor = SourceKind::Audio; } }

        // Enter confirms.
        KeyCode::Enter => {
            let port_name = if let Some(Modal::SourcePicker(s)) = &app.active_modal {
                s.midi_ports.get(s.port_cursor).cloned()
            } else { None };
            match cursor {
                SourceKind::Midi => {
                    let port = port_name.unwrap_or_default();
                    app.active_modal = None;
                    dispatch_command(app, AppCommand::AssignMidiPort { row, col, port });
                }
                SourceKind::Sf2 => {
                    app.active_modal = Some(modal::Modal::FilePicker(
                        FilePickerState::new(FilePickerTarget::AssignSf2 { row, col }),
                    ));
                }
                SourceKind::Audio => {
                    app.active_modal = Some(modal::Modal::FilePicker(
                        FilePickerState::new(FilePickerTarget::AssignAudioFile { row, col }),
                    ));
                }
            }
        }
        _ => {}
    }
}

fn handle_file_picker_key(app: &mut App, key: crossterm::event::KeyEvent) {
    let Some(Modal::FilePicker(state)) = &mut app.active_modal else { return; };
    let is_open       = state.target.mode() == modal::FilePickerMode::Open;
    let tree_focused  = state.tree_focused;
    let input_focused = state.input_focused;

    match key.code {
        KeyCode::Esc => { app.active_modal = None; }

        // Tab: toggle sidebar focus (Open mode) or filename input (Save mode).
        KeyCode::Tab => {
            if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                if is_open {
                    s.tree_focused = !s.tree_focused;
                } else {
                    s.input_focused = !s.input_focused;
                }
            }
        }

        // ── Save-mode filename input ──────────────────────────────────────────
        _ if input_focused => {
            match key.code {
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && state.filename_input.len() < 60 =>
                {
                    state.filename_input.push(c);
                }
                KeyCode::Backspace => { state.filename_input.pop(); }
                KeyCode::Enter => {
                    if let Some(path) = state.selected_path() {
                        let cmd = state.target.into_confirm_command(path);
                        app.active_modal = None;
                        dispatch_command(app, cmd);
                    }
                }
                _ => {}
            }
        }

        // ── Sidebar navigation (when tree is focused) ─────────────────────────
        KeyCode::Up | KeyCode::Char('k') if tree_focused => {
            if let Some(Modal::FilePicker(s)) = &mut app.active_modal { s.sidebar_move_up(); }
        }
        KeyCode::Down | KeyCode::Char('j') if tree_focused => {
            if let Some(Modal::FilePicker(s)) = &mut app.active_modal { s.sidebar_move_down(); }
        }
        KeyCode::Enter if tree_focused => {
            let path_opt = if let Some(Modal::FilePicker(s)) = &app.active_modal {
                s.sidebar.get(s.sidebar_cursor).and_then(|e| e.path.clone())
            } else { None };
            if let Some(path) = path_opt {
                if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                    s.navigate_to(path);
                }
            }
        }

        // ── File list navigation ──────────────────────────────────────────────
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                s.cursor = s.cursor.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                let max = s.visible_entries().len().saturating_sub(1);
                s.cursor = (s.cursor + 1).min(max);
            }
        }
        KeyCode::Enter => {
            let (is_dir, target, path_opt) = {
                let s = if let Some(Modal::FilePicker(s)) = &mut app.active_modal { s } else { return; };
                let entry_is_dir = s.visible_entries().get(s.cursor).map(|e| e.is_dir).unwrap_or(false);
                let t = s.target;
                let p = s.selected_visible_path();
                (entry_is_dir, t, p)
            };
            if is_dir {
                if let Some(Modal::FilePicker(s)) = &mut app.active_modal { s.descend(); }
            } else if let Some(path) = path_opt {
                let cmd = target.into_confirm_command(path);
                app.active_modal = None;
                dispatch_command(app, cmd);
            }
        }
        // Backspace: clear search char or go up one dir.
        KeyCode::Backspace => {
            if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                if is_open && !s.search_input.is_empty() {
                    s.search_input.pop();
                    s.cursor = 0;
                    s.scroll = 0;
                } else {
                    s.ascend();
                }
            }
        }
        KeyCode::Delete if is_open => {
            if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                s.search_input.clear();
                s.cursor = 0;
                s.scroll = 0;
            }
        }
        // H: jump to home.
        KeyCode::Char('~') | KeyCode::Char('H') if {
            matches!(&app.active_modal, Some(Modal::FilePicker(s)) if s.search_input.is_empty())
        } => {
            if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                if let Ok(home) = std::env::var("HOME").map(std::path::PathBuf::from) {
                    s.navigate_to(home);
                }
            }
        }
        // Open mode: printable chars go to search filter.
        KeyCode::Char(c) if is_open && !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                if s.search_input.len() < 60 && !tree_focused {
                    s.search_input.push(c);
                    s.cursor = 0;
                    s.scroll = 0;
                }
            }
        }
        _ => {}
    }
}

fn handle_help_key(app: &mut App, key: crossterm::event::KeyEvent) {
    let Some(Modal::Help(state)) = &mut app.active_modal else { return; };

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => { app.active_modal = None; }
        KeyCode::Up   | KeyCode::Char('k') => { state.scroll = state.scroll.saturating_sub(1); }
        KeyCode::Down | KeyCode::Char('j') => { state.scroll += 1; }
        KeyCode::PageUp                     => { state.scroll = state.scroll.saturating_sub(10); }
        KeyCode::PageDown                   => { state.scroll += 10; }
        KeyCode::Left | KeyCode::Char('h') => {
            let idx = state.sidebar_cursor.saturating_sub(1);
            state.sidebar_cursor = idx;
            state.topic = HelpTopic::all()[idx].clone();
            state.scroll = 0;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            let max = HelpTopic::all().len() - 1;
            let idx = (state.sidebar_cursor + 1).min(max);
            state.sidebar_cursor = idx;
            state.topic = HelpTopic::all()[idx].clone();
            state.scroll = 0;
        }
        _ => {}
    }
}

fn handle_audio_settings_key(app: &mut App, key: crossterm::event::KeyEvent) {
    let Some(Modal::AudioSettings(state)) = &mut app.active_modal else { return; };

    match key.code {
        KeyCode::Esc => { app.active_modal = None; }
        KeyCode::Up | KeyCode::Char('k') => {
            state.cursor = state.cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.cursor = (state.cursor + 1).min(3);
        }
        KeyCode::Left | KeyCode::Char('h') => {
            adjust_audio_setting(app, -1);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            adjust_audio_setting(app, 1);
        }
        KeyCode::Enter => {
            // Capture what changed before closing the modal.
            let (orig_backend, orig_sr) =
                if let Some(Modal::AudioSettings(s)) = &app.active_modal {
                    (s.original_backend.clone(), s.original_sample_rate)
                } else {
                    (String::new(), 0)
                };
            app.active_modal = None;
            let _ = seqterm_persistence::save_settings(&app.settings);
            // Warn if backend or sample rate changed — these require a restart.
            let backend_changed     = app.settings.audio.backend != orig_backend;
            let sample_rate_changed = app.settings.audio.sample_rate != orig_sr;
            if backend_changed || sample_rate_changed {
                app.active_modal = Some(Modal::alert(
                    "Restart Required",
                    "Backend / sample rate changes take effect after restarting SeqTerm.",
                ));
            } else {
                app.set_timed_status("Audio settings saved".to_string(), 2);
            }
        }
        _ => {}
    }
}

fn adjust_audio_setting(app: &mut App, delta: i32) {
    let cursor = if let Some(Modal::AudioSettings(s)) = &app.active_modal { s.cursor } else { return; };
    match cursor {
        0 => {
            // Cycle through available backends.
            let backends = ["AUTO", "JACK", "PIPEWIRE", "ALSA"];
            let cur = backends.iter().position(|&b| b == app.settings.audio.backend.to_uppercase().as_str()).unwrap_or(0);
            let next = (cur as i32 + delta).rem_euclid(backends.len() as i32) as usize;
            app.settings.audio.backend = backends[next].to_string();
        }
        1 => {
            // Cycle through available output devices (or reset to "default").
            let devices: Vec<String> = {
                if let Some(ae) = &app.audio_engine {
                    let mut devs: Vec<String> = ae.list_devices()
                        .into_iter()
                        .map(|d| d.name)
                        .collect();
                    devs.insert(0, "default".to_string());
                    devs
                } else {
                    vec!["default".to_string()]
                }
            };
            let cur = devices.iter().position(|d| d == &app.settings.audio.device).unwrap_or(0);
            let next = (cur as i32 + delta).rem_euclid(devices.len() as i32) as usize;
            app.settings.audio.device = devices[next].clone();
        }
        2 => {
            let rates = [44100u32, 48000, 88200, 96000];
            let cur = rates.iter().position(|&r| r == app.settings.audio.sample_rate).unwrap_or(1);
            let next = (cur as i32 + delta).rem_euclid(rates.len() as i32) as usize;
            app.settings.audio.sample_rate = rates[next];
        }
        3 => {
            let bufs = [64u32, 128, 256, 512, 1024];
            let cur = bufs.iter().position(|&b| b == app.settings.audio.buffer_size).unwrap_or(2);
            let next = (cur as i32 + delta).rem_euclid(bufs.len() as i32) as usize;
            app.settings.audio.buffer_size = bufs[next];
        }
        _ => {}
    }
}

fn handle_command_palette_key(app: &mut App, key: crossterm::event::KeyEvent) {
    let Some(Modal::CommandPalette(state)) = &mut app.active_modal else { return; };

    match key.code {
        KeyCode::Esc => { app.active_modal = None; }
        KeyCode::Up | KeyCode::Char('k') => {
            state.cursor = state.cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            let max = state.results.len().saturating_sub(1);
            state.cursor = (state.cursor + 1).min(max);
        }
        KeyCode::Enter => {
            let cmd = if let Some(Modal::CommandPalette(s)) = &app.active_modal {
                s.selected()
            } else {
                None
            };
            app.active_modal = None;
            if let Some(cmd) = cmd {
                dispatch_command(app, cmd);
            }
        }
        KeyCode::Backspace => {
            if let Some(Modal::CommandPalette(s)) = &mut app.active_modal {
                s.query.pop();
                s.update_filter();
            }
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(Modal::CommandPalette(s)) = &mut app.active_modal {
                if s.query.len() < 40 {
                    s.query.push(c);
                    s.update_filter();
                }
            }
        }
        _ => {}
    }
}

fn handle_input_dialog_key(app: &mut App, key: crossterm::event::KeyEvent) {
    let Some(Modal::Input(state)) = &mut app.active_modal else { return };

    match key.code {
        KeyCode::Esc => { app.active_modal = None; }
        KeyCode::Backspace => { state.value.pop(); }
        KeyCode::Enter => {
            if let Some(Modal::Input(s)) = &app.active_modal {
                let value = s.value.clone();
                let cmd = (s.on_submit)(value);
                app.active_modal = None;
                dispatch_command(app, cmd);
            }
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL)
            && app.active_modal
                .as_ref()
                .map(|m| matches!(m, Modal::Input(s) if s.value.len() < 60))
                .unwrap_or(false) =>
        {
            if let Some(Modal::Input(s)) = &mut app.active_modal {
                s.value.push(c);
            }
        }
        _ => {}
    }
}

fn handle_midi_import_options_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Esc => { app.active_modal = None; }
        KeyCode::Up   | KeyCode::Char('k') => {
            if let Some(Modal::MidiImportOptions(s)) = &mut app.active_modal {
                s.cursor = s.cursor.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(Modal::MidiImportOptions(s)) = &mut app.active_modal {
                s.cursor = (s.cursor + 1).min(3); // now 4 rows (0-3)
            }
        }
        KeyCode::Left | KeyCode::Char('h') => {
            if let Some(Modal::MidiImportOptions(s)) = &mut app.active_modal {
                if s.cursor == 3 {
                    s.opts.sf2_path = None; // ← clears SF2 selection
                } else {
                    adjust_import_option(s, -1);
                }
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if let Some(Modal::MidiImportOptions(s)) = &mut app.active_modal {
                adjust_import_option(s, 1);
            }
        }
        KeyCode::Enter => {
            let cursor = if let Some(Modal::MidiImportOptions(s)) = &app.active_modal {
                s.cursor
            } else { return };

            if cursor == 3 {
                // Row 3 = SF2 selection — save state and open file picker.
                use modal::{FilePickerState, FilePickerTarget};
                if let Some(Modal::MidiImportOptions(s)) = &app.active_modal {
                    app.pending_midi_import = Some((s.path.clone(), s.opts.clone()));
                }
                app.active_modal = Some(Modal::FilePicker(
                    FilePickerState::new(FilePickerTarget::AssignSf2ForMidiImport),
                ));
            } else {
                let cmd = if let Some(Modal::MidiImportOptions(s)) = &app.active_modal {
                    Some(AppCommand::ImportMidiWithOptions(s.path.clone(), s.opts.clone()))
                } else { None };
                app.active_modal = None;
                if let Some(cmd) = cmd { dispatch_command(app, cmd); }
            }
        }
        _ => {}
    }
}

fn handle_midi_import_options_scroll(app: &mut App, delta: i32) {
    if let Some(Modal::MidiImportOptions(s)) = &mut app.active_modal {
        adjust_import_option(s, delta);
    }
}

fn adjust_import_option(state: &mut crate::modal::MidiImportOptionsState, delta: i32) {
    match state.cursor {
        0 => {
            // 0 = Full piece (one pattern per track), then 1, 2, 4, 8 bars.
            let choices = [0usize, 1, 2, 4, 8];
            let cur = choices.iter().position(|&v| v == state.opts.bars_per_pattern).unwrap_or(0);
            let next = (cur as i32 + delta).rem_euclid(choices.len() as i32) as usize;
            state.opts.bars_per_pattern = choices[next];
        }
        1 => {
            let choices = [4u32, 8];
            let cur = choices.iter().position(|&v| v == state.opts.steps_per_beat).unwrap_or(0);
            let next = (cur as i32 + delta).rem_euclid(choices.len() as i32) as usize;
            state.opts.steps_per_beat = choices[next];
        }
        2 => { state.opts.detect_drums = !state.opts.detect_drums; }
        _ => {}
    }
}

fn handle_keybindings_editor_key(app: &mut App, key: crossterm::event::KeyEvent) {
    let Some(Modal::KeybindingsEditor(state)) = &mut app.active_modal else { return; };

    // In rebinding mode: any key press becomes the new binding.
    if let Some(action) = state.rebinding.clone() {
        match key.code {
            KeyCode::Esc => { state.rebinding = None; }
            _ => {
                let key_str = keycode_to_str(key.code);
                let mods = keymodifiers_to_str(key.modifiers);
                if let Some(b) = state.bindings.iter_mut().find(|b| b.action == action) {
                    b.key       = key_str;
                    b.modifiers = mods;
                    state.dirty = true;
                }
                state.rebinding = None;
            }
        }
        return;
    }

    let n = state.bindings.len();
    match key.code {
        KeyCode::Esc => {
            if state.dirty {
                app.settings.keybindings = if let Some(Modal::KeybindingsEditor(s)) = &app.active_modal {
                    s.bindings.clone()
                } else { return };
                let _ = seqterm_persistence::save_settings(&app.settings);
            }
            app.active_modal = None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.cursor = state.cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if n > 0 { state.cursor = (state.cursor + 1).min(n - 1); }
        }
        KeyCode::Enter => {
            // Enter rebind mode for the current row.
            if let Some(b) = state.bindings.get(state.cursor) {
                state.rebinding = Some(b.action.clone());
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            // Reset all bindings to defaults.
            state.bindings = seqterm_persistence::default_keybindings();
            state.dirty = true;
        }
        KeyCode::Delete | KeyCode::Char('d') => {
            // Clear the binding for the current row (set key to empty).
            if let Some(b) = state.bindings.get_mut(state.cursor) {
                b.key.clear();
                b.modifiers.clear();
                state.dirty = true;
            }
        }
        KeyCode::Char('e') | KeyCode::Char('E') => {
            // Export keybindings to a file.
            app.active_modal = Some(Modal::FilePicker(
                FilePickerState::new(FilePickerTarget::ExportKeybindings),
            ));
        }
        KeyCode::Char('i') | KeyCode::Char('I') => {
            // Import keybindings from a file.
            app.active_modal = Some(Modal::FilePicker(
                FilePickerState::new(FilePickerTarget::ImportKeybindings),
            ));
        }
        _ => {}
    }
}

/// Convert a `KeyCode` to a human-readable string for storage.
fn keycode_to_str(code: KeyCode) -> String {
    match code {
        KeyCode::Char(c)  => c.to_string(),
        KeyCode::F(n)     => format!("F{n}"),
        KeyCode::Enter    => "Enter".to_string(),
        KeyCode::Esc      => "Esc".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete   => "Delete".to_string(),
        KeyCode::Tab      => "Tab".to_string(),
        KeyCode::Up       => "Up".to_string(),
        KeyCode::Down     => "Down".to_string(),
        KeyCode::Left     => "Left".to_string(),
        KeyCode::Right    => "Right".to_string(),
        KeyCode::Home     => "Home".to_string(),
        KeyCode::End      => "End".to_string(),
        KeyCode::PageUp   => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Insert   => "Insert".to_string(),
        _ => format!("{code:?}"),
    }
}

/// Convert `KeyModifiers` to a "ctrl+alt+shift" style string.
fn keymodifiers_to_str(mods: KeyModifiers) -> String {
    let mut parts = Vec::new();
    if mods.contains(KeyModifiers::CONTROL) { parts.push("ctrl"); }
    if mods.contains(KeyModifiers::ALT)     { parts.push("alt"); }
    if mods.contains(KeyModifiers::SHIFT)   { parts.push("shift"); }
    parts.join("+")
}

fn handle_midi_settings_key(app: &mut App, key: crossterm::event::KeyEvent) {
    let Some(Modal::MidiSettings(state)) = &mut app.active_modal else { return; };

    match key.code {
        KeyCode::Esc => { app.active_modal = None; }
        KeyCode::Tab => { state.tab = (state.tab + 1) % 3; state.cursor = 0; }
        KeyCode::Up   | KeyCode::Char('k') => { state.cursor = state.cursor.saturating_sub(1); }
        KeyCode::Down | KeyCode::Char('j') => {
            let proj = app.project.lock();
            let max = match state.tab { 0 => proj.midi_inputs.len(), 1 => proj.midi_outputs.len(), _ => 4 };
            drop(proj);
            state.cursor = (state.cursor + 1).min(max.saturating_sub(1));
        }
        KeyCode::Char('e') => {
            let cursor = state.cursor;
            let tab = state.tab;
            let mut proj = app.project.lock();
            match tab {
                0 => {
                    if let Some(p) = proj.midi_inputs.get_mut(cursor) { p.enabled = !p.enabled; }
                    drop(proj);
                    app.sync_midi_input_bus();
                    return;
                }
                1 => { if let Some(p) = proj.midi_outputs.get_mut(cursor) { p.enabled = !p.enabled; } }
                2 => {
                    use seqterm_core::SyncMode;
                    let modes = [SyncMode::Internal, SyncMode::Usb, SyncMode::Midi, SyncMode::Clock];
                    if let Some(m) = modes.get(cursor) {
                        proj.sync_mode = m.clone();
                        let is_clock = matches!(m, SyncMode::Clock);
                        drop(proj); // release lock before rebuilding ports
                        rebuild_clock_ports(app, is_clock);
                        return;
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

// ─── Audio export options keyboard handler ────────────────────────────────────

fn handle_audio_export_options_key(app: &mut App, key: crossterm::event::KeyEvent) {
    let Some(Modal::AudioExportOptions(state)) = &mut app.active_modal else { return };
    const ROWS: usize = 3; // sample_rate, bit_depth, mode

    match key.code {
        KeyCode::Esc => { app.active_modal = None; }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.cursor > 0 { state.cursor -= 1; }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.cursor + 1 < ROWS { state.cursor += 1; }
        }
        KeyCode::Left | KeyCode::Char('h') => {
            match state.cursor {
                0 => { if state.sample_rate_idx > 0 { state.sample_rate_idx -= 1; } }
                1 => { if state.bit_depth_idx > 0 { state.bit_depth_idx -= 1; } }
                2 => { state.stems = false; }
                _ => {}
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => {
            match state.cursor {
                0 => {
                    if state.sample_rate_idx + 1 < modal::EXPORT_SAMPLE_RATES.len() {
                        state.sample_rate_idx += 1;
                    }
                }
                1 => {
                    if state.bit_depth_idx + 1 < modal::EXPORT_BIT_DEPTHS.len() {
                        state.bit_depth_idx += 1;
                    }
                }
                2 => { state.stems = !state.stems; }
                _ => {}
            }
        }
        KeyCode::Enter => {
            let opts = state.to_opts();
            app.audio_export_opts = opts;
            app.active_modal = Some(Modal::FilePicker(
                FilePickerState::new(FilePickerTarget::ExportAudio),
            ));
        }
        _ => {}
    }
}

// ─── Menu keyboard handler ────────────────────────────────────────────────────

fn handle_menu_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    let Some(kind) = app.menu_open else { return false; };

    match key.code {
        KeyCode::Esc => {
            app.menu_open = None;
            return true;
        }
        KeyCode::Left => {
            let idx = (kind.index() as i32 - 1).rem_euclid(MenuKind::ALL.len() as i32) as usize;
            app.menu_open   = Some(MenuKind::ALL[idx]);
            app.menu_cursor = 0;
            return true;
        }
        KeyCode::Right => {
            let idx = (kind.index() + 1) % MenuKind::ALL.len();
            app.menu_open   = Some(MenuKind::ALL[idx]);
            app.menu_cursor = 0;
            return true;
        }
        KeyCode::Up => {
            let max = kind.selectable_count();
            if max > 0 {
                app.menu_cursor = (app.menu_cursor + max - 1) % max;
            }
            return true;
        }
        KeyCode::Down => {
            let max = kind.selectable_count();
            if max > 0 {
                app.menu_cursor = (app.menu_cursor + 1) % max;
            }
            return true;
        }
        KeyCode::Enter => {
            let item_idx = kind.item_index_for_cursor(app.menu_cursor);
            let action = kind.items().get(item_idx).map(|i| i.action);
            app.menu_open   = None;
            app.menu_cursor = 0;
            if let Some(action) = action {
                if let Some(cmd) = action.to_command() {
                    dispatch_command(app, cmd);
                }
            }
            return true;
        }
        _ => {}
    }
    true // consume all keys while menu is open
}

fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) {
    // Arranger track name editing: intercepts all keystrokes.
    if app.arranger_track_name_editing {
        match key.code {
            KeyCode::Esc => {
                app.arranger_track_name_editing = false;
                app.arranger_track_name_buffer.clear();
                app.status_msg = "Name edit cancelled".to_string();
            }
            KeyCode::Enter => {
                let buf = std::mem::take(&mut app.arranger_track_name_buffer);
                if !buf.is_empty() {
                    app.commit_track_name(&buf);
                }
                app.arranger_track_name_editing = false;
            }
            KeyCode::Backspace => {
                app.arranger_track_name_buffer.pop();
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && app.arranger_track_name_buffer.len() < 12 =>
            {
                let uc = c.to_ascii_uppercase();
                if uc.is_ascii_alphanumeric() || uc == '_' || uc == '-' || uc == ' ' {
                    app.arranger_track_name_buffer.push(uc);
                }
            }
            _ => {}
        }
        return;
    }

    // Pattern name text-edit mode: intercepts all keystrokes.
    if app.pattern_name_editing {
        match key.code {
            KeyCode::Esc => {
                app.pattern_name_editing = false;
                app.pattern_name_buffer.clear();
                app.status_msg = "Name edit cancelled".to_string();
            }
            KeyCode::Enter => {
                let buf = std::mem::take(&mut app.pattern_name_buffer);
                if !buf.is_empty() {
                    app.commit_pattern_name(&buf);
                    app.status_msg = format!("Pattern name → {}", buf);
                }
                app.pattern_name_editing = false;
            }
            KeyCode::Backspace => {
                app.pattern_name_buffer.pop();
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && app.pattern_name_buffer.len() < 8 =>
            {
                let uc = c.to_ascii_uppercase();
                if uc.is_ascii_alphanumeric() || uc == '_' || uc == '-' {
                    app.pattern_name_buffer.push(uc);
                }
            }
            _ => {}
        }
        return;
    }

    // Modal intercept.
    if handle_modal_key(app, key) { return; }

    // Menu intercept.
    if handle_menu_key(app, key) { return; }

    // Alt+key opens menus.
    if key.modifiers.contains(KeyModifiers::ALT) {
        match key.code {
            KeyCode::Char('f') | KeyCode::Char('F') => {
                app.menu_open   = Some(MenuKind::File);
                app.menu_cursor = 0;
                return;
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                app.menu_open   = Some(MenuKind::Edit);
                app.menu_cursor = 0;
                return;
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                app.menu_open   = Some(MenuKind::About);
                app.menu_cursor = 0;
                return;
            }
            KeyCode::Char('h') | KeyCode::Char('H') => {
                app.menu_open   = Some(MenuKind::Help);
                app.menu_cursor = 0;
                return;
            }
            _ => {}
        }
    }

    // Ctrl shortcuts.
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('n') => { dispatch_command(app, AppCommand::NewProject); return; }
            KeyCode::Char('o') => { dispatch_command(app, AppCommand::OpenProject); return; }
            KeyCode::Char('s') => {
                let cmd = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    AppCommand::SaveProjectAs
                } else {
                    AppCommand::SaveProject
                };
                dispatch_command(app, cmd);
                return;
            }
            KeyCode::Char('i') => { dispatch_command(app, AppCommand::ImportMidi); return; }
            KeyCode::Char('e') => { dispatch_command(app, AppCommand::ExportMidi); return; }
            KeyCode::Char('z') => { dispatch_command(app, AppCommand::Undo); return; }
            KeyCode::Char('y') => { dispatch_command(app, AppCommand::Redo); return; }
            KeyCode::Char('p') => { dispatch_command(app, AppCommand::ShowCommandPalette); return; }
            _ => {}
        }
    }

    // F-keys.
    match key.code {
        KeyCode::F(1) => {
            let topic = match app.current_view {
                ViewKind::Tracker  => HelpTopic::PatternEditor,
                ViewKind::Matrix   => HelpTopic::WorkflowGuide,
                ViewKind::Arranger => HelpTopic::WorkflowGuide,
                ViewKind::Mixer    => HelpTopic::WorkflowGuide,
                ViewKind::Config   => HelpTopic::Troubleshooting,
                ViewKind::Granular => HelpTopic::WorkflowGuide,
            };
            dispatch_command(app, AppCommand::ShowHelp(topic));
            return;
        }
        KeyCode::F(12) => { dispatch_command(app, AppCommand::ShowAbout); return; }
        _ => {}
    }

    // Ctrl+R = toggle realtime capture.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
        dispatch_command(app, AppCommand::ToggleCapture);
        return;
    }

    // Ctrl+K = toggle MIDI clock sync.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('k') {
        dispatch_command(app, AppCommand::ToggleMidiClockSync);
        return;
    }

    // ── Tab management ────────────────────────────────────────────────────────
    // Ctrl+T = new tab, Ctrl+W = close tab, Alt+1-9 = switch to tab N.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('t') {
        app.new_tab();
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('w') {
        app.close_tab();
        return;
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        if let KeyCode::Char(c) = key.code {
            if let Some(n) = c.to_digit(10) {
                let idx = (n as usize).saturating_sub(1);
                app.switch_tab(idx);
                return;
            }
        }
    }

    // Quit.
    if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
        dispatch_command(app, AppCommand::Exit);
        return;
    }
    if key.code == KeyCode::Char('q') && !app.tracker_editing && !app.mixer_state.editing {
        dispatch_command(app, AppCommand::Exit);
        return;
    }

    // Escape exits edit modes / returns to Normal vim mode.
    if key.code == KeyCode::Esc {
        if app.vim_mode != crate::app::VimMode::Normal {
            app.vim_mode = crate::app::VimMode::Normal;
            app.visual_start = None;
            app.tracker_editing = false;
            app.set_timed_status("-- NORMAL --", 2);
            return;
        }
        if app.tracker_editing {
            app.tracker_editing = false;
            app.status_msg = "Navigate mode | Enter=edit | i=insert | v=visual".to_string();
            return;
        }
        if app.mixer_state.fx_panel_focused {
            if app.mixer_state.fx_row > 0 {
                app.mixer_state.fx_row = 0; // go back to slot header level
            } else {
                app.mixer_state.fx_panel_focused = false; // unfocus sidebar
            }
            return;
        }
        if app.mixer_state.editing {
            app.mixer_state.editing = false;
            app.status_msg = "MIXER: ←→=channel | ↑↓=volume | m=mute".to_string();
            return;
        }
        if app.current_view == ViewKind::Granular {
            app.switch_view(ViewKind::Matrix);
            return;
        }
        app.status_msg = "Escaped".to_string();
        return;
    }

    // Enter: view-specific action.
    if key.code == KeyCode::Enter {
        match app.current_view {
            ViewKind::Matrix => {
                if app.matrix_section == 1 {
                    match app.transport_cursor {
                        0 => app.play_stop(),
                        1 => app.stop(),
                        2 => app.rewind(),
                        3 => app.toggle_record(),
                        4 => app.tap_tempo(),
                        _ => {}
                    }
                } else if app.matrix_section == 3 {
                    let (row, col) = app.matrix_state.cursor;
                    let row_key = ((b'A' + row as u8) as char).to_string();

                    if app.routing_tab == 1 {
                        // cursor 0 = "Change Source" button → open source picker.
                        if app.routing_source_cursor == 0 {
                            dispatch_command(app, AppCommand::OpenSourcePicker { row, col });
                            return;
                        }
                        // cursor 1..n = assign existing project source (index = cursor - 1).
                        let src_idx = app.routing_source_cursor.saturating_sub(1);
                        let new_source: Option<seqterm_core::PatternSource> = {
                            let proj = app.project.lock();
                            let mut sources: Vec<seqterm_core::PatternSource> = Vec::new();
                            for slots in proj.matrix.values() {
                                for opt in slots {
                                    let Some(clip) = opt else { continue };
                                    let src = &clip.source;
                                    let is_dup = sources.iter().any(|s| {
                                        use seqterm_core::PatternSource::*;
                                        match (s, src) {
                                            (Sf2{path:p1,bank:b1,preset:pr1,..}, Sf2{path:p2,bank:b2,preset:pr2,..}) => p1==p2 && b1==b2 && pr1==pr2,
                                            (AudioFile{path:p1,..}, AudioFile{path:p2,..}) => p1==p2,
                                            _ => false,
                                        }
                                    });
                                    if !is_dup && !matches!(src, seqterm_core::PatternSource::Midi) {
                                        sources.push(src.clone());
                                    }
                                }
                            }
                            sources.into_iter().nth(src_idx)
                        };
                        if let Some(new_src) = new_source {
                            {
                                let mut proj = app.project.lock();
                                if let Some(slots) = proj.matrix.get_mut(&row_key) {
                                    if let Some(Some(clip)) = slots.get_mut(col) {
                                        clip.source = new_src.clone();
                                    }
                                }
                            }
                            app.project_dirty = true;
                            rebuild_audio_slots(app);
                            let label = match &new_src {
                                seqterm_core::PatternSource::Sf2  { preset_name, .. } => format!("Source → SF2 {}", preset_name),
                                seqterm_core::PatternSource::AudioFile { path, .. } => format!("Source → {}", path.file_name().and_then(|n| n.to_str()).unwrap_or("?")),
                                seqterm_core::PatternSource::Midi => "Source → MIDI".to_string(),
                            };
                            app.set_timed_status(label, 2);
                        } else {
                            // No existing sources to browse — open the picker directly.
                            dispatch_command(app, AppCommand::OpenSourcePicker { row, col });
                        }
                    } else {
                        // MIDI tab: assign MIDI output.
                        let cursor = app.routing_cursor;
                        let midi_name: Option<String> = {
                            let proj = app.project.lock();
                            if cursor == 0 {
                                None
                            } else {
                                proj.midi_outputs.get(cursor - 1).map(|p| p.name.clone())
                            }
                        };
                        {
                            let mut proj = app.project.lock();
                            if let Some(slots) = proj.matrix.get_mut(&row_key) {
                                if let Some(Some(clip)) = slots.get_mut(col) {
                                    clip.midi_out = midi_name.clone();
                                }
                            }
                        }
                        rebuild_midi_ports(app);
                        app.status_msg = match &midi_name {
                            Some(name) => format!("MIDI out → {}", name),
                            None       => "MIDI out cleared".to_string(),
                        };
                    }
                } else {
                    app.navigate_matrix_to_tracker();
                }
                return;
            }
            ViewKind::Tracker if app.tracker_section == 1 => {
                // Toggle note at piano cursor crosshair position.
                let (row, step) = app.piano_cursor;
                app.toggle_piano_note_at(row, step);
                return;
            }
            ViewKind::Tracker if app.tracker_section == 2 => {
                match app.generative_cursor {
                    0 => {
                        // Start pattern name editing — seed buffer from the HashMap key
                        // (the canonical name; pat.name is always kept in sync with it).
                        app.pattern_name_buffer = app
                            .tracker_state
                            .pattern_key
                            .clone()
                            .unwrap_or_default();
                        app.pattern_name_editing = true;
                        app.status_msg = "TYPE=edit name  Enter=confirm  Esc=cancel".to_string();
                    }
                    10 => {
                        // Toggle probability lock.
                        let key = app.tracker_state.pattern_key.clone().unwrap_or_default();
                        let new_state = {
                            let mut proj = app.project.lock();
                            proj.patterns.get_mut(&key).map(|pat| {
                                pat.prob_lock = !pat.prob_lock;
                                pat.prob_lock
                            })
                        };
                        if let Some(locked) = new_state {
                            app.status_msg = format!("Prob lock: {}", if locked { "ACTIVE" } else { "OFF" });
                        }
                    }
                    _ => {}
                }
                return;
            }
            ViewKind::Tracker if app.tracker_section == 4 => {
                // FX chain: Enter on empty slot = add FX.
                handle_tracker_fx_enter(app);
                return;
            }
            ViewKind::Arranger if app.arranger_state.section == 0 => {
                let row_key = ((b'A' + app.arranger_state.selected_track as u8) as char).to_string();
                let current_name = {
                    let proj = app.project.lock();
                    proj.track_names.get(&row_key).cloned().unwrap_or(row_key)
                };
                app.arranger_track_name_buffer = current_name;
                app.arranger_track_name_editing = true;
                app.status_msg = "TYPE=track name  Enter=confirm  Esc=cancel".to_string();
                return;
            }
            ViewKind::Arranger if app.arranger_state.section == 2 => {
                match app.arranger_state.song_transport_cursor {
                    0 => app.song_play_stop(),
                    1 => app.song_stop(),
                    2 => app.toggle_record(),
                    _ => {}
                }
                return;
            }
            ViewKind::Mixer => {
                if app.mixer_state.fx_panel_focused {
                    // Enter on slot header → dive into param rows.
                    if app.mixer_state.fx_row == 0 {
                        app.mixer_state.fx_row = 3;
                        app.mixer_state.fx_col = 1;
                    } else {
                        app.mixer_state.fx_row = 0;
                    }
                } else {
                    app.toggle_edit_mode();
                }
                return;
            }
            _ => {
                app.toggle_edit_mode();
                return;
            }
        }
    }

    // 'a' in any Config section syncs the routing graph.
    if key.code == KeyCode::Char('a') && app.current_view == ViewKind::Config {
        app.sync_routing_nodes();
        return;
    }

    // Config audio engine sub-panel keys (section 5).
    if app.current_view == ViewKind::Config && app.config_state.section == 5 {
        handle_config_audio_key(app, key);
        return;
    }

    // Config routing sub-panel keys (section 4 = routing graph).
    if app.current_view == ViewKind::Config && app.config_state.section == 4 {
        handle_routing_key(app, key);
        return;
    }

    // Mixer FX sidebar captures keys when focused.
    if app.current_view == ViewKind::Mixer && app.mixer_state.fx_panel_focused {
        handle_fx_routing_key(app, key);
        return;
    }

    // Tracker FX chain panel captures keys when active.
    if app.current_view == ViewKind::Tracker && app.tracker_section == 4 {
        if handle_tracker_fx_keys(app, key) { return; }
    }

    // View switching with Tab or 1-5.
    if !app.tracker_editing {
        match key.code {
            KeyCode::Tab => {
                if app.current_view == ViewKind::Matrix {
                    // Tab cycles the right-sidebar tab: PANELS ↔ HYBRID.
                    app.sidebar_tab = (app.sidebar_tab + 1) % 2;
                    // Keep matrix_section in sync so keyboard shortcuts still work.
                    match app.sidebar_tab {
                        0 => { app.matrix_section = 2; } // PANELS — focus polymeter
                        _ => { app.matrix_section = 0; } // HYBRID — back to grid focus
                    }
                    app.status_msg = match app.sidebar_tab {
                        0 => "PANELS: ↑↓=select  ←→=scroll  Tab=switch".to_string(),
                        _ => "HYBRID: click clips to select  click steps to seek  Tab=switch".to_string(),
                    };
                } else if app.current_view == ViewKind::Tracker {
                    app.tracker_section = (app.tracker_section + 1) % 5;
                    app.status_msg = match app.tracker_section {
                        0 => "TRACKER: Step editor | hjkl=move  Enter=edit  [/]=len".to_string(),
                        1 => "PIANO ROLL: L-click=place note  L-drag=extend gate  R-click=erase  R-drag=paint erase".to_string(),
                        2 => "GENERATIVE ENGINE: ←→=param  ↑↓=adjust  Tab=next".to_string(),
                        3 => "TRACK MODULATION: ←→=param  ↑↓=adjust  Tab=next".to_string(),
                        _ => String::new(),
                    };
                } else if app.current_view == ViewKind::Arranger {
                    app.arranger_state.section = (app.arranger_state.section + 1) % 3;
                    app.status_msg = match app.arranger_state.section {
                        1 => "ARRANGER: Automation | hjkl=navigate  a=add/remove  Tab=next".to_string(),
                        2 => "ARRANGER: Song transport | ←→=navigate  Enter=trigger  Tab=back".to_string(),
                        _ => "ARRANGER: Tracks | ↑↓=select  ←→=scroll  Tab=next".to_string(),
                    };
                } else if app.current_view == ViewKind::Mixer {
                    app.mixer_state.fx_panel_focused = !app.mixer_state.fx_panel_focused;
                    app.mixer_state.fx_row = 0;
                } else {
                    app.next_view();
                }
                return;
            }
            KeyCode::Char('1') => {
                app.switch_view(ViewKind::Matrix);
                return;
            }
            KeyCode::Char('2') => {
                app.switch_view(ViewKind::Tracker);
                return;
            }
            KeyCode::Char('3') => {
                app.switch_view(ViewKind::Arranger);
                return;
            }
            KeyCode::Char('4') => {
                app.switch_view(ViewKind::Mixer);
                return;
            }
            KeyCode::Char('5') => {
                app.switch_view(ViewKind::Config);
                return;
            }
            KeyCode::Char('6') => {
                app.switch_view(ViewKind::Granular);
                return;
            }
            _ => {}
        }
    }

    // Arranger view: chain editor keys (work anywhere in the Arranger).
    if app.current_view == ViewKind::Arranger {
        match key.code {
            KeyCode::Char('C') => {
                dispatch_command(app, AppCommand::ToggleChainMode);
                return;
            }
            KeyCode::Char('a') if app.arranger_state.section != 0 => {
                // Add current active_scene (0) with 4 bars as a default.
                let scene_count = { app.project.lock().scenes.len() };
                if scene_count > 0 {
                    dispatch_command(app, AppCommand::AddChainEntry { scene_idx: 0, bars: 4 });
                } else {
                    app.set_timed_status("No scenes defined — create scenes first", 3);
                }
                return;
            }
            KeyCode::Delete if app.chain_pos < { app.project.lock().chain.len() } => {
                let pos = app.chain_pos;
                dispatch_command(app, AppCommand::RemoveChainEntry { pos });
                if app.chain_pos > 0 { app.chain_pos -= 1; }
                return;
            }
            _ => {}
        }
    }

    // Global transport (only outside edit mode).
    if !app.tracker_editing && !app.mixer_state.editing {
        match key.code {
            KeyCode::Char(' ') => {
                if app.current_view == ViewKind::Arranger {
                    app.song_play_stop();
                } else {
                    app.play_stop();
                }
                return;
            }
            KeyCode::Char('s') => {
                if app.current_view == ViewKind::Arranger {
                    app.song_stop();
                } else {
                    app.stop();
                }
                return;
            }
            KeyCode::Char('r') => {
                app.toggle_record();
                return;
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                app.adjust_bpm(1.0);
                return;
            }
            KeyCode::Char('-') => {
                app.adjust_bpm(-1.0);
                return;
            }
            _ => {}
        }
    }

    // Navigation (hjkl / arrows).
    let (dr, dc) = match key.code {
        KeyCode::Char('h') | KeyCode::Left => (0, -1),
        KeyCode::Char('l') | KeyCode::Right => (0, 1),
        KeyCode::Char('k') | KeyCode::Up => (-1, 0),
        KeyCode::Char('j') | KeyCode::Down => (1, 0),
        _ => (0, 0),
    };
    if dr != 0 || dc != 0 {
        app.move_cursor(dr, dc);
    }

    // Delete / Backspace: remove clip from matrix slot.
    if matches!(key.code, KeyCode::Delete | KeyCode::Backspace)
        && app.current_view == ViewKind::Matrix
    {
        app.remove_clip_at_cursor();
        return;
    }

    // Vim mode key transitions for tracker step table.
    if app.current_view == ViewKind::Tracker && app.tracker_section == 0 {
        match key.code {
            KeyCode::Char('i') if app.vim_mode == crate::app::VimMode::Normal => {
                app.vim_mode = crate::app::VimMode::Insert;
                app.tracker_editing = true;
                app.set_timed_status("-- INSERT --", 2);
                return;
            }
            KeyCode::Char('v') if app.vim_mode == crate::app::VimMode::Normal => {
                app.vim_mode = crate::app::VimMode::Visual;
                app.visual_start = Some(app.tracker_state.cursor.0);
                app.set_timed_status("-- VISUAL -- ↑↓=extend  y=yank  d=delete  Esc=cancel", 5);
                return;
            }
            // In Visual mode: yank selected steps.
            KeyCode::Char('y') if app.vim_mode == crate::app::VimMode::Visual => {
                let cursor = app.tracker_state.cursor.0;
                let vs = app.visual_start.unwrap_or(cursor);
                let lo = vs.min(cursor);
                let hi = vs.max(cursor);
                let key = app.tracker_state.pattern_key.clone().unwrap_or_default();
                let yanked: Vec<seqterm_core::Note> = {
                    let proj = app.project.lock();
                    (lo..=hi).map(|s| {
                        proj.patterns.get(&key)
                            .and_then(|p| p.steps.get(s))
                            .cloned()
                            .unwrap_or_default()
                    }).collect()
                };
                app.vim_yank_buffer = yanked;
                let n = hi - lo + 1;
                app.vim_mode = crate::app::VimMode::Normal;
                app.visual_start = None;
                app.set_timed_status(format!("Yanked {} step(s) → p to paste", n), 3);
                return;
            }
            // In Visual mode: delete (clear) selected steps.
            KeyCode::Char('d') if app.vim_mode == crate::app::VimMode::Visual => {
                let cursor = app.tracker_state.cursor.0;
                let vs = app.visual_start.unwrap_or(cursor);
                let lo = vs.min(cursor);
                let hi = vs.max(cursor);
                let key = app.tracker_state.pattern_key.clone().unwrap_or_default();
                {
                    let mut proj = app.project.lock();
                    if let Some(pat) = proj.patterns.get_mut(&key) {
                        for s in lo..=hi {
                            if let Some(note) = pat.steps.get_mut(s) {
                                *note = seqterm_core::Note::default();
                            }
                        }
                    }
                }
                app.vim_mode = crate::app::VimMode::Normal;
                app.visual_start = None;
                app.project_dirty = true;
                app.set_timed_status(format!("Deleted {} step(s)", hi - lo + 1), 2);
                return;
            }
            // In Normal mode: paste yank buffer at cursor.
            KeyCode::Char('p') if app.vim_mode == crate::app::VimMode::Normal
                && !app.vim_yank_buffer.is_empty() =>
            {
                let cursor = app.tracker_state.cursor.0;
                let key = app.tracker_state.pattern_key.clone().unwrap_or_default();
                let buf = app.vim_yank_buffer.clone();
                {
                    let mut proj = app.project.lock();
                    if let Some(pat) = proj.patterns.get_mut(&key) {
                        for (offset, note) in buf.iter().enumerate() {
                            let idx = cursor + offset;
                            if idx < pat.steps.len() {
                                pat.steps[idx] = note.clone();
                            }
                        }
                    }
                }
                app.project_dirty = true;
                app.set_timed_status(format!("Pasted {} step(s)", buf.len()), 2);
                return;
            }
            // Normal mode: 0 = go to first step, $ = go to last step.
            KeyCode::Char('0') if app.vim_mode == crate::app::VimMode::Normal => {
                app.tracker_state.cursor.0 = 0;
                app.tracker_scroll = 0;
                return;
            }
            KeyCode::Char('$') if app.vim_mode == crate::app::VimMode::Normal => {
                let len = app.project.lock()
                    .patterns.get(app.tracker_state.pattern_key.as_deref().unwrap_or(""))
                    .map(|p| p.length).unwrap_or(16);
                app.tracker_state.cursor.0 = len.saturating_sub(1);
                return;
            }
            _ => {}
        }
    }

    // View-specific actions.
    match key.code {
        KeyCode::Char('m') => {
            // In matrix grid section: m = grab/drop clip for move. Elsewhere: toggle mute.
            if app.current_view == ViewKind::Matrix && app.matrix_section == 0 {
                let (row, col) = app.matrix_state.cursor;
                if let Some((from_row, from_col)) = app.matrix_state.grabbed_clip {
                    dispatch_command(app, AppCommand::MoveClip {
                        from_row, from_col, to_row: row, to_col: col,
                    });
                } else {
                    let has_clip = app.project.lock()
                        .matrix
                        .get(&((b'A' + row as u8) as char).to_string())
                        .and_then(|s| s.get(col))
                        .map(|s| s.is_some())
                        .unwrap_or(false);
                    if has_clip {
                        app.matrix_state.grabbed_clip = Some((row, col));
                        app.set_timed_status(
                            format!("Clip grabbed from {}{}  — navigate + m=drop  Esc=cancel",
                                (b'A' + row as u8) as char, col + 1),
                            8,
                        );
                    }
                }
            } else {
                app.toggle_mute();
            }
        }
        KeyCode::Char('e')
            if app.current_view == ViewKind::Matrix && app.matrix_section == 0 =>
        {
            app.toggle_clip_enabled();
        }
        // 'f' = assign audio source file (SF2 or audio) to the selected matrix clip.
        KeyCode::Char('f')
            if app.current_view == ViewKind::Matrix && app.matrix_section == 0 =>
        {
            let (row, col) = app.matrix_state.cursor;
            dispatch_command(app, AppCommand::AssignSf2ToClip { row, col });
        }
        // 'F' (shift-f) = assign audio file (WAV/FLAC/MP3) to the selected matrix clip.
        KeyCode::Char('F')
            if app.current_view == ViewKind::Matrix && app.matrix_section == 0 =>
        {
            let (row, col) = app.matrix_state.cursor;
            dispatch_command(app, AppCommand::AssignAudioFileToClip { row, col });
        }
        KeyCode::Char('e') if app.current_view == ViewKind::Config => {
            app.toggle_config_item_enabled();
        }
        // o = toggle OSC server on/off when in Config OSC section (section 2).
        KeyCode::Char('o')
            if app.current_view == ViewKind::Config && app.config_state.section == 2 =>
        {
            if app.osc_port > 0 {
                dispatch_command(app, seqterm_command::AppCommand::StopOscServer);
            } else {
                dispatch_command(app, seqterm_command::AppCommand::StartOscServer(57120));
            }
        }
        // R in routing section forces a MIDI port refresh so new apps (QSynth etc.) appear.
        KeyCode::Char('r') | KeyCode::Char('R')
            if app.current_view == ViewKind::Matrix && app.matrix_section == 3 =>
        {
            app.refresh_midi_ports();
            app.set_timed_status("MIDI ports refreshed".to_string(), 2);
            return;
        }
        // s: in SOURCE tab → open Source Picker; in MIDI OUT tab → switch to SOURCE tab.
        KeyCode::Char('s')
            if app.current_view == ViewKind::Matrix && app.matrix_section == 3 =>
        {
            if app.routing_tab == 1 {
                // Already in SOURCE tab: open the picker.
                let (row, col) = app.matrix_state.cursor;
                dispatch_command(app, AppCommand::OpenSourcePicker { row, col });
            } else {
                // Switch from MIDI OUT to SOURCE tab.
                app.routing_tab = 1;
            }
            return;
        }
        // → in routing panel: switch to MIDI OUT tab (SOURCE→MIDI) or back to grid.
        KeyCode::Right | KeyCode::Char('l')
            if app.current_view == ViewKind::Matrix && app.matrix_section == 3 =>
        {
            if app.routing_tab == 1 {
                app.routing_tab = 0; // SOURCE → MIDI OUT
                app.routing_cursor = 0;
            } else {
                // MIDI OUT → back to grid
                app.matrix_section = 0;
            }
            return;
        }
        // f/F/x work from routing section (source browser tab) as well as section 0.
        KeyCode::Char('f')
            if app.current_view == ViewKind::Matrix && app.matrix_section == 3 =>
        {
            let (row, col) = app.matrix_state.cursor;
            dispatch_command(app, AppCommand::AssignSf2ToClip { row, col });
        }
        KeyCode::Char('F')
            if app.current_view == ViewKind::Matrix && app.matrix_section == 3 =>
        {
            let (row, col) = app.matrix_state.cursor;
            dispatch_command(app, AppCommand::AssignAudioFileToClip { row, col });
        }
        KeyCode::Char('x')
            if app.current_view == ViewKind::Matrix && app.matrix_section == 3 =>
        {
            let (row, col) = app.matrix_state.cursor;
            dispatch_command(app, AppCommand::ClearClipSource { row, col });
        }
        // Esc cancels an in-progress clip move.
        KeyCode::Esc
            if app.current_view == ViewKind::Matrix
            && app.matrix_state.grabbed_clip.is_some() =>
        {
            app.matrix_state.grabbed_clip = None;
            app.set_timed_status("Move cancelled", 2);
        }
        // Pattern length adjustment (tracker section 0).
        KeyCode::Char(']') if app.current_view == ViewKind::Tracker
            && app.tracker_section == 0 =>
        {
            app.adjust_pattern_len(1);
            let key_s = app.tracker_state.pattern_key.clone().unwrap_or_default();
            let len = app.project.lock().patterns.get(&key_s).map(|p| p.length).unwrap_or(0);
            app.status_msg = format!("LEN → {}", len);
            return;
        }
        KeyCode::Char('[') if app.current_view == ViewKind::Tracker
            && app.tracker_section == 0 =>
        {
            app.adjust_pattern_len(-1);
            let key_s = app.tracker_state.pattern_key.clone().unwrap_or_default();
            let len = app.project.lock().patterns.get(&key_s).map(|p| p.length).unwrap_or(0);
            app.status_msg = format!("LEN → {}", len);
            return;
        }
        // Piano roll draw mode toggle.
        KeyCode::Char('d') if app.current_view == ViewKind::Tracker => {
            app.piano_draw_mode = !app.piano_draw_mode;
            app.status_msg = if app.piano_draw_mode {
                "Piano roll: DRAW mode".to_string()
            } else {
                "Piano roll: SELECT mode".to_string()
            };
        }
        // Mixer: reset clip indicators.
        KeyCode::Char('c')
            if app.current_view == ViewKind::Mixer =>
        {
            for v in app.audio_slot_clip.iter_mut() { *v = false; }
            app.master_clip = [false; 2];
            app.set_timed_status("Clip indicators reset".to_string(), 2);
            return;
        }
        // Mixer: toggle stereo/mono for the selected channel.
        KeyCode::Char('S')
            if app.current_view == ViewKind::Mixer =>
        {
            let idx = app.mixer_state.selected_channel;
            let dest = {
                let proj = app.project.lock();
                let entries = views::mixer::collect_mixer_entries(&proj);
                entries.get(idx).map(|e| e.dest.clone())
            };
            if let Some(dest) = dest {
                let mut proj = app.project.lock();
                if !proj.channels.iter().any(|c| c.midi_port.as_deref() == Some(dest.as_str())) {
                    let mut ch = seqterm_core::Channel::new(dest.clone());
                    ch.midi_port = Some(dest.clone());
                    proj.channels.push(ch);
                }
                if let Some(ch) = proj.channels.iter_mut()
                    .find(|c| c.midi_port.as_deref() == Some(dest.as_str()))
                {
                    ch.stereo = !ch.stereo;
                    let mode = if ch.stereo { "STEREO" } else { "MONO" };
                    drop(proj);
                    app.set_timed_status(format!("{} → {}", dest, mode), 2);
                }
            }
            return;
        }
        // Arranger automation: add/remove point at cursor.
        KeyCode::Char('a')
            if app.current_view == ViewKind::Arranger && app.arranger_state.section == 1 =>
        {
            let lane_idx = app.arranger_state.automation_lane;
            let bar = app.arranger_state.automation_cursor as u32;
            let mut proj = app.project.lock();
            if let Some(lane) = proj.automation.get_mut(lane_idx) {
                if let Some(pos) = lane.points.iter().position(|(b, _)| *b == bar) {
                    lane.points.remove(pos);
                    drop(proj);
                    app.status_msg = format!("Removed automation point at bar {}", bar + 1);
                } else {
                    lane.points.push((bar, 64));
                    lane.points.sort_by_key(|(b, _)| *b);
                    drop(proj);
                    app.status_msg = format!("Added automation point at bar {}", bar + 1);
                }
            }
        }

        // In Granular view: g = back to Matrix.
        KeyCode::Char('g') if app.current_view == ViewKind::Granular => {
            app.switch_view(ViewKind::Matrix);
            return;
        }

        // In Granular view: f = freeze, F = unfreeze.
        KeyCode::Char('f') if app.current_view == ViewKind::Granular => {
            if let Some((bank, pad)) = app.granular_state.pad {
                dispatch_command(app, AppCommand::GranularFreeze { bank, pad });
            }
            return;
        }
        KeyCode::Char('F') if app.current_view == ViewKind::Granular => {
            if let Some((bank, pad)) = app.granular_state.pad {
                dispatch_command(app, AppCommand::GranularUnfreeze { bank, pad });
            }
            return;
        }

        // In Granular view: Tab = cycle sub-field within mod matrix row (shape/target/rate/depth).
        KeyCode::Tab if app.current_view == ViewKind::Granular
            && app.granular_state.cursor >= 17 =>
        {
            app.granular_mod_cursor = (app.granular_mod_cursor + 1) % 4;
            return;
        }
        // In Granular view: Enter on mod row = toggle enabled.
        KeyCode::Enter if app.current_view == ViewKind::Granular
            && app.granular_state.cursor >= 17 =>
        {
            if app.granular_state.pad.is_some() {
                app.adjust_granular_param(0);
            }
            return;
        }

        // In Granular view: V = cycle live input source (audio slots → None).
        KeyCode::Char('V') if app.current_view == ViewKind::Granular => {
            if let Some((bank, pad)) = app.granular_state.pad {
                // Collect available audio slot IDs sorted.
                let mut slot_ids: Vec<u32> = app.audio_slots.values().copied().collect();
                slot_ids.sort();
                slot_ids.dedup();
                let cur = app.granular_live_source;
                let next = match cur {
                    None => slot_ids.first().copied(),
                    Some(id) => {
                        let pos = slot_ids.iter().position(|&s| s == id);
                        match pos {
                            Some(i) if i + 1 < slot_ids.len() => Some(slot_ids[i + 1]),
                            _ => None, // wrap back to None (off)
                        }
                    }
                };
                dispatch_command(app, AppCommand::SetGranularLiveSource { bank, pad, source_slot_id: next });
            }
            return;
        }

        // In Granular view: L = live texture capture to current sampler pad.
        KeyCode::Char('L') if app.current_view == ViewKind::Granular => {
            if let Some((bank, pad)) = app.granular_state.pad {
                dispatch_command(app, AppCommand::CaptureGranularToPad { bank, pad });
            }
            return;
        }

        // In Granular view: r = happy accidents (randomise preset).
        KeyCode::Char('r') if app.current_view == ViewKind::Granular => {
            if app.granular_state.pad.is_some() {
                dispatch_command(app, AppCommand::RandomiseGranularPreset);
            }
            return;
        }

        // Granular scene slots: W = write current scene (prompts name),
        // 1-8 = recall slot, X = delete focused slot.
        KeyCode::Char('W') if app.current_view == ViewKind::Granular => {
            let slot = app.granular_scene_slot;
            const SAVE_FNS: [fn(String) -> AppCommand; 8] = [
                |n| AppCommand::SaveGranularScene { slot: 0, name: n },
                |n| AppCommand::SaveGranularScene { slot: 1, name: n },
                |n| AppCommand::SaveGranularScene { slot: 2, name: n },
                |n| AppCommand::SaveGranularScene { slot: 3, name: n },
                |n| AppCommand::SaveGranularScene { slot: 4, name: n },
                |n| AppCommand::SaveGranularScene { slot: 5, name: n },
                |n| AppCommand::SaveGranularScene { slot: 6, name: n },
                |n| AppCommand::SaveGranularScene { slot: 7, name: n },
            ];
            app.active_modal = Some(crate::modal::Modal::input(
                &format!("Save Scene {}", slot + 1),
                "Scene name",
                SAVE_FNS[slot.min(7)],
            ));
            return;
        }
        KeyCode::Char(c @ '1'..='8') if app.current_view == ViewKind::Granular => {
            let slot = (c as u8 - b'1') as usize;
            app.granular_scene_slot = slot;
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                // Shift+number = morph to that scene over 4 beats.
                dispatch_command(app, AppCommand::MorphGranularScene { to_slot: slot, beats: 4 });
            } else {
                dispatch_command(app, AppCommand::RecallGranularScene { slot });
            }
            return;
        }
        KeyCode::Char('X') if app.current_view == ViewKind::Granular => {
            let slot = app.granular_scene_slot;
            dispatch_command(app, AppCommand::DeleteGranularScene { slot });
            return;
        }
        KeyCode::Char('[') if app.current_view == ViewKind::Granular => {
            app.granular_scene_slot = app.granular_scene_slot.saturating_sub(1);
            return;
        }
        KeyCode::Char(']') if app.current_view == ViewKind::Granular => {
            app.granular_scene_slot = (app.granular_scene_slot + 1).min(7);
            return;
        }

        _ => {}
    }
}

fn handle_mouse(app: &mut App, event: crossterm::event::MouseEvent) {
    match event.kind {
        MouseEventKind::ScrollDown => handle_scroll(app, event.column, event.row, -1),
        MouseEventKind::ScrollUp => handle_scroll(app, event.column, event.row, 1),
        MouseEventKind::Moved => handle_hover(app, event.column, event.row),
        MouseEventKind::Down(MouseButton::Left) => {
            handle_click(app, event.column, event.row);
        }
        MouseEventKind::Down(MouseButton::Right) => {
            handle_right_click(app, event.column, event.row);
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            handle_drag(app, event.column, event.row);
        }
        MouseEventKind::Drag(MouseButton::Right) => {
            handle_right_drag(app, event.column, event.row);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let was_drag = app.mouse_drag;
            app.mouse_drag = false;
            app.piano_key_down = false;
            app.piano_key_last_row = None;

            // Time-based gate: on a pure click (no horizontal drag), set the note's
            // gate duration based on how long the mouse button was held.
            if !was_drag
                && app.current_view == ViewKind::Tracker
                && app.tracker_section == 1
            {
                if let Some(start) = app.note_click_start.take() {
                    if let Some((drag_step, _)) = app.piano_drag_note.take() {
                        // One 16th-note step duration at the current BPM.
                        let step_dur_secs = 60.0 / (app.bpm * 4.0);
                        let elapsed = start.elapsed().as_secs_f64();
                        // gate 100 = 1 step, 200 = 2 steps, etc. Min 1 step, max 32 steps.
                        let gate = ((elapsed / step_dur_secs * 100.0).round() as u16)
                            .clamp(100, 3200);
                        app.set_piano_note_gate(drag_step, gate);
                        app.status_msg = format!(
                            "PIANO: step {} gate {}% ({:.2}s)",
                            drag_step + 1, gate, elapsed
                        );
                    }
                }
            } else {
                app.note_click_start = None;
                app.piano_drag_note = None;
            }
        }
        MouseEventKind::Up(MouseButton::Right) => {
            app.mouse_drag = false;
        }
        _ => {}
    }
}

fn handle_scroll(app: &mut App, col: u16, row: u16, delta: i32) {
    // ── SF2 browser: scroll preset list ──────────────────────────────────────
    if matches!(app.active_modal, Some(Modal::Sf2Browser(_))) {
        let list = app.sf2_list_rect.get();
        if list.width > 0
            && col >= list.x && col < list.x + list.width
            && row >= list.y && row < list.y + list.height
        {
            if let Some(Modal::Sf2Browser(s)) = &mut app.active_modal {
                let vp = list.height as usize;
                let total = s.filtered_presets().len();
                if delta < 0 {
                    s.cursor = (s.cursor + 1).min(total.saturating_sub(1));
                } else {
                    s.cursor = s.cursor.saturating_sub(1);
                }
                s.clamp_scroll(vp);
            }
        }
        return;
    }

    // ── File picker scroll (sidebar or file list) ─────────────────────────────
    if matches!(app.active_modal, Some(Modal::FilePicker(_))) {
        // Sidebar scroll.
        let sidebar_area = app.file_picker_sidebar_area.get();
        if sidebar_area.width > 0
            && col >= sidebar_area.x && col < sidebar_area.x + sidebar_area.width
            && row >= sidebar_area.y && row < sidebar_area.y + sidebar_area.height
        {
            if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                if delta < 0 { s.sidebar_move_down(); } else { s.sidebar_move_up(); }
            }
            return;
        }
        // File list scroll.
        let list_area = app.file_picker_list_area.get();
        if list_area.width > 0 && list_area.height > 0
            && col >= list_area.x && col < list_area.x + list_area.width
            && row >= list_area.y && row < list_area.y + list_area.height
        {
            if let Some(Modal::FilePicker(state)) = &mut app.active_modal {
                let total = state.entries.len();
                let vh    = list_area.height as usize;
                if delta < 0 {
                    state.cursor = state.cursor.saturating_add(1).min(total.saturating_sub(1));
                } else {
                    state.cursor = state.cursor.saturating_sub(1);
                }
                state.clamp_scroll(vh);
            }
        }
        return;
    }

    // ── AudioExportOptions scroll: change selected parameter ─────────────────
    if matches!(app.active_modal, Some(Modal::AudioExportOptions(_))) {
        let modal_area = app.modal_area.get();
        if modal_area.width > 0
            && col >= modal_area.x && col < modal_area.x + modal_area.width
            && row >= modal_area.y && row < modal_area.y + modal_area.height
        {
            if let Some(Modal::AudioExportOptions(st)) = &mut app.active_modal {
                let n_sr = modal::EXPORT_SAMPLE_RATES.len();
                let n_bd = modal::EXPORT_BIT_DEPTHS.len();
                match st.cursor {
                    0 => {
                        st.sample_rate_idx = if delta > 0 {
                            st.sample_rate_idx.saturating_sub(1)
                        } else {
                            (st.sample_rate_idx + 1).min(n_sr - 1)
                        };
                    }
                    1 => {
                        st.bit_depth_idx = if delta > 0 {
                            st.bit_depth_idx.saturating_sub(1)
                        } else {
                            (st.bit_depth_idx + 1).min(n_bd - 1)
                        };
                    }
                    2 => { st.stems = !st.stems; }
                    _ => {}
                }
            }
        }
        return;
    }

    // ── MidiImportOptions scroll: move cursor to hovered row, then adjust ───
    if matches!(app.active_modal, Some(Modal::MidiImportOptions(_))) {
        let modal_area = app.modal_area.get();
        if modal_area.width > 0
            && col >= modal_area.x && col < modal_area.x + modal_area.width
            && row >= modal_area.y && row < modal_area.y + modal_area.height
        {
            // inner starts at y+1; hint at +0, blank at +1, options from +2.
            let inner_y = modal_area.y + 1;
            let row_rel = row.saturating_sub(inner_y);
            if row_rel >= 2 {
                let opt_idx = (row_rel - 2) as usize;
                if opt_idx < 4 {
                    if let Some(Modal::MidiImportOptions(s)) = &mut app.active_modal {
                        s.cursor = opt_idx;
                    }
                }
            }
            handle_midi_import_options_scroll(app, delta);
        }
        return;
    }

    // ── Generative Engine panel scroll: hover any row to adjust it ───────────
    if app.current_view == ViewKind::Tracker {
        let gen_area = app.tracker_panel_rects.get()[2];
        if gen_area.width > 0
            && col >= gen_area.x && col < gen_area.x + gen_area.width
            && row >= gen_area.y && row < gen_area.y + gen_area.height
        {
            app.tracker_section = 2;
            if let Some(new_gc) = generative_row_to_gc(row, col, gen_area) {
                app.generative_cursor = new_gc;
            }
            app.adjust_generative_param(delta);
            return;
        }

        // ── Track Modulation panel scroll: hover anywhere to adjust current param
        let mod_area = app.tracker_panel_rects.get()[3];
        if mod_area.width > 0
            && col >= mod_area.x && col < mod_area.x + mod_area.width
            && row >= mod_area.y && row < mod_area.y + mod_area.height
        {
            app.tracker_section = 3;
            // If hovering the tab row, switch parameter first then adjust.
            let tab_row_y = mod_area.y + 6;
            if row == tab_row_y {
                if let Some(tab) = mod_tab_from_x(col, mod_area) {
                    app.modulation_cursor = tab;
                }
            }
            app.adjust_modulation_param(delta);
            return;
        }
    }

    // ── Mixer: position-aware scroll over param rows ──────────────────────────
    if app.current_view == ViewKind::Mixer {
        let strips = app.mixer_strips_area.get();
        if strips.width > 0
            && col >= strips.x && col < strips.x + strips.width
            && row >= strips.y && row < strips.y + strips.height
        {
            // Horizontal mouse-wheel: scroll the strip window.
            // delta>0 = wheel up = scroll left; delta<0 = wheel down = scroll right.
            let total = app.mixer_strip_count.get() as usize;
            let visible = (strips.width / views::mixer::MIN_STRIP_W as u16).max(1) as usize;
            if delta.abs() == 1 && total > visible {
                let max_scroll = total.saturating_sub(visible);
                if delta < 0 {
                    app.mixer_state.strip_scroll = (app.mixer_state.strip_scroll + 1).min(max_scroll);
                } else {
                    app.mixer_state.strip_scroll = app.mixer_state.strip_scroll.saturating_sub(1);
                }
                return;
            }

            // Determine channel from x.
            let strip_count = visible.min(total);
            if strip_count > 0 {
                let col_w = (strips.width / strip_count as u16).max(1);
                let strip_col = ((col.saturating_sub(strips.x)) / col_w) as usize;
                app.mixer_state.selected_channel = app.mixer_state.strip_scroll + strip_col;
            }

            // Determine param from y.
            let param_ys = app.mixer_param_ys.get();
            let param = if row >= param_ys[9] && param_ys[9] > 0 { 6 }
                else if row >= param_ys[8] && param_ys[8] > 0 { 5 }
                else if row >= param_ys[7] && param_ys[7] > 0 { 4 }
                else if row >= param_ys[6] && param_ys[6] > 0 { 3 }
                else if row >= param_ys[5] && param_ys[5] > 0 { 2 }
                else if row >= param_ys[4] && param_ys[4] > 0 { 1 }
                else { 0 };
            app.mixer_state.active_param = param;

            let idx = app.mixer_state.selected_channel;
            app.adjust_mixer_param(idx, param, delta);
            return;
        }
    }

    // ── Matrix: scroll on ROWS/COLS line of transport panel ──────────────────
    if app.current_view == ViewKind::Matrix {
        let tr = app.matrix_panel_rects.get()[1];
        if tr.width > 0 && hit(col, row, tr) {
            let inner_x = tr.x + 1;
            let inner_y = tr.y + 1;
            // MATRIX SIZE line is the 4th content line (index 3, after the 3 button rows).
            if col >= inner_x && row == inner_y + 3 {
                let x_off = (col - inner_x) as usize;
                app.matrix_section = 1;
                // "MATRIX SIZE : " = 14 chars, rows = 3 chars (0-16), " × " (17-19), cols (20+).
                if x_off < 18 {
                    app.transport_cursor = 6;
                } else {
                    app.transport_cursor = 7;
                }
                app.adjust_transport_param(delta);
                return;
            }
        }
    }

    // ── Default view scroll ───────────────────────────────────────────────────
    app.handle_scroll_delta(delta);
}

/// Map a click (row, col) inside the generative panel to a gc index.
/// Mirrors the line order of `draw_generative_panel`.
fn generative_row_to_gc(row: u16, col: u16, area: ratatui::layout::Rect) -> Option<usize> {
    if area.height == 0 { return None; }
    // Content starts at area.y + 1 (block top border).
    if row < area.y + 1 || row >= area.y + area.height.saturating_sub(1) { return None; }
    let line = (row - area.y - 1) as usize;
    match line {
        0  => Some(0),  // PAT NAME
        1  => Some(1),  // PAT LEN
        2  => {
            // TIME SIG  : " — label is 12 chars, then num (3), then " / ", then den.
            // Split at col offset 15 (label + num).
            Some(if col < area.x + 1 + 15 { 2 } else { 3 })
        }
        3  => Some(4),  // BEAT GROUP
        4  => None,     // ─── separator
        5  => Some(5),  // SWING
        6  => Some(6),  // PROB
        7  => Some(7),  // RANDOM MUTATION
        8  => {
            // EUCL STEPS     : " — label is 17 chars, then fill (2), then " / ", then len.
            // Split at col offset 19 (label + fill).
            Some(if col < area.x + 1 + 19 { 8 } else { 9 })
        }
        9  => Some(10), // PROB LOCK
        10 => Some(11), // MICROSHIFT
        11 | 12 => None, // blank + PATTERN visualization
        13 => Some(12), // EVOLUTION MODE
        14 => Some(13), // HUMANIZATION
        _  => None,     // hint row or beyond
    }
}

/// Map an X position inside the modulation panel to a tab index (0-7).
/// Tab row layout: "←→:" (3 chars) then " VEL "(5)," GAIN "(6)," PAN "(5),
/// " LP "(4)," HP "(4)," LFO "(5)," SPD "(5)," AMP "(5).
fn mod_tab_from_x(col: u16, area: ratatui::layout::Rect) -> Option<usize> {
    if area.width == 0 { return None; }
    // Content starts at area.x + 1 (border).
    let x_in = col.saturating_sub(area.x + 1) as usize;
    if x_in < 3 { return None; } // "←→:" prefix
    let x_tab = x_in - 3;
    const WIDTHS: [usize; 8] = [5, 6, 5, 4, 4, 5, 5, 5];
    let mut x = 0usize;
    for (i, &w) in WIDTHS.iter().enumerate() {
        if x_tab < x + w { return Some(i); }
        x += w;
    }
    None
}

/// Map a scrollbar y-click to a scroll offset.
fn scrollbar_click_to_scroll(y_abs: u16, bar_y: u16, bar_h: u16, total: usize, vh: usize) -> usize {
    let scroll_max = total.saturating_sub(vh);
    if bar_h <= 1 || scroll_max == 0 {
        return 0;
    }
    let y_rel = (y_abs - bar_y) as usize;
    (y_rel * scroll_max / (bar_h as usize - 1)).min(scroll_max)
}

/// Set the current automation parameter (modulation_cursor 0-7) for step `s`.
/// Returns `true` if the step exists and has a note (was modified).
fn set_step_mod_param(app: &mut App, s: usize, val: u8) -> bool {
    let key = match app.tracker_state.pattern_key.clone() {
        Some(k) => k,
        None => return false,
    };
    let mc = app.modulation_cursor.min(7);
    let mut proj = app.project.lock();
    if let Some(pat) = proj.patterns.get_mut(&key) {
        if let Some(note) = pat.steps.get_mut(s) {
            if note.is_empty() { return false; }
            match mc {
                0 => note.velocity = val,
                1 => note.gain     = val,
                2 => note.pan      = val,
                3 => note.lp       = val,
                4 => note.hp       = val,
                5 => note.lfo      = val,
                6 => note.speed    = val,
                7 => note.amp      = val,
                _ => {}
            }
            return true;
        }
    }
    false
}

/// Convert a y position inside the velocity chart to a MIDI velocity value (0-127).
/// y_rel=0 (top) → 127,  y_rel=n_rows-1 (bottom) → 0.
fn vel_from_chart_y(y_rel: usize, n_rows: usize) -> u8 {
    if n_rows <= 1 {
        return 127;
    }
    (127 * (n_rows - 1 - y_rel.min(n_rows - 1)) / (n_rows - 1)) as u8
}

/// Handle a left-click inside an active modal (other than the [×] button).
fn handle_modal_click(app: &mut App, col: u16, row: u16) {
    let modal_area = app.modal_area.get();

    match &app.active_modal {
        // Alert / About / Help: click anywhere → close.
        Some(Modal::Alert { .. }) | Some(Modal::About) | Some(Modal::Help(_)) => {
            app.active_modal = None;
        }

        // Confirm: click Yes → confirm, click Cancel → close, click elsewhere → ignore.
        Some(Modal::Confirm { on_confirm, .. }) => {
            let cmd = on_confirm.clone();
            let yes_rect = app.confirm_yes_rect.get();
            let no_rect  = app.confirm_no_rect.get();
            if yes_rect.width > 0 && hit(col, row, yes_rect) {
                app.active_modal = None;
                dispatch_command(app, cmd);
            } else if no_rect.width > 0 && hit(col, row, no_rect) {
                app.active_modal = None;
            }
            // Clicks outside both buttons are ignored (no accidental dismiss).
        }

        // Input dialog: OK button submits, Cancel button dismisses.
        Some(Modal::Input(_)) => {
            let ok     = app.modal_ok_rect.get();
            let cancel = app.modal_cancel_rect.get();
            if ok.width > 0 && hit(col, row, ok) {
                // Replicate Enter key: call on_submit with the current value.
                let cmd = if let Some(Modal::Input(s)) = &app.active_modal {
                    Some((s.on_submit)(s.value.clone()))
                } else { None };
                app.active_modal = None;
                if let Some(c) = cmd { dispatch_command(app, c); }
            } else if cancel.width > 0 && hit(col, row, cancel) {
                app.active_modal = None;
            }
        }

        // QuitConfirm: three-button quit dialog.
        Some(Modal::QuitConfirm) => {
            let save_rect   = app.quit_save_rect.get();
            let exit_rect   = app.quit_nosave_rect.get();
            let cancel_rect = app.quit_cancel_rect.get();
            if save_rect.width > 0 && hit(col, row, save_rect) {
                app.active_modal = None;
                dispatch_command(app, AppCommand::SaveAndExit);
            } else if exit_rect.width > 0 && hit(col, row, exit_rect) {
                app.active_modal = None;
                dispatch_command(app, AppCommand::ExitConfirmed);
            } else if cancel_rect.width > 0 && hit(col, row, cancel_rect) {
                app.active_modal = None;
            }
        }

        // AudioExportOptions: click on value rows to select / toggle.
        Some(Modal::AudioExportOptions(_)) => {
            // Check Export / Cancel buttons first.
            let ok = app.modal_ok_rect.get();
            let cancel = app.modal_cancel_rect.get();
            if ok.width > 0 && hit(col, row, ok) {
                if let Some(Modal::AudioExportOptions(st)) = &app.active_modal {
                    app.audio_export_opts = st.to_opts();
                }
                app.active_modal = None;
                dispatch_command(app, AppCommand::ExportAudio);
                return;
            } else if cancel.width > 0 && hit(col, row, cancel) {
                app.active_modal = None;
                return;
            }
            if modal_area.width == 0 { return; }
            let inner_y = modal_area.y + 2;
            let inner_x = modal_area.x + 2;
            let row_rel = row.saturating_sub(inner_y);

            match row_rel {
                // rows[0..=1] → select sample rate.
                0 | 1 => {
                    if let Some(Modal::AudioExportOptions(st)) = &mut app.active_modal {
                        st.cursor = 0;
                        // If clicking on value row, pick the clicked option.
                        if row_rel == 1 {
                            let span_w = 12u16; // each sample-rate span is 12 chars
                            let idx = col.saturating_sub(inner_x) / span_w;
                            let n = modal::EXPORT_SAMPLE_RATES.len();
                            if (idx as usize) < n { st.sample_rate_idx = idx as usize; }
                        }
                    }
                }
                // rows[3..=4] → select bit depth.
                3 | 4 => {
                    if let Some(Modal::AudioExportOptions(st)) = &mut app.active_modal {
                        st.cursor = 1;
                        if row_rel == 4 {
                            let span_w = 11u16;
                            let idx = col.saturating_sub(inner_x) / span_w;
                            let n = modal::EXPORT_BIT_DEPTHS.len();
                            if (idx as usize) < n { st.bit_depth_idx = idx as usize; }
                        }
                    }
                }
                // rows[6..=7] → select mode (Full Mix / Stems).
                6 | 7 => {
                    if let Some(Modal::AudioExportOptions(st)) = &mut app.active_modal {
                        st.cursor = 2;
                        if row_rel == 7 {
                            // "  Full Mix  " = 12 chars, then "  Stems  " = 9 chars
                            let x_rel = col.saturating_sub(inner_x);
                            st.stems = x_rel >= 12;
                        }
                    }
                }
                // Hint row → confirm (same as Enter).
                11 => {
                    if let Some(Modal::AudioExportOptions(st)) = &app.active_modal {
                        let opts = st.to_opts();
                        app.audio_export_opts = opts;
                    }
                    app.active_modal = None;
                    dispatch_command(app, AppCommand::ExportAudio);
                }
                _ => {}
            }
        }

        // MidiImportOptions: click to select row and change value.
        Some(Modal::MidiImportOptions(_)) => {
            // Check Import / Cancel buttons first.
            let ok = app.modal_ok_rect.get();
            let cancel = app.modal_cancel_rect.get();
            if ok.width > 0 && hit(col, row, ok) {
                let cmd = if let Some(Modal::MidiImportOptions(s)) = &app.active_modal {
                    Some(AppCommand::ImportMidiWithOptions(s.path.clone(), s.opts.clone()))
                } else { None };
                app.active_modal = None;
                if let Some(cmd) = cmd { dispatch_command(app, cmd); }
                return;
            } else if cancel.width > 0 && hit(col, row, cancel) {
                app.active_modal = None;
                return;
            }
            if modal_area.width == 0 { return; }
            let inner_y = modal_area.y + 1; // block inner starts at y+1
            let row_rel = row.saturating_sub(inner_y);
            // Header (0), blank (1), option rows start at 2.
            if row_rel >= 2 {
                let opt_idx = (row_rel - 2) as usize;
                if opt_idx < 4 {
                    if let Some(Modal::MidiImportOptions(st)) = &mut app.active_modal {
                        st.cursor = opt_idx;
                    }
                    if opt_idx < 3 {
                        // Cycle value for rows 0-2 on click.
                        if let Some(Modal::MidiImportOptions(st)) = &mut app.active_modal {
                            adjust_import_option(st, 1);
                        }
                    } else {
                        // Row 3 = SF2 — open file picker (same as Enter on that row).
                        use modal::{FilePickerState, FilePickerTarget};
                        if let Some(Modal::MidiImportOptions(s)) = &app.active_modal {
                            app.pending_midi_import = Some((s.path.clone(), s.opts.clone()));
                        }
                        app.active_modal = Some(Modal::FilePicker(
                            FilePickerState::new(FilePickerTarget::AssignSf2ForMidiImport),
                        ));
                    }
                }
            }
        }

        // FilePicker: sidebar click or file-list click.
        Some(Modal::FilePicker(_)) => {
            // ── Aceptar / Cancelar buttons (SF2 for MIDI import picker) ────────
            let ok_rect     = app.modal_ok_rect.get();
            let cancel_rect = app.modal_cancel_rect.get();
            if ok_rect.width > 0 && hit(col, row, ok_rect) {
                // Confirm selected file — same logic as pressing Enter.
                let data = if let Some(Modal::FilePicker(s)) = &app.active_modal {
                    let is_dir = s.visible_entries().get(s.cursor).map(|e| e.is_dir).unwrap_or(false);
                    let target = s.target;
                    let path   = s.selected_visible_path();
                    Some((is_dir, target, path))
                } else { None };
                if let Some((false, target, Some(path))) = data {
                    let cmd = target.into_confirm_command(path);
                    app.active_modal = None;
                    dispatch_command(app, cmd);
                }
                return;
            } else if cancel_rect.width > 0 && hit(col, row, cancel_rect) {
                app.active_modal = None;
                return;
            }

            // ── Sidebar click ─────────────────────────────────────────────────
            let sidebar_area = app.file_picker_sidebar_area.get();
            if sidebar_area.width > 0
                && col >= sidebar_area.x && col < sidebar_area.x + sidebar_area.width
                && row >= sidebar_area.y && row < sidebar_area.y + sidebar_area.height
            {
                let (abs_idx, path_opt) = if let Some(Modal::FilePicker(s)) = &app.active_modal {
                    let rel_row = (row - sidebar_area.y) as usize;
                    let abs_idx = s.sidebar_scroll + rel_row;
                    let p = s.sidebar.get(abs_idx)
                        .filter(|e| e.kind != SidebarItemKind::Header)
                        .and_then(|e| e.path.clone());
                    (abs_idx, p)
                } else { (0, None) };
                if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                    if s.sidebar.get(abs_idx).map_or(false, |e| e.kind != SidebarItemKind::Header) {
                        s.sidebar_cursor = abs_idx;
                        s.tree_focused = true;
                    }
                }
                if let Some(path) = path_opt {
                    if let Some(Modal::FilePicker(s)) = &mut app.active_modal {
                        s.navigate_to(path);
                    }
                }
                return;
            }

            // ── File list click ───────────────────────────────────────────────
            let list_area = app.file_picker_list_area.get();
            if list_area.width > 0 && list_area.height > 0
                && col >= list_area.x && col < list_area.x + list_area.width
                && row >= list_area.y && row < list_area.y + list_area.height
            {
                if let Some(Modal::FilePicker(state)) = &mut app.active_modal {
                    state.tree_focused = false;
                    let rel_row = (row - list_area.y) as usize;
                    let visible_entries = state.visible_entries();
                    let abs_idx = state.scroll + rel_row;
                    if abs_idx < visible_entries.len() {
                        let is_dir = visible_entries[abs_idx].is_dir;
                        let path   = visible_entries[abs_idx].path.clone();
                        let name   = visible_entries[abs_idx].name.clone();
                        if state.cursor == abs_idx && is_dir {
                            state.descend();
                        } else {
                            state.cursor = abs_idx;
                            if !is_dir {
                                if let modal::FilePickerMode::Save = state.target.mode() {
                                    state.filename_input = name;
                                }
                            }
                            let _ = (path, is_dir); // suppress unused
                        }
                    }
                }
            }
        }

        // SourcePicker: click option block → select; click port → select port.
        // Single click on option = select it; if SF2 or Audio, also confirms immediately.
        Some(Modal::SourcePicker(_)) => {
            // Check Confirm / Cancel buttons first.
            let ok = app.modal_ok_rect.get();
            let cancel = app.modal_cancel_rect.get();
            if ok.width > 0 && hit(col, row, ok) {
                // Simulate Enter key for source picker.
                let ev = crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Enter,
                    crossterm::event::KeyModifiers::NONE,
                );
                handle_source_picker_key(app, ev);
                return;
            } else if cancel.width > 0 && hit(col, row, cancel) {
                app.active_modal = None;
                return;
            }
            use modal::{Modal as M, SourceKind};
            use modal::{FilePickerState, FilePickerTarget};

            // Extract rects snapshot to avoid borrow conflict.
            let (option_rects, port_rects, _cursor_before, row_sp, col_sp) = {
                let Some(M::SourcePicker(s)) = &app.active_modal else { return };
                (s.option_rects, s.port_rects.clone(), s.cursor, s.row, s.col)
            };

            // Check if click is on a port row (inside MIDI option when selected).
            for (pi, pr) in port_rects.iter().enumerate() {
                if hit(col, row, *pr) {
                    if let Some(M::SourcePicker(s)) = &mut app.active_modal {
                        s.port_cursor = pi;
                    }
                    return;
                }
            }

            // Check which option block was clicked.
            let kinds = [SourceKind::Midi, SourceKind::Sf2, SourceKind::Audio];
            for (i, &opt_rect) in option_rects.iter().enumerate() {
                if hit(col, row, opt_rect) {
                    let kind = kinds[i];
                    if let Some(M::SourcePicker(s)) = &mut app.active_modal {
                        s.cursor = kind;
                    }
                    // For SF2 / Audio: single click → open the file browser immediately.
                    // For MIDI: click selects the option, Enter or port click confirms.
                    match kind {
                        SourceKind::Sf2 => {
                            app.active_modal = Some(modal::Modal::FilePicker(
                                FilePickerState::new(FilePickerTarget::AssignSf2 { row: row_sp, col: col_sp }),
                            ));
                        }
                        SourceKind::Audio => {
                            app.active_modal = Some(modal::Modal::FilePicker(
                                FilePickerState::new(FilePickerTarget::AssignAudioFile { row: row_sp, col: col_sp }),
                            ));
                        }
                        SourceKind::Midi => {
                            // Already selected by cursor update above; user picks port via ←→ or port click.
                        }
                    }
                    return;
                }
            }
        }

        // ── Shared OK / Cancel buttons for configuration & search modals ────────
        // These are rendered by render_modal_buttons; rects are shared since only
        // one modal is visible at a time.
        Some(Modal::AudioSettings(_)) => {
            let ok = app.modal_ok_rect.get();
            let cancel = app.modal_cancel_rect.get();
            if ok.width > 0 && hit(col, row, ok) {
                // Same logic as Enter in handle_audio_settings_key.
                let (orig_backend, orig_sr) =
                    if let Some(Modal::AudioSettings(s)) = &app.active_modal {
                        (s.original_backend.clone(), s.original_sample_rate)
                    } else { (String::new(), 0) };
                app.active_modal = None;
                let _ = seqterm_persistence::save_settings(&app.settings);
                let backend_changed = app.settings.audio.backend != orig_backend;
                let sr_changed      = app.settings.audio.sample_rate != orig_sr;
                if backend_changed || sr_changed {
                    app.active_modal = Some(Modal::alert(
                        "Restart Required",
                        "Backend / sample rate changes take effect after restarting SeqTerm.",
                    ));
                } else {
                    app.set_timed_status("Audio settings saved".to_string(), 2);
                }
            } else if cancel.width > 0 && hit(col, row, cancel) {
                app.active_modal = None;
            }
        }

        Some(Modal::MidiSettings(_)) => {
            // OK / Cancel buttons.
            let ok_rect     = app.modal_ok_rect.get();
            let cancel_rect = app.modal_cancel_rect.get();
            if ok_rect.width > 0 && hit(col, row, ok_rect) {
                app.active_modal = None;
                return;
            }
            if cancel_rect.width > 0 && hit(col, row, cancel_rect) {
                app.active_modal = None;
                return;
            }

            // Tab bar clicks — switch active tab.
            let tab_rects = app.midi_settings_tab_rects.get();
            for (i, tr) in tab_rects.iter().enumerate() {
                if tr.width > 0 && hit(col, row, *tr) {
                    if let Some(Modal::MidiSettings(s)) = &mut app.active_modal {
                        s.tab = i;
                        s.cursor = 0;
                    }
                    return;
                }
            }

            // List area clicks — select row, and toggle/activate on second click.
            let list_rect = app.midi_settings_list_rect.get();
            if list_rect.width > 0 && hit(col, row, list_rect) {
                let row_rel = (row - list_rect.y) as usize;

                // Get current tab and cursor.
                let (tab, cursor_before) = if let Some(Modal::MidiSettings(s)) = &app.active_modal {
                    (s.tab, s.cursor)
                } else { return };

                // Clamp row_rel to valid range.
                let max = {
                    let proj = app.project.lock();
                    match tab {
                        0 => proj.midi_inputs.len(),
                        1 => proj.midi_outputs.len(),
                        _ => 4,
                    }
                };
                if row_rel >= max { return; }

                // First click: move cursor. Second click on same row: toggle/activate.
                if row_rel == cursor_before {
                    // Toggle or activate — same as pressing 'e'.
                    let mut proj = app.project.lock();
                    match tab {
                        0 => {
                            if let Some(p) = proj.midi_inputs.get_mut(row_rel) { p.enabled = !p.enabled; }
                            drop(proj);
                            app.sync_midi_input_bus();
                        }
                        1 => { if let Some(p) = proj.midi_outputs.get_mut(row_rel) { p.enabled = !p.enabled; } }
                        2 => {
                            use seqterm_core::SyncMode;
                            let modes = [SyncMode::Internal, SyncMode::Usb, SyncMode::Midi, SyncMode::Clock];
                            if let Some(m) = modes.get(row_rel) {
                                proj.sync_mode = m.clone();
                                let is_clock = matches!(m, SyncMode::Clock);
                                drop(proj);
                                rebuild_clock_ports(app, is_clock);
                            }
                        }
                        _ => {}
                    }
                } else {
                    // Move cursor to clicked row.
                    if let Some(Modal::MidiSettings(s)) = &mut app.active_modal {
                        s.cursor = row_rel;
                    }
                }
            }
        }

        Some(Modal::KeybindingsEditor(_)) => {
            let ok = app.modal_ok_rect.get();
            let cancel = app.modal_cancel_rect.get();
            if ok.width > 0 && hit(col, row, ok) {
                app.settings.keybindings = if let Some(Modal::KeybindingsEditor(s)) = &app.active_modal {
                    s.bindings.clone()
                } else { Default::default() };
                let _ = seqterm_persistence::save_settings(&app.settings);
                app.active_modal = None;
                app.set_timed_status("Keybindings saved".to_string(), 2);
            } else if cancel.width > 0 && hit(col, row, cancel) {
                app.active_modal = None;
            }
        }

        Some(Modal::Sf2Browser(_)) => {
            let ok     = app.modal_ok_rect.get();
            let cancel = app.modal_cancel_rect.get();
            let bl     = app.sf2_bank_left_rect.get();
            let br     = app.sf2_bank_right_rect.get();
            let list   = app.sf2_list_rect.get();

            if ok.width > 0 && hit(col, row, ok) {
                // Accept
                let data = if let Some(Modal::Sf2Browser(s)) = &app.active_modal {
                    s.selected().map(|(b, p, _)| (s.path.clone(), s.row, s.col, b, p))
                } else { None };
                if let Some((path, row_v, col_v, bank, preset)) = data {
                    app.active_modal = None;
                    dispatch_command(app, AppCommand::ConfirmSf2Assignment { row: row_v, col: col_v, path, bank, preset });
                }
            } else if cancel.width > 0 && hit(col, row, cancel) {
                // Cancel
                app.active_modal = None;
            } else if bl.width > 0 && hit(col, row, bl) {
                // ◄ previous bank
                if let Some(Modal::Sf2Browser(s)) = &mut app.active_modal {
                    s.shift_bank(-1);
                }
            } else if br.width > 0 && hit(col, row, br) {
                // ► next bank
                if let Some(Modal::Sf2Browser(s)) = &mut app.active_modal {
                    s.shift_bank(1);
                }
            } else if list.width > 0
                && col >= list.x && col < list.x + list.width
                && row >= list.y && row < list.y + list.height
            {
                // Click on a preset row
                let clicked_row = (row - list.y) as usize;
                if let Some(Modal::Sf2Browser(s)) = &mut app.active_modal {
                    let absolute_idx = s.scroll + clicked_row;
                    let fp_len = s.filtered_presets().len();
                    if absolute_idx < fp_len {
                        s.cursor = absolute_idx;
                    }
                }
            }
        }

        // Progress: cancelable — [×] button handled above; ignore body clicks.
        _ => {}
    }
}

/// Return (midi_out, channel_0indexed) for the clip at the current matrix cursor.
fn active_clip_routing(app: &App) -> (Option<String>, u8) {
    let (row, col) = app.matrix_state.cursor;
    let row_key = ((b'A' + row as u8) as char).to_string();
    let proj = app.project.lock();
    proj.matrix
        .get(&row_key)
        .and_then(|r| r.get(col))
        .and_then(|c| c.as_ref())
        .map(|c| (c.midi_out.clone(), c.midi_channel.saturating_sub(1) & 0x0F))
        .unwrap_or((None, 0))
}

/// Send a piano-key preview note for `note_row` to the active clip's routed synth.
/// For SF2 clips the note goes directly to the audio engine (the MIDI-out path is None).
fn preview_piano_key(app: &mut App, note_row: usize, vel: u8) {
    let midi = (108usize).saturating_sub(note_row) as u8;
    if midi < 21 || midi > 108 { return; }

    // Try direct audio engine path first (SF2 clips have no midi_out).
    let (row, col) = app.matrix_state.cursor;
    let row_key = ((b'A' + row as u8) as char).to_string();
    let clip_key = format!("{}{}", row_key, col);
    if let Some(&slot_id) = app.audio_slots.get(&clip_key) {
        let ch = {
            let proj = app.project.lock();
            proj.matrix
                .get(&row_key)
                .and_then(|r| r.get(col))
                .and_then(|c| c.as_ref())
                .map(|c| c.midi_channel.saturating_sub(1) & 0x0F)
                .unwrap_or(0)
        };
        if let Some(ae) = &mut app.audio_engine {
            ae.send(seqterm_audio_engine::AudioCommand::NoteOn { slot_id, channel: ch, note: midi, velocity: vel });
            // Schedule NoteOff after a short gate (100 ms ≈ preview duration).
            // We send it immediately; the SF2 synth will release the note.
            ae.send(seqterm_audio_engine::AudioCommand::NoteOff { slot_id, channel: ch, note: midi });
        }
        return;
    }

    // Fallback: MIDI-out path for external-MIDI clips.
    let (dest, ch) = active_clip_routing(app);
    app.engine.preview_note(midi, vel, dest, ch);
}

fn hit(col: u16, row: u16, r: ratatui::layout::Rect) -> bool {
    r.width > 0 && r.height > 0
        && col >= r.x && col < r.x + r.width
        && row >= r.y && row < r.y + r.height
}

fn handle_click(app: &mut App, col: u16, row: u16) {
    app.last_mouse_pos = (col, row);
    app.mouse_drag = false;
    app.piano_drag_note = None;
    app.piano_key_down = false;
    app.piano_key_last_row = None;

    // ── Modal [×] close button ────────────────────────────────────────────────
    let close = app.modal_close_rect.get();
    if close.width > 0
        && col >= close.x && col < close.x + close.width
        && row >= close.y && row < close.y + close.height
    {
        app.active_modal  = None;
        app.midi_import_rx  = None;
        app.audio_export_rx = None;
        return;
    }

    // ── Modal content clicks ──────────────────────────────────────────────────
    if app.active_modal.is_some() {
        handle_modal_click(app, col, row);
        return;
    }

    // ── Transport bar: view-label tabs only (no transport buttons) ───────────
    let ta = app.transport_area.get();
    if ta.width > 0 && row >= ta.y && row < ta.y + ta.height {
        if row == ta.y + 1 {
            let mut x = ta.x + 2;
            for (i, &label) in VIEW_LABELS.iter().enumerate() {
                let label_w = label.len() as u16 + 2;
                if col >= x && col < x + label_w {
                    if let Some(v) = ViewKind::from_index(i) {
                        app.switch_view(v);
                    }
                    return;
                }
                x += label_w;
                if i + 1 < VIEW_LABELS.len() { x += 1; }
            }
        }
        return;
    }

    // ── Menu bar / dropdown mouse support ─────────────────────────────────────
    // Menu bar is always at row 0 (top row of the terminal).
    if row == 0 {
        // Find which menu label was clicked.
        let mut x = 0u16;
        for &kind in MenuKind::ALL {
            let w = kind.label().len() as u16;
            if col >= x && col < x + w {
                if app.menu_open == Some(kind) {
                    // Clicking the already-open label closes the menu.
                    app.menu_open = None;
                } else {
                    app.menu_open   = Some(kind);
                    app.menu_cursor = 0;
                }
                return;
            }
            x += w;
        }
        // Click in the right-side "unsaved" area — close any open menu.
        app.menu_open = None;
        return;
    }

    // Open dropdown click: activate the item under the pointer.
    if let Some(kind) = app.menu_open {
        // Compute the dropdown area the same way the renderer does.
        let mut bar_x = 0u16;
        for &k in MenuKind::ALL {
            if k == kind { break; }
            bar_x += k.label().len() as u16;
        }
        let items = kind.items();
        let term_w = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80);
        let panel_w = (items
            .iter()
            .map(|i| i.label.len() + i.shortcut.len() + 6)
            .max()
            .unwrap_or(20) as u16)
            .max(20)
            .min(term_w.saturating_sub(bar_x));
        let panel_h = (items.len() as u16 + 2).min(40);
        let area_x  = bar_x.min(term_w.saturating_sub(panel_w));
        let area_y  = 1u16; // bar_y(0) + 1
        let inner_x = area_x + 1;
        let inner_y = area_y + 1;
        let inner_w = panel_w.saturating_sub(2);
        let inner_h = panel_h.saturating_sub(2);

        if col >= inner_x && col < inner_x + inner_w
            && row >= inner_y && row < inner_y + inner_h
        {
            let item_row = (row - inner_y) as usize; // index into items[]
            if let Some(item) = items.get(item_row) {
                if !item.separator && !item.disabled {
                    // Convert item index to cursor position (skips seps/disabled).
                    let cursor = items[..item_row]
                        .iter()
                        .filter(|i| !i.separator && !i.disabled)
                        .count();
                    app.menu_cursor = cursor;
                    let action = item.action;
                    app.menu_open   = None;
                    app.menu_cursor = 0;
                    if let Some(cmd) = action.to_command() {
                        dispatch_command(app, cmd);
                    }
                }
            }
            return;
        }
        // Click outside the dropdown — close it.
        app.menu_open = None;
    }

    // ── Matrix view: transport panel + routing panel clicks ───────────────────
    if app.current_view == ViewKind::Matrix {
        let rects = app.matrix_panel_rects.get();

        // Grid cell click → single click selects cursor; double click opens Tracker/P.Roll.
        let gr = rects[0];
        if gr.width > 0 && col >= gr.x && col < gr.x + gr.width
            && row >= gr.y && row < gr.y + gr.height
        {
            let (cell_w, cell_h) = app.matrix_cell_size.get();
            const ROW_LBL: u16 = 3;
            let x0 = gr.x + 1 + ROW_LBL + 1;
            let y0 = gr.y + 3;
            if cell_w > 0 && cell_h > 0 && col >= x0 && row >= y0 {
                let cell_col = ((col - x0) / (cell_w as u16 + 1)) as usize
                    + app.matrix_col_scroll.get();
                let cell_row = ((row - y0) / (cell_h as u16 + 1)) as usize;
                if cell_col < app.matrix_cols && cell_row < app.matrix_rows {
                    let now = std::time::Instant::now();
                    let is_double = app.last_matrix_click
                        .as_ref()
                        .map(|&((lr, lc), ref t)| {
                            lr == cell_row && lc == cell_col
                                && now.duration_since(*t).as_millis() < 400
                        })
                        .unwrap_or(false);

                    app.matrix_state.cursor = (cell_row, cell_col);
                    app.matrix_section = 0;

                    if is_double {
                        // Double-click: open Tracker/P.Roll for this cell.
                        app.last_matrix_click = None;
                        app.navigate_matrix_to_tracker();
                    } else {
                        // Single click: just move cursor, record for potential double-click.
                        app.last_matrix_click = Some(((cell_row, cell_col), now));
                    }
                    return;
                }
            }
        }

        // Transport panel buttons: PLAY/PAUSE(0-8) STOP(10-16) REWIND(18-25) REC(27-33) TAP(35-41) BPM(43-53).
        let tr = rects[1];
        if tr.width > 0 && col >= tr.x && col < tr.x + tr.width
            && row >= tr.y && row < tr.y + tr.height
        {
            let inner_x = tr.x + 1;
            let inner_y = tr.y + 1;
            if col >= inner_x && row >= inner_y && row - inner_y <= 2 {
                match col - inner_x {
                    0..=8  => { app.play_stop(); return; }
                    10..=17 => { app.stop(); return; }
                    19..=26 => { app.rewind(); return; }
                    28..=34 => { app.toggle_record(); return; }
                    36..=43 => { app.tap_tempo(); return; }
                    45..=55 => {
                        app.matrix_section = 1;
                        app.transport_cursor = 5;
                        return;
                    }
                    _ => {}
                }
            }
            // MATRIX SIZE row: click ROWS or COLS to focus that control.
            if col >= inner_x && row == inner_y + 3 {
                let x_off = (col - inner_x) as usize;
                app.matrix_section = 1;
                app.transport_cursor = if x_off < 18 { 5 } else { 6 };
                return;
            }
        }

        // Routing panel: clicking anywhere activates it on the SOURCE tab.
        let rr = rects[3];
        if app.sidebar_tab == 0
            && rr.width > 0 && col >= rr.x && col < rr.x + rr.width
            && row >= rr.y && row < rr.y + rr.height
        {
            app.matrix_section = 3;
            app.routing_tab = 1; // always land on SOURCE tab

            let (mat_row, mat_col) = app.matrix_state.cursor;

            // "Change Bank/Preset" button (SF2 clips only).
            let bp_y = app.sf2_reopen_btn_y.get();
            if bp_y > 0 && row == bp_y {
                dispatch_command(app, AppCommand::ReopenSf2Browser { row: mat_row, col: mat_col });
                return;
            }

            // Try btn_y from the last frame first (works from the 2nd click onward).
            let btn_y = app.routing_source_btn_y.get();

            // Use btn_y if it was recorded in the current SOURCE-tab render,
            // otherwise fall back to the channel-arrow Y minus a fixed offset
            // (the button sits ~4 rows above the channel arrows area).
            // If both are 0 (very first interaction) we open the picker unconditionally
            // since clicking the routing panel = user wants to change the source.
            let btn_y_effective = if btn_y > 0 {
                btn_y
            } else {
                // First click: btn_y not yet recorded — open picker on any click
                // in the upper two-thirds of the routing panel.
                let upper_bound = rr.y + rr.height * 2 / 3;
                if row < upper_bound {
                    dispatch_command(app, AppCommand::OpenSourcePicker { row: mat_row, col: mat_col });
                    return;
                }
                0 // lower third falls through to MIDI port list / channel arrow handling
            };

            // Click exactly on the "Change Source" button row.
            if btn_y_effective > 0 && row == btn_y_effective {
                dispatch_command(app, AppCommand::OpenSourcePicker { row: mat_row, col: mat_col });
                return;
            }

            // Click on a project source list item (rows after button + empty line).
            if btn_y_effective > 0 {
                let src_list_start = btn_y_effective + 2; // skip button + blank line
                if row >= src_list_start {
                    let item_idx = (row - src_list_start) as usize;
                    if item_idx > 0 { // skip "Project sources:" header line
                        app.routing_source_cursor = item_idx;
                    }
                    return;
                }
            }

            // ── Channel arrow clicks (◄ CH N ►) ──────────────────────────────
            let ch_y = app.routing_channel_y.get();
            if ch_y > 0 && row == ch_y {
                // Layout inside panel: "  ◄ CH 01 ►"
                //   offset 0-1: spaces, 2: ◄, 3: space → left arrow zone = [rr.x+1+2, rr.x+1+3]
                //   offset 9: space, 10: ► → right arrow zone = [rr.x+1+9, rr.x+1+10]
                let x_rel = col.saturating_sub(rr.x + 1);
                let dc: i32 = if x_rel <= 3 { -1 } else if x_rel >= 9 { 1 } else { 0 };
                if dc != 0 {
                    let (mat_row, mat_col) = app.matrix_state.cursor;
                    let row_key = ((b'A' + mat_row as u8) as char).to_string();
                    let mut proj = app.project.lock();
                    if let Some(slots) = proj.matrix.get_mut(&row_key) {
                        if let Some(Some(clip)) = slots.get_mut(mat_col) {
                            clip.midi_channel = (clip.midi_channel as i32 + dc).clamp(1, 16) as u8;
                            app.status_msg = format!("MIDI channel → {}", clip.midi_channel);
                        }
                    }
                }
                return;
            }

            // ── MIDI output list clicks (toggle assign/unassign) ──────────────
            let list_y = app.routing_list_item_y.get();
            if list_y > 0 && row >= list_y {
                let item_idx = (row - list_y) as usize;
                let (mat_row, mat_col) = app.matrix_state.cursor;
                let row_key = ((b'A' + mat_row as u8) as char).to_string();
                let assigned_out = {
                    let proj = app.project.lock();
                    proj.matrix.get(&row_key)
                        .and_then(|r| r.get(mat_col))
                        .and_then(|c| c.as_ref())
                        .and_then(|c| c.midi_out.clone())
                };
                let n_ports = app.project.lock().midi_outputs.len();
                let has_unavail = assigned_out.as_deref()
                    .map(|o| app.unavailable_midi_routes.contains(o))
                    .unwrap_or(false);
                let cursor = if has_unavail && item_idx >= 1 {
                    if item_idx == 1 { return; } // unavailable display row, no-op
                    item_idx - 1
                } else {
                    item_idx
                };
                if cursor <= n_ports {
                    app.routing_cursor = cursor;
                    // Resolve the port name for this cursor position.
                    let clicked_name: Option<String> = {
                        let proj = app.project.lock();
                        if cursor == 0 { None }
                        else { proj.midi_outputs.get(cursor - 1).map(|p| p.name.clone()) }
                    };
                    // Toggle: clicking the already-assigned port unassigns it.
                    let midi_name = if clicked_name.as_deref() == assigned_out.as_deref() {
                        None
                    } else {
                        clicked_name
                    };
                    {
                        let mut proj = app.project.lock();
                        if let Some(slots) = proj.matrix.get_mut(&row_key) {
                            if let Some(Some(clip)) = slots.get_mut(mat_col) {
                                clip.midi_out = midi_name.clone();
                            }
                        }
                    }
                    rebuild_midi_ports(app);
                    app.status_msg = match &midi_name {
                        Some(name) => format!("MIDI out → {}", name),
                        None => "MIDI out cleared".to_string(),
                    };
                }
                return;
            }
        }

        // ── Sidebar tab bar click ─────────────────────────────────────────────
        let tab_rects = app.sidebar_tab_rects.get();
        for (i, &tr) in tab_rects.iter().enumerate() {
            if tr.width > 0 && col >= tr.x && col < tr.x + tr.width
                && row >= tr.y && row < tr.y + tr.height
            {
                app.sidebar_tab = i as u8;
                match i {
                    0 => { app.matrix_section = 2; } // PANELS — focus polymeter
                    _ => { app.matrix_section = 0; } // HYBRID — grid focus
                }
                return;
            }
        }

        // ── Hybrid View: Active Patterns row click → select clip ──────────────
        if app.sidebar_tab == 1 {
            let pi = app.hv_patterns_inner.get();
            if pi.width > 0 && col >= pi.x && col < pi.x + pi.width
                && row >= pi.y && row < pi.y + pi.height
            {
                let row_idx = (row - pi.y) as usize;
                // Reconstruct entry list (same order as draw function).
                let mut entries: Vec<(usize, usize)> = Vec::new(); // (matrix_row, matrix_col)
                let proj = app.project.lock();
                'outer: for r in 0..app.matrix_rows {
                    let row_key = ((b'A' + r as u8) as char).to_string();
                    if let Some(slots) = proj.matrix.get(&row_key) {
                        for (c, slot) in slots.iter().enumerate() {
                            if let Some(clip) = slot {
                                if clip.enabled && clip.pattern_key.is_some() {
                                    entries.push((r, c));
                                    if entries.len() > row_idx { break 'outer; }
                                }
                            }
                        }
                    }
                }
                drop(proj);
                if let Some(&(mat_row, mat_col)) = entries.get(row_idx) {
                    app.matrix_state.cursor = (mat_row, mat_col);
                    app.matrix_section = 0;
                }
                return;
            }

            // ── Hybrid View: Tracker Monitor row click → seek step ────────────
            let mi = app.hv_monitor_inner.get();
            if mi.width > 0 && col >= mi.x && col < mi.x + mi.width
                && row >= mi.y && row < mi.y + mi.height
            {
                // row 0 = header, rows 1+ = steps.
                if row > mi.y {
                    let offset = (row - mi.y - 1) as usize; // 0-based step offset within view
                    let start = app.hv_monitor_start_step.get();
                    let target_step = start + offset;
                    // Seek the engine position to this step within the current pattern.
                    let (cursor_row, cursor_col) = app.matrix_state.cursor;
                    let row_key = ((b'A' + cursor_row as u8) as char).to_string();
                    let pat_len = {
                        let proj = app.project.lock();
                        proj.matrix
                            .get(&row_key)
                            .and_then(|r| r.get(cursor_col))
                            .and_then(|s| s.as_ref())
                            .and_then(|c| c.pattern_key.as_deref())
                            .and_then(|k| proj.patterns.get(k))
                            .map(|p| p.length)
                            .unwrap_or(0)
                    };
                    if pat_len > 0 && target_step < pat_len {
                        app.current_step = target_step;
                        app.set_timed_status(format!("Seek → step {target_step}"), 1);
                    }
                }
                return;
            }
        }
    }

    // ── Config view ───────────────────────────────────────────────────────────
    if app.current_view == ViewKind::Config {
        // Audio engine panel (section 5 — middle strip of Config).
        let ap = app.config_audio_panel_rect.get();
        if ap.width > 0 && hit(col, row, ap) {
            app.config_state.section = 5;
            return;
        }

        // Routing graph (section 4 — bottom half of Config).
        let rg = app.routing_graph_area.get();
        if rg.width > 0 && hit(col, row, rg) {
            app.config_state.section = 4;

            let scroll = app.routing_state.scroll;
            let sorted_ids = {
                let proj = app.project.lock();
                proj.routing.sorted_ids()
            };
            let n = sorted_ids.len();

            // Node list panel: click sets cursor.
            let ni = app.routing_node_list_inner.get();
            if ni.width > 0 && hit(col, row, ni) {
                app.routing_state.section = 0;
                if row >= ni.y {
                    let rel = (row - ni.y) as usize;
                    app.routing_state.node_cursor = (rel + scroll).min(n.saturating_sub(1));
                }
                return;
            }

            // Connection matrix panel: click sets cursor AND toggles edge.
            let mi = app.routing_matrix_inner.get();
            let cw = app.routing_matrix_col_w.get();
            if mi.width > 0 && hit(col, row, mi) {
                app.routing_state.section = 1;
                if row > mi.y {
                    let rel_row = (row - mi.y - 1) as usize;
                    app.routing_state.node_cursor = (rel_row + scroll).min(n.saturating_sub(1));
                }
                if cw > 0 && col >= mi.x + 14 {
                    let rel_col = ((col - mi.x - 14) / cw) as usize;
                    app.routing_state.col_cursor = rel_col.min(n.saturating_sub(1));
                }
                // Toggle edge (skip header row and self-edges).
                if cw > 0 && row > mi.y && col >= mi.x + 14 {
                    let rel_row = (row - mi.y - 1) as usize;
                    let rel_col = ((col - mi.x - 14) / cw) as usize;
                    let row_i = (rel_row + scroll).min(n.saturating_sub(1));
                    let col_i = rel_col.min(n.saturating_sub(1));
                    if let (Some(&from_id), Some(&to_id)) =
                        (sorted_ids.get(row_i), sorted_ids.get(col_i))
                    {
                        if from_id != to_id {
                            let mut proj = app.project.lock();
                            if proj.routing.has_edge(from_id, to_id) {
                                proj.routing.remove_edge(from_id, to_id);
                            } else {
                                proj.routing.add_edge(from_id, to_id);
                            }
                            drop(proj);
                            app.project_dirty = true;
                        }
                    }
                }
                return;
            }

            return;
        }

        // Top panels (sections 0-3): click to focus + toggle item.
        let rects = app.config_panel_rects.get();
        for (section, &rect) in rects.iter().enumerate() {
            if rect.width == 0 { continue; }
            if col >= rect.x && col < rect.x + rect.width
                && row >= rect.y && row < rect.y + rect.height
            {
                let inner_y = rect.y + 2;
                if row >= inner_y {
                    let item_idx = (row - inner_y) as usize;
                    app.config_state.section = section;
                    app.config_state.cursor = item_idx;
                    app.toggle_config_item_enabled();
                    return;
                }
            }
        }
    }

    // Velocity bar-chart click: set velocity of the clicked step.
    if app.current_view == ViewKind::Tracker {
        let chart = app.vel_chart_area.get();
        if chart.width > 0 && chart.height > 0
            && col >= chart.x && col < chart.x + chart.width
            && row >= chart.y && row < chart.y + chart.height
        {
            let s   = (col - chart.x) as usize + app.piano_step_scroll;
            let vel = vel_from_chart_y((row - chart.y) as usize, chart.height as usize);
            let param_name = ["VEL","GAIN","PAN","LP","HP","LFO","SPD","AMP"][app.modulation_cursor.min(7)];
            if set_step_mod_param(app, s, vel) {
                app.tracker_state.cursor.0 = s;
                app.status_msg = format!("{} step {} → {}", param_name, s + 1, vel);
            }
            return;
        }
    }

    // Tracker step table: scrollbar click or row-click to jump cursor.
    if app.current_view == ViewKind::Tracker && app.tracker_section == 0 {
        let area = app.tracker_table_area.get();
        if area.width > 0 && area.height > 3 {
            let data_y   = area.y + 2;                         // first data row
            let data_end = area.y + area.height.saturating_sub(1); // last row (border)
            let sb_x     = area.x + area.width.saturating_sub(1); // scrollbar column

            if row >= data_y && row < data_end {
                let pat_len = {
                    let proj = app.project.lock();
                    proj.patterns
                        .get(app.tracker_state.pattern_key.as_deref().unwrap_or(""))
                        .map(|p| p.length)
                        .unwrap_or(16)
                };
                let vh = app.tracker_view_height.get().max(1);

                if col == sb_x {
                    // Scrollbar track click: jump scroll position.
                    app.tracker_scroll = scrollbar_click_to_scroll(
                        row, data_y, data_end - data_y, pat_len, vh,
                    );
                } else if col > area.x && col < sb_x {
                    // Table body click: jump cursor to that step.
                    let view_row  = (row - data_y) as usize;
                    let abs_row   = view_row + app.tracker_scroll;
                    if abs_row < pat_len {
                        app.tracker_state.cursor.0 = abs_row;
                    }
                }
                return;
            }
        }
    }

    if app.current_view == ViewKind::Tracker && app.tracker_section == 1 {
        let area = app.piano_roll_area.get();
        if area.width == 0 || area.height == 0 {
            return;
        }
        let inner_x = area.x + 1;
        let key_w: u16 = 5;
        let step_start_x = inner_x + key_w;
        let header_row = area.y + 1;

        // Click on horizontal scrollbar row → scroll step viewport, no note placed.
        let h_sb_row = area.y + area.height.saturating_sub(2);
        if row == h_sb_row && col >= step_start_x && col < area.x + area.width.saturating_sub(1) {
            let pat_len = {
                let proj = app.project.lock();
                proj.patterns.get(app.tracker_state.pattern_key.as_deref().unwrap_or(""))
                    .map(|p| p.length).unwrap_or(1)
            };
            let sb_x = step_start_x;
            let sb_w = (area.x + area.width.saturating_sub(1)).saturating_sub(sb_x);
            if sb_w > 0 {
                let frac = (col - sb_x) as f64 / sb_w as f64;
                app.piano_step_scroll = (frac * pat_len as f64) as usize;
                app.clamp_piano_step_scroll(app.piano_step_scroll);
            }
            return;
        }

        // Click on vertical scrollbar column → scroll note viewport, no note placed.
        let v_sb_col = area.x + area.width.saturating_sub(1);
        if col == v_sb_col && row > header_row && row < area.y + area.height.saturating_sub(1) {
            let total = crate::views::tracker::NOTE_ROWS.len();
            let visible_h = area.height.saturating_sub(4) as usize;
            app.piano_note_scroll = scrollbar_click_to_scroll(
                row, header_row + 1, area.height.saturating_sub(3), total, visible_h,
            );
            return;
        }

        // Click on the piano keys (left column) → preview only, no step placed.
        if row > header_row
            && row < area.y + area.height.saturating_sub(2) // -2 excludes horizontal scrollbar row
            && col >= inner_x
            && col < step_start_x
        {
            let note_row = (row - header_row - 1) as usize + app.piano_note_scroll;
            preview_piano_key(app, note_row, 100);
            app.piano_key_down = true;
            app.piano_key_last_row = Some(note_row);
            return;
        }

        if row <= header_row
            || row >= area.y + area.height.saturating_sub(2) // -2 excludes horizontal scrollbar row
            || col < step_start_x
            || col >= area.x + area.width.saturating_sub(1)
        {
            return;
        }

        let note_row_rel = (row - header_row - 1) as usize;
        let note_row = note_row_rel + app.piano_note_scroll;
        let step_x = col - step_start_x;
        let step = (step_x / 2) as usize + app.piano_step_scroll;

        // Left-click PLACES a note + records click time for duration-based gate on release.
        app.place_piano_note_at(note_row, step);
        app.piano_drag_note = Some((step, note_row));
        app.note_click_start = Some(std::time::Instant::now());
        app.piano_cursor = (note_row, step);
        app.tracker_state.cursor.0 = step;
        app.clamp_piano_step_scroll(step);
        app.clamp_tracker_scroll();

        // MIDI preview: audition the note on the clip's routed synth.
        let vel = {
            let proj = app.project.lock();
            proj.patterns
                .get(app.tracker_state.pattern_key.as_deref().unwrap_or(""))
                .and_then(|p| p.steps.get(step))
                .map(|n| n.velocity)
                .unwrap_or(100)
        };
        preview_piano_key(app, note_row, vel);
    }

    // ── Generative Engine panel click ─────────────────────────────────────────
    if app.current_view == ViewKind::Tracker {
        let gen_area = app.tracker_panel_rects.get()[2];
        if gen_area.width > 0
            && col >= gen_area.x && col < gen_area.x + gen_area.width
            && row >= gen_area.y && row < gen_area.y + gen_area.height
        {
            app.tracker_section = 2;
            if let Some(new_gc) = generative_row_to_gc(row, col, gen_area) {
                app.generative_cursor = new_gc;
            }
            return;
        }

        // ── Track Modulation tab click ────────────────────────────────────────
        let mod_area = app.tracker_panel_rects.get()[3];
        if mod_area.width > 0
            && col >= mod_area.x && col < mod_area.x + mod_area.width
            && row >= mod_area.y && row < mod_area.y + mod_area.height
        {
            app.tracker_section = 3;
            // Tab row is at area.y + 1 + N_CHART (N_CHART=5).
            let tab_row_y = mod_area.y + 6;
            if row == tab_row_y {
                if let Some(tab) = mod_tab_from_x(col, mod_area) {
                    app.modulation_cursor = tab;
                }
            }
            return;
        }
    }
}

fn handle_right_click(app: &mut App, col: u16, row: u16) {
    app.last_mouse_pos = (col, row);
    app.piano_drag_note = None;

    // ── Matrix: right-click disables the clip at the clicked cell ─────────────
    if app.current_view == ViewKind::Matrix {
        let rects = app.matrix_panel_rects.get();
        let gr = rects[0];
        if gr.width > 0 && col >= gr.x && col < gr.x + gr.width
            && row >= gr.y && row < gr.y + gr.height
        {
            let (cell_w, cell_h) = app.matrix_cell_size.get();
            const ROW_LBL: u16 = 3;
            let x0 = gr.x + 1 + ROW_LBL + 1;
            let y0 = gr.y + 3;
            if cell_w > 0 && cell_h > 0 && col >= x0 && row >= y0 {
                let cell_col = ((col - x0) / (cell_w as u16 + 1)) as usize;
                let cell_row = ((row - y0) / (cell_h as u16 + 1)) as usize;
                if cell_col < app.matrix_cols && cell_row < app.matrix_rows {
                    let row_key = ((b'A' + cell_row as u8) as char).to_string();
                    let mut proj = app.project.lock();
                    if let Some(Some(clip)) = proj.matrix
                        .get_mut(&row_key)
                        .and_then(|r| r.get_mut(cell_col))
                    {
                        clip.enabled = !clip.enabled;
                        let enabled = clip.enabled;
                        let pat = clip.pattern_key.clone().unwrap_or_default();
                        drop(proj);
                        app.project_dirty = true;
                        let state = if enabled { "Enabled" } else { "Disabled" };
                        app.status_msg = if pat.is_empty() {
                            format!("{} {}{:02}", state, row_key, cell_col + 1)
                        } else {
                            format!("{} {} ({})", state, pat, row_key)
                        };
                    }
                    app.matrix_state.cursor = (cell_row, cell_col);
                    return;
                }
            }
        }
    }

    if app.current_view == ViewKind::Tracker && app.tracker_section == 1 {
        let area = app.piano_roll_area.get();
        if area.width == 0 || area.height == 0 {
            return;
        }
        let inner_x = area.x + 1;
        let key_w: u16 = 5;
        let step_start_x = inner_x + key_w;
        let header_row = area.y + 1;

        if row <= header_row
            || row >= area.y + area.height.saturating_sub(2) // -2 excludes horizontal scrollbar row
            || col < step_start_x
            || col >= area.x + area.width.saturating_sub(1)
        {
            return;
        }

        let note_row_rel = (row - header_row - 1) as usize;
        let note_row = note_row_rel + app.piano_note_scroll;
        let step_x = col - step_start_x;
        let step = (step_x / 2) as usize + app.piano_step_scroll;

        // Right-click ERASES the note at this position.
        app.remove_piano_note_at(note_row, step);
        app.piano_cursor = (note_row, step);
        app.tracker_state.cursor.0 = step;
    }

    // ── Mixer: click to select channel and/or active param ────────────────────
    if app.current_view == ViewKind::Mixer {
        let strips = app.mixer_strips_area.get();
        if strips.width == 0 { return; }

        // Is the click inside the strips area?
        if col >= strips.x && col < strips.x + strips.width
            && row >= strips.y && row < strips.y + strips.height
        {
            // Determine which strip column was clicked.
            let strip_count = app.mixer_strip_count.get() as usize;
            if strip_count > 0 {
                let col_w = (strips.width / strip_count as u16).max(1);
                let strip_col = ((col.saturating_sub(strips.x)) / col_w) as usize;

                // Map strip column to entry index.
                let proj = app.project.lock();
                let entries = views::mixer::collect_mixer_entries(&proj);
                drop(proj);
                let mut c = 0usize;
                let mut entry_idx = None;
                for (ei, e) in entries.iter().enumerate() {
                    let cols = if e.ch.stereo { 2 } else { 1 };
                    if strip_col >= c && strip_col < c + cols {
                        entry_idx = Some(ei);
                        break;
                    }
                    c += cols;
                }
                // MASTER L/R or audio slot
                if entry_idx.is_none() && strip_col >= c {
                    let offset = strip_col - c;
                    if offset < 2 {
                        entry_idx = Some(entries.len() + offset);
                    } else {
                        let n_audio = app.audio_slots.len();
                        let audio_idx = (offset - 2).min(n_audio.saturating_sub(1));
                        entry_idx = Some(entries.len() + 2 + audio_idx);
                    }
                }
                if let Some(ei) = entry_idx {
                    app.mixer_state.selected_channel = ei;
                }
            }

            // Determine which param row was clicked based on y.
            let param_ys = app.mixer_param_ys.get();
            // param_ys: [mute, vol_label, fader_start, fader_end, eq_lo, eq_lm, eq_hm, eq_hi, pan, fx]
            let param = if row >= param_ys[9] && param_ys[9] > 0 { 6 }       // FX
                else if row >= param_ys[8] && param_ys[8] > 0 { 5 }           // PAN
                else if row >= param_ys[7] && param_ys[7] > 0 { 4 }           // EQ HI
                else if row >= param_ys[6] && param_ys[6] > 0 { 3 }           // EQ HM
                else if row >= param_ys[5] && param_ys[5] > 0 { 2 }           // EQ LM
                else if row >= param_ys[4] && param_ys[4] > 0 { 1 }           // EQ LO
                else { 0 };                                                      // VOL/fader
            app.mixer_state.active_param = param;
        }
    }
}

fn handle_drag(app: &mut App, col: u16, row: u16) {
    let (prev_col, prev_row) = app.last_mouse_pos;
    app.last_mouse_pos = (col, row);
    app.mouse_drag = true;

    let dcol = col as i32 - prev_col as i32;
    let drow = row as i32 - prev_row as i32;

    // Velocity chart drag: paint velocities across steps as the mouse moves.
    if app.current_view == ViewKind::Tracker {
        let chart = app.vel_chart_area.get();
        if chart.width > 0 && chart.height > 0
            && col >= chart.x && col < chart.x + chart.width
            && row >= chart.y && row < chart.y + chart.height
        {
            let s   = (col - chart.x) as usize + app.piano_step_scroll;
            let vel = vel_from_chart_y((row - chart.y) as usize, chart.height as usize);
            let param_name = ["VEL","GAIN","PAN","LP","HP","LFO","SPD","AMP"][app.modulation_cursor.min(7)];
            if set_step_mod_param(app, s, vel) {
                app.tracker_state.cursor.0 = s;
                app.status_msg = format!("{} step {} → {}", param_name, s + 1, vel);
            }
            return;
        }
    }

    match app.current_view {
        ViewKind::Tracker if app.tracker_section == 0 => {
            // Tracker scrollbar drag: scrub scroll position.
            let area = app.tracker_table_area.get();
            if area.width > 0 && area.height > 3 {
                let sb_x     = area.x + area.width.saturating_sub(1);
                let data_y   = area.y + 2;
                let data_end = area.y + area.height.saturating_sub(1);
                if col == sb_x && row >= data_y && row < data_end {
                    let pat_len = {
                        let proj = app.project.lock();
                        proj.patterns
                            .get(app.tracker_state.pattern_key.as_deref().unwrap_or(""))
                            .map(|p| p.length)
                            .unwrap_or(16)
                    };
                    let vh = app.tracker_view_height.get().max(1);
                    app.tracker_scroll = scrollbar_click_to_scroll(
                        row, data_y, data_end - data_y, pat_len, vh,
                    );
                }
            }
        }
        ViewKind::Tracker if app.tracker_section == 1 => {
            let area = app.piano_roll_area.get();
            let key_w: u16 = 5;
            let step_start_x = area.x + 1 + key_w;
            let header_row = area.y + 1;

            // Glissando: drag over piano keys plays each new row once.
            if app.piano_key_down
                && col >= area.x + 1
                && col < step_start_x
                && row > header_row
                && row < area.y + area.height.saturating_sub(2) // -2 excludes scrollbar row
            {
                let note_row = (row - header_row - 1) as usize + app.piano_note_scroll;
                if app.piano_key_last_row != Some(note_row) {
                    preview_piano_key(app, note_row, 100);
                    app.piano_key_last_row = Some(note_row);
                }
            }

            // Left-drag over grid: extend the gate of the note placed on left-click.
            if let Some((drag_step, _)) = app.piano_drag_note {
                if col >= step_start_x {
                    let cur_step_x = col - step_start_x;
                    let cur_step = (cur_step_x / 2) as usize + app.piano_step_scroll;
                    let steps_held = cur_step.saturating_sub(drag_step) + 1;
                    let gate = (steps_held * 100).min(400) as u16;
                    app.set_piano_note_gate(drag_step, gate);
                    app.status_msg = format!("PIANO: gate step {} → {}%", drag_step + 1, gate);
                }
            }
        }
        ViewKind::Mixer => {
            if drow != 0 {
                let idx = app.mixer_state.selected_channel;
                let param = app.mixer_state.active_param;
                app.adjust_mixer_param(idx, param, -drow);
            }
            if dcol.abs() > 2 {
                let n = {
                    let proj = app.project.lock();
                    let n_audio = views::mixer::collect_audio_slot_entries(app).len();
                    drop(proj);
                    views::mixer::mixer_entry_count(&app.project.lock()).saturating_add(n_audio).saturating_sub(1)
                };
                if dcol > 0 {
                    app.mixer_state.selected_channel =
                        (app.mixer_state.selected_channel + 1).min(n);
                } else {
                    app.mixer_state.selected_channel =
                        app.mixer_state.selected_channel.saturating_sub(1);
                }
            }
        }
        ViewKind::Arranger => {
            match app.arranger_state.section {
                2 => {
                    // Song transport: ←→ navigates buttons, ↑↓ adjusts BPM when on button 3.
                    if dcol != 0 {
                        let tc = app.arranger_state.song_transport_cursor as i32 + dcol.signum();
                        app.arranger_state.song_transport_cursor = tc.clamp(0, 3) as usize;
                    }
                    if drow != 0 && app.arranger_state.song_transport_cursor == 3 {
                        app.adjust_bpm(-drow as f64);
                    }
                }
                1 => {
                    // Automation section.
                    if drow != 0 {
                        app.arranger_state.automation_lane =
                            (app.arranger_state.automation_lane as i32 + drow)
                                .clamp(0, 1) as usize;
                    }
                    if dcol != 0 {
                        let new_cur =
                            (app.arranger_state.automation_cursor as i32 + dcol).max(0) as usize;
                        app.arranger_state.automation_cursor = new_cur;
                        app.arranger_state.bar_offset =
                            (app.arranger_state.automation_cursor as u32).saturating_sub(4);
                    }
                }
                _ => {
                    // Tracks section: ↑↓ selects row, ←→ scrolls bars.
                    if drow != 0 {
                        app.arranger_state.selected_track =
                            (app.arranger_state.selected_track as i32 + drow)
                                .clamp(0, app.matrix_rows.saturating_sub(1) as i32) as usize;
                    }
                    if dcol != 0 {
                        let new_offset =
                            (app.arranger_state.bar_offset as i32 - dcol.signum()).max(0) as u32;
                        app.arranger_state.bar_offset = new_offset;
                    }
                }
            }
        }
        _ => {}
    }
}

/// Switch the active subsection when the mouse hovers over a different panel.
/// Mirrors the Tab-key behaviour but driven by pointer position.
fn handle_hover(app: &mut App, col: u16, row: u16) {
    app.routing_graph_hovered.set(false);
    if app.current_view != ViewKind::Matrix {
        app.hovered_matrix_cell.set(None);
    }
    // Dropdown hover: update menu_cursor to highlight the item under the pointer.
    if let Some(kind) = app.menu_open {
        let mut bar_x = 0u16;
        for &k in MenuKind::ALL {
            if k == kind { break; }
            bar_x += k.label().len() as u16;
        }
        let items = kind.items();
        let term_w = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80);
        let panel_w = (items
            .iter()
            .map(|i| i.label.len() + i.shortcut.len() + 6)
            .max()
            .unwrap_or(20) as u16)
            .max(20)
            .min(term_w.saturating_sub(bar_x));
        let area_x  = bar_x.min(term_w.saturating_sub(panel_w));
        let inner_x = area_x + 1;
        let inner_y = 2u16; // area_y(1) + border(1)
        let inner_w = panel_w.saturating_sub(2);

        if col >= inner_x && col < inner_x + inner_w && row >= inner_y {
            let item_row = (row - inner_y) as usize;
            if let Some(item) = items.get(item_row) {
                if !item.separator && !item.disabled {
                    let cursor = items[..item_row]
                        .iter()
                        .filter(|i| !i.separator && !i.disabled)
                        .count();
                    app.menu_cursor = cursor;
                }
            }
        }
        return;
    }


    match app.current_view {
        ViewKind::Matrix => {
            let rects = app.matrix_panel_rects.get();
            for (i, &rect) in rects.iter().enumerate() {
                if hit(col, row, rect) && app.matrix_section != i {
                    app.matrix_section = i;
                    app.status_msg = match i {
                        1 => "TRANSPORT: Enter=trigger  ←→=navigate  ↑↓=adjust  Tab=next".to_string(),
                        2 => "POLYMETER: ↑↓=select pattern  ←→=scroll steps  Tab=next".to_string(),
                        3 => "ROUTING: click=toggle output  ◄/►=channel  ↑↓=navigate  Enter=assign  R=refresh".to_string(),
                        _ => "MATRIX: hjkl=navigate  Enter=open  e=enable  Del=remove  Tab=routing".to_string(),
                    };
                    break;
                }
            }
            // Grid cell hover: compute (row, col) from mouse position.
            {
                const ROW_LBL: usize = 3;
                let grid = rects[0];
                if grid.width > 0 && hit(col, row, grid) {
                    let (cell_w, cell_h) = app.matrix_cell_size.get();
                    let inner_x = grid.x + 1;
                    let inner_y = grid.y + 1;
                    if cell_w > 0 && cell_h > 0 && col >= inner_x && row >= inner_y {
                        let rx = (col - inner_x) as usize;
                        let ry = (row - inner_y) as usize;
                        // Skip column-header line (1 line).
                        if ry >= 1 {
                            let ry = ry - 1;
                            let mat_row = (ry / (1 + cell_h)).min(app.matrix_rows.saturating_sub(1));
                            let mat_col = if rx >= ROW_LBL + 1 {
                                ((rx - ROW_LBL - 1) / (1 + cell_w)).min(app.matrix_cols.saturating_sub(1))
                            } else {
                                0
                            };
                            app.hovered_matrix_cell.set(Some((mat_row, mat_col)));
                        } else {
                            app.hovered_matrix_cell.set(None);
                        }
                    } else {
                        app.hovered_matrix_cell.set(None);
                    }
                } else {
                    app.hovered_matrix_cell.set(None);
                }
            }
            // Update transport button hover state.
            let tr = rects[1];
            app.hovered_transport_btn = if tr.width > 0 && hit(col, row, tr) {
                let inner_x = tr.x + 1;
                let inner_y = tr.y + 1;
                if col >= inner_x && row >= inner_y && row - inner_y <= 2 {
                    match col - inner_x {
                        0..=7   => Some(0), // PLAY
                        9..=16  => Some(1), // STOP
                        18..=25 => Some(2), // REC
                        27..=34 => Some(3), // TAP
                        36..=46 => Some(4), // BPM
                        _ => None,
                    }
                } else if col >= inner_x && row == inner_y + 3 {
                    // MATRIX SIZE line: ROWS or COLS hover.
                    let x_off = (col - inner_x) as usize;
                    Some(if x_off < 18 { 5 } else { 6 })
                } else {
                    None
                }
            } else {
                None
            };
        }
        ViewKind::Tracker => {
            let rects = app.tracker_panel_rects.get();
            // Index: 0=step_table, 1=piano_roll, 2=generative, 3=modulation
            for (i, &rect) in rects.iter().enumerate() {
                if hit(col, row, rect) && app.tracker_section != i {
                    app.tracker_section = i;
                    app.status_msg = match i {
                        0 => "TRACKER: Step editor | hjkl=move  Enter=edit  [/]=len".to_string(),
                        1 => "PIANO ROLL: L-click=place note  L-drag=extend gate  R-click=erase  R-drag=paint erase".to_string(),
                        2 => "GENERATIVE ENGINE: ←→=param  ↑↓=adjust  Tab=next".to_string(),
                        3 => "TRACK MODULATION: ←→=param  ↑↓=adjust  Tab=next".to_string(),
                        _ => String::new(),
                    };
                    break;
                }
            }
        }
        ViewKind::Arranger => {
            let rects = app.arranger_panel_rects.get();
            // Index: 0=tracks, 1=automation, 2=song_transport
            for (i, &rect) in rects.iter().enumerate() {
                if hit(col, row, rect) && app.arranger_state.section != i {
                    app.arranger_state.section = i;
                    app.status_msg = match i {
                        1 => "ARRANGER: Automation | hjkl=navigate  a=add/remove  Tab=next".to_string(),
                        2 => "ARRANGER: Song transport | ←→=navigate  Enter=trigger  Tab=back".to_string(),
                        _ => "ARRANGER: Tracks | ↑↓=select  ←→=scroll  Tab=next".to_string(),
                    };
                    break;
                }
            }
        }
        ViewKind::Mixer => {
            let strips = app.mixer_strips_area.get();
            if strips.width > 0 && hit(col, row, strips) {
                // Hovering over the strips — select channel by x position.
                let strip_count = app.mixer_strip_count.get() as usize;
                if strip_count > 0 {
                    let col_w = (strips.width / strip_count as u16).max(1);
                    let strip_col = ((col.saturating_sub(strips.x)) / col_w) as usize;
                    let proj = app.project.lock();
                    let entries = views::mixer::collect_mixer_entries(&proj);
                    drop(proj);
                    let mut c = 0usize;
                    let mut found = false;
                    for (ei, e) in entries.iter().enumerate() {
                        let cols = if e.ch.stereo { 2 } else { 1 };
                        if strip_col >= c && strip_col < c + cols {
                            app.mixer_state.selected_channel = ei;
                            found = true;
                            break;
                        }
                        c += cols;
                    }
                    if !found && strip_col >= c {
                        let offset = strip_col - c;
                        if offset < 2 {
                            // MASTER L or R
                            app.mixer_state.selected_channel = entries.len() + offset;
                        } else {
                            // Audio engine slot
                            let n_audio = app.audio_slots.len();
                            let audio_idx = (offset - 2).min(n_audio.saturating_sub(1));
                            app.mixer_state.selected_channel = entries.len() + 2 + audio_idx;
                        }
                    }
                }
            }
        }
        ViewKind::Config => {
            // Routing graph hover highlight.
            let rg = app.routing_graph_area.get();
            app.routing_graph_hovered.set(rg.width > 0 && hit(col, row, rg));

            // Top panels (sections 0-3): switch focus on hover.
            let rects = app.config_panel_rects.get();
            for (i, &rect) in rects.iter().enumerate() {
                if hit(col, row, rect) {
                    if app.config_state.section != i {
                        app.config_state.section = i;
                        app.config_state.cursor = 0;
                    }
                    return;
                }
            }
        }
        ViewKind::Granular => {}
    }
}

// ─── Routing view keyboard handler ───────────────────────────────────────────

fn handle_fx_routing_key(app: &mut App, key: crossterm::event::KeyEvent) {
    // Branch: audio engine slot, master bus, or MIDI channel.
    if let Some(slot_id) = app.selected_audio_slot_id() {
        handle_audio_fx_key(app, key, slot_id);
        return;
    }
    if app.is_master_channel_selected() {
        handle_master_fx_key(app, key);
        return;
    }

    // ── MIDI channel FX panel ─────────────────────────────────────────────────
    const MAX_PARAM_ROW: usize = 10;
    let on_header = app.mixer_state.fx_row == 0;

    match key.code {
        KeyCode::Esc => {
            if on_header {
                app.mixer_state.fx_panel_focused = false;
            } else {
                app.mixer_state.fx_row = 0;
            }
        }
        KeyCode::Tab => {
            app.mixer_state.fx_panel_focused = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if on_header {
                app.mixer_state.fx_slot_idx = (app.mixer_state.fx_slot_idx + 1) % 3;
            } else {
                app.mixer_state.fx_row = (app.mixer_state.fx_row + 1).min(MAX_PARAM_ROW);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if on_header {
                app.mixer_state.fx_slot_idx = (app.mixer_state.fx_slot_idx + 2) % 3;
            } else {
                let next = app.mixer_state.fx_row.saturating_sub(1);
                app.mixer_state.fx_row = if next < 3 { 0 } else { next };
            }
        }
        KeyCode::Char('h') | KeyCode::Left => {
            if on_header {
                app.adjust_fx_slot_param(-1);
            } else if app.mixer_state.fx_col == 1 {
                app.mixer_state.fx_col = 0;
            } else {
                app.adjust_fx_slot_param(-1);
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if on_header {
                app.adjust_fx_slot_param(1);
            } else if app.mixer_state.fx_col == 0 {
                app.mixer_state.fx_col = 1;
            } else {
                app.adjust_fx_slot_param(1);
            }
        }
        _ => {}
    }
}

fn handle_audio_fx_key(app: &mut App, key: crossterm::event::KeyEvent, slot_id: u32) {
    use crate::app::{AudioFxEntry, AudioFxKind, fx_param_descs};

    let n = app.audio_slot_fx.get(&slot_id).map(|c| c.len()).unwrap_or(0);
    let idx = app.mixer_state.fx_slot_idx.min(n.saturating_sub(1));
    let fx_row = app.mixer_state.fx_row;
    let in_param_mode = fx_row > 0;

    match key.code {
        KeyCode::Esc => {
            if in_param_mode {
                app.mixer_state.fx_row = 0;
            } else {
                app.mixer_state.fx_panel_focused = false;
            }
        }
        KeyCode::Tab => {
            app.mixer_state.fx_row = 0;
            app.mixer_state.fx_panel_focused = false;
        }

        KeyCode::Char('j') | KeyCode::Down => {
            if in_param_mode {
                let n_params = app.audio_slot_fx.get(&slot_id)
                    .and_then(|c| c.get(idx))
                    .map(|e| fx_param_descs(e.kind).len())
                    .unwrap_or(0);
                if app.mixer_state.fx_row < n_params {
                    app.mixer_state.fx_row += 1;
                }
            } else if n > 0 {
                app.mixer_state.fx_slot_idx = (idx + 1) % n;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if in_param_mode {
                if app.mixer_state.fx_row > 1 {
                    app.mixer_state.fx_row -= 1;
                } else {
                    app.mixer_state.fx_row = 0;
                }
            } else if n > 0 {
                app.mixer_state.fx_slot_idx = idx.checked_sub(1).unwrap_or(n - 1);
            }
        }

        KeyCode::Char('h') | KeyCode::Left => {
            if in_param_mode {
                let param_idx = fx_row - 1;
                app.adjust_audio_fx_param(slot_id, idx, param_idx, -0.05);
            } else if let Some(chain) = app.audio_slot_fx.get_mut(&slot_id) {
                if let Some(entry) = chain.get_mut(idx) {
                    let new_kind = entry.kind.prev();
                    *entry = AudioFxEntry::new(new_kind);
                }
                app.rebuild_audio_fx_chain(slot_id);
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if in_param_mode {
                let param_idx = fx_row - 1;
                app.adjust_audio_fx_param(slot_id, idx, param_idx, 0.05);
            } else if let Some(chain) = app.audio_slot_fx.get_mut(&slot_id) {
                if let Some(entry) = chain.get_mut(idx) {
                    let new_kind = entry.kind.next();
                    *entry = AudioFxEntry::new(new_kind);
                }
                app.rebuild_audio_fx_chain(slot_id);
            }
        }

        KeyCode::Enter => {
            if in_param_mode {
                // Reset param to its default.
                let param_idx = fx_row - 1;
                if let Some(chain) = app.audio_slot_fx.get_mut(&slot_id) {
                    if let Some(entry) = chain.get_mut(idx) {
                        let default = fx_param_descs(entry.kind)
                            .get(param_idx)
                            .map(|d| d.default)
                            .unwrap_or(0.0);
                        if let Some(v) = entry.params.get_mut(param_idx) {
                            *v = default;
                            entry.sync_wet();
                        }
                    }
                }
                app.rebuild_audio_fx_chain(slot_id);
            } else {
                if let Some(chain) = app.audio_slot_fx.get_mut(&slot_id) {
                    if let Some(entry) = chain.get_mut(idx) {
                        entry.enabled = !entry.enabled;
                    }
                }
                app.rebuild_audio_fx_chain(slot_id);
            }
        }

        KeyCode::Char(' ') => {
            if n > 0 {
                app.mixer_state.fx_row = if in_param_mode { 0 } else { 1 };
            }
        }

        KeyCode::Char('a') => {
            app.mixer_state.fx_row = 0;
            let chain = app.audio_slot_fx.entry(slot_id).or_default();
            chain.push(AudioFxEntry::new(AudioFxKind::Delay));
            let new_idx = chain.len() - 1;
            app.mixer_state.fx_slot_idx = new_idx;
            app.rebuild_audio_fx_chain(slot_id);
            app.set_timed_status("FX added — hl=type  jk=params".to_string(), 2);
        }

        KeyCode::Delete | KeyCode::Backspace => {
            app.mixer_state.fx_row = 0;
            let chain = app.audio_slot_fx.entry(slot_id).or_default();
            if !chain.is_empty() && idx < chain.len() {
                chain.remove(idx);
                let new_n = chain.len();
                if app.mixer_state.fx_slot_idx >= new_n && new_n > 0 {
                    app.mixer_state.fx_slot_idx = new_n - 1;
                }
                app.rebuild_audio_fx_chain(slot_id);
                app.set_timed_status("FX removed".to_string(), 2);
            }
        }

        KeyCode::Char('J') => {
            app.mixer_state.fx_row = 0;
            let chain = app.audio_slot_fx.entry(slot_id).or_default();
            if idx + 1 < chain.len() {
                chain.swap(idx, idx + 1);
                app.mixer_state.fx_slot_idx = idx + 1;
                app.rebuild_audio_fx_chain(slot_id);
            }
        }

        KeyCode::Char('K') => {
            app.mixer_state.fx_row = 0;
            if idx > 0 {
                let chain = app.audio_slot_fx.entry(slot_id).or_default();
                chain.swap(idx, idx - 1);
                app.mixer_state.fx_slot_idx = idx - 1;
                app.rebuild_audio_fx_chain(slot_id);
            }
        }

        KeyCode::Char('+') | KeyCode::Char('=') => {
            if let Some(chain) = app.audio_slot_fx.get_mut(&slot_id) {
                if let Some(entry) = chain.get_mut(idx) {
                    entry.wet = (entry.wet + 0.05).min(1.0);
                    entry.sync_wet();
                }
            }
            app.rebuild_audio_fx_chain(slot_id);
        }
        KeyCode::Char('-') => {
            if let Some(chain) = app.audio_slot_fx.get_mut(&slot_id) {
                if let Some(entry) = chain.get_mut(idx) {
                    entry.wet = (entry.wet - 0.05).max(0.0);
                    entry.sync_wet();
                }
            }
            app.rebuild_audio_fx_chain(slot_id);
        }

        _ => {}
    }
}

fn handle_master_fx_key(app: &mut App, key: crossterm::event::KeyEvent) {
    use crate::app::{AudioFxEntry, AudioFxKind, fx_param_descs};
    use crossterm::event::KeyCode;

    let n = app.master_fx.len();
    let idx = app.mixer_state.fx_slot_idx.min(n.saturating_sub(1));
    let fx_row = app.mixer_state.fx_row;
    let in_param_mode = fx_row > 0;

    match key.code {
        KeyCode::Esc => {
            if in_param_mode {
                app.mixer_state.fx_row = 0;
            } else {
                app.mixer_state.fx_panel_focused = false;
            }
        }
        KeyCode::Tab => {
            app.mixer_state.fx_row = 0;
            app.mixer_state.fx_panel_focused = false;
        }

        KeyCode::Char('j') | KeyCode::Down => {
            if in_param_mode {
                let n_params = app.master_fx.get(idx)
                    .map(|e| fx_param_descs(e.kind).len())
                    .unwrap_or(0);
                if app.mixer_state.fx_row < n_params {
                    app.mixer_state.fx_row += 1;
                }
            } else if n > 0 {
                app.mixer_state.fx_slot_idx = (idx + 1) % n;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if in_param_mode {
                if app.mixer_state.fx_row > 1 {
                    app.mixer_state.fx_row -= 1;
                } else {
                    app.mixer_state.fx_row = 0;
                }
            } else if n > 0 {
                app.mixer_state.fx_slot_idx = idx.checked_sub(1).unwrap_or(n - 1);
            }
        }

        KeyCode::Char('h') | KeyCode::Left => {
            if in_param_mode {
                app.adjust_master_fx_param(idx, fx_row - 1, -0.05);
            } else if let Some(entry) = app.master_fx.get_mut(idx) {
                let new_kind = entry.kind.prev();
                *entry = AudioFxEntry::new(new_kind);
                app.rebuild_master_fx_chain();
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if in_param_mode {
                app.adjust_master_fx_param(idx, fx_row - 1, 0.05);
            } else if let Some(entry) = app.master_fx.get_mut(idx) {
                let new_kind = entry.kind.next();
                *entry = AudioFxEntry::new(new_kind);
                app.rebuild_master_fx_chain();
            }
        }

        KeyCode::Enter => {
            if in_param_mode {
                let param_idx = fx_row - 1;
                if let Some(entry) = app.master_fx.get_mut(idx) {
                    let default = fx_param_descs(entry.kind)
                        .get(param_idx)
                        .map(|d| d.default)
                        .unwrap_or(0.0);
                    if let Some(v) = entry.params.get_mut(param_idx) {
                        *v = default;
                        entry.sync_wet();
                    }
                }
                app.rebuild_master_fx_chain();
            } else {
                if let Some(entry) = app.master_fx.get_mut(idx) { entry.enabled = !entry.enabled; }
                app.rebuild_master_fx_chain();
            }
        }

        KeyCode::Char(' ') => {
            if n > 0 {
                app.mixer_state.fx_row = if in_param_mode { 0 } else { 1 };
            }
        }

        KeyCode::Char('a') => {
            app.mixer_state.fx_row = 0;
            app.master_fx.push(AudioFxEntry::new(AudioFxKind::Delay));
            app.mixer_state.fx_slot_idx = app.master_fx.len() - 1;
            app.rebuild_master_fx_chain();
            app.set_timed_status("Master FX added".to_string(), 2);
        }
        KeyCode::Delete | KeyCode::Backspace => {
            app.mixer_state.fx_row = 0;
            if !app.master_fx.is_empty() && idx < n {
                app.master_fx.remove(idx);
                let new_n = app.master_fx.len();
                if app.mixer_state.fx_slot_idx >= new_n && new_n > 0 {
                    app.mixer_state.fx_slot_idx = new_n - 1;
                }
                app.rebuild_master_fx_chain();
                app.set_timed_status("Master FX removed".to_string(), 2);
            }
        }
        KeyCode::Char('J') => {
            app.mixer_state.fx_row = 0;
            if idx + 1 < n {
                app.master_fx.swap(idx, idx + 1);
                app.mixer_state.fx_slot_idx = idx + 1;
                app.rebuild_master_fx_chain();
            }
        }
        KeyCode::Char('K') => {
            app.mixer_state.fx_row = 0;
            if idx > 0 {
                app.master_fx.swap(idx, idx - 1);
                app.mixer_state.fx_slot_idx = idx - 1;
                app.rebuild_master_fx_chain();
            }
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if let Some(entry) = app.master_fx.get_mut(idx) {
                entry.wet = (entry.wet + 0.05).min(1.0);
                entry.sync_wet();
            }
            app.rebuild_master_fx_chain();
        }
        KeyCode::Char('-') => {
            if let Some(entry) = app.master_fx.get_mut(idx) {
                entry.wet = (entry.wet - 0.05).max(0.0);
                entry.sync_wet();
            }
            app.rebuild_master_fx_chain();
        }
        _ => {}
    }
}

fn handle_config_audio_key(app: &mut App, key: crossterm::event::KeyEvent) {
    const BUFFER_SIZES:  &[u32] = &[64, 128, 256, 512, 1024, 2048];
    const SAMPLE_RATES:  &[u32] = &[44100, 48000, 88200, 96000];

    match key.code {
        // ↑↓ cycle buffer size
        KeyCode::Up | KeyCode::Char('k') => {
            let cur = BUFFER_SIZES.iter().position(|&b| b == app.settings.audio.buffer_size).unwrap_or(2);
            let next = BUFFER_SIZES[(cur + BUFFER_SIZES.len() - 1) % BUFFER_SIZES.len()];
            app.settings.audio.buffer_size = next;
            app.set_timed_status(format!("Buffer size: {next} frames"), 2);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let cur = BUFFER_SIZES.iter().position(|&b| b == app.settings.audio.buffer_size).unwrap_or(2);
            let next = BUFFER_SIZES[(cur + 1) % BUFFER_SIZES.len()];
            app.settings.audio.buffer_size = next;
            app.set_timed_status(format!("Buffer size: {next} frames"), 2);
        }
        // ←→ cycle sample rate
        KeyCode::Left | KeyCode::Char('h') => {
            let cur = SAMPLE_RATES.iter().position(|&r| r == app.settings.audio.sample_rate).unwrap_or(1);
            if cur > 0 {
                app.settings.audio.sample_rate = SAMPLE_RATES[cur - 1];
                app.set_timed_status(format!("Sample rate: {} Hz", SAMPLE_RATES[cur - 1]), 2);
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            let cur = SAMPLE_RATES.iter().position(|&r| r == app.settings.audio.sample_rate).unwrap_or(1);
            if cur + 1 < SAMPLE_RATES.len() {
                app.settings.audio.sample_rate = SAMPLE_RATES[cur + 1];
                app.set_timed_status(format!("Sample rate: {} Hz", SAMPLE_RATES[cur + 1]), 2);
            }
        }
        // Enter: apply new settings by restarting the engine.
        KeyCode::Enter => {
            if let Some(ae) = &mut app.audio_engine {
                let cfg = audio_cfg_from_settings(&app.settings.audio);
                match ae.restart(cfg) {
                    Ok(()) => {
                        app.engine.set_audio_latency(
                            app.settings.audio.buffer_size,
                            app.settings.audio.sample_rate,
                        );
                        let _ = seqterm_persistence::save_settings(&app.settings);
                        app.set_timed_status("Audio engine restarted".to_string(), 2);
                    }
                    Err(e) => app.set_timed_status(format!("Restart failed: {e}"), 5),
                }
            }
        }
        // J: toggle JACK mode (only if JACK/PipeWire detected).
        KeyCode::Char('J') => {
            if app.jack_available {
                app.settings.audio.backend = if app.settings.audio.backend == "JACK" {
                    "ALSA".to_string()
                } else {
                    "JACK".to_string()
                };
                let mode = app.settings.audio.backend.clone();
                app.set_timed_status(format!("Backend → {}  (Enter=restart)", mode), 3);
            } else {
                app.set_timed_status("JACK not available — start JACK/PipeWire first".to_string(), 3);
            }
        }
        // s: toggle start/stop
        KeyCode::Char('s') => {
            if let Some(ae) = &mut app.audio_engine {
                if ae.is_running() {
                    ae.stop();
                    app.audio_engine_running = false;
                    app.set_timed_status("Audio engine stopped".to_string(), 2);
                } else {
                    let cfg = audio_cfg_from_settings(&app.settings.audio);
                    match ae.restart(cfg) {
                        Ok(()) => {
                            app.engine.set_audio_latency(
                                app.settings.audio.buffer_size,
                                app.settings.audio.sample_rate,
                            );
                            app.set_timed_status("Audio engine started".to_string(), 2);
                        }
                        Err(e) => app.set_timed_status(format!("Audio start failed: {e}"), 5),
                    }
                }
            }
        }
        // Tab: switch to routing section.
        KeyCode::Tab => { app.config_state.section = 4; }
        // Esc: go to section 0.
        KeyCode::Esc => { app.config_state.section = 0; }
        _ => {}
    }
}

/// Build `AudioEngineConfig` from persisted `AudioSettings`.
///
/// "AUTO"     → use PipeWire-JACK when PW socket is present, else JACK if jack_lsp
///              succeeds, else ALSA.
/// "PIPEWIRE" → always attempt JACK path (PipeWire's JACK compat layer).
/// "JACK"     → native JACK.
/// "ALSA"     → CPAL default host (no JACK).
// ─── Tracker FX chain panel helpers ──────────────────────────────────────────

/// Handle Enter key in the FX chain panel.
fn handle_tracker_fx_enter(app: &mut App) {
    let slot_id = match app.tracker_current_slot_id() { Some(id) => id, None => return };
    let has_entry = app.audio_slot_fx
        .get(&slot_id)
        .map(|c| c.get(app.tracker_fx_slot).is_some())
        .unwrap_or(false);
    if !has_entry {
        // Add a default FX at this slot.
        tracker_fx_add(app, crate::app::AudioFxKind::Reverb);
    }
}

/// Add a new FX to the current tracker slot.
fn tracker_fx_add(app: &mut App, kind: crate::app::AudioFxKind) {
    let slot_id = match app.tracker_current_slot_id() { Some(id) => id, None => return };
    let chain = app.audio_slot_fx.entry(slot_id).or_default();
    if chain.len() < 3 {
        // Insert at the focused slot position (or append).
        let idx = app.tracker_fx_slot.min(chain.len());
        chain.insert(idx, crate::app::AudioFxEntry::new(kind));
        app.rebuild_audio_fx_chain(slot_id);
        app.set_timed_status(format!("FX added: {}", kind.label()), 2);
    } else {
        app.set_timed_status("Max 3 FX per slot".to_string(), 2);
    }
}

/// Remove the FX at the focused slot in the tracker FX panel.
fn tracker_fx_remove(app: &mut App) {
    let slot_id = match app.tracker_current_slot_id() { Some(id) => id, None => return };
    if let Some(chain) = app.audio_slot_fx.get_mut(&slot_id) {
        let idx = app.tracker_fx_slot;
        if idx < chain.len() {
            chain.remove(idx);
            app.tracker_fx_slot  = app.tracker_fx_slot.saturating_sub(1);
            app.tracker_fx_param = 0;
            app.rebuild_audio_fx_chain(slot_id);
            app.set_timed_status("FX removed".to_string(), 2);
        }
    }
}

/// Cycle the FX type at the focused slot.
fn tracker_fx_cycle_type(app: &mut App, delta: i32) {
    let slot_id = match app.tracker_current_slot_id() { Some(id) => id, None => return };
    if let Some(chain) = app.audio_slot_fx.get_mut(&slot_id) {
        if let Some(entry) = chain.get_mut(app.tracker_fx_slot) {
            entry.kind = if delta > 0 { entry.kind.next() } else { entry.kind.prev() };
            // Reset params to defaults for the new type.
            let descs = crate::app::fx_param_descs(entry.kind);
            entry.params      = descs.iter().map(|d| d.default).collect();
            entry.cc_bindings = vec![None; descs.len()];
            entry.sync_wet();
            let label = entry.kind.label().to_string();
            app.rebuild_audio_fx_chain(slot_id);
            app.set_timed_status(format!("FX type → {}", label), 2);
        }
    }
}

/// Handle all keys while tracker section 4 (FX chain) is active.
pub fn handle_tracker_fx_keys(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use crossterm::event::KeyCode;

    // MIDI learn mode: any next incoming CC will bind to the selected parameter.
    if app.tracker_fx_midi_learn.is_some() {
        if key.code == KeyCode::Esc {
            app.tracker_fx_midi_learn = None;
            app.set_timed_status("MIDI learn cancelled".to_string(), 2);
        }
        return true; // swallow all keys while learning
    }

    match key.code {
        // ←→: change FX slot
        KeyCode::Left  | KeyCode::Char('h') => { app.move_cursor(0, -1); return true; }
        KeyCode::Right | KeyCode::Char('l') => { app.move_cursor(0,  1); return true; }
        // ↑↓: select parameter
        KeyCode::Up    | KeyCode::Char('k') => { app.move_cursor(-1, 0); return true; }
        KeyCode::Down  | KeyCode::Char('j') => { app.move_cursor( 1, 0); return true; }

        // +/=: increase parameter value
        KeyCode::Char('+') | KeyCode::Char('=') => {
            app.tracker_fx_adjust_param(0.02);
            return true;
        }
        // -/_: decrease parameter value
        KeyCode::Char('-') | KeyCode::Char('_') => {
            app.tracker_fx_adjust_param(-0.02);
            return true;
        }
        // a: add FX (cycles type selector)
        KeyCode::Char('a') => {
            tracker_fx_add(app, crate::app::AudioFxKind::Reverb);
            return true;
        }
        // Delete: remove focused FX slot
        KeyCode::Delete | KeyCode::Char('x') => {
            tracker_fx_remove(app);
            return true;
        }
        // [/]: cycle FX type on focused slot
        KeyCode::Char('[') => { tracker_fx_cycle_type(app, -1); return true; }
        KeyCode::Char(']') => { tracker_fx_cycle_type(app,  1); return true; }

        // e: toggle enable/disable
        KeyCode::Char('e') => {
            let slot_id = match app.tracker_current_slot_id() { Some(id) => id, None => return true };
            if let Some(chain) = app.audio_slot_fx.get_mut(&slot_id) {
                if let Some(entry) = chain.get_mut(app.tracker_fx_slot) {
                    entry.enabled = !entry.enabled;
                    app.rebuild_audio_fx_chain(slot_id);
                }
            }
            return true;
        }

        // m: start MIDI learn for the selected parameter
        KeyCode::Char('m') => {
            let has_param = app.tracker_fx_param_count() > 0;
            if has_param {
                app.tracker_fx_midi_learn = Some((app.tracker_fx_slot, app.tracker_fx_param));
                app.set_timed_status("MIDI learn: move a CC on your controller".to_string(), 5);
            }
            return true;
        }

        _ => {}
    }
    false
}

fn audio_cfg_from_settings(s: &seqterm_persistence::AudioSettings) -> seqterm_ports::AudioEngineConfig {
    use seqterm_ports::AudioEngineConfig;
    let backend = s.backend.to_uppercase();
    let pw_running = seqterm_audio_engine::pipewire_is_running();
    let use_jack = matches!(backend.as_str(), "JACK" | "PIPEWIRE")
        || (backend == "AUTO" && pw_running)
        || (backend == "AUTO"
            && std::process::Command::new("jack_lsp")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false));
    AudioEngineConfig {
        sample_rate:      s.sample_rate,
        buffer_size:      s.buffer_size,
        output_device:    if s.device.is_empty() || s.device == "default" { None } else { Some(s.device.clone()) },
        use_jack,
        pipewire_quantum: s.pipewire_quantum,
        ..Default::default()
    }
}

fn handle_routing_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        // Navigation (same as global hjkl/arrows).
        KeyCode::Char('h') | KeyCode::Left  => app.move_cursor(0, -1),
        KeyCode::Char('l') | KeyCode::Right => app.move_cursor(0,  1),
        KeyCode::Char('k') | KeyCode::Up    => app.move_cursor(-1, 0),
        KeyCode::Char('j') | KeyCode::Down  => app.move_cursor( 1, 0),

        // Tab: switch between node list and matrix panels.
        KeyCode::Tab => {
            app.routing_state.section = (app.routing_state.section + 1) % 2;
            app.status_msg = if app.routing_state.section == 0 {
                "ROUTING [nodes]: ↑↓=select  Tab=matrix  a=sync  Del=delete node".to_string()
            } else {
                "ROUTING [matrix]: hjkl=navigate  Enter=toggle edge  Tab=nodes".to_string()
            };
        }

        // Enter: toggle connection (only in matrix panel).
        KeyCode::Enter if app.routing_state.section == 1 => {
            let sorted_ids = {
                let proj = app.project.lock();
                proj.routing.sorted_ids()
            };
            let row_i = app.routing_state.node_cursor;
            let col_i = app.routing_state.col_cursor;
            if let (Some(&from_id), Some(&to_id)) = (sorted_ids.get(row_i), sorted_ids.get(col_i)) {
                if from_id == to_id {
                    app.status_msg = "Cannot connect a node to itself".to_string();
                } else {
                    let mut proj = app.project.lock();
                    if proj.routing.has_edge(from_id, to_id) {
                        proj.routing.remove_edge(from_id, to_id);
                        app.status_msg = "Edge removed".to_string();
                    } else if proj.routing.add_edge(from_id, to_id) {
                        app.status_msg = "Edge added".to_string();
                    } else {
                        app.status_msg = "Cannot add edge (would create cycle)".to_string();
                    }
                    app.project_dirty = true;
                }
            }
        }

        // Delete: remove the selected node.
        KeyCode::Delete | KeyCode::Backspace => {
            let cursor = app.routing_state.node_cursor;
            let (label, new_len) = {
                let mut proj = app.project.lock();
                let sorted_ids = proj.routing.sorted_ids();
                if let Some(&id) = sorted_ids.get(cursor) {
                    let label = proj.routing.nodes.get(&id).map(|n| n.label()).unwrap_or_default();
                    proj.routing.remove_node(id);
                    let n = proj.routing.nodes.len();
                    (Some(label), n)
                } else {
                    (None, 0)
                }
            };
            if let Some(label) = label {
                app.routing_state.node_cursor = cursor.min(new_len.saturating_sub(1));
                app.project_dirty = true;
                app.status_msg = format!("Removed node: {label}");
            }
        }

        // 'a': handled at the Config level before this handler is reached.

        // Global transport shortcuts pass through.
        KeyCode::Char(' ') => app.play_stop(),
        KeyCode::Char('s') => app.stop(),
        KeyCode::Char('w') => app.rewind(),   // w=rewind (r is taken by REC)
        KeyCode::Char('+') | KeyCode::Char('=') => app.adjust_bpm(1.0),
        KeyCode::Char('-') => app.adjust_bpm(-1.0),

        // View switch digits.
        KeyCode::Char('1') => { app.switch_view(ViewKind::Matrix); }
        KeyCode::Char('2') => { app.switch_view(ViewKind::Tracker); }
        KeyCode::Char('3') => { app.switch_view(ViewKind::Arranger); }
        KeyCode::Char('4') => { app.switch_view(ViewKind::Mixer); }
        KeyCode::Char('5') => { app.switch_view(ViewKind::Config); }

        _ => {}
    }
}

fn handle_right_drag(app: &mut App, col: u16, row: u16) {
    app.last_mouse_pos = (col, row);
    app.mouse_drag = true;

    if app.current_view == ViewKind::Tracker && app.tracker_section == 1 {
        // Right-drag: paint-erase notes as the mouse moves.
        let area = app.piano_roll_area.get();
        let key_w: u16 = 5;
        let step_start_x = area.x + 1 + key_w;
        let header_row = area.y + 1;
        if row > header_row
            && row < area.y + area.height.saturating_sub(2) // -2 excludes horizontal scrollbar row
            && col >= step_start_x
            && col < area.x + area.width.saturating_sub(1)
        {
            let note_row_rel = (row - header_row - 1) as usize;
            let note_row = note_row_rel + app.piano_note_scroll;
            let step_x = col - step_start_x;
            let step = (step_x / 2) as usize + app.piano_step_scroll;
            app.remove_piano_note_at(note_row, step);
            app.piano_cursor = (note_row, step);
        }
    }
}
