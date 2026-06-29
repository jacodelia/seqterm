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
/// Locked/selected channel highlight (the one whose FX are being edited).
/// Distinct from the yellow used for mouse hover.
const SELECTED: Color = Color::Rgb(0, 200, 255);

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

/// One logical mixer entry — one per matrix pattern (clip cell) with a MIDI source.
pub struct MixerEntry {
    /// Short display name (pattern key / clip name / "track:col").
    pub label: String,
    /// Per-pattern channel key ("A0", "A3", …). First char is the matrix row.
    pub dest: String,
    /// Channel settings (volume, EQ, pan, mute, stereo…).
    pub ch: Channel,
    /// Color assigned to this pattern.
    pub color: Color,
}

/// Display name for a pattern (clip cell): its pattern key, else its clip name,
/// else a "track:col" fallback. Used in every channel header so the source
/// pattern is identifiable at a glance.
fn pattern_label(clip: &seqterm_core::Clip, track: &str, col: usize) -> String {
    clip.pattern_key.clone()
        .filter(|s| !s.is_empty())
        .or_else(|| if clip.name.is_empty() { None } else { Some(clip.name.clone()) })
        .unwrap_or_else(|| format!("{}:{}", track, col))
}

/// Collect one mixer entry per matrix pattern (clip cell) that has a MIDI source.
/// Each pattern gets its own channel strip, keyed per-cell ("A0", "A3", …) so its
/// volume/EQ/pan/FX/mute are stored and recalled independently. Patterns whose
/// source is SF2 / AudioFile appear in the audio slot section instead.
pub fn collect_mixer_entries(proj: &Project) -> Vec<MixerEntry> {
    let mut result = Vec::new();
    let mut color_idx = 0usize;

    for row in b'A'..=b'P' {
        let row_key = (row as char).to_string();
        let Some(slots) = proj.matrix.get(&row_key) else { continue };

        for (col, slot) in slots.iter().enumerate() {
            let Some(clip) = slot else { continue };
            // Only MIDI-source patterns belong here; audio-engine sources (SF2 /
            // AudioFile) are shown in the audio slot section.
            if !matches!(clip.source, PatternSource::Midi) { continue; }

            // Per-pattern identity key: row letter + column.
            let clip_key = format!("{}{}", row_key, col);

            // Per-pattern label: the pattern's own name so the header identifies
            // which matrix pattern feeds this channel.
            let track = proj.track_names.get(&row_key)
                .cloned()
                .unwrap_or_else(|| row_key.clone());
            let label = pattern_label(clip, &track, col);

            // Resolve Channel from proj.channels keyed by the per-pattern key, or
            // create a default carrying that key so edits persist per pattern.
            let mut ch = proj.channels.iter()
                .find(|c| c.midi_port.as_deref() == Some(clip_key.as_str()))
                .cloned()
                .unwrap_or_else(|| {
                    let mut c = Channel::new(label.clone());
                    c.midi_port = Some(clip_key.clone());
                    c
                });
            ch.mute   = !clip.enabled;
            ch.stereo = false; // MIDI-out patterns are mono in this section

            // Use channel's explicit color palette (0=auto-cycle, 1-7=palette override).
            let color = if ch.color > 0 {
                DEST_COLORS[ch.color as usize % DEST_COLORS.len()]
            } else {
                DEST_COLORS[color_idx % DEST_COLORS.len()]
            };
            result.push(MixerEntry { label, dest: clip_key, ch, color });
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
    /// Linear gain 0.0–2.0 (1.0 = 0 dB) shown on the fader.
    pub volume:   f32,
    /// MIDI channel (0-based) this instrument plays on within its slot. For SF2
    /// slots that host several instruments, volume is controlled per channel.
    pub channel:  u8,
    /// True when the slot is an SF2 synth (per-channel volume via CC7) rather
    /// than a one-shot audio file (per-slot gain).
    pub is_sf2:   bool,
}

fn collect_audio_slot_entries_inner(proj: &Project, app: &App) -> Vec<AudioSlotEntry> {
    // One strip per audio pattern (clip cell), so each pattern has its own channel.
    let mut result: Vec<AudioSlotEntry> = Vec::new();

    for row in b'A'..=b'P' {
        let row_key = (row as char).to_string();
        let Some(slots) = proj.matrix.get(&row_key) else { continue };

        for (col, slot) in slots.iter().enumerate() {
            let Some(clip) = slot else { continue };
            let clip_key = format!("{}{}", row_key, col);
            let Some(&slot_id) = app.audio_slots.get(&clip_key) else { continue };

            let is_sf2 = matches!(clip.source, PatternSource::Sf2 { .. });
            let track = proj.track_names.get(&row_key)
                .cloned()
                .unwrap_or_else(|| row_key.clone());
            // Lead with the pattern name so the header identifies the source
            // pattern, then append a compact source detail (preset / file / plugin).
            let pat = pattern_label(clip, &track, col);
            let label = match &clip.source {
                PatternSource::Sf2 { preset_name, .. } => {
                    let preset: String = preset_name.chars().take(7).collect();
                    format!("{} {}", pat, preset)
                }
                PatternSource::AudioFile { path, .. } => {
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                    let stem: String = stem.chars().take(6).collect();
                    format!("{} {}", pat, stem)
                }
                PatternSource::Plugin { name, .. } => {
                    // Plugin instruments (e.g. LV2) install a sounding source into an
                    // audio slot, so they belong here with a per-pattern strip.
                    let n: String = name.chars().take(7).collect();
                    format!("{} {}", pat, n)
                }
                // MIDI patterns have no audio slot — they appear in the MIDI section.
                PatternSource::Midi => continue,
            };
            // SF2 slots can host many instruments on one synth → per-channel volume
            // (CC7). Audio-file slots have a dedicated slot → per-slot gain.
            let channel = clip.midi_channel.saturating_sub(1) & 0x0F;
            let volume = if is_sf2 {
                let cc7 = app.audio_slot_channel_vol.get(&(slot_id, channel)).copied().unwrap_or(100);
                cc7 as f32 / 100.0
            } else {
                app.audio_slot_volumes.get(&slot_id).copied().unwrap_or(1.0)
            };
            result.push(AudioSlotEntry {
                clip_key, slot_id, label, volume, channel, is_sf2,
            });
        }
    }
    result
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
    // When routing matrix mode is active, show the full-area routing grid.
    if app.mixer_state.routing_matrix {
        let v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(4), Constraint::Length(1)])
            .split(area);
        draw_audio_routing_matrix(f, app, v[0]);
        f.render_widget(
            Paragraph::new(Span::styled(
                "  hjkl=navigate  Enter=assign group bus  ↑↓=send  \\=exit matrix",
                Style::default().fg(BORDER),
            )),
            v[1],
        );
        return;
    }

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

    let hint = if app.focus == crate::app::FocusId::MixerFxSidebar {
        if app.mixer_state.fx_row == 0 {
            "  jk=slot  hl=type  Enter=toggle  Space=params  J/K=reorder  a=add  Del=rm  Tab=strips".to_string()
        } else {
            "  jk=param  hl=adjust  Enter=reset  Esc=slots  +/-=wet".to_string()
        }
    } else if app.mixer_state.editing {
        let param_labels = ["VOL", "EQ LO", "EQ LM", "EQ HM", "EQ HI", "PAN", "FX"];
        let lbl = param_labels.get(app.mixer_state.active_param).copied().unwrap_or("VOL");
        format!("  ↑↓=adjust [{}]  ←→=param  m=mute  s=solo  S=stereo  c=clip rst  Esc=stop", lbl)
    } else {
        "  ←→=channel  ↑↓=volume  Enter=edit  Tab=FX  G=group bus  \\=routing  m=mute  s=solo".to_string()
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(BORDER))),
        hint_area,
    );
}

/// Minimum chars wide a strip must be to be rendered.
pub const MIN_STRIP_W: u16 = 8;

// ─── Channel strips ───────────────────────────────────────────────────────────

fn draw_channel_strips(f: &mut Frame, app: &App, area: Rect) {
    let (entries, audio_entries) = {
        let proj = app.project.lock();
        let midi  = collect_mixer_entries(&proj);
        let audio = collect_audio_slot_entries_inner(&proj, app);
        (midi, audio)
    };

    // Build the flat list of all logical strips (preserves stereo L/R pairs).
    // Each element: (global_logical_index, is_audio_slot_entry_index_opt, is_stereo_side)
    // We only need the total count and per-strip widths for layout.
    let n_entry_strips: usize = entries.iter()
        .map(|e| if e.ch.stereo { 2 } else { 1 })
        .sum();
    let n_audio   = audio_entries.len();
    let total_strips = (n_entry_strips + 2 + n_audio).max(2); // +2 MASTER L/R

    // How many strips fit in the available width?
    let strips_visible = ((area.width / MIN_STRIP_W) as usize).max(1);
    let needs_scroll   = total_strips > strips_visible;

    // Reserve bottom row for scroll bar when needed; otherwise render full height.
    let (render_area, scroll_area_opt) = if needs_scroll && area.height > 2 {
        let v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        (v[0], Some(v[1]))
    } else {
        (area, None)
    };

    // Clamp scroll so selected channel is always visible.
    let scroll = {
        let sel = app.mixer_state.selected_channel;
        let old = app.mixer_state.strip_scroll;
        let clamped_max = total_strips.saturating_sub(strips_visible);
        let s = if sel < old {
            sel
        } else if sel >= old + strips_visible {
            sel + 1 - strips_visible
        } else {
            old
        };
        s.min(clamped_max)
    };
    // Store updated scroll back (Cell so we can write from &App).
    // We use a small trick: borrow app.mixer_state as a pointer write.
    // Safety: single-threaded UI loop; no concurrent access.
    #[allow(clippy::cast_ref_to_ptr)]
    unsafe {
        let state = &app.mixer_state as *const _ as *mut crate::app::MixerState;
        (*state).strip_scroll = scroll;
    }

    let visible_count = strips_visible.min(total_strips);
    let constraints: Vec<Constraint> =
        std::iter::repeat_n(Constraint::Ratio(1, visible_count as u32), visible_count).collect();

    let ch_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(render_area);

    app.mixer_strips_area.set(render_area);
    app.mixer_strip_count.set(total_strips.min(36) as u16);

    // Record strip x-start positions (relative to scroll window).
    let mut strip_xs = [0u16; 36];
    for (i, rect) in ch_chunks.iter().enumerate().take(36) {
        strip_xs[i] = rect.x;
    }
    app.mixer_strip_xs.set(strip_xs);

    // Build the flat ordered list of all logical strips with their global index.
    // global_idx is used to compare against selected_channel and scroll window.
    struct StripItem<'a> {
        global: usize,
        kind:   StripKind<'a>,
    }
    enum StripKind<'a> {
        MidiMono(&'a MixerEntry),
        MidiStereoL(&'a MixerEntry),
        MidiStereoR(&'a MixerEntry),
        MasterL,
        MasterR,
        AudioSlot(&'a AudioSlotEntry),
    }

    let n_entries = entries.len();
    let mut all_strips: Vec<StripItem> = Vec::with_capacity(total_strips);
    let mut gi = 0usize;
    // Order: pattern channels (MIDI + audio slots) first, then the MASTER L/R
    // output strips, so MASTER always sits to the right of every pattern channel.
    for entry in &entries {
        if entry.ch.stereo {
            all_strips.push(StripItem { global: gi, kind: StripKind::MidiStereoL(entry) }); gi += 1;
            all_strips.push(StripItem { global: gi, kind: StripKind::MidiStereoR(entry) }); gi += 1;
        } else {
            all_strips.push(StripItem { global: gi, kind: StripKind::MidiMono(entry) }); gi += 1;
        }
    }
    for ae in &audio_entries {
        all_strips.push(StripItem { global: gi, kind: StripKind::AudioSlot(ae) }); gi += 1;
    }
    all_strips.push(StripItem { global: gi, kind: StripKind::MasterL }); gi += 1;
    all_strips.push(StripItem { global: gi, kind: StripKind::MasterR });

    // Render only the visible window [scroll .. scroll+visible_count].
    let mut param_ys_recorded = false;
    let master_peak_l  = app.audio_master_peak[0];
    let master_peak_r  = app.audio_master_peak[1];
    let master_rms_l   = app.audio_master_rms[0];
    let master_rms_r   = app.audio_master_rms[1];
    let master_clip_l  = app.master_clip[0];
    let master_clip_r  = app.master_clip[1];
    // Master output volume as dB for the fader display (linear 0..2 → -60..+6 dB).
    let master_db: f32 = if app.master_volume <= 0.0 {
        -60.0
    } else {
        (20.0 * app.master_volume.log10()).clamp(-60.0, 6.0)
    };
    let correlation    = app.master_correlation;
    let (lufs_m, lufs_s, lufs_i) = app.master_lufs;

    let visible_strips = &all_strips[scroll..(scroll + visible_count).min(all_strips.len())];

    if visible_strips.is_empty() && entries.is_empty() && audio_entries.is_empty() {
        // Empty-state message.
        let msg_rect = if let Some(&r) = ch_chunks.first() { r } else { area };
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("  No clips in matrix.", Style::default().fg(BORDER))),
            Line::from(Span::styled("  Import a MIDI file or assign clips to see channels.", Style::default().fg(Color::DarkGray))),
        ])
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(BORDER)).style(Style::default().bg(PANEL)));
        f.render_widget(msg, msg_rect);
    }

    let _audio_sel_offset = n_entries + 2;
    for (chunk_i, item) in visible_strips.iter().enumerate() {
        let Some(&rect) = ch_chunks.get(chunk_i) else { break };
        let sel = app.mixer_state.selected_channel == item.global;
        let edit = sel && app.mixer_state.editing;
        let active_param = if sel { Some(app.mixer_state.active_param) } else { None };
        // Mouse hover: yellow highlight on the strip under the cursor. Purely visual
        // (selection follows clicks, not hover) and skipped on the already-selected
        // strip so its selection styling isn't double-layered.
        let (mx, my) = app.last_mouse_pos;
        let hov = !sel
               && mx >= rect.x && mx < rect.x + rect.width
               && my >= rect.y && my < rect.y + rect.height;

        match &item.kind {
            StripKind::MidiStereoL(e) => {
                let pys = draw_strip(f, &e.ch, rect, &format!("{} L", e.label),
                    e.color, sel, edit, app.playing, StripSide::Left, active_param, hov);
                if sel && !param_ys_recorded { app.mixer_param_ys.set(pys); param_ys_recorded = true; }
            }
            StripKind::MidiStereoR(e) => {
                draw_strip(f, &e.ch, rect, &format!("{} R", e.label),
                    e.color, sel, edit, app.playing, StripSide::Right, active_param, hov);
            }
            StripKind::MidiMono(e) => {
                let pys = draw_strip(f, &e.ch, rect, &e.label,
                    e.color, sel, edit, app.playing, StripSide::Mono, active_param, hov);
                if sel && !param_ys_recorded { app.mixer_param_ys.set(pys); param_ys_recorded = true; }
            }
            StripKind::MasterL => {
                let mut ch = Channel::new("MASTER L");
                ch.volume = master_db;
                let pys = draw_strip(f, &ch, rect, "MASTER L",
                    Color::Rgb(200, 200, 200), sel, false, app.playing, StripSide::Left, None, hov);
                draw_rms_overlay(f, rect, master_rms_l);
                draw_vu_overlay(f, rect, master_peak_l);
                if master_clip_l { draw_clip_overlay(f, rect); }
                if sel && !param_ys_recorded { app.mixer_param_ys.set(pys); param_ys_recorded = true; }
            }
            StripKind::MasterR => {
                let mut ch = Channel::new("MASTER R");
                ch.volume = master_db;
                draw_strip(f, &ch, rect, "MASTER R",
                    Color::Rgb(200, 200, 200), sel, false, app.playing, StripSide::Right, None, hov);
                draw_rms_overlay(f, rect, master_rms_r);
                draw_vu_overlay(f, rect, master_peak_r);
                if master_clip_r { draw_clip_overlay(f, rect); }
                draw_lufs_correlation_overlay(f, rect, lufs_m, lufs_s, lufs_i, correlation);
                draw_spectrum_overlay(f, rect, &app.master_spectrum);
            }
            StripKind::AudioSlot(ae) => {
                let peak    = app.audio_slot_peaks.get(ae.slot_id as usize).copied().unwrap_or(0.0);
                let rms     = app.audio_slot_rms.get(ae.slot_id as usize).copied().unwrap_or(0.0);
                let clipped = app.audio_slot_clip.get(ae.slot_id as usize).copied().unwrap_or(false);
                draw_audio_slot_strip(f, ae, rect, sel, hov, peak, rms, clipped);
            }
        }
    }

    // Scroll indicator bar.
    if let Some(sbar_rect) = scroll_area_opt {
        let sel = app.mixer_state.selected_channel;
        let indicator = format!(
            "  ◄ {}/{} ►  ch {}/{}",
            scroll + 1,
            (total_strips.saturating_sub(strips_visible) + 1).max(1),
            sel + 1,
            total_strips,
        );
        let left_arrow  = if scroll > 0 { "◄" } else { " " };
        let right_arrow = if scroll + strips_visible < total_strips { "►" } else { " " };
        let bar = format!("  {} ─── {}/{} ch:{}/{} ──── {} ",
            left_arrow, scroll + 1,
            total_strips.saturating_sub(strips_visible - 1).max(1),
            sel + 1, total_strips, right_arrow);
        let _ = indicator;
        f.render_widget(
            Paragraph::new(Span::styled(bar, Style::default().fg(BORDER))),
            sbar_rect,
        );
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
    hovered: bool,
) -> [u16; 10] {
    let mut param_ys = [0u16; 10];

    // Border colour priority: editing/selected → SELECTED (cyan), hover → yellow,
    // otherwise the channel's own group colour.
    let border_style = if editing {
        Style::default().fg(SELECTED).add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(SELECTED)
    } else if hovered {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(group_color)
    };

    let borders = match side {
        StripSide::Mono  => Borders::ALL,
        StripSide::Left  => Borders::LEFT | Borders::TOP | Borders::BOTTOM,
        StripSide::Right => Borders::RIGHT | Borders::TOP | Borders::BOTTOM,
    };

    let title_style = Style::default().fg(group_color).add_modifier(Modifier::BOLD);
    // Channel type short suffix + optional group bus destination appended to title.
    let type_badge: String = if ch.is_drum {
        " DR".to_string()
    } else {
        let base = match ch.channel_type {
            seqterm_core::ChannelType::Audio      => " AU",
            seqterm_core::ChannelType::Instrument => " IN",
            seqterm_core::ChannelType::GroupBus   => " GR",
            seqterm_core::ChannelType::Return     => " RE",
            seqterm_core::ChannelType::Master     => " MA",
        };
        if ch.group_bus > 0 {
            format!("{}→G{}", base, ch.group_bus)
        } else {
            base.to_string()
        }
    };

    let block = Block::default()
        .title(format!(" {}{} ", label, type_badge))
        .title_style(if selected || editing {
            Style::default().fg(SELECTED).add_modifier(Modifier::BOLD)
        } else if hovered {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            title_style
        })
        .borders(borders)
        .border_style(border_style)
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

    // ── Mute/Solo/Flags row ────────────────────────────────────────────────────
    // Compact: M=mute  S=solo  ⊘=phase  ◉=mono  ●=rec
    let mute_part = if ch.mute {
        Span::styled("M", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("·", Style::default().fg(BORDER))
    };
    let solo_part = if ch.solo {
        Span::styled("S", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("·", Style::default().fg(BORDER))
    };
    let phase_part = if ch.phase_invert {
        Span::styled("⊘", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("·", Style::default().fg(BORDER))
    };
    let mono_part = if ch.mono {
        Span::styled("◉", Style::default().fg(Color::Rgb(100, 220, 220)).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("·", Style::default().fg(BORDER))
    };
    let rec_part = if ch.record_arm {
        Span::styled("●", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("·", Style::default().fg(BORDER))
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![mute_part, solo_part, phase_part, mono_part, rec_part])),
        vert[0],
    );

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

    // ── FX + routing row ──────────────────────────────────────────────────────
    if vert.len() > 8 {
        let style = param_highlight(selected, active_param, 6, Color::Rgb(200, 160, 255));
        let bar = fx_bar(ch.fx_amount, (w.saturating_sub(7)).max(3));
        // Show routing destination when there is enough width.
        let route_label = if ch.group_bus > 0 {
            format!("→G{}", ch.group_bus)
        } else {
            "→MST".to_string()
        };
        let text = if w >= 12 {
            format!("FX{} {}", bar, route_label)
        } else {
            format!("FX {} {:>3}", bar, ch.fx_amount)
        };
        let route_col = if ch.group_bus > 0 {
            Color::Rgb(80, 200, 240)
        } else {
            Color::Rgb(140, 140, 140)
        };
        if w >= 12 && ch.group_bus > 0 {
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!("FX{}", bar), style),
                    Span::styled(format!(" {}", route_label), Style::default().fg(route_col)),
                ])),
                vert[8],
            );
        } else {
            f.render_widget(
                Paragraph::new(Span::styled(text, style)),
                vert[8],
            );
        }
    }

    param_ys
}

/// Render a simplified strip for an audio engine slot (volume + VU meter + RMS bar).
fn draw_audio_slot_strip(f: &mut Frame, entry: &AudioSlotEntry, area: Rect, selected: bool, hovered: bool, peak: f32, rms: f32, clipped: bool) {
    const AUDIO_COLOR: Color = Color::Rgb(140, 200, 255);

    let border_style = if selected {
        Style::default().fg(SELECTED)
    } else if hovered {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(BORDER)
    };
    let block = Block::default()
        .title(format!(" {} ", entry.label))
        .title_style(if selected {
            Style::default().fg(SELECTED).add_modifier(Modifier::BOLD)
        } else if hovered {
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
    let vol_pct = (entry.volume * 50.0).clamp(0.0, 100.0) as usize;

    let show_rms = inner.height >= 4;
    let constraints: Vec<Constraint> = if show_rms {
        vec![Constraint::Length(1), Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]
    } else {
        vec![Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]
    };
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // dB label + clip indicator.
    let db = 20.0_f32 * entry.volume.max(1e-6).log10();
    let headroom_db = if peak > 1e-6 { -20.0_f32 * peak.log10() } else { 99.0 };
    let (label_text, label_style) = if clipped {
        (
            format!("CLIP{:>+4.0}", db),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else if headroom_db < 6.0 && peak > 0.001 {
        (
            format!("HR{:>+5.1}", -headroom_db),
            Style::default().fg(Color::Rgb(240, 180, 60)),
        )
    } else {
        (
            format!("{:>+5.1}dB", db),
            if selected { Style::default().fg(SELECTED) } else { Style::default().fg(Color::White) },
        )
    };
    f.render_widget(
        Paragraph::new(Span::styled(label_text, label_style)),
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

    // Peak VU meter row.
    let (bar_str, peak_color) = vu_bar(peak, w);
    f.render_widget(
        Paragraph::new(Span::styled(bar_str, Style::default().fg(peak_color))),
        vert[2],
    );

    // RMS bar (teal, shown only when strip is tall enough).
    if show_rms {
        let rms_fill = ((rms * w as f32).round() as usize).min(w);
        let rms_bar: String = (0..w).map(|i| if i < rms_fill { '▬' } else { '▁' }).collect();
        f.render_widget(
            Paragraph::new(Span::styled(rms_bar, Style::default().fg(Color::Rgb(60, 180, 210)))),
            vert[3],
        );
    }
}

/// Render a 1-row VU bar at the bottom of a strip rect (inside the border).
fn draw_vu_overlay(f: &mut Frame, strip_rect: Rect, peak: f32) {
    if strip_rect.height < 3 || strip_rect.width < 4 { return; }
    let vu_rect = Rect {
        x:      strip_rect.x + 1,
        y:      strip_rect.y + strip_rect.height - 2,
        width:  strip_rect.width.saturating_sub(2),
        height: 1,
    };
    let w = vu_rect.width as usize;
    let (bar_str, bar_color) = vu_bar(peak, w);
    f.render_widget(
        Paragraph::new(Span::styled(bar_str, Style::default().fg(bar_color).bg(PANEL))),
        vu_rect,
    );
}

/// Render a 1-row RMS bar two rows above the border bottom (teal, above the peak VU overlay).
fn draw_rms_overlay(f: &mut Frame, strip_rect: Rect, rms: f32) {
    if strip_rect.height < 4 || strip_rect.width < 4 { return; }
    let rms_rect = Rect {
        x:      strip_rect.x + 1,
        y:      strip_rect.y + strip_rect.height - 3,
        width:  strip_rect.width.saturating_sub(2),
        height: 1,
    };
    let w = rms_rect.width as usize;
    let fill = ((rms * w as f32).round() as usize).min(w);
    let bar: String = (0..w).map(|i| if i < fill { '▬' } else { '▁' }).collect();
    f.render_widget(
        Paragraph::new(Span::styled(bar, Style::default().fg(Color::Rgb(60, 180, 210)).bg(PANEL))),
        rms_rect,
    );
}

/// Render a 1-cell CLIP indicator in the top-left inside the border.
fn draw_clip_overlay(f: &mut Frame, strip_rect: Rect) {
    if strip_rect.height < 2 || strip_rect.width < 5 { return; }
    let clip_rect = Rect {
        x:      strip_rect.x + 1,
        y:      strip_rect.y + 1,
        width:  4,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Span::styled("CLIP", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD).bg(PANEL))),
        clip_rect,
    );
}

/// Draw a spectrum analyzer bar graph overlay at the top of a strip.
/// Uses Unicode block characters to render amplitude bars per frequency band.
fn draw_spectrum_overlay(f: &mut Frame, strip_rect: Rect, bands: &[f32]) {
    if strip_rect.height < 6 || strip_rect.width < 4 || bands.is_empty() { return; }

    // Draw a 3-row mini spectrum in the strip's inner area, just below the title.
    const BARS: usize = 8; // downsample bands to 8 columns to fit narrow strips
    const HEIGHT: usize = 3;
    const BLOCK: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    let w = strip_rect.width.saturating_sub(2) as usize;
    let n_bars = BARS.min(w);
    if n_bars == 0 { return; }

    // Downsample bands to n_bars.
    let step = bands.len() / n_bars;
    let bar_vals: Vec<f32> = (0..n_bars)
        .map(|b| {
            let lo = b * step;
            let hi = ((b + 1) * step).min(bands.len());
            if hi > lo {
                bands[lo..hi].iter().fold(0.0f32, |m, &v| m.max(v))
            } else { 0.0 }
        })
        .collect();

    let max_val = bar_vals.iter().cloned().fold(0.0f32, f32::max).max(1e-6);

    for row in 0..HEIGHT {
        let y = strip_rect.y + 1 + row as u16;
        if y >= strip_rect.y + strip_rect.height { break; }
        let mut spans: Vec<Span> = Vec::new();
        for &v in &bar_vals {
            // Normalize and scale to HEIGHT*8 eighths.
            let norm = (v / max_val).clamp(0.0, 1.0);
            let total_eighths = (norm * HEIGHT as f32 * 8.0) as usize;
            let row_bot = HEIGHT - 1 - row;
            let char_idx = {
                let filled_rows = total_eighths / 8;
                let partial = total_eighths % 8;
                if row_bot < filled_rows { 8 }
                else if row_bot == filled_rows && partial > 0 { partial }
                else { 0 }
            };
            let intensity = v / max_val;
            let color = if intensity > 0.8 {
                Color::Red
            } else if intensity > 0.5 {
                Color::Yellow
            } else {
                Color::Rgb(40, 160, 80)
            };
            let ch = BLOCK[char_idx.min(8)].to_string();
            spans.push(Span::styled(ch, Style::default().fg(color).bg(PANEL)));
        }
        let r = Rect { x: strip_rect.x + 1, y, width: n_bars as u16, height: 1 };
        f.render_widget(Paragraph::new(Line::from(spans)), r);
    }
}

/// Draw LUFS + correlation overlay on the MASTER R strip (bottom area).
fn draw_lufs_correlation_overlay(
    f: &mut Frame,
    strip_rect: Rect,
    lufs_m: f32, lufs_s: f32, lufs_i: f32,
    corr: f32,
) {
    if strip_rect.height < 6 || strip_rect.width < 6 { return; }

    let w = strip_rect.width.saturating_sub(2) as usize;

    // Place the LUFS lines near the bottom of the strip.
    let base_y = strip_rect.y + strip_rect.height.saturating_sub(5);

    let lufs_label = |v: f32| -> String {
        if v.is_finite() { format!("{:>5.1}", v) } else { " -inf".to_string() }
    };

    let corr_bar = {
        // Render -1..+1 as a 7-char bar "  ←  " centered
        let filled = ((corr + 1.0) * 3.0).clamp(0.0, 6.0).round() as usize;
        let mut b = vec!['─'; 6];
        if filled < 6 { b[filled] = '▸'; }
        b.iter().collect::<String>()
    };

    let lines = [
        format!("M{}", lufs_label(lufs_m)),
        format!("S{}", lufs_label(lufs_s)),
        format!("I{}", lufs_label(lufs_i)),
        format!("φ{:>+5.2}", corr),
    ];

    for (i, line) in lines.iter().enumerate() {
        let r = Rect {
            x: strip_rect.x + 1,
            y: base_y + i as u16,
            width: (w.min(line.len() + 1)) as u16,
            height: 1,
        };
        if r.y >= strip_rect.y + strip_rect.height { break; }
        let color = if i == 3 {
            // Correlation: green = in phase, red = out of phase.
            if corr > 0.5 { Color::Green } else if corr < -0.1 { Color::Red } else { Color::Yellow }
        } else {
            Color::Rgb(100, 200, 200)
        };
        f.render_widget(
            Paragraph::new(Span::styled(line.clone(), Style::default().fg(color).bg(PANEL))),
            r,
        );
    }
    let _ = corr_bar; // used for design purposes
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

/// VU meter bar: `peak` is 0.0–1.0+. Returns (bar_string, color).
fn vu_bar(peak: f32, width: usize) -> (String, Color) {
    let w = width.max(1);
    let fill = ((peak * w as f32).round() as usize).min(w);
    let bar: String = (0..w).map(|i| if i < fill { '█' } else { '▁' }).collect();
    let color = if peak >= 1.0 {
        Color::Rgb(255, 60, 60)
    } else if peak >= 0.7 {
        Color::Rgb(240, 200, 60)
    } else {
        Color::Rgb(56, 200, 100)
    };
    (bar, color)
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
    let focused  = app.focus == crate::app::FocusId::MixerFxSidebar;
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

    // Add / Move toolbar (clickable; also a=add, ,/.=move from the keyboard).
    lines.push(Line::from(vec![
        Span::styled(" [+ Add] ", Style::default().fg(Color::Black).bg(OK).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(" [▲ Up] ", Style::default().fg(Color::Black).bg(ACCENT)),
        Span::raw(" "),
        Span::styled(" [▼ Dn] ", Style::default().fg(Color::Black).bg(ACCENT)),
    ]));
    // Cache button rects (inner-relative → absolute) for mouse hit-testing.
    {
        let y = inner.y; // first content row
        app.mixer_fx_add_rect.set(Rect::new(inner.x, y, 8, 1));
        app.mixer_fx_up_rect.set(Rect::new(inner.x + 9, y, 8, 1));
        app.mixer_fx_dn_rect.set(Rect::new(inner.x + 18, y, 8, 1));
    }

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
    draw_fx_chain_sidebar(f, app, area, chain, &format!(" FX :: slot {} ", slot_id), focused, sel_slot, slot_id);
}

fn draw_master_fx_sidebar(f: &mut Frame, app: &App, area: Rect, focused: bool, sel_slot: usize) {
    draw_fx_chain_sidebar(f, app, area, &app.master_fx, " FX :: MASTER BUS ", focused, sel_slot,
        crate::app::MASTER_FX_METER_KEY);
}

/// Render an `AudioFxEntry` chain (audio-slot or master bus) in the mixer
/// sidebar using the SAME widgets/styles as the PATTERN/FX panel
/// (`fx_button_box`, knob arcs) and publish clickable rects so mouse and
/// keyboard edit identically. Layout is stacked vertically to fit the narrow
/// 28-col sidebar (PATTERN/FX is a wide bottom panel). ponytail: reuses the
/// tracker widgets rather than a second box style.
fn draw_fx_chain_sidebar(
    f: &mut Frame,
    app: &App,
    area: Rect,
    chain: &[crate::app::AudioFxEntry],
    title: &str,
    focused: bool,
    sel_slot: usize,
    meter_key: u32,
) {
    use crate::app::fx_param_descs;
    use crate::views::tracker::{fx_button_box, knob_arc, knob_indicator};

    let fx_row = app.mixer_state.fx_row;

    // Reset every-frame mouse rects; set below only where a control is drawn.
    app.mixer_fx_slot_rects.set([Rect::default(); 8]);
    app.mixer_fx_param_rects.set([Rect::default(); crate::app::FX_MAX_PARAMS]);
    app.mixer_fx_add_rect.set(Rect::default());
    app.mixer_fx_enable_rect.set(Rect::default());
    app.mixer_fx_delete_rect.set(Rect::default());
    app.mixer_fx_move_prev_rect.set(Rect::default());
    app.mixer_fx_move_next_rect.set(Rect::default());
    app.mixer_fx_cat_prev_rect.set(Rect::default());
    app.mixer_fx_cat_next_rect.set(Rect::default());
    app.mixer_fx_preset_prev_rect.set(Rect::default());
    app.mixer_fx_preset_next_rect.set(Rect::default());

    let outer = Block::default()
        .title(title.to_string())
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { ACCENT } else { BORDER }))
        .style(Style::default().bg(BG));
    let inner = outer.inner(area);
    f.render_widget(outer, area);
    if inner.width == 0 || inner.height == 0 { return; }

    let cx = inner.x;
    let max_x = inner.x + inner.width;
    let max_y = inner.y + inner.height;
    let bg = Style::default().bg(BG);
    let put = |f: &mut Frame, line: Line, y: u16| {
        if y < max_y {
            f.render_widget(Paragraph::new(line).style(bg), Rect::new(inner.x, y, inner.width, 1));
        }
    };
    let mut y = inner.y;

    // ── Effect tabs: one clickable line per FX, then "+ Add FX". ──────────────
    let mut slot_rects = [Rect::default(); 8];
    for (i, entry) in chain.iter().enumerate().take(8) {
        let is_sel = focused && i == sel_slot;
        let dot = if entry.enabled { "●" } else { "○" };
        let st = if is_sel {
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else if entry.enabled {
            Style::default().fg(OK)
        } else {
            Style::default().fg(BORDER).add_modifier(Modifier::CROSSED_OUT)
        };
        put(f, Line::from(Span::styled(
            format!(" {} {}:{} ", dot, i + 1, entry.kind.label()), st)), y);
        slot_rects[i] = Rect::new(cx, y, inner.width, 1);
        y += 1;
    }
    app.mixer_fx_slot_rects.set(slot_rects);
    if chain.len() < 8 {
        put(f, Line::from(Span::styled(" + Add FX ",
            Style::default().fg(Color::Black).bg(OK).add_modifier(Modifier::BOLD))), y);
        app.mixer_fx_add_rect.set(Rect::new(cx, y, inner.width, 1));
        y += 1;
    }

    // ── Routing: IN → 1 → … → OUT ─────────────────────────────────────────────
    let dim = Style::default().fg(Color::Rgb(120, 130, 150));
    let mut rt: Vec<Span> = vec![Span::styled(" IN", dim)];
    for (i, e) in chain.iter().enumerate() {
        rt.push(Span::styled("→", dim));
        rt.push(Span::styled(format!("{}", i + 1),
            Style::default().fg(if e.enabled { OK } else { BORDER })));
    }
    rt.push(Span::styled("→OUT", dim));
    put(f, Line::from(rt), y);
    y += 1;
    put(f, Line::from(Span::styled("─".repeat(inner.width as usize), Style::default().fg(Color::Rgb(40, 46, 54)))), y);
    y += 1;

    // ── Selected effect: clickable param knobs (one per line). ────────────────
    if let Some(entry) = chain.get(sel_slot) {
        put(f, Line::from(vec![
            Span::styled(format!(" {} {}", if entry.enabled { "●" } else { "○" }, entry.kind.label()),
                Style::default().fg(if entry.enabled { OK } else { BORDER }).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {:>3}% wet", (entry.wet * 100.0) as u8),
                Style::default().fg(Color::Rgb(150, 160, 180))),
        ]), y);
        y += 1;

        let descs = fx_param_descs(entry.kind);
        // Category + preset comboboxes (grouped effects like Z5Texture) so the
        // narrow sidebar shows ~8 knobs at a time — MIDI-surface friendly.
        let cats    = crate::app::fx_param_categories(entry.kind);
        let presets = crate::app::fx_presets(entry.kind);
        let (lo, hi) = if cats.is_empty() {
            (0usize, descs.len())
        } else {
            let c = cats[app.mixer_state.fx_category.min(cats.len() - 1)];
            (c.start.min(descs.len()), (c.start + c.len).min(descs.len()))
        };
        if !cats.is_empty() && y < max_y {
            let ci = app.mixer_state.fx_category.min(cats.len() - 1);
            let cb = format!(" ◀ {} ({}/{}) ▶", cats[ci].name, ci + 1, cats.len());
            put(f, Line::from(Span::styled(cb,
                Style::default().fg(HEADER).add_modifier(Modifier::BOLD))), y);
            app.mixer_fx_cat_prev_rect.set(Rect::new(cx + 1, y, 2, 1));
            app.mixer_fx_cat_next_rect.set(Rect::new(max_x.saturating_sub(2), y, 2, 1));
            y += 1;
        }
        if !presets.is_empty() && y < max_y {
            let pi = app.mixer_state.fx_preset.min(presets.len() - 1);
            let pb = format!(" PRESET ◀ {} ▶", presets[pi].0);
            put(f, Line::from(Span::styled(pb,
                Style::default().fg(Color::Rgb(150, 195, 245)).add_modifier(Modifier::BOLD))), y);
            app.mixer_fx_preset_prev_rect.set(Rect::new(cx + 8, y, 2, 1));
            app.mixer_fx_preset_next_rect.set(Rect::new(max_x.saturating_sub(2), y, 2, 1));
            y += 1;
        }
        let mut param_rects = [Rect::default(); crate::app::FX_MAX_PARAMS];
        for pi in lo..hi {
            if y >= max_y { break; }
            let desc = &descs[pi];
            let row_focused = focused && fx_row == pi + 1;
            let val = entry.params.get(pi).copied().unwrap_or(desc.default);
            let short: String = desc.name.chars().take(6).collect();
            let (lbl_st, val_st) = if row_focused {
                (Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD),
                 Style::default().fg(Color::Yellow))
            } else {
                (Style::default().fg(Color::Rgb(120, 140, 180)),
                 Style::default().fg(Color::Rgb(180, 180, 200)))
            };
            put(f, Line::from(vec![
                Span::styled(format!(" {}{:<6} ", knob_indicator(val), short), lbl_st),
                Span::styled(format!("[{}]", knob_arc(val, 8)), Style::default().fg(if row_focused { Color::Yellow } else { Color::Rgb(100,160,220) })),
                Span::styled(format!(" {:>3}%", (val * 100.0).round() as u32), val_st),
            ]), y);
            param_rects[pi] = Rect::new(cx, y, inner.width, 1);
            y += 1;
        }
        app.mixer_fx_param_rects.set(param_rects);

        // Live buffer scope (Z5 Texture) — real waveform via the shared meter.
        if entry.kind == crate::app::AudioFxKind::Z5Texture && y < max_y {
            let meter = app.z5_meters.get(&meter_key)
                .and_then(|v| v.iter().find(|(i, _)| *i == sel_slot).map(|(_, m)| m));
            crate::views::tracker::draw_z5_buffer_viz(f, app, &entry.params, meter, cx + 1, y, inner.width.saturating_sub(2));
            y += 1;
        }

        // ── Control boxes (PATTERN/FX style): ON/OFF · DEL, then MOVE◀ · MOVE▶.
        y += 1;
        let (en_lbl, en_border, en_face) = if entry.enabled {
            ("● ON", Color::Rgb(56, 200, 100),
             Style::default().fg(Color::Black).bg(Color::Rgb(56, 200, 100)).add_modifier(Modifier::BOLD))
        } else {
            ("○ OFF", Color::Rgb(90, 95, 105),
             Style::default().fg(Color::Rgb(180, 185, 195)).bg(PANEL))
        };
        let w1 = fx_button_box(f, cx, y, max_x, max_y, en_lbl, en_border, en_face);
        if w1 > 0 { app.mixer_fx_enable_rect.set(Rect::new(cx, y, w1, 3)); }
        let bx = cx + w1 + 1;
        let w2 = fx_button_box(f, bx, y, max_x, max_y, "✖ DEL", Color::Rgb(200, 70, 70),
            Style::default().fg(Color::White).bg(Color::Rgb(170, 50, 50)).add_modifier(Modifier::BOLD));
        if w2 > 0 { app.mixer_fx_delete_rect.set(Rect::new(bx, y, w2, 3)); }
        y += 3;

        let mv_col = |on: bool| if on { Color::Rgb(100, 160, 220) } else { Color::Rgb(80, 85, 95) };
        let mv_face = |on: bool| if on {
            Style::default().fg(Color::Rgb(150, 195, 245)).bg(PANEL)
        } else {
            Style::default().fg(Color::Rgb(90, 95, 105)).bg(PANEL)
        };
        let can_prev = sel_slot > 0;
        let can_next = sel_slot + 1 < chain.len();
        let w3 = fx_button_box(f, cx, y, max_x, max_y, "◀ MOVE", mv_col(can_prev), mv_face(can_prev));
        if w3 > 0 { app.mixer_fx_move_prev_rect.set(Rect::new(cx, y, w3, 3)); }
        let bx = cx + w3 + 1;
        let w4 = fx_button_box(f, bx, y, max_x, max_y, "MOVE ▶", mv_col(can_next), mv_face(can_next));
        if w4 > 0 { app.mixer_fx_move_next_rect.set(Rect::new(bx, y, w4, 3)); }
        y += 3;
    } else if chain.is_empty() {
        put(f, Line::from(Span::styled("  No FX — click [+ Add FX] (or press a)",
            Style::default().fg(Color::DarkGray))), y);
        y += 1;
    }

    // ── Hint ──────────────────────────────────────────────────────────────────
    let hint = if !focused {
        "  Tab=focus fx · click boxes"
    } else if fx_row > 0 {
        "  jk=param hl=val Esc=back · click"
    } else {
        "  jk=sel hl=type a=add J/K · click"
    };
    put(f, Line::from(Span::styled(hint, Style::default().fg(BORDER))), y);
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

// ─── Audio Routing Matrix ─────────────────────────────────────────────────────

const COL_LABELS: &[&str] = &[
    "MSTR", "GRP1", "GRP2", "GRP3", "GRP4", "GRP5", "GRP6", "GRP7", "GRP8", "S.A", "S.B",
];

pub fn draw_audio_routing_matrix(f: &mut Frame, app: &App, area: Rect) {
    const HEADER: Color = Color::Rgb(240, 136, 62);
    const ACCENT: Color = Color::Rgb(31, 111, 235);
    const OK:     Color = Color::Rgb(56, 200, 100);
    const N_COLS: usize = 11;

    let outer = Block::default()
        .title(" AUDIO ROUTING MATRIX  (G=group  \\ =exit) ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    if inner.height < 3 || inner.width < 30 {
        return;
    }

    let proj = app.project.lock();
    let channels = &proj.channels;

    // Column width: distribute remaining width evenly across 11 columns.
    let label_w: u16 = 14;
    let avail = inner.width.saturating_sub(label_w);
    let col_w = ((avail / N_COLS as u16).max(4)).min(7);

    // ── Header row ────────────────────────────────────────────────────────────
    let mut header_spans = vec![Span::styled(
        format!("{:width$}", "CHANNEL", width = label_w as usize),
        Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
    )];
    for (ci, &lbl) in COL_LABELS.iter().enumerate() {
        let is_cur_col = ci == app.mixer_state.routing_col;
        header_spans.push(Span::styled(
            format!("{:^width$}", lbl, width = col_w as usize),
            if is_cur_col {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ));
    }

    let mut lines: Vec<Line> = vec![Line::from(header_spans)];

    // ── Channel rows ──────────────────────────────────────────────────────────
    let visible_h = (inner.height as usize).saturating_sub(1);
    for (row_i, ch) in channels.iter().enumerate().take(visible_h) {
        let is_sel_row = row_i == app.mixer_state.routing_row;
        let row_key = ch.midi_port.as_deref().unwrap_or("?");
        let name: String = row_key.chars().take(2).collect::<String>()
            + " "
            + &ch.name.chars().take((label_w as usize).saturating_sub(3)).collect::<String>();
        let row_style = if is_sel_row {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let mut spans = vec![Span::styled(
            format!("{:width$}", name, width = label_w as usize),
            row_style,
        )];

        for ci in 0..N_COLS {
            let is_sel_cell = is_sel_row && ci == app.mixer_state.routing_col;
            let cell_str = if ci <= 8 {
                // Group bus routing columns (radio-button style).
                let active = (ch.group_bus as usize) == ci;
                let sym = if active { "●" } else { "○" };
                format!("{:^width$}", sym, width = col_w as usize)
            } else {
                // Send level columns (SendA=9, SendB=10).
                let val = if ci == 9 { ch.send_a } else { ch.send_b };
                format!("{:^width$}", val, width = col_w as usize)
            };
            let cell_style = if is_sel_cell {
                Style::default().fg(Color::Black).bg(ACCENT)
            } else if ci <= 8 && (ch.group_bus as usize) == ci {
                Style::default().fg(OK)
            } else if ci <= 8 {
                Style::default().fg(BORDER)
            } else {
                Style::default().fg(Color::Cyan)
            };
            spans.push(Span::styled(cell_str, cell_style));
        }
        lines.push(Line::from(spans));
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(PANEL)),
        inner,
    );
}
