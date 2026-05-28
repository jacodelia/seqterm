use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use seqterm_core::{Channel, FxKind, PatternSource, Project};

use crate::app::App;

const PANEL:  Color = Color::Rgb(22, 27, 34);
const BORDER: Color = Color::Rgb(48, 54, 61);

/// Per-destination color palette — 8 distinct colors cycling for each new MIDI destination.
const DEST_COLORS: &[Color] = &[
    Color::Rgb(31,  111, 235),   // blue
    Color::Rgb(56,  200, 100),   // green
    Color::Rgb(240, 136,  62),   // orange
    Color::Rgb(160,  80, 220),   // purple
    Color::Rgb(220,  60,  60),   // red
    Color::Rgb(60,  200, 220),   // cyan
    Color::Rgb(220, 220,  60),   // yellow
    Color::Rgb(220, 120, 160),   // pink
];

/// One logical mixer entry derived from the matrix.
pub struct MixerEntry {
    /// Short display name (MIDI destination, truncated).
    pub label: String,
    /// Full destination port name for persistence lookup.
    pub dest: String,
    /// Resolved channel settings (from proj.channels or default).
    pub ch: Channel,
    /// Color assigned to this destination.
    pub color: Color,
}

/// Collect the ordered list of unique MIDI destinations from the matrix.
/// Returns entries in row-A-col-0 … row-P-col-N order, deduped.
pub fn collect_mixer_entries(proj: &Project) -> Vec<MixerEntry> {
    let mut seen  = std::collections::HashSet::new();
    let mut result = Vec::new();
    let mut color_idx = 0usize;

    for row in b'A'..=b'P' {
        let row_key = (row as char).to_string();
        let Some(slots) = proj.matrix.get(&row_key) else { continue };
        for slot in slots {
            let Some(clip) = slot else { continue };
            let Some(dest) = &clip.midi_out else { continue };
            if !seen.insert(dest.clone()) { continue; }

            let ch = proj.channels.iter()
                .find(|c| c.midi_port.as_deref() == Some(dest.as_str()))
                .cloned()
                .unwrap_or_else(|| {
                    let mut c = Channel::new(short_dest(dest));
                    c.midi_port = Some(dest.clone());
                    c
                });

            result.push(MixerEntry {
                label: short_dest(dest),
                dest:  dest.clone(),
                ch,
                color: DEST_COLORS[color_idx % DEST_COLORS.len()],
            });
            color_idx += 1;
        }
    }
    result
}

/// Number of selectable positions: MIDI entries + 2 (MASTER L/R).
pub fn mixer_entry_count(proj: &Project) -> usize {
    collect_mixer_entries(proj).len() + 2
}

/// Total selectable positions including audio engine slots.
pub fn total_mixer_count(proj: &Project, n_audio: usize) -> usize {
    mixer_entry_count(proj) + n_audio
}

// ─── Audio engine slot entries ────────────────────────────────────────────────

/// One audio engine slot (SF2 / AudioFile source) entry for the mixer display.
pub struct AudioSlotEntry {
    pub clip_key: String,
    pub slot_id:  u32,
    /// Short human-readable label (clip key + source name).
    pub label:    String,
    /// Linear gain 0.0–2.0 (1.0 = 0 dB).
    pub volume:   f32,
}

fn collect_audio_slot_entries_inner(proj: &Project, app: &App) -> Vec<AudioSlotEntry> {
    let mut keys: Vec<String> = app.audio_slots.keys().cloned().collect();
    keys.sort();
    keys.into_iter().filter_map(|clip_key| {
        let &slot_id = app.audio_slots.get(&clip_key)?;
        let row_key = clip_key.get(..1)?;
        let col: usize = clip_key.get(1..)?.parse().ok()?;
        let slots = proj.matrix.get(row_key)?;
        let clip = slots.get(col)?.as_ref()?;
        let label = match &clip.source {
            PatternSource::Sf2 { preset_name, .. } => {
                let name: String = preset_name.chars().take(8).collect();
                format!("{}: {}", clip_key, name)
            }
            PatternSource::AudioFile { path, .. } => {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                format!("{}: {}", clip_key, &stem[..stem.len().min(8)])
            }
            PatternSource::Midi => return None,
        };
        let volume = app.audio_slot_volumes.get(&slot_id).copied().unwrap_or(1.0);
        Some(AudioSlotEntry { clip_key, slot_id, label, volume })
    }).collect()
}

/// Collect audio engine slots sorted by clip key for the mixer display.
pub fn collect_audio_slot_entries(app: &App) -> Vec<AudioSlotEntry> {
    let proj = app.project.lock();
    collect_audio_slot_entries_inner(&proj, app)
}

/// Shorten a MIDI destination name for display (strip ALSA client prefix "NN:").
fn short_dest(dest: &str) -> String {
    let s = dest.rsplit(':').next().unwrap_or(dest).trim();
    let s = s.trim_start_matches(|c: char| c.is_ascii_digit());
    let s = s.trim();
    if s.is_empty() { dest.to_string() } else { s.to_string() }
}

// ─── Public draw entry point ──────────────────────────────────────────────────

pub fn draw_mixer(f: &mut Frame, app: &App, area: Rect) {
    // Horizontal split: channel strips (left) | FX sidebar (right).
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(28)])
        .split(area);

    let right_area   = h[0];
    let sidebar_area = h[1];

    // Right: strips + hint line at bottom.
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .split(right_area);
    let strips_area = v[0];
    let hint_area   = v[1];

    app.mixer_panel_rects.set([strips_area, strips_area]);
    draw_fx_sidebar(f, app, sidebar_area);
    draw_channel_strips(f, app, strips_area);

    let hint = if app.mixer_state.fx_panel_focused {
        if app.mixer_state.fx_row == 0 {
            "  ↑↓=slot  ←→=type  Enter=params  Tab=strips".to_string()
        } else {
            "  ↑↓=param  ←→=CC# or val  h=CC# col  l=val col  Esc=slot".to_string()
        }
    } else if app.mixer_state.editing {
        let param_labels = ["VOL", "EQ LO", "EQ LM", "EQ HM", "EQ HI", "PAN", "FX"];
        let lbl = param_labels.get(app.mixer_state.active_param).copied().unwrap_or("VOL");
        format!("  ↑↓=adjust [{}]  ←→=param  m=mute  s=solo  S=stereo  Esc=stop", lbl)
    } else {
        "  ←→=channel  ↑↓=volume  Enter=edit  Tab=FX sidebar  m=mute  s=solo".to_string()
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(BORDER))),
        hint_area,
    );
}

// ─── Channel strips ───────────────────────────────────────────────────────────

fn draw_channel_strips(f: &mut Frame, app: &App, area: Rect) {
    let (entries, audio_entries) = {
        let proj = app.project.lock();
        let midi  = collect_mixer_entries(&proj);
        let audio = collect_audio_slot_entries_inner(&proj, app);
        (midi, audio)
    };

    // Count total strip columns: each mono entry = 1, stereo = 2, MASTER L+R = 2, audio = 1 each.
    let n_entry_strips: usize = entries.iter()
        .map(|e| if e.ch.stereo { 2 } else { 1 })
        .sum();
    let n_audio = audio_entries.len();
    let total_strips = (n_entry_strips + 2 + n_audio).max(2);

    let constraints: Vec<Constraint> =
        std::iter::repeat_n(Constraint::Ratio(1, total_strips as u32), total_strips).collect();

    let ch_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    app.mixer_strips_area.set(area);
    app.mixer_strip_count.set(total_strips.min(36) as u16);

    // Record strip x-start positions.
    let mut strip_xs = [0u16; 36];
    for (i, rect) in ch_chunks.iter().enumerate().take(36) {
        strip_xs[i] = rect.x;
    }
    app.mixer_strip_xs.set(strip_xs);

    // Track which chunk index we're at.
    let mut strip_idx = 0usize;
    let mut param_ys_recorded = false;

    for (entry_idx, entry) in entries.iter().enumerate() {
        let is_sel = entry_idx == app.mixer_state.selected_channel;
        let is_edit = is_sel && app.mixer_state.editing;
        let active_param = if is_sel { Some(app.mixer_state.active_param) } else { None };

        if entry.ch.stereo {
            if let Some(&rect) = ch_chunks.get(strip_idx) {
                let pys = draw_strip(
                    f, &entry.ch, rect,
                    &format!("{} L", entry.label),
                    entry.color, is_sel, is_edit, app.playing,
                    StripSide::Left,
                    active_param,
                );
                if !param_ys_recorded && is_sel {
                    app.mixer_param_ys.set(pys);
                    param_ys_recorded = true;
                }
            }
            strip_idx += 1;
            if let Some(&rect) = ch_chunks.get(strip_idx) {
                draw_strip(
                    f, &entry.ch, rect,
                    &format!("{} R", entry.label),
                    entry.color, is_sel, is_edit, app.playing,
                    StripSide::Right,
                    active_param,
                );
            }
            strip_idx += 1;
        } else {
            if let Some(&rect) = ch_chunks.get(strip_idx) {
                let pys = draw_strip(
                    f, &entry.ch, rect,
                    &entry.label,
                    entry.color, is_sel, is_edit, app.playing,
                    StripSide::Mono,
                    active_param,
                );
                if !param_ys_recorded && is_sel {
                    app.mixer_param_ys.set(pys);
                    param_ys_recorded = true;
                }
            }
            strip_idx += 1;
        }
    }

    // Empty-state message when no matrix clips are routed.
    if entries.is_empty() {
        let msg_rect = if let Some(&r) = ch_chunks.first() { r } else { area };
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No MIDI routes in matrix.",
                Style::default().fg(BORDER),
            )),
            Line::from(Span::styled(
                "  Assign midi_out to a clip to see channels here.",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(BORDER)).style(Style::default().bg(PANEL)));
        f.render_widget(msg, msg_rect);
        strip_idx = 0;
    }

    // MASTER L / R.
    let n_entries = entries.len();
    let master_l_sel = app.mixer_state.selected_channel == n_entries;
    let master_r_sel = app.mixer_state.selected_channel == n_entries + 1;
    let master_color = Color::Rgb(200, 200, 200);

    if let Some(&rect) = ch_chunks.get(strip_idx) {
        let ch = Channel::new("MASTER L");
        let pys = draw_strip(f, &ch, rect, "MASTER L", master_color,
                   master_l_sel, false, app.playing, StripSide::Left, None);
        if !param_ys_recorded && master_l_sel {
            app.mixer_param_ys.set(pys);
        }
    }
    if let Some(&rect) = ch_chunks.get(strip_idx + 1) {
        let ch = Channel::new("MASTER R");
        draw_strip(f, &ch, rect, "MASTER R", master_color,
                   master_r_sel, false, app.playing, StripSide::Right, None);
    }

    // ── Audio engine slots ────────────────────────────────────────────────────
    let audio_sel_offset = n_entries + 2;
    for (ai, ae) in audio_entries.iter().enumerate() {
        let is_sel = app.mixer_state.selected_channel == audio_sel_offset + ai;
        if let Some(&rect) = ch_chunks.get(strip_idx + 2 + ai) {
            draw_audio_slot_strip(f, ae, rect, is_sel);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum StripSide {
    Mono,
    Left,
    Right,
}

/// Render one strip; return absolute y positions for each param row:
/// [mute, vol_label, fader_start, fader_end, eq_lo, eq_lm, eq_hm, eq_hi, pan, fx]
fn draw_strip(
    f: &mut Frame,
    ch: &Channel,
    area: Rect,
    label: &str,
    group_color: Color,
    selected: bool,
    editing: bool,
    playing: bool,
    side: StripSide,
    active_param: Option<usize>,
) -> [u16; 10] {
    let mut param_ys = [0u16; 10];

    let border_style = if editing {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(BORDER)
    };

    let borders = match side {
        StripSide::Mono  => Borders::ALL,
        StripSide::Left  => Borders::LEFT | Borders::TOP | Borders::BOTTOM,
        StripSide::Right => Borders::RIGHT | Borders::TOP | Borders::BOTTOM,
    };

    let title_style = Style::default().fg(group_color).add_modifier(Modifier::BOLD);

    let block = Block::default()
        .title(format!(" {} ", label))
        .title_style(if selected { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { title_style })
        .borders(borders)
        .border_style(if selected { border_style } else { Style::default().fg(group_color) })
        .style(Style::default().bg(PANEL));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 4 || inner.width < 2 { return param_ys; }

    let w = inner.width as usize;
    let vol_pct = ch.volume_pct() as usize;

    let fader_color = if ch.mute {
        Color::DarkGray
    } else if playing {
        if side == StripSide::Left { Color::Rgb(56, 220, 100) } else { Color::Rgb(40, 180, 80) }
    } else {
        group_color
    };

    // Vertical layout: mute, vol_label, fader, eq_lo, eq_lm, eq_hm, eq_hi, pan, fx
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // 0: mute/solo
            Constraint::Length(1), // 1: dB label
            Constraint::Min(2),    // 2: fader
            Constraint::Length(1), // 3: EQ LO
            Constraint::Length(1), // 4: EQ LM
            Constraint::Length(1), // 5: EQ HM
            Constraint::Length(1), // 6: EQ HI
            Constraint::Length(1), // 7: PAN
            Constraint::Length(1), // 8: FX
        ])
        .split(inner);

    // Record absolute y positions.
    if vert.len() >= 9 {
        param_ys[0] = vert[0].y;                           // mute
        param_ys[1] = vert[1].y;                           // vol label
        param_ys[2] = vert[2].y;                           // fader start
        param_ys[3] = vert[2].y + vert[2].height - 1;     // fader end
        param_ys[4] = vert[3].y;                           // EQ LO
        param_ys[5] = vert[4].y;                           // EQ LM
        param_ys[6] = vert[5].y;                           // EQ HM
        param_ys[7] = vert[6].y;                           // EQ HI
        param_ys[8] = vert[7].y;                           // PAN
        param_ys[9] = vert[8].y;                           // FX
    }

    // ── Mute/Solo row ─────────────────────────────────────────────────────────
    let flags = if ch.mute { " [M] " } else if ch.solo { " [S] " } else { "     " };
    let flags_style = if ch.mute {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if ch.solo {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(Paragraph::new(Span::styled(flags, flags_style)), vert[0]);

    // ── dB label ──────────────────────────────────────────────────────────────
    let vol_style = param_highlight(selected, active_param, 0, Color::White);
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("{:>+5.1}dB", ch.volume),
            vol_style,
        )),
        vert[1],
    );

    // ── Fader bars ────────────────────────────────────────────────────────────
    let fader_area = vert[2];
    let fa_h = fader_area.height as usize;
    let filled = (vol_pct * fa_h / 100).min(fa_h);
    let empty  = fa_h - filled;

    let fader_hl = active_param == Some(0) && selected;
    let fader_lines: Vec<Line> = (0..fa_h)
        .map(|row| {
            if row < empty {
                Line::from(Span::styled(
                    format!("{:^width$}", "▒", width = w),
                    Style::default().fg(if fader_hl { Color::Yellow } else { BORDER }).bg(PANEL),
                ))
            } else {
                Line::from(Span::styled(
                    format!("{:^width$}", "█", width = w),
                    Style::default().fg(if fader_hl { Color::Yellow } else { fader_color }).bg(PANEL),
                ))
            }
        })
        .collect();
    f.render_widget(Paragraph::new(fader_lines), fader_area);

    // ── EQ knobs ──────────────────────────────────────────────────────────────
    if vert.len() > 3 {
        let eq_data = [
            ("LO", ch.eq_low,      3usize, 1),
            ("LM", ch.eq_low_mid,  4,      2),
            ("HM", ch.eq_high_mid, 5,      3),
            ("HI", ch.eq_high,     6,      4),
        ];
        for (lbl, val, vi, param_idx) in eq_data {
            if vert.len() <= vi { break; }
            let style = param_highlight(selected, active_param, param_idx, Color::Rgb(140, 180, 255));
            let bar = eq_bar(val, (w.saturating_sub(7)).max(3));
            let text = format!("{} {}  {:+3}", lbl, bar, val);
            f.render_widget(
                Paragraph::new(Span::styled(text, style)),
                vert[vi],
            );
        }
    }

    // ── PAN row ───────────────────────────────────────────────────────────────
    if vert.len() > 7 {
        let pan_str = match side {
            StripSide::Left  => format!("L: {}", ch.pan.label()),
            StripSide::Right => format!("R: {}", ch.pan.label()),
            StripSide::Mono  => format!("P: {}", ch.pan.label()),
        };
        let style = param_highlight(selected, active_param, 5, Color::Cyan);
        f.render_widget(
            Paragraph::new(Span::styled(pan_str, style)),
            vert[7],
        );
    }

    // ── FX row ────────────────────────────────────────────────────────────────
    if vert.len() > 8 {
        let style = param_highlight(selected, active_param, 6, Color::Rgb(200, 160, 255));
        let bar = fx_bar(ch.fx_amount, (w.saturating_sub(7)).max(3));
        let text = format!("FX {} {:>3}", bar, ch.fx_amount);
        f.render_widget(
            Paragraph::new(Span::styled(text, style)),
            vert[8],
        );
    }

    param_ys
}

/// Render a simplified strip for an audio engine slot (volume only, no EQ/FX/pan).
fn draw_audio_slot_strip(f: &mut Frame, entry: &AudioSlotEntry, area: Rect, selected: bool) {
    const AUDIO_COLOR: Color = Color::Rgb(140, 200, 255);

    let border_style = if selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(BORDER)
    };
    let block = Block::default()
        .title(format!(" {} ", entry.label))
        .title_style(if selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(AUDIO_COLOR)
        })
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 2 || inner.width < 2 { return; }

    let w = inner.width as usize;
    // Linear gain 0.0–2.0 mapped to 0–100%.
    let vol_pct = (entry.volume * 50.0).clamp(0.0, 100.0) as usize;

    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    // dB label.
    let db = 20.0_f32 * entry.volume.max(1e-6).log10();
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("{:>+5.1}dB", db),
            if selected { Style::default().fg(Color::Yellow) } else { Style::default().fg(Color::White) },
        )),
        vert[0],
    );

    // Fader bars.
    let fa_h = vert[1].height as usize;
    let filled = (vol_pct * fa_h / 100).min(fa_h);
    let empty  = fa_h - filled;
    let bar_color = if selected { Color::Yellow } else { AUDIO_COLOR };
    let fader_lines: Vec<Line> = (0..fa_h).map(|row| {
        if row < empty {
            Line::from(Span::styled(format!("{:^w$}", "▒", w = w), Style::default().fg(BORDER).bg(PANEL)))
        } else {
            Line::from(Span::styled(format!("{:^w$}", "█", w = w), Style::default().fg(bar_color).bg(PANEL)))
        }
    }).collect();
    f.render_widget(Paragraph::new(fader_lines), vert[1]);
}

/// Return the style for a knob row: yellow if this param is active & selected, else default.
fn param_highlight(selected: bool, active_param: Option<usize>, param: usize, base: Color) -> Style {
    if selected && active_param == Some(param) {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(base)
    }
}

/// Bipolar bar for EQ (-12..+12 dB), always `width` chars wide.
fn eq_bar(val: i8, width: usize) -> String {
    let w = width.max(3);
    let center = w / 2;
    let pct = ((val.clamp(-12, 12) + 12) as usize * w / 24).min(w);
    let mut chars = vec!['─'; w];
    if pct <= center {
        for i in pct..center { chars[i] = '◀'; }
    } else {
        for i in center..pct { chars[i] = '▶'; }
    }
    chars[center] = '┼';
    chars.iter().collect()
}

/// Unipolar bar for FX (0..127), always `width` chars wide.
fn fx_bar(val: u8, width: usize) -> String {
    let w = width.max(3);
    let filled = (val as usize * w / 127).min(w);
    let mut chars = vec!['─'; w];
    for i in 0..filled { chars[i] = '█'; }
    chars.iter().collect()
}

// ─── FX sidebar (routing-style, always visible) ───────────────────────────────

const BG:     Color = Color::Rgb(13, 17, 23);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const OK:     Color = Color::Rgb(56, 200, 100);
const HEADER: Color = Color::Rgb(240, 136, 62);

fn draw_fx_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let focused  = app.mixer_state.fx_panel_focused;
    let sel_slot = app.mixer_state.fx_slot_idx;
    let fx_row   = app.mixer_state.fx_row;
    let fx_col   = app.mixer_state.fx_col;

    // Determine if an audio engine slot or master bus is selected.
    let audio_slot_id = app.selected_audio_slot_id();

    if let Some(slot_id) = audio_slot_id {
        draw_audio_fx_sidebar(f, app, area, slot_id, focused, sel_slot);
        return;
    }

    if app.is_master_channel_selected() {
        draw_master_fx_sidebar(f, app, area, focused, sel_slot);
        return;
    }

    let proj = app.project.lock();
    let entries = collect_mixer_entries(&proj);
    let sel_idx = app.mixer_state.selected_channel;
    let (ch_label, slots) = entries.get(sel_idx).map(|e| {
        let s = proj.channels.iter()
            .find(|c| c.midi_port.as_deref() == Some(e.dest.as_str()))
            .map(|c| c.fx.clone())
            .unwrap_or_default();
        (e.label.clone(), s)
    }).unwrap_or_else(|| ("MASTER".to_string(), Default::default()));
    drop(proj);

    let outer = Block::default()
        .title(format!(" FX :: {} ", ch_label))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { ACCENT } else { BORDER }))
        .style(Style::default().bg(BG));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let w = inner.width as usize;
    let bar_w = w.saturating_sub(16).max(4);

    let mut lines: Vec<Line> = Vec::new();

    for slot_i in 0..3usize {
        let slot     = &slots[slot_i];
        let is_sel   = slot_i == sel_slot;
        let is_none  = slot.kind == FxKind::None;

        // ── Slot header ───────────────────────────────────────────────────────
        let header_style = if is_sel && focused && fx_row == 0 {
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else if is_sel {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else if slot.enabled {
            Style::default().fg(OK)
        } else {
            Style::default().fg(BORDER)
        };
        let dot = if slot.enabled { "●" } else { "○" };
        lines.push(Line::from(vec![
            Span::styled(format!(" {} [{}] ", dot, slot_i + 1), header_style),
            Span::styled(
                slot.kind.label().to_string(),
                if is_sel {
                    Style::default().fg(if focused && fx_row == 0 { Color::Black } else { ACCENT })
                        .add_modifier(Modifier::BOLD)
                } else if slot.enabled {
                    Style::default().fg(OK)
                } else {
                    Style::default().fg(BORDER)
                },
            ),
        ]));

        // ── Expanded params for selected slot ─────────────────────────────────
        if is_sel {
            let labels = slot.kind.param_labels();

            if is_none {
                lines.push(Line::from(Span::styled(
                    "   (no effect — ← → to choose)",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for p in 0..8usize {
                    let lbl = labels[p];
                    if lbl.starts_with('─') { continue; }

                    let param_row = p + 3;
                    let row_focused = focused && fx_row == param_row;

                    let val = slot.cc_vals[p];
                    let cc  = slot.cc_nums[p];

                    let lbl_style = if row_focused {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(160, 170, 220))
                    };
                    let bar_style = if row_focused {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(BORDER)
                    };
                    let cc_style = if row_focused && fx_col == 0 {
                        Style::default().fg(Color::Black).bg(ACCENT)
                    } else if cc > 0 {
                        Style::default().fg(OK)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    let val_style = if row_focused && fx_col == 1 {
                        Style::default().fg(Color::Black).bg(ACCENT)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let short: String = lbl.chars().take(7).collect();
                    let bar = fx_bar(val, bar_w.min(6));
                    let cc_str = if cc == 0 { " ─ ".to_string() } else { format!("{:>3}", cc) };

                    lines.push(Line::from(vec![
                        Span::styled(format!("   {:<7} ", short), lbl_style),
                        Span::styled(bar, bar_style),
                        Span::styled(format!(" {:>3}", val), val_style),
                        Span::styled(format!(" c{}", cc_str), cc_style),
                    ]));
                }
            }
        }

        // ── Separator ─────────────────────────────────────────────────────────
        lines.push(Line::from(Span::styled(
            format!("  {}", "─".repeat(w.saturating_sub(4))),
            Style::default().fg(if is_sel { Color::Rgb(60, 70, 90) } else { Color::Rgb(30, 35, 42) }),
        )));
    }

    // ── MIDI routing summary (active slot) ────────────────────────────────────
    let slot = &slots[sel_slot];
    if !slot.midi_port.is_empty() || slot.midi_channel != 1 {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  PORT ", Style::default().fg(HEADER)),
            Span::styled(
                short_dest(&slot.midi_port),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  CH   ", Style::default().fg(HEADER)),
            Span::styled(
                format!("{:02}", slot.midi_channel),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        if focused {
            if fx_row == 0 { "  ↑↓=slot  ←→=type  Tab=strip" }
            else           { "  ↑↓=param  ←→=edit  Esc=back" }
        } else {
            "  Tab=focus fx panel"
        },
        Style::default().fg(BORDER),
    )));

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(BG)),
        inner,
    );
}

fn draw_audio_fx_sidebar(
    f: &mut Frame,
    app: &App,
    area: Rect,
    slot_id: u32,
    focused: bool,
    sel_slot: usize,
) {
    use crate::app::AudioFxEntry;

    let empty_chain: Vec<AudioFxEntry> = Vec::new();
    let chain = app.audio_slot_fx.get(&slot_id).unwrap_or(&empty_chain);

    let outer = Block::default()
        .title(format!(" FX :: slot {} ", slot_id))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { ACCENT } else { BORDER }))
        .style(Style::default().bg(BG));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let w = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    if chain.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no FX — press 'a' to add)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, entry) in chain.iter().enumerate() {
            let is_sel = focused && i == sel_slot;
            let dot  = if entry.enabled { "●" } else { "○" };
            let wet_pct = (entry.wet * 100.0) as u8;

            let row_style = if is_sel {
                Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
            } else if entry.enabled {
                Style::default().fg(OK)
            } else {
                Style::default().fg(BORDER)
            };

            let label: String = format!(
                " {} {:>9}  {:>3}%",
                dot,
                entry.kind.label(),
                wet_pct,
            );
            // Pad to full width.
            let padded = format!("{:<width$}", label, width = w.saturating_sub(1));
            lines.push(Line::from(Span::styled(padded, row_style)));

            if is_sel {
                lines.push(Line::from(Span::styled(
                    format!("   ←→=type  +−=wet  Enter=toggle"),
                    Style::default().fg(Color::Rgb(100, 140, 220)),
                )));
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        if focused { "  ↑↓=sel  a=add  Del=rm  J/K=reorder" }
        else       { "  Tab=focus fx panel" },
        Style::default().fg(BORDER),
    )));

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(BG)),
        inner,
    );
}

fn draw_master_fx_sidebar(f: &mut Frame, app: &App, area: Rect, focused: bool, sel_slot: usize) {
    use crate::app::AudioFxEntry;
    use ratatui::style::Color;

    let chain: &Vec<AudioFxEntry> = &app.master_fx;

    let outer = Block::default()
        .title(" FX :: MASTER BUS ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { ACCENT } else { BORDER }))
        .style(Style::default().bg(BG));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let w = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    if chain.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no FX — press 'a' to add)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, entry) in chain.iter().enumerate() {
            let is_sel = focused && i == sel_slot;
            let dot  = if entry.enabled { "●" } else { "○" };
            let wet_pct = (entry.wet * 100.0) as u8;

            let row_style = if is_sel {
                Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
            } else if entry.enabled {
                Style::default().fg(OK)
            } else {
                Style::default().fg(BORDER)
            };

            let label = format!(" {} {:>9}  {:>3}%", dot, entry.kind.label(), wet_pct);
            let padded = format!("{:<width$}", label, width = w.saturating_sub(1));
            lines.push(Line::from(Span::styled(padded, row_style)));

            if is_sel {
                lines.push(Line::from(Span::styled(
                    "   ←→=type  +−=wet  Enter=toggle",
                    Style::default().fg(Color::Rgb(100, 140, 220)),
                )));
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        if focused { "  ↑↓=sel  a=add  Del=rm  J/K=reorder" }
        else       { "  Tab=focus fx panel" },
        Style::default().fg(BORDER),
    )));

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(BG)),
        inner,
    );
}

// ─── Old full-screen FX panel (kept for draw_fx_routing_panel call sites) ────

pub fn draw_fx_routing_panel(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let entries = collect_mixer_entries(&proj);
    let sel_idx  = app.mixer_state.selected_channel;
    let slot_idx = app.mixer_state.fx_slot_idx;
    let fx_row   = app.mixer_state.fx_row;
    let fx_col   = app.mixer_state.fx_col;

    let (ch_label, slots) = entries.get(sel_idx).map(|e| {
        let s = proj.channels.iter()
            .find(|c| c.midi_port.as_deref() == Some(e.dest.as_str()))
            .map(|c| c.fx.clone())
            .unwrap_or_default();
        (e.label.clone(), s)
    }).unwrap_or_else(|| ("MASTER".to_string(), Default::default()));

    let midi_ports: Vec<String> = proj.midi_outputs.iter().map(|p| p.name.clone()).collect();
    drop(proj);

    let slot = &slots[slot_idx];
    let labels = slot.kind.param_labels();

    // ── Outer block ───────────────────────────────────────────────────────────
    let block = Block::default()
        .title(format!(" FX SLOTS :: {} ", ch_label))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split: slot-tab row (1) + separator (1) + content.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    // ── Slot tabs ─────────────────────────────────────────────────────────────
    let mut tab_spans: Vec<Span> = Vec::new();
    for i in 0..3 {
        let active = i == slot_idx;
        let kind_lbl = slots[i].kind.short_label();
        let tab = format!("  [SLOT {}: {}]  ", i + 1, kind_lbl);
        tab_spans.push(Span::styled(tab, if active {
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(BORDER)
        }));
    }
    tab_spans.push(Span::styled("  Tab=switch slot", Style::default().fg(BORDER)));
    f.render_widget(Paragraph::new(Line::from(tab_spans)), rows[0]);

    f.render_widget(
        Paragraph::new(Span::styled(
            "  ─────────────────────────────────────────────────────────────────",
            Style::default().fg(BORDER),
        )),
        rows[1],
    );

    let content = rows[2];

    // Split content: left=type+routing (30%) | right=param table (70%).
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(content);

    // ── Left: Type + MIDI routing ─────────────────────────────────────────────
    let mut left_lines: Vec<Line> = Vec::new();

    // Type row.
    let type_sel = fx_row == 0;
    left_lines.push(Line::from(vec![
        Span::styled(
            if type_sel { " ▶ TYPE   " } else { "   TYPE   " },
            if type_sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) }
            else { Style::default().fg(Color::Rgb(180, 180, 180)) },
        ),
        Span::styled(
            format!("← {} →", slot.kind.label()),
            if type_sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) }
            else { Style::default().fg(Color::White) },
        ),
    ]));

    left_lines.push(Line::from(Span::styled(
        "  ────────────────────",
        Style::default().fg(BORDER),
    )));

    // MIDI port.
    let port_sel = fx_row == 1;
    let port_lbl = if slot.midi_port.is_empty() {
        midi_ports.first().map(|s| s.as_str()).unwrap_or("(none)").to_string()
    } else {
        slot.midi_port.clone()
    };
    left_lines.push(Line::from(vec![
        Span::styled(
            if port_sel { " ▶ PORT   " } else { "   PORT   " },
            sel_style(port_sel),
        ),
        Span::styled(port_lbl, val_style(port_sel)),
    ]));

    // MIDI channel.
    let ch_sel = fx_row == 2;
    left_lines.push(Line::from(vec![
        Span::styled(
            if ch_sel { " ▶ MIDI CH" } else { "   MIDI CH" },
            sel_style(ch_sel),
        ),
        Span::styled(format!("  {:02}", slot.midi_channel), val_style(ch_sel)),
    ]));

    left_lines.push(Line::from(""));
    left_lines.push(Line::from(Span::styled(
        "  LEGEND",
        Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
    )));
    left_lines.push(Line::from(Span::styled("  ←→ = change CC#/val", Style::default().fg(BORDER))));
    left_lines.push(Line::from(Span::styled("  hl  = CC# column",    Style::default().fg(BORDER))));
    left_lines.push(Line::from(Span::styled("  jk  = param row",     Style::default().fg(BORDER))));

    f.render_widget(Paragraph::new(left_lines).style(Style::default().bg(PANEL)), cols[0]);

    // ── Right: 8-parameter table ──────────────────────────────────────────────
    let mut param_lines: Vec<Line> = Vec::new();

    // Header.
    param_lines.push(Line::from(vec![
        Span::styled(format!("{:<12}", "PARAMETER"), Style::default().fg(HEADER).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:^5}", "CC#"),        Style::default().fg(HEADER).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:<16}", "VALUE"),     Style::default().fg(HEADER).add_modifier(Modifier::BOLD)),
        Span::styled("VAL", Style::default().fg(HEADER).add_modifier(Modifier::BOLD)),
    ]));
    param_lines.push(Line::from(Span::styled(
        "────────────────────────────────────────",
        Style::default().fg(BORDER),
    )));

    for p in 0..8usize {
        let row_sel = fx_row == p + 3;
        let is_dash = labels[p].starts_with('─');

        let label_style = if is_dash {
            Style::default().fg(Color::DarkGray)
        } else if row_sel {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(200, 200, 255))
        };

        let cc = slot.cc_nums[p];
        let val = slot.cc_vals[p];

        let cc_style = if row_sel && fx_col == 0 && !is_dash {
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else if is_dash {
            Style::default().fg(Color::DarkGray)
        } else if cc == 0 {
            Style::default().fg(BORDER)
        } else {
            Style::default().fg(OK)
        };

        let val_style_cell = if row_sel && fx_col == 1 && !is_dash {
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else if is_dash {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };

        let bar = if is_dash {
            "─────────────────".to_string()
        } else {
            fx_bar(val, 16)
        };

        param_lines.push(Line::from(vec![
            Span::styled(format!("{:<12}", labels[p]), label_style),
            Span::styled(
                if is_dash { "  ── ".to_string() } else { format!("  {:>3} ", if cc == 0 { "─".to_string() } else { cc.to_string() }) },
                cc_style,
            ),
            Span::styled(bar, if is_dash { Style::default().fg(Color::DarkGray) } else { Style::default().fg(BORDER) }),
            Span::styled(
                if is_dash { "    ".to_string() } else { format!(" {:>3}", val) },
                val_style_cell,
            ),
        ]));
    }

    f.render_widget(Paragraph::new(param_lines).style(Style::default().bg(PANEL)), cols[1]);
}

fn sel_style(selected: bool) -> Style {
    if selected {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(180, 180, 180))
    }
}

fn val_style(selected: bool) -> Style {
    if selected {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    }
}
