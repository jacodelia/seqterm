//! Audio Source Editor view.
//!
//! Layout (vertical):
//!   ┌─ WAVEFORM ──────────────────────────────────┐  8 lines
//!   │ TRANSPORT: [▶/⏸] [■] [⏪] [●REC]             │  1 line
//!   │ section grid: [SAMPLE][AMPLITUDE][FREQUENCY] │  3 lines
//!   │               [ENVELOPE][FILTER][LAYERS] ... │
//!   ├─ active param panel ─────────────────────────┤  fill
//!   └─ pattern bar ───────────────────────────────┘  2 lines
//!      hint                                            1 line
//!
//! Mouse:
//!   • Click transport btn → play/pause/stop/rwd/rec
//!   • Click section cell  → switch active section
//!   • Click waveform      → move playhead / start selection
//!   • Scroll wheel on waveform → zoom in/out
//!   • Click param row     → focus row
//!   • Scroll wheel on param row → adjust value
//!   • Click pattern button → toggle that pattern row as live source

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use seqterm_core::{GrainDirection, GrainEnvelope, MOD_SLOTS};

use crate::app::{App, EditorTab};

const BG:     Color = Color::Rgb(13, 17, 23);
const PANEL:  Color = Color::Rgb(18, 24, 32);
const BORDER: Color = Color::Rgb(48, 54, 61);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const OK:     Color = Color::Rgb(56, 200, 100);
const WARM:   Color = Color::Rgb(240, 136, 62);
const DIM:    Color = Color::Rgb(60, 70, 85);
const SEL:    Color = Color::Rgb(70, 40, 100);
const LIVE:   Color = Color::Rgb(200, 60, 200);

pub fn draw_granular(f: &mut Frame, app: &App, area: Rect) {
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // waveform strip
            Constraint::Length(3),  // transport bar (boxed buttons)
            Constraint::Length(3),  // section selector grid (3 rows)
            Constraint::Min(6),     // active param panel
            Constraint::Length(2),  // pattern bar
            Constraint::Length(1),  // hint
        ])
        .split(area);

    draw_waveform(f, app, vchunks[0]);
    draw_transport_bar(f, app, vchunks[1]);
    draw_section_grid(f, app, vchunks[2]);
    draw_param_panel(f, app, vchunks[3]);
    draw_pattern_bar(f, app, vchunks[4]);

    let hint = match app.editor_state.tab {
        EditorTab::Sample | EditorTab::Envelope | EditorTab::Filter => {
            "  ↑↓=param ←→=adjust Tab=section +/-=zoom Ctrl+A=sel n=norm i/o=fade  Space=play s=stop"
        }
        EditorTab::Amplitude | EditorTab::Frequency | EditorTab::Layers => {
            "  ↑↓/click=param  ←→/wheel=adjust  Tab=section  Space=play  s=stop  R=rec capture"
        }
        EditorTab::Granular => {
            "  ↑↓/click=param  ←→/wheel=adjust  Tab=section  f=freeze  F=unfreeze  W=save scene  click pattern=live src"
        }
        EditorTab::Mod => {
            "  ↑↓/click=param  ←→/wheel=adjust  Tab=section  V=live src  L=capture  click pattern=route audio"
        }
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(DIM))),
        vchunks[5],
    );
}

// ─── Waveform strip ───────────────────────────────────────────────────────────

fn draw_waveform(f: &mut Frame, app: &App, area: Rect) {
    let es = &app.editor_state;
    let gs = &app.granular_state;

    let block = Block::default()
        .title(" WAVEFORM ")
        .title_style(Style::default().fg(WARM).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Store waveform rect for mouse hit-testing.
    app.editor_waveform_rect.set(inner);

    if inner.width == 0 || inner.height == 0 { return; }

    let w = inner.width as usize;
    let h = inner.height as usize;

    // SF2 editor: derive peaks from the selected zone's PCM. Otherwise use the
    // pad's cached waveform.
    let waveform: Option<Vec<f32>> = if let Some(pcm) = es.sf2.as_ref().and_then(|s| s.zone_wave()) {
        Some(peaks(pcm, w))
    } else {
        es.pad.and_then(|(bank, pad)| {
            let proj = app.project.lock();
            let path = proj.sampler.banks.get(bank)
                .and_then(|b| b.slots[pad].as_ref())
                .map(|s| s.path.clone())?;
            drop(proj);
            app.waveform_cache.get(&path).cloned()
        })
    }
    // Last resort (LV2/VST synths and other slots with no static PCM): derive
    // peaks from the live oscilloscope capture of the edited slot.
    .or_else(|| {
        if app.live_waveform.is_empty() { None } else { Some(peaks(&app.live_waveform, w)) }
    });

    let zoom   = es.zoom_x.max(1.0);
    let scroll = es.scroll_x.clamp(0.0, (1.0 - 1.0 / zoom).max(0.0));

    let sel_cols: Option<(usize, usize)> = es.selection.map(|(sl, sr)| {
        let l = (((sl - scroll) * zoom * w as f32) as isize).clamp(0, w as isize) as usize;
        let r = (((sr - scroll) * zoom * w as f32) as isize).clamp(0, w as isize) as usize;
        (l, r.max(l + 1).min(w))
    });

    let sp_col = (((es.sample.start - scroll) * zoom * w as f32) as isize).clamp(-1, w as isize);
    let ep_col = (((es.sample.end   - scroll) * zoom * w as f32) as isize).clamp(-1, w as isize);
    let gpos_col = (((gs.zone.position - scroll) * zoom * w as f32) as isize).clamp(-1, w as isize);

    let mut lines: Vec<Line> = Vec::with_capacity(h);

    for row in 0..h {
        let mut chars: Vec<char>  = vec![' '; w];
        let mut fg_col: Vec<Color> = vec![DIM; w];
        let mut bg_col: Vec<Color> = vec![PANEL; w];

        if let Some(ref peaks) = waveform {
            let n = peaks.len();
            for col in 0..w {
                let clip_frac = scroll + (col as f32 / w as f32) / zoom;
                let peak_idx = (clip_frac * n as f32) as usize;
                let amp = peaks.get(peak_idx).copied().unwrap_or(0.0);
                let bar_h = (amp * h as f32) as usize;
                let mid = h / 2;
                let half_bar = (bar_h / 2).max(1);
                if row >= mid.saturating_sub(half_bar) && row <= mid + half_bar {
                    chars[col] = match (amp * 4.0) as usize {
                        0 => '░', 1 => '▒', 2 => '▓', _ => '█',
                    };
                }
            }
        } else if row == h / 2 {
            let msg = "  no source — open from Matrix (g on pad)  ";
            let start = w.saturating_sub(msg.len()) / 2;
            for (i, c) in msg.chars().enumerate() {
                if start + i < w { chars[start + i] = c; }
            }
        }

        // Selection overlay.
        if let Some((sl, sr)) = sel_cols {
            for col in sl..sr.min(w) {
                bg_col[col] = SEL;
                if chars[col] == ' ' { chars[col] = '·'; }
            }
        }

        // Grain zone overlay on mid row (granular only; not in SF2 mode).
        if row == h / 2 && es.sf2.is_none() {
            let range = gs.zone.range.clamp(0.0, 1.0);
            let pos   = gs.zone.position;
            let zl = ((((pos - range / 2.0).max(0.0) - scroll) * zoom * w as f32) as isize).clamp(0, w as isize) as usize;
            let zr = ((((pos + range / 2.0).min(1.0) - scroll) * zoom * w as f32) as isize).clamp(0, w as isize) as usize;
            for col in zl..zr.min(w) {
                if chars[col] == ' ' || chars[col] == '·' { chars[col] = '·'; }
                fg_col[col] = ACCENT;
            }
        }

        // Playhead on top row.
        if row == 0 && gpos_col >= 0 && (gpos_col as usize) < w {
            chars[gpos_col as usize] = '▼';
            fg_col[gpos_col as usize] = WARM;
        }

        // Start / end sample markers.
        for (cv, color) in [(sp_col, OK), (ep_col, Color::Red)] {
            if cv >= 0 && (cv as usize) < w {
                chars[cv as usize] = '│';
                fg_col[cv as usize] = color;
            }
        }

        // User markers.
        for mk in &es.markers {
            let mc = ((mk.position - scroll) * zoom * w as f32) as isize;
            if mc >= 0 && (mc as usize) < w {
                let c = mc as usize;
                if chars[c] == ' ' { chars[c] = '┊'; }
                fg_col[c] = match mk.kind {
                    seqterm_core::MarkerKind::Start | seqterm_core::MarkerKind::End => OK,
                    seqterm_core::MarkerKind::LoopStart | seqterm_core::MarkerKind::LoopEnd => Color::Cyan,
                    seqterm_core::MarkerKind::Slice       => Color::Yellow,
                    seqterm_core::MarkerKind::GrainRegion => Color::Magenta,
                };
                // Label on row 0.
                if row == 0 {
                    for (j, lc) in mk.kind.label().chars().enumerate() {
                        let lp = c + 1 + j;
                        if lp < w { chars[lp] = lc; fg_col[lp] = fg_col[c]; }
                    }
                }
            }
        }

        // Zoom indicator (top-right corner, row 0).
        if row == 0 && zoom > 1.01 {
            let label = format!(" Z:{:.1}x ", zoom);
            let x = w.saturating_sub(label.len());
            for (i, c) in label.chars().enumerate() {
                if x + i < w { chars[x + i] = c; fg_col[x + i] = WARM; }
            }
        }

        // Assemble line with per-column colors.
        let spans: Vec<Span> = (0..w).map(|col| {
            Span::styled(
                chars[col].to_string(),
                Style::default().fg(fg_col[col]).bg(bg_col[col]),
            )
        }).collect();
        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines).style(Style::default().bg(PANEL)), inner);

    // FROZEN overlay.
    if gs.zone.frozen {
        let msg = " ❄ FROZEN ";
        let x = inner.x + (inner.width.saturating_sub(msg.len() as u16)) / 2;
        let r = Rect { x, y: inner.y, width: msg.len() as u16, height: 1 };
        f.render_widget(
            Paragraph::new(Span::styled(msg, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                .style(Style::default().bg(Color::Rgb(20, 50, 70))),
            r,
        );
    }
}

// ─── Transport bar ───────────────────────────────────────────────────────────

fn draw_transport_bar(f: &mut Frame, app: &App, area: Rect) {
    let playing = app.editor_state.preview_playing;

    // TRANSPORT-style boxed buttons, matching MATRIX/TRANSPORT. Each button is a
    // three-line box: top border / label / bottom border. The middle label keeps
    // the inner `│…│` walls so it reads as a single box.
    // (top, mid, bottom, border_color, label_color, bold)
    let play_col = if playing { OK } else { Color::Rgb(20, 80, 30) };
    let buttons: [(&str, &str, &str, Color, Color, bool); 4] = [
        ("╭───────╮",
         if playing { "│⏸ PAUSE│" } else { "│▶ PLAY │" },
         "╰───────╯", play_col, play_col, playing),
        ("╭──────╮", "│■ STOP│", "╰──────╯", Color::Rgb(80, 80, 95), Color::Rgb(170, 170, 185), false),
        ("╭──────╮", "│◀◀ RWD│", "╰──────╯", Color::Rgb(60, 80, 120), ACCENT, false),
        ("╭──────╮", "│● REC │", "╰──────╯", Color::Rgb(150, 40, 40), Color::Rgb(220, 90, 90), false),
    ];

    let label = Span::styled(
        " TRANSPORT ", Style::default().fg(WARM).bg(BG).add_modifier(Modifier::BOLD),
    );
    let label_w = " TRANSPORT ".chars().count() as u16;

    let mut rects = [Rect::default(); 4];
    let mut top: Vec<Span> = vec![Span::styled("           ", Style::default().bg(BG))];
    let mut mid: Vec<Span> = vec![label];
    let mut bot: Vec<Span> = vec![Span::styled("           ", Style::default().bg(BG))];

    let mut x = area.x + label_w;
    for (i, (t, m, b, border, lblc, bold)) in buttons.iter().enumerate() {
        let w = m.chars().count() as u16;
        rects[i] = Rect { x, y: area.y, width: w, height: 3 };
        x += w + 1;
        let border_style = Style::default().fg(*border).bg(BG)
            .add_modifier(if *bold { Modifier::BOLD } else { Modifier::empty() });
        let label_style = Style::default().fg(*lblc).bg(BG)
            .add_modifier(if *bold { Modifier::BOLD } else { Modifier::empty() });
        top.push(Span::styled(*t, border_style));
        mid.push(Span::styled(*m, label_style));
        bot.push(Span::styled(*b, border_style));
        let gap = Span::styled(" ", Style::default().bg(BG));
        top.push(gap.clone());
        mid.push(gap.clone());
        bot.push(gap);
    }
    app.editor_transport_rects.set(rects);

    let bg = Style::default().bg(BG);
    f.render_widget(Paragraph::new(Line::from(top)).style(bg),
        Rect { x: area.x, y: area.y, width: area.width, height: 1 });
    f.render_widget(Paragraph::new(Line::from(mid)).style(bg),
        Rect { x: area.x, y: area.y + 1, width: area.width, height: 1 });
    f.render_widget(Paragraph::new(Line::from(bot)).style(bg),
        Rect { x: area.x, y: area.y + 2, width: area.width, height: 1 });
}

// ─── Section selector grid ───────────────────────────────────────────────────

fn draw_section_grid(f: &mut Frame, app: &App, area: Rect) {
    let active = app.editor_state.tab;
    let cols = EditorTab::GRID_COLS as u16;
    let cell_w = (area.width / cols).max(8);
    let mut rects = [Rect::default(); 8];

    for (i, &tab) in EditorTab::ALL.iter().enumerate() {
        let col = i as u16 % cols;
        let row = i as u16 / cols;
        let cx = area.x + col * cell_w;
        let cy = area.y + row;
        if cy >= area.y + area.height { break; }
        let rect = Rect { x: cx, y: cy, width: cell_w.saturating_sub(1).max(1), height: 1 };
        rects[i] = rect;

        let selected = tab == active;
        let style = if selected {
            Style::default().fg(BG).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(150, 170, 210)).bg(PANEL)
        };
        let mark = if selected { "▸" } else { " " };
        let label = format!("{} {:<width$}", mark, tab.label(), width = (cell_w as usize).saturating_sub(3));
        f.render_widget(Paragraph::new(Span::styled(label, style)), rect);
    }
    app.editor_tab_rects.set(rects);
}

// ─── Param panel dispatcher ───────────────────────────────────────────────────

fn draw_param_panel(f: &mut Frame, app: &App, area: Rect) {
    if app.editor_state.sf2.is_some() {
        draw_sf2_panel(f, app, area);
        return;
    }
    match app.editor_state.tab {
        EditorTab::Sample    => draw_sample_panel(f, app, area),
        EditorTab::Amplitude => draw_amplitude_panel(f, app, area),
        EditorTab::Frequency => draw_frequency_panel(f, app, area),
        EditorTab::Envelope  => draw_envelope_panel(f, app, area),
        EditorTab::Filter    => draw_filter_panel(f, app, area),
        EditorTab::Layers    => draw_layers_panel(f, app, area),
        EditorTab::Granular  => draw_granular_panel(f, app, area),
        EditorTab::Mod       => draw_mod_panel(f, app, area),
    }
}

// ─── SF2 editor panels (reuse the EDITOR view for an SF2 zone) ───────────────

fn draw_sf2_panel(f: &mut Frame, app: &App, area: Rect) {
    let Some(sess) = &app.editor_state.sf2 else { return };
    let tab = app.editor_state.tab;
    let inst = &sess.loaded.instrument;

    let title = format!(" SF2 · {} · zone {}/{} ", inst.name, inst.selected + 1, inst.zones.len().max(1));
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(WARM).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Layers tab = zone selector.
    if tab == EditorTab::Layers {
        let rows: Vec<(&str, String, Option<f32>)> = inst.zones.iter().map(|z| {
            ("zone",
             format!("{:<14} k{:>3}-{:<3} v{:>3}-{:<3}", trunc_name(&z.sample_name, 14), z.key_low, z.key_high, z.vel_low, z.vel_high),
             None)
        }).collect();
        render_param_rows(f, app, inner, &rows, inst.selected, WARM);
        return;
    }

    let Some(z) = inst.selected_zone() else { return };
    let rows: Vec<(&str, String, Option<f32>)> = match tab {
        EditorTab::Sample => vec![
            ("root_key",   midi_note_name(z.root_key),            Some(z.root_key as f32 / 127.0)),
            ("key_low",    midi_note_name(z.key_low),             Some(z.key_low as f32 / 127.0)),
            ("key_high",   midi_note_name(z.key_high),            Some(z.key_high as f32 / 127.0)),
            ("vel_low",    z.vel_low.to_string(),                 Some(z.vel_low as f32 / 127.0)),
            ("vel_high",   z.vel_high.to_string(),                Some(z.vel_high as f32 / 127.0)),
            ("gain",       format!("{:+.1} dB", z.gain_db),       Some((z.gain_db + 48.0) / 60.0)),
            ("loop_mode",  z.loop_mode.label().to_string(),       None),
            ("loop_start", z.loop_start.to_string(),              None),
            ("loop_end",   z.loop_end.to_string(),                None),
            ("xfade",      format!("{:.0} ms", z.loop_crossfade), Some(z.loop_crossfade / 200.0)),
        ],
        EditorTab::Envelope => vec![
            ("attack",  format!("{:.0} ms", z.attack  * 1000.0), Some((z.attack  / 4.0).min(1.0))),
            ("hold",    format!("{:.0} ms", z.hold    * 1000.0), Some((z.hold    / 4.0).min(1.0))),
            ("decay",   format!("{:.0} ms", z.decay   * 1000.0), Some((z.decay   / 4.0).min(1.0))),
            ("sustain", format!("{:.0}%",   z.sustain * 100.0),  Some(z.sustain)),
            ("release", format!("{:.0} ms", z.release * 1000.0), Some((z.release / 4.0).min(1.0))),
        ],
        EditorTab::Filter => vec![
            ("type",     z.filter_type.label().to_string(),          None),
            ("cutoff",   format!("{:.0} Hz", z.cutoff),              Some((z.cutoff / 20_000.0).clamp(0.0, 1.0))),
            ("resonance",format!("{:.0}%", z.resonance * 100.0),     Some(z.resonance)),
            ("tracking", format!("{:.0}%", z.key_tracking * 100.0),  Some(z.key_tracking)),
        ],
        EditorTab::Amplitude => vec![
            ("lfo_wave",  z.lfo_waveform.label().to_string(),       None),
            ("lfo_freq",  format!("{:.2} Hz", z.lfo_freq),          Some((z.lfo_freq / 20.0).clamp(0.0, 1.0))),
            ("lfo_delay", format!("{:.0} ms", z.lfo_delay * 1000.0),Some((z.lfo_delay / 5.0).min(1.0))),
            ("lfo_depth", format!("{:.0}%", z.lfo_depth * 100.0),   Some(z.lfo_depth)),
        ],
        EditorTab::Frequency => vec![
            ("coarse", format!("{:+} st", z.coarse_tune), Some((z.coarse_tune as f32 + 64.0) / 128.0)),
            ("fine",   format!("{:+} ct", z.fine_tune),   Some((z.fine_tune as f32 + 100.0) / 200.0)),
        ],
        _ => vec![("(n/a)", "use Sample/Env/Filter/Amp/Freq/Layers".to_string(), None)],
    };
    render_param_rows(f, app, inner, &rows, app.editor_state.cursor, WARM);
}

fn trunc_name(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() }
    else { format!("{}…", s.chars().take(max - 1).collect::<String>()) }
}

fn midi_note_name(n: u8) -> String {
    const NAMES: [&str; 12] = ["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"];
    let oct = (n / 12) as i32 - 1;
    format!("{}{} ({})", NAMES[(n % 12) as usize], oct, n)
}

// ─── Helper: render a param row, store its Rect ──────────────────────────────

/// Most a flat list panel ever renders (headroom over the largest panel, Layers
/// at 16). Bounds the take()/cursor; distinct from `GRAN_PARAM_COUNT` (the size
/// of the shared rect table, which the Granular/Mod tabs fill across sub-panels).
const MAX_LIST_ROWS: usize = 25;

/// Default non-selected label colour for list-panel rows.
const ROW_LABEL_FG: Color = Color::Rgb(140, 160, 200);

/// Render one parameter row (label · value · optional bar) into `lines`, and — when
/// `register` — record its row/bar `Rect` at `idx` for mouse hit-testing. The single
/// row renderer behind every EDITOR panel (flat lists, grain, zone). `sel_bg` is the
/// selected-row wash; `label_fg` the non-selected label colour.
#[allow(clippy::too_many_arguments)]
fn push_param_row(
    lines: &mut Vec<Line>,
    rects: &mut [Rect],
    bars: &mut [Rect],
    idx: usize,
    area: Rect,
    row_y: u16,
    lbl: &str,
    val: &str,
    frac: Option<f32>,
    is_sel: bool,
    sel_bg: Color,
    label_fg: Color,
    label_w: usize,
    value_w: usize,
    register: bool,
) {
    let (bg, fg_lbl, fg_val) = if is_sel {
        (sel_bg, Color::Black, Color::Black)
    } else {
        (BG, label_fg, Color::White)
    };
    if register {
        if let Some(r) = rects.get_mut(idx) {
            *r = Rect { x: area.x, y: row_y, width: area.width, height: 1 };
        }
    }
    let label_span = format!(" {:<w$}", lbl, w = label_w);
    let value_span = format!("{:<w$}", val, w = value_w);
    let bar = frac.map(|fr| param_value_bar(fr, area.width)).unwrap_or_default();
    if register && !bar.is_empty() {
        if let Some(b) = bars.get_mut(idx) {
            let bar_x = area.x
                + label_span.chars().count() as u16
                + value_span.chars().count() as u16;
            *b = Rect { x: bar_x, y: row_y, width: bar.chars().count() as u16, height: 1 };
        }
    }
    lines.push(Line::from(vec![
        Span::styled(label_span, Style::default().fg(fg_lbl).bg(bg)),
        Span::styled(
            value_span,
            Style::default().fg(fg_val).bg(bg)
                .add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() }),
        ),
        Span::styled(bar, Style::default().fg(if is_sel { Color::White } else { DIM }).bg(bg)),
    ]));
}

/// Render a flat list panel: cursor-base 0, fresh rect table, 12-col label/value.
fn render_param_rows(
    f: &mut Frame,
    app: &App,
    inner: Rect,
    rows: &[(&str, String, Option<f32>)],
    cursor: usize,
    accent: Color,
) {
    let mut rects = [Rect::default(); crate::app::GRAN_PARAM_COUNT];
    let mut bar_rects = [Rect::default(); crate::app::GRAN_PARAM_COUNT];
    let count = rows.len().min(MAX_LIST_ROWS);

    let mut lines: Vec<Line> = Vec::with_capacity(count);
    for (i, (lbl, val, frac)) in rows.iter().take(MAX_LIST_ROWS).enumerate() {
        push_param_row(
            &mut lines, &mut rects, &mut bar_rects, i, inner, inner.y + i as u16,
            lbl, val, *frac, i == cursor, accent, ROW_LABEL_FG, 12, 12, true,
        );
    }

    app.editor_param_rects.set(rects);
    app.editor_param_bar_rects.set(bar_rects);
    app.editor_param_count.set(count);
    f.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), inner);
}

/// Fixed-width parameter bar matching the PATTERN/SETTINGS/HUMANIZATION bars in
/// the tracker view: `█` for the filled portion, `░` for the rest.
fn value_bar(frac: f32, width: usize) -> String {
    let filled = (frac.clamp(0.0, 1.0) * width as f32).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// Value bar sized to fit a param row after its label/value columns (26 cols),
/// returning empty when there is no room.
fn param_value_bar(frac: f32, area_w: u16) -> String {
    if area_w < 30 { return String::new(); }
    let bar_w = (area_w as usize).saturating_sub(26).min(14).max(4);
    value_bar(frac, bar_w)
}

// ─── SAMPLE panel ─────────────────────────────────────────────────────────────

fn draw_sample_panel(f: &mut Frame, app: &App, area: Rect) {
    let s = &app.editor_state.sample;
    let block = Block::default()
        .title(" SAMPLE PLAYBACK ")
        .title_style(Style::default().fg(WARM).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let gain_db = if s.gain > 1e-6 { 20.0 * s.gain.log10() } else { -96.0 };

    let rows: Vec<(&str, String, Option<f32>)> = vec![
        ("start",     format!("{:.1}%", s.start * 100.0),    Some(s.start)),
        ("end",       format!("{:.1}%", s.end   * 100.0),    Some(s.end)),
        ("gain",      format!("{:+.1} dB", gain_db),         Some(s.gain / 4.0)),
        ("pan",       format!("{:+.2}", s.pan),              Some((s.pan + 1.0) / 2.0)),
        ("pitch",     format!("{:+.1} st", s.pitch),         Some((s.pitch + 24.0) / 48.0)),
        ("fine_tune", format!("{:+.0} ct", s.fine_tune),     Some((s.fine_tune + 100.0) / 200.0)),
        ("reverse",   if s.reverse { "YES".to_string() } else { "no".to_string() }, None),
        ("loop",      if s.loop_on { "YES".to_string() } else { "no".to_string() }, None),
        ("loop_mode", s.loop_mode.label().to_string(),       None),
    ];

    render_param_rows(f, app, inner, &rows, app.editor_state.cursor, WARM);
}

// ─── AMPLITUDE panel ──────────────────────────────────────────────────────────

fn draw_amplitude_panel(f: &mut Frame, app: &App, area: Rect) {
    let a = &app.editor_state.amplitude;
    let block = Block::default()
        .title(" AMPLITUDE ")
        .title_style(Style::default().fg(OK).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let level_db = if a.level > 1e-6 { 20.0 * a.level.log10() } else { -96.0 };
    let rows: Vec<(&str, String, Option<f32>)> = vec![
        ("level",     format!("{:.2}  ({:+.1} dB)", a.level, level_db), Some(a.level / 2.0)),
        ("amp env",   if a.env_enabled { "ON (ADSR)".into() } else { "off".into() }, None),
        ("amp lfo",   if a.lfo_enabled { "ON (tremolo)".into() } else { "off".into() }, None),
        ("lfo rate",  format!("{:.2} Hz", a.lfo_rate),     Some(a.lfo_rate / 20.0)),
        ("lfo depth", format!("{:.0}%", a.lfo_depth * 100.0), Some(a.lfo_depth)),
        ("lfo shape", a.lfo_shape.label().to_string(),     None),
    ];
    render_param_rows(f, app, inner, &rows, app.editor_state.cursor, OK);
}

// ─── FREQUENCY panel ──────────────────────────────────────────────────────────

fn draw_frequency_panel(f: &mut Frame, app: &App, area: Rect) {
    let fr = &app.editor_state.frequency;
    let block = Block::default()
        .title(" FREQUENCY ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows: Vec<(&str, String, Option<f32>)> = vec![
        ("detune",    format!("{:+.0} cents", fr.detune_cents), Some((fr.detune_cents + 100.0) / 200.0)),
        ("octave",    format!("{:+}", fr.octave),    Some((fr.octave + 4) as f32 / 8.0)),
        ("harmonics", format!("{}", fr.harmonics),   Some((fr.harmonics as f32 - 1.0) / 15.0)),
    ];
    render_param_rows(f, app, inner, &rows, app.editor_state.cursor, ACCENT);
}

// ─── LAYERS panel ─────────────────────────────────────────────────────────────

fn draw_layers_panel(f: &mut Frame, app: &App, area: Rect) {
    let ls = &app.editor_state.layers;
    let block = Block::default()
        .title(" LAYERS ")
        .title_style(Style::default().fg(WARM).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Each layer contributes 4 rows: enabled, gain, pitch, pan.
    let mut rows: Vec<(&str, String, Option<f32>)> = Vec::with_capacity(ls.layers.len() * 4);
    let labels = ["L1 on", "L1 gain", "L1 pitch", "L1 pan",
                  "L2 on", "L2 gain", "L2 pitch", "L2 pan",
                  "L3 on", "L3 gain", "L3 pitch", "L3 pan",
                  "L4 on", "L4 gain", "L4 pitch", "L4 pan"];
    for (i, layer) in ls.layers.iter().enumerate() {
        rows.push((labels[i * 4],     if layer.enabled { "ON".into() } else { "off".into() }, None));
        rows.push((labels[i * 4 + 1], format!("{:.2}", layer.gain),       Some(layer.gain / 2.0)));
        rows.push((labels[i * 4 + 2], format!("{:+.1} st", layer.pitch_st), Some((layer.pitch_st + 24.0) / 48.0)));
        rows.push((labels[i * 4 + 3], format!("{:+.2}", layer.pan),       Some((layer.pan + 1.0) / 2.0)));
    }
    render_param_rows(f, app, inner, &rows, app.editor_state.cursor, WARM);
}

// ─── ENVELOPE panel ──────────────────────────────────────────────────────────

fn draw_envelope_panel(f: &mut Frame, app: &App, area: Rect) {
    let e = &app.editor_state.envelope;
    let block = Block::default()
        .title(" ENVELOPE (ADSR) ")
        .title_style(Style::default().fg(OK).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 { return; }

    // Split: ASCII curve (top 4 rows) + params (rest).
    let curve_h = 4u16.min(inner.height.saturating_sub(5));
    let param_h = inner.height.saturating_sub(curve_h);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(curve_h), Constraint::Min(0)])
        .split(inner);

    // Draw ASCII ADSR curve.
    if curve_h >= 3 {
        draw_adsr_curve(f, app, chunks[0]);
    }

    let rows: Vec<(&str, String, Option<f32>)> = vec![
        ("enabled", if e.enabled { "ON".into() } else { "off (bypass)".into() }, None),
        ("attack",  format!("{:.0} ms", e.attack_ms),   Some(e.attack_ms / 5000.0)),
        ("hold",    format!("{:.0} ms", e.hold_ms),     Some(e.hold_ms / 5000.0)),
        ("decay",   format!("{:.0} ms", e.decay_ms),    Some(e.decay_ms / 5000.0)),
        ("sustain", format!("{:.0}%",   e.sustain * 100.0), Some(e.sustain)),
        ("release", format!("{:.0} ms", e.release_ms),  Some(e.release_ms / 10000.0)),
    ];

    let param_area = if param_h > 0 { chunks[1] } else { return };
    render_param_rows(f, app, param_area, &rows, app.editor_state.cursor, OK);
}

fn draw_adsr_curve(f: &mut Frame, app: &App, area: Rect) {
    let e = &app.editor_state.envelope;
    let w = area.width as usize;
    if w < 8 { return; }

    // Normalise each segment width proportionally (total = w-1 cols).
    let total_ms = e.attack_ms + e.hold_ms + e.decay_ms + e.release_ms + 1.0;
    let a = ((e.attack_ms  / total_ms) * (w - 1) as f32) as usize;
    let h_ = ((e.hold_ms   / total_ms) * (w - 1) as f32) as usize;
    let d  = ((e.decay_ms  / total_ms) * (w - 1) as f32) as usize;
    let r  = (w - 1).saturating_sub(a + h_ + d);

    let rows = area.height as usize;
    let sus_row = ((1.0 - e.sustain) * (rows as f32 - 1.0)) as usize;

    let mut lines: Vec<Line> = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut chars = vec![' '; w];
        // Attack: rise from bottom to top.
        let atk_row = ((1.0 - row as f32 / (rows as f32 - 1.0)) * a as f32) as usize;
        if atk_row < w { chars[atk_row] = '╱'; }
        // Hold: flat at top.
        for col in a..(a + h_).min(w) { if row == 0 { chars[col] = '─'; } }
        // Decay: fall from top to sustain.
        let dec_start = a + h_;
        if d > 0 {
            let dec_row_at_col = |col: usize| -> usize {
                let t = (col - dec_start) as f32 / d as f32;
                (t * sus_row as f32) as usize
            };
            for col in dec_start..(dec_start + d).min(w) {
                if row == dec_row_at_col(col) { chars[col] = '╲'; }
            }
        }
        // Sustain: flat at sus_row.
        let sus_start = dec_start + d;
        for col in sus_start..(sus_start + 4).min(w) {
            if row == sus_row { chars[col] = '─'; }
        }
        // Release: fall to zero.
        let rel_start = sus_start + 4;
        if r > 0 {
            let rel_row_at_col = |col: usize| -> usize {
                let t = (col - rel_start) as f32 / r.max(1) as f32;
                sus_row + (t * (rows - sus_row) as f32) as usize
            };
            for col in rel_start..(rel_start + r).min(w) {
                if row == rel_row_at_col(col) { chars[col] = '╲'; }
            }
        }
        let s: String = chars.into_iter().collect();
        lines.push(Line::from(Span::styled(s, Style::default().fg(OK).bg(BG))));
    }
    f.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), area);
}

// ─── FILTER panel ─────────────────────────────────────────────────────────────

fn draw_filter_panel(f: &mut Frame, app: &App, area: Rect) {
    let fi = &app.editor_state.filter;
    let block = Block::default()
        .title(" FILTER ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Exponential Hz mapping: 20 * (20000/20)^cutoff = 20 * 1000^cutoff.
    let cutoff_hz = 20.0 * (1000.0f32).powf(fi.cutoff);

    let rows: Vec<(&str, String, Option<f32>)> = vec![
        ("type",      fi.kind.label().to_string(),     None),
        ("cutoff",    format!("{:.0} Hz", cutoff_hz),  Some(fi.cutoff)),
        ("resonance", format!("{:.2}", fi.resonance),  Some(fi.resonance)),
    ];

    render_param_rows(f, app, inner, &rows, app.editor_state.cursor, Color::Cyan);
}

// ─── GRANULAR panel (original params + zone) ─────────────────────────────────

fn draw_granular_panel(f: &mut Frame, app: &App, area: Rect) {
    let state = &app.granular_state;
    let cursor = app.editor_state.cursor;

    let hchunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(3, 5), Constraint::Ratio(2, 5)])
        .split(area);

    // Grain params.
    let gblock = Block::default()
        .title(" GRAIN PARAMS ")
        .title_style(Style::default().fg(WARM).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if state.pad.is_some() { BORDER } else { DIM }))
        .style(Style::default().bg(BG));
    let ginner = gblock.inner(hchunks[0]);
    f.render_widget(gblock, hchunks[0]);

    let p = &state.params;
    // (label, value, 0..1 fraction for the bar). Cursor indices 0..12.
    let grain_rows: Vec<(&str, String, Option<f32>)> = vec![
        ("size_ms",   format!("{:.0} ms", p.size_ms),  Some(grain_param_frac(0, p))),
        ("density",   format!("{:.1} /s", p.density),  Some(grain_param_frac(1, p))),
        ("spray",     format!("{:.3}", p.spray),       Some(grain_param_frac(2, p))),
        ("overlap",   format!("{:.2}", p.overlap),     Some(grain_param_frac(3, p))),
        ("pitch",     format!("{:+.1} st", p.pitch_st),Some(grain_param_frac(4, p))),
        ("direction", p.direction.label().to_string(), Some(grain_param_frac(5, p))),
        ("pan",       format!("{:+.2}", p.pan),        Some(grain_param_frac(6, p))),
        ("gain",      format!("{:.2}", p.gain),        Some(grain_param_frac(7, p))),
        ("jitter",    format!("{:.3}", p.jitter),      Some(grain_param_frac(8, p))),
        ("spread",    format!("{:.2}", p.stereo_spread),Some(grain_param_frac(9, p))),
        ("envelope",  p.envelope.label().to_string(),  Some(grain_param_frac(10, p))),
        ("voices",    format!("{}", p.max_voices),     Some(grain_param_frac(11, p))),
    ];

    // Grain params occupy cursor indices 0..12; zone (12..17) and the mod matrix
    // (17..) register their rects below so the whole panel is mouse-editable.
    let mut rects = [Rect::default(); crate::app::GRAN_PARAM_COUNT];
    let mut bar_rects = [Rect::default(); crate::app::GRAN_PARAM_COUNT];
    let mut glines: Vec<Line> = Vec::with_capacity(grain_rows.len());
    for (i, (lbl, val, frac)) in grain_rows.iter().enumerate() {
        push_param_row(
            &mut glines, &mut rects, &mut bar_rects, i, ginner, ginner.y + i as u16,
            lbl, val, *frac, cursor == i, ACCENT, ROW_LABEL_FG, 10, 10, true,
        );
    }

    app.editor_param_rects.set(rects);
    app.editor_param_bar_rects.set(bar_rects);
    app.editor_param_count.set(crate::app::GRAN_PARAM_COUNT);
    f.render_widget(Paragraph::new(glines).style(Style::default().bg(BG)), ginner);

    // Zone + mod in right column.
    let right_v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(0)])
        .split(hchunks[1]);

    draw_zone_panel(f, app, right_v[0]);
    draw_mod_matrix_inner(f, app, right_v[1], cursor);
}

fn draw_zone_panel(f: &mut Frame, app: &App, area: Rect) {
    let state = &app.granular_state;
    let cursor = app.editor_state.cursor;

    let zblock = Block::default()
        .title(" ZONE ")
        .title_style(Style::default().fg(OK).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if state.pad.is_some() { BORDER } else { DIM }))
        .style(Style::default().bg(BG));
    let zinner = zblock.inner(area);
    f.render_widget(zblock, area);

    let z = &state.zone;
    let live_label = match app.granular_live_source {
        Some(id) => format!("slot {id}"),
        None     => "off".to_string(),
    };
    // (label, value, optional 0..1 fraction for the bar). cursor = 12 + index.
    let zone_rows: Vec<(&str, String, Option<f32>)> = vec![
        ("position",   format!("{:.3}", z.position),   Some(z.position)),
        ("range",      format!("{:.3}", z.range),      Some(z.range)),
        ("scan_speed", format!("{:.2}", z.scan_speed), Some(z.scan_speed / 2.0)),
        ("scan_mode",  z.scan_mode.label().to_string(), None),
        ("frozen",     if z.frozen { "YES".to_string() } else { "no".to_string() }, None),
        ("live src",   live_label, None),
    ];

    // Merge zone row rects (cursor 12..=16) into the shared param-rect table so
    // the click/scroll handlers can focus and adjust them like any other section.
    // "live src" (index 5 / cursor 17) is display-only and collides with mod slot
    // 0; leave it unregistered so the mod slot owns that cursor.
    let mut prects = app.editor_param_rects.get();
    let mut pbars = app.editor_param_bar_rects.get();
    let mut zlines: Vec<Line> = Vec::with_capacity(zone_rows.len());
    for (i, (lbl, val, frac)) in zone_rows.iter().enumerate() {
        let zone_cursor = 12 + i;
        push_param_row(
            &mut zlines, &mut prects, &mut pbars, zone_cursor, zinner, zinner.y + i as u16,
            lbl, val, *frac, cursor == zone_cursor, ACCENT, Color::Rgb(140, 200, 140),
            12, 10, zone_cursor <= 16,
        );
    }
    app.editor_param_rects.set(prects);
    app.editor_param_bar_rects.set(pbars);

    f.render_widget(Paragraph::new(zlines).style(Style::default().bg(BG)), zinner);
}

// ─── MOD panel ────────────────────────────────────────────────────────────────

fn draw_mod_panel(f: &mut Frame, app: &App, area: Rect) {
    let state = &app.granular_state;
    let cursor = app.editor_state.cursor;

    let block = Block::default()
        .title(" MOD MATRIX ")
        .title_style(Style::default().fg(Color::Rgb(200, 140, 255)).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if state.pad.is_some() { BORDER } else { DIM }))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // In the dedicated MOD tab nothing above cursor 17 is drawn, so clear any
    // stale grain/zone rects left over from the Granular tab before the mod
    // matrix re-registers its own rows (cursor 17..=24).
    app.editor_param_rects.set([Rect::default(); crate::app::GRAN_PARAM_COUNT]);
    app.editor_param_bar_rects.set([Rect::default(); crate::app::GRAN_PARAM_COUNT]);
    draw_mod_matrix_inner(f, app, inner, cursor);
}

fn draw_mod_matrix_inner(f: &mut Frame, app: &App, area: Rect, cursor: usize) {
    // Merge mod-slot (cursor 17..=20) and macro (cursor 21..=24) rects into the
    // shared param-rect table so they are mouse-clickable/scrollable.
    let mut prects = app.editor_param_rects.get();
    let mut pbars = app.editor_param_bar_rects.get();

    let mlines: Vec<Line> = (0..MOD_SLOTS).map(|i| {
        let slot = &app.granular_mod.slots[i];
        let mod_cursor = 17 + i;
        prects[mod_cursor] = Rect { x: area.x, y: area.y + i as u16, width: area.width, height: 1 };
        let is_sel = cursor == mod_cursor;
        let (bg, fg) = if is_sel { (ACCENT, Color::Black) } else { (BG, Color::Rgb(180, 140, 220)) };
        let on_off = if slot.enabled { "●" } else { "○" };
        let on_off_style = if slot.enabled {
            Style::default().fg(OK).bg(bg)
        } else {
            Style::default().fg(DIM).bg(bg)
        };
        // Depth bar (tracker style) shown after the slot summary; register its
        // rect so clicking/dragging it sets the slot's depth.
        let dbar = value_bar(slot.depth, 6);
        // When this slot is routed to an FX destination, show "→fx" instead of
        // the granular target so the binding is visible (press F to cycle).
        let target_label: String = match app.editor_fx_mod_target[i] {
            Some(dest) => {
                let l = app.editor_fx_destinations().into_iter()
                    .find(|(d, _)| *d == dest).map(|(_, l)| l)
                    .unwrap_or_else(|| "fx".to_string());
                format!("→{}", l)
            }
            None => slot.target.label().to_string(),
        };
        let prefix = format!(
            " {} {:<4} {:<10} {:>4.1}hz ",
            i + 1,
            slot.shape.label(),
            target_label.chars().take(10).collect::<String>(),
            slot.rate_hz,
        );
        let bar_x = area.x + 1 + prefix.chars().count() as u16;
        if bar_x + dbar.chars().count() as u16 <= area.x + area.width {
            pbars[mod_cursor] = Rect { x: bar_x, y: area.y + i as u16, width: dbar.chars().count() as u16, height: 1 };
        }
        let label = format!("{}{}", prefix, dbar);
        Line::from(vec![
            Span::styled(on_off, on_off_style),
            Span::styled(
                format!("{:<width$}", label, width = area.width.saturating_sub(1) as usize),
                Style::default().fg(fg).bg(bg)
                    .add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() }),
            ),
        ])
    }).collect();

    let mut all_lines: Vec<Line> = mlines;

    let sep_y = area.y + MOD_SLOTS as u16;
    all_lines.push(Line::from(Span::styled(
        format!(" {}", "─".repeat(area.width.saturating_sub(2) as usize)),
        Style::default().fg(DIM),
    )));

    // Macros 1-16 in two columns of 8 (1-8 left, 9-16 right). Macros 1-4 morph
    // granular sound (SPRAY/DENS/PITCH/SIZE); each macro may also drive an FX
    // param — its assigned target (or that default) is shown after the value bar.
    const MACRO_N: usize = seqterm_core::MACRO_COUNT;
    let macro_sound_labels = ["SPRAY", "DENS", "PITCH", "SIZE"];
    let rows = MACRO_N / 2; // 8
    let col_w = area.width / 2;
    let bar_w = (col_w.saturating_sub(11)) as usize;
    for i in 0..MACRO_N {
        let mac_cursor = 21 + i;
        let col = i / rows;            // 0 = left, 1 = right
        let row = i % rows;
        let x = area.x + col as u16 * col_w;
        let y = sep_y + 1 + row as u16;
        prects[mac_cursor] = Rect { x, y, width: col_w, height: 1 };
        let is_sel = cursor == mac_cursor;
        let val = app.granular_macros[i];
        let (bg, fg) = if is_sel { (ACCENT, Color::Black) } else { (BG, Color::Rgb(220, 180, 255)) };
        // Label: FX target if assigned, else granular default for 1-4, else blank.
        let tgt = app.editor_macro_fx_target[i].is_some();
        let name: String = if tgt {
            "FX".to_string()
        } else if i < 4 {
            macro_sound_labels[i].to_string()
        } else {
            String::new()
        };
        let label_span = format!("M{:<2}{:<4}", i + 1, name.chars().take(4).collect::<String>());
        let bar = value_bar(val, bar_w);
        if !bar.is_empty() {
            pbars[mac_cursor] = Rect {
                x: x + label_span.chars().count() as u16,
                y,
                width: bar.chars().count() as u16,
                height: 1,
            };
        }
        let line = Line::from(vec![
            Span::styled(label_span, Style::default().fg(fg).bg(bg)),
            Span::styled(bar, Style::default().fg(if is_sel { Color::White } else { Color::Rgb(160, 100, 220) }).bg(bg)),
        ]);
        if col == 0 {
            // Start a new combined row; the right column is appended below.
            all_lines.push(line);
        } else {
            // Append right-column spans onto the matching left-column line.
            let line_idx = all_lines.len() - rows + row;
            if let Some(existing) = all_lines.get_mut(line_idx) {
                existing.spans.extend(line.spans);
            }
        }
    }

    app.editor_param_rects.set(prects);
    app.editor_param_bar_rects.set(pbars);
    app.editor_param_count.set(crate::app::GRAN_PARAM_COUNT);
    f.render_widget(Paragraph::new(all_lines).style(Style::default().bg(BG)), area);
}

// ─── PATTERN bar (replaces SCENES) ───────────────────────────────────────────
//
// Shows each active matrix row (A–P). Clicking a row routes its primary audio
// slot to the granular engine as a live resampling source. The active source is
// highlighted. A second click deactivates (set to None).

fn draw_pattern_bar(f: &mut Frame, app: &App, area: Rect) {
    // Collect rows that have at least one audio slot assigned.
    let rows = app.matrix_rows; // number of visible rows in the matrix (1-16)

    // Build one button per row, two text-lines tall.
    let btn_w = (area.width / rows.max(1) as u16).max(5).min(12);
    let live = app.granular_live_source;

    let mut rects = [Rect::default(); 16];
    let mut spans_top: Vec<Span> = Vec::new();
    let mut spans_bot: Vec<Span> = Vec::new();

    // Title on left.
    spans_top.push(Span::styled(" PATTERNS ", Style::default().fg(WARM)));
    spans_bot.push(Span::styled("           ", Style::default().fg(DIM)));

    let label_w = " PATTERNS ".len() as u16;

    for row in 0..rows.min(16) {
        let row_key = ((b'A' + row as u8) as char).to_string();
        // Find the first audio slot for this row.
        let slot_id: Option<u32> = app.audio_slots.iter()
            .filter(|(k, _)| k.starts_with(&row_key))
            .map(|(_, &v)| v)
            .next();

        let is_live = slot_id.is_some() && slot_id == live;
        let has_slot = slot_id.is_some();

        let style = if is_live {
            Style::default().fg(BG).bg(LIVE).add_modifier(Modifier::BOLD)
        } else if has_slot {
            Style::default().fg(Color::White).bg(Color::Rgb(30, 50, 40))
        } else {
            Style::default().fg(DIM).bg(BG)
        };

        let letter = format!(" {} ", row_key);
        let indicator = if is_live { " ◉ " } else if has_slot { " ○ " } else { "   " };

        rects[row] = Rect {
            x: area.x + label_w + row as u16 * btn_w,
            y: area.y,
            width: btn_w,
            height: 2,
        };

        let top_label = format!("{:^width$}", letter, width = btn_w as usize);
        let bot_label = format!("{:^width$}", indicator, width = btn_w as usize);

        spans_top.push(Span::styled(top_label, style));
        spans_bot.push(Span::styled(bot_label, style));
    }

    app.editor_pattern_rects.set(rects);
    app.editor_pattern_count.set(rows.min(16));

    // Render two text rows inside area.
    let top_area = Rect { x: area.x, y: area.y,          width: area.width, height: 1 };
    let bot_area = Rect { x: area.x, y: area.y + 1,      width: area.width, height: 1 };

    f.render_widget(
        Paragraph::new(Line::from(spans_top)).style(Style::default().bg(BG)),
        top_area,
    );
    if area.height >= 2 {
        f.render_widget(
            Paragraph::new(Line::from(spans_bot)).style(Style::default().bg(BG)),
            bot_area,
        );
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Down-sample `samples` to `w` peak bands (max abs per chunk) for the waveform
/// strip. Capped at 512 bands regardless of width.
fn peaks(samples: &[f32], w: usize) -> Vec<f32> {
    let bands = w.max(1).min(512);
    let chunk = (samples.len() / bands).max(1);
    (0..bands)
        .map(|b| {
            let s = b * chunk;
            let e = (s + chunk).min(samples.len());
            samples[s..e].iter().fold(0.0f32, |m, &x| m.max(x.abs()))
        })
        .collect()
}

/// Normalised 0..1 value for a grain parameter row, used to size its value bar.
fn grain_param_frac(param_idx: usize, p: &seqterm_core::GrainParams) -> f32 {
    match param_idx {
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
    }
}
