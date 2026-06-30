use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use seqterm_core::PatternSource;

use crate::app::App;

const PANEL: Color = Color::Rgb(22, 27, 34);
const BORDER: Color = Color::Rgb(48, 54, 61);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const HEADER: Color = Color::Rgb(240, 136, 62);

// ─── Color palette from the reference matrix project (Indexed 256-color) ────
const C_CURRENT:    Color = Color::Indexed(15);  // white        – playing + note hit
const C_ACTIVE:     Color = Color::Indexed(250); // light gray   – cursor  + enabled
const C_ENABLED:    Color = Color::Indexed(18);  // dark blue    – enabled, idle
const C_DISABLED:   Color = Color::Indexed(237); // dark gray    – disabled (same structure, gray)
const C_DIS_CURSOR: Color = Color::Indexed(243); // medium gray  – cursor on disabled
const C_INACTIVE:   Color = Color::Indexed(238); // medium gray  – cursor, empty slot
const C_INACT_DIM:  Color = Color::Indexed(234); // very dark    – empty, not cursor
const C_TEXT_DARK:  Color = Color::Indexed(232); // near-black text (on bright bg)
const C_TEXT_BLUE:  Color = Color::Indexed(252); // light text on dark blue
const C_TEXT_GRAY:  Color = Color::Indexed(246); // light-gray text on dark gray (disabled)
const C_TEXT_MED:   Color = Color::Indexed(242); // medium-gray text (empty slots)
const C_ROUTE_FAIL: Color = Color::Indexed(130); // dark amber  – MIDI destination unavailable
const C_ROUTE_CUR:  Color = Color::Indexed(172); // bright amber – cursor on route-fail cell

// ─── 3×3 micro-font (same glyphs as the reference project) ──────────────────

fn glyph(c: char) -> [&'static str; 3] {
    match c.to_ascii_uppercase() {
        '0' => ["█▀█", "█ █", "█▄█"],
        '1' => ["▀█ ", " █ ", "▄█▄"],
        '2' => ["█▀█", "▄▀▀", "█▄▄"],
        '3' => ["█▀█", " ▀█", "█▄█"],
        '4' => ["█ █", "▀▀█", "  █"],
        '5' => ["█▀▀", "▀▀█", "▄▄█"],
        '6' => ["█▀▀", "█▀█", "█▄█"],
        '7' => ["▀▀█", "  █", "  █"],
        '8' => ["█▀█", "█▀█", "█▄█"],
        '9' => ["█▀█", "▀▀█", "  █"],
        'A' => ["█▀█", "█▀█", "█ █"],
        'B' => ["█▀█", "█▀▄", "█▄█"],
        'C' => ["█▀█", "█  ", "█▄█"],
        'D' => ["█▀▄", "█ █", "█▄▀"],
        'E' => ["█▀▀", "█▀ ", "█▄▄"],
        'F' => ["█▀▀", "█▀ ", "█  "],
        'G' => ["█▀▀", "█ █", "█▄█"],
        'H' => ["█ █", "███", "█ █"],
        'I' => ["▀█▀", " █ ", "▄█▄"],
        'J' => ["  █", "  █", "▄▄█"],
        'K' => ["█ █", "██ ", "█ █"],
        'L' => ["█  ", "█  ", "█▄▄"],
        'M' => ["█▄█", "█ █", "█ █"],
        'N' => ["█▀▄", "█ █", "█ █"],
        'O' => ["█▀█", "█ █", "█▄█"],
        'P' => ["█▀█", "█▀▀", "█  "],
        'Q' => ["█▀█", "█▀▄", "▀▄▀"],
        'R' => ["█▀█", "█▀▄", "█ █"],
        'S' => ["█▀█", "▀▀▄", "█▄█"],
        'T' => ["▀█▀", " █ ", " █ "],
        'U' => ["█ █", "█ █", "█▄█"],
        'V' => ["█ █", "█ █", " ▀ "],
        'W' => ["█ █", "█ █", "▀▄▀"],
        'X' => ["█ █", " ▀ ", "█ █"],
        'Y' => ["█ █", " ▀ ", " █ "],
        'Z' => ["▀▀█", " ▀ ", "█▄▄"],
        '-' => ["   ", "▄▄▄", "   "],
        _   => ["   ", "   ", "   "],
    }
}

/// Render `text` (up to 3 chars) as 3 rows of the micro-font,
/// each padded/centred to exactly `cell_w` visible columns.
fn ascii_rows(text: &str, cell_w: usize) -> [String; 3] {
    let mut rows = [String::new(), String::new(), String::new()];
    let mut n = 0usize;
    for c in text.chars() {
        let g = glyph(c);
        for r in 0..3 {
            rows[r].push_str(g[r]); // 3 visible cols
            rows[r].push(' ');      // 1 space separator
        }
        n += 1;
    }
    // content is n*4 visible columns wide; centre inside cell_w
    let content = n * 4;
    let pad_l = cell_w.saturating_sub(content) / 2;
    let pad_r = cell_w.saturating_sub(content).saturating_sub(pad_l);
    for r in 0..3 {
        let inner = std::mem::take(&mut rows[r]);
        rows[r] = format!("{}{}{}", " ".repeat(pad_l), inner, " ".repeat(pad_r));
    }
    rows
}

/// Render amplitude peaks as a row of block characters, padded to `width` columns.
fn waveform_bar(peaks: &[f32], width: usize) -> String {
    const BLOCKS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if peaks.is_empty() || width == 0 { return " ".repeat(width); }
    (0..width).map(|i| {
        let idx = i * peaks.len() / width;
        let peak = peaks[idx.min(peaks.len() - 1)];
        BLOCKS[(peak * 8.0).round() as usize % 9]
    }).collect()
}

pub fn draw_matrix(f: &mut Frame, app: &App, area: Rect) {
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(60), Constraint::Length(38)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(20), Constraint::Length(9)])
        .split(h_chunks[0]);

    // Right sidebar: 2-row tab bar, then full-height content.
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(h_chunks[1]);

    // Cache grid and transport rects for mouse; polymeter/routing rects are
    // updated inside their own draw functions or here with dummy values when
    // a different tab is active.
    app.matrix_panel_rects.set([
        left_chunks[0],                  // [0] clip grid
        left_chunks[1],                  // [1] transport
        right_chunks[1],                 // [2] right content area (poly/route/hybrid)
        right_chunks[1],                 // [3] same — routing panel shares this slot
    ]);

    draw_clip_grid(f, app, left_chunks[0]);
    draw_transport_buttons(f, app, left_chunks[1]);
    draw_sidebar_tabs(f, app, right_chunks[0]);
    draw_sidebar_content(f, app, right_chunks[1]);
}

/// Draw the sidebar tab strip (top 2 rows of the right column).
fn draw_sidebar_tabs(f: &mut Frame, app: &App, area: Rect) {
    if area.height < 2 { return; }

    // 0=VISUALIZER (merged panels)  1=WAVE (oscilloscope/heartbeat)
    // 2=METR (per-pattern beat pulses)  3=SHAPES (time-signature polygons)
    // 4=CURVES (Lissajous/harmonograph from the active patterns' ratios).
    const LABELS: [&str; 5] = ["VIEW4", "WAVE", "METR", "SHAPES", "CURVES"];

    let tab_row = Rect::new(area.x, area.y, area.width, 1);
    let sep_row = Rect::new(area.x, area.y + 1, area.width, 1);

    let mut x = area.x;
    let mut spans: Vec<Span> = Vec::new();
    let mut tab_rects = [ratatui::layout::Rect::default(); 5];

    // Render in the user's customised order; `tab_rects[i]` maps a screen slot to
    // the logical tab id at that slot for hit-testing.
    for i in 0..LABELS.len() {
        let id = app.sidebar_tab_order[i] as usize;
        let label = LABELS[id];
        let w = label.len() as u16 + 2; // " LABEL "
        tab_rects[i] = Rect::new(x, area.y, w, 1);

        let active = app.sidebar_tab == id as u8;
        let style = if active {
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(HEADER)
        };
        spans.push(Span::styled(format!(" {label} "), style));
        x += w;

        if i + 1 < LABELS.len() {
            spans.push(Span::styled("│", Style::default().fg(BORDER)));
            x += 1;
        }
    }

    // Pad the rest of the row with the panel background.
    if x < area.x + area.width {
        spans.push(Span::styled(
            " ".repeat((area.x + area.width - x) as usize),
            Style::default().bg(PANEL),
        ));
    }

    app.sidebar_tab_rects.set(tab_rects);

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(PANEL)),
        tab_row,
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(BORDER),
        ))
        .style(Style::default().bg(PANEL)),
        sep_row,
    );
}

/// Single merged VISUALIZER tab: polymeter + hybrid view stacked, with a row of
/// action buttons (CLIP / CHANGE SOURCE / CHANGE BANK·PRESET) at the bottom.
/// The full source editor now lives in the TRACKER/P.ROLL view.
fn draw_sidebar_content(f: &mut Frame, app: &App, area: Rect) {
    // Matrix sidebar is now just the visualizer; the source/CLIP actions moved
    // to the PATTERN → SOURCE section.
    //
    // POLYMETER VISUALIZER and TRACKER MONITOR are swapped: TRACKER MONITOR now
    // occupies the top slot and POLYMETER VISUALIZER sits in the hybrid stack
    // where TRACKER MONITOR used to be. ACTIVE PATTERNS and VOICE ACTIVITY keep
    // their relative positions.
    match app.sidebar_tab {
        1 => { draw_waveform_viz(f, app, area); return; }
        2 => { draw_metr_viz(f, app, area); return; }
        3 => { draw_polyshape_viz(f, app, area); return; }
        4 => { draw_harmonograph_viz(f, app, area); return; }
        _ => {}
    }
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45), // tracker monitor (was polymeter)
            Constraint::Length(8),      // active patterns
            Constraint::Min(6),         // polymeter (was tracker monitor)
            Constraint::Length(7),      // voice/channel activity
        ])
        .split(area);
    draw_hv_tracker_monitor(f, app, vchunks[0]);
    draw_hv_active_patterns(f, app, vchunks[1]);
    draw_polymeter(f, app, vchunks[2]);
    draw_hv_voice_activity(f, app, vchunks[3]);
}

// ─── New visualizer tabs (WAVE / METR / SHAPES) ──────────────────────────────

/// Blend toward white by `t` (0..1) — used for decaying beat-onset flash.
fn flash(base: (u8, u8, u8), t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let f = |c: u8| (c as f32 + (255.0 - c as f32) * t) as u8;
    Color::Rgb(f(base.0), f(base.1), f(base.2))
}

/// Continuous beat position within a bar of `n` beats, plus the freshly-passed
/// vertex/beat and a 0..1 onset "freshness" that decays to 0 before the next
/// beat. Driven by `transport_beat` (smooth, sub-step) so motion is fluid.
/// Freshness is 0 when stopped (no flashing).
fn beat_phase(beat: f64, n: usize, playing: bool) -> (f32, usize, f32) {
    let n = n.max(1);
    let pos = (beat.rem_euclid(n as f64)) as f32; // 0..n
    let cur = (pos.floor() as usize) % n;
    let frac = pos - pos.floor();
    // Sharp attack, quick decay: (1-frac)^2 gives a punchy onset flash.
    let fresh = if playing { (1.0 - frac).powi(2) } else { 0.0 };
    (pos, cur, fresh)
}

/// Collect patterns assigned to matrix cells, in grid order, de-duplicated.
fn assigned_patterns<'a>(
    proj: &'a seqterm_core::Project,
    rows: usize,
    cols: usize,
    active_only: bool,
) -> Vec<(String, &'a seqterm_core::Pattern)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in 0..rows {
        let row_key = ((b'A' + row as u8) as char).to_string();
        if let Some(slots) = proj.matrix.get(&row_key) {
            for col in 0..cols.min(slots.len()) {
                if let Some(Some(clip)) = slots.get(col) {
                    // `active_only`: only enabled clips (matches the scheduler's
                    // playback gate) so SHAPES shows figures of active patterns.
                    if active_only && !clip.enabled {
                        continue;
                    }
                    if let Some(pk) = &clip.pattern_key {
                        if seen.insert(pk.clone()) {
                            if let Some(pat) = proj.patterns.get(pk) {
                                out.push((pk.clone(), pat));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

// ─── Braille sub-cell canvas (2×4 dots/cell → 8× the resolution) ─────────────

const SHAPE_COLORS: [(u8, u8, u8); 6] = [
    (99, 179, 237), (56, 200, 100), (240, 180, 40),
    (220, 130, 200), (240, 136, 62), (120, 200, 220),
];

/// High-resolution drawing surface: each terminal cell holds a 2×4 grid of
/// braille dots, so the effective canvas is `width*2 × height*4` pixels.
struct Canvas {
    cw: usize,
    ch: usize,
    pw: usize,
    ph: usize,
    bits: Vec<u8>,
    col: Vec<Color>,
}

impl Canvas {
    fn new(cw: usize, ch: usize) -> Self {
        Canvas { cw, ch, pw: cw * 2, ph: ch * 4, bits: vec![0; cw * ch], col: vec![HV_DIM; cw * ch] }
    }
    // Braille dot bit for sub-cell position (x%2, y%4).
    #[inline]
    fn dot_bit(x: usize, y: usize) -> u8 {
        const B: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
        B[y % 4][x % 2]
    }
    fn set(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || x as usize >= self.pw || y as usize >= self.ph { return; }
        let (x, y) = (x as usize, y as usize);
        let i = (y / 4) * self.cw + (x / 2);
        self.bits[i] |= Self::dot_bit(x, y);
        self.col[i] = color;
    }
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: Color) {
        let n = (x1 - x0).abs().max((y1 - y0).abs()).max(1.0) as usize;
        for s in 0..=n {
            let t = s as f32 / n as f32;
            self.set((x0 + (x1 - x0) * t).round() as i32, (y0 + (y1 - y0) * t).round() as i32, color);
        }
    }
    /// 2×2-dot blob centred near (x,y) so nodes/dots read clearly.
    fn blob(&mut self, x: f32, y: f32, color: Color) {
        let (xi, yi) = (x.round() as i32, y.round() as i32);
        for dy in -1..=1 { for dx in -1..=1 { self.set(xi + dx, yi + dy, color); } }
    }
    fn render(&self, f: &mut Frame, area: Rect) {
        for cy in 0..self.ch.min(area.height as usize) {
            let spans: Vec<Span> = (0..self.cw.min(area.width as usize)).map(|cx| {
                let i = cy * self.cw + cx;
                let b = self.bits[i];
                let chr = if b == 0 { ' ' } else { char::from_u32(0x2800 + b as u32).unwrap_or(' ') };
                Span::styled(chr.to_string(), Style::default().fg(self.col[i]))
            }).collect();
            f.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().bg(PANEL)),
                Rect::new(area.x, area.y + cy as u16, area.width, 1),
            );
        }
    }
}

fn viz_block<'a>(title: &'a str, active: bool) -> Block<'a> {
    Block::default()
        .title(title)
        .title_style(Style::default().fg(HEADER))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if active { Color::Yellow } else { BORDER }))
        .style(Style::default().bg(PANEL))
}

/// Catmull-Rom spline interpolation of a value through 4 control points (p1→p2,
/// parameter t in 0..1). Used to smooth the band profile into organic curves.
#[inline]
fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

// ─── WAVE tab tuning knobs ───────────────────────────────────────────────────
/// Extra amplitude multiplier on top of per-buffer auto-normalisation. Raise for
/// taller ridges, lower if peaks clip flat against the horizon.
const WAVE_GAIN: f32 = 0.95;
/// Perceptual amplitude curve (1.0 = linear, <1 lifts quiet detail).
const WAVE_AMP_CURVE: f32 = 0.6;
/// Depth fog strength (0 = no fade, 1 = back ridges go black).
const WAVE_FOG: f32 = 0.65;
/// Catmull-Rom subdivisions per band segment (higher = smoother, costlier).
const WAVE_STEPS: usize = 4;
/// Beat-reaction ridge pump and horizontal shear for the tilted-camera mode.
const WAVE_BEAT_PUMP: f32 = 0.8;
const WAVE_TILT: f32 = 0.30;
/// Selectable WAVE line colours (cycled with `c`). Index 0 = classic white.
const WAVE_COLORS: [(u8, u8, u8); 5] = [
    (235, 60, 50),   // red (default)
    (235, 235, 235), // white
    (240, 180, 40),  // amber
    (40, 200, 255),  // cyan
    (80, 230, 120),  // green
];

/// Neon amplitude → blue glow ramp (deep blue → bright blue → white-hot peaks),
/// dimmed by `bright`.
fn neon_color(a: f32, bright: f32) -> Color {
    let a = a.clamp(0.0, 1.0);
    // Blue core that brightens with amplitude; hot peaks bloom toward white.
    let b = 120.0 + 135.0 * a;            // 120 → 255
    let w = (a - 0.6).max(0.0) / 0.4 * 220.0; // peaks add red+green → white-hot
    let (r, g) = (10.0 + w, 10.0 + w * 1.3);
    let k = bright.clamp(0.0, 1.0);
    Color::Rgb((r * k).min(255.0) as u8, (g * k).min(255.0) as u8, (b * k).min(255.0) as u8)
}

/// WAVE tab — Joy Division "Unknown Pleasures" evolved into a live 3D road of sound.
/// Each frame's FFT band profile is one ridge; ridges recede in perspective toward a
/// horizon (narrower, bunched, fogged), with hidden-line removal so nearer ridges
/// occlude farther ones — a tunnel of waveforms scrolling into the distance. Band
/// amplitudes are Catmull-Rom smoothed. Modes: neon colour, tilted camera, beat pump.
fn draw_waveform_viz(f: &mut Frame, app: &App, area: Rect) {
    let active = app.matrix_section == 2;
    let mut title = String::from(" WAVE ");
    if app.wave_neon { title.push_str("· NEON "); }
    if app.wave_tilt { title.push_str("· TILT "); }
    if app.wave_beat { title.push_str("· BEAT "); }
    let block = viz_block(&title, active);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 4 || inner.height < 4 { return; }

    let plot = Rect::new(inner.x, inner.y, inner.width, inner.height - 1);
    let mut cv = Canvas::new(plot.width as usize, plot.height as usize);
    let ph = cv.ph as f32;
    let pw = cv.pw as f32;
    let center = pw * 0.5;
    let beat = if app.wave_beat { app.wave_beat_env } else { 0.0 };

    let hist = &app.wave_history;
    let rows = hist.len();
    if rows >= 2 {
        let top_margin = 2.0;
        let bottom = ph - 1.5;
        let span = (bottom - top_margin).max(1.0);
        let base_ridge = (span / rows as f32 * 9.0).max(4.0);
        // Auto-gain: normalise to the loudest band in the buffer so quiet signals
        // still fill the road. powf below gives perceptual (not linear) height.
        let peak = hist.iter().flat_map(|r| r.iter()).cloned().fold(1e-3, f32::max);
        let norm = WAVE_GAIN / peak;
        // Per screen-column silhouette: a back ridge shows only where it pokes
        // above every nearer ridge already drawn (hidden-line removal).
        let mut floor = vec![ph; cv.pw];
        // Reusable per-row scratch: amplitude, screen-x, screen-y for each band.
        // Precomputed once per row so the costly `powf` runs n times (not n×STEPS),
        // keeping the WAVE render cheap enough not to starve the audio scheduler.
        let n_max = hist.iter().map(|r| r.len()).max().unwrap_or(0).max(2);
        let mut av = vec![0f32; n_max];
        let mut xs = vec![0f32; n_max];
        let mut ys = vec![0f32; n_max];
        let palette = WAVE_COLORS[(app.wave_color as usize).min(4)];
        // r=0 newest (front/bottom) … r=rows-1 oldest (back/top). Draw front→back.
        for r in 0..rows {
            let row = &hist[r];
            let n = row.len();
            if n < 2 { continue; }
            // Depth: 0 at the front (newest, r=0), 1 at the far back (oldest).
            let d = r as f32 / (rows - 1) as f32;
            // Perspective scale: rows narrow toward centre as they recede.
            let scale_z = 1.0 - 0.62 * d;
            // Bunch rows toward the horizon (non-linear spacing = pseudo-3D depth).
            let base_y = bottom - d.powf(0.62) * span;
            // Beat pump: front ridges swell on onsets.
            let ridge_h = base_ridge * scale_z * (1.0 + beat * WAVE_BEAT_PUMP * (1.0 - d));
            // Tilted camera: shear the vanishing point sideways with depth.
            let cx = center + if app.wave_tilt { (d - 0.5) * pw * WAVE_TILT } else { 0.0 };
            // Depth fog + beat flash on the front rows.
            let bright = ((1.0 - WAVE_FOG * d) * (1.0 + beat * 0.5 * (1.0 - d))).min(1.0);
            // Precompute control points for this row (one powf per band).
            let inv = 1.0 / (n - 1) as f32;
            for i in 0..n {
                let a = (row[i].max(0.0) * norm).min(1.0).powf(WAVE_AMP_CURVE);
                av[i] = a;
                xs[i] = cx + (i as f32 * inv - 0.5) * pw * scale_z;
                ys[i] = base_y - a * ridge_h;
            }
            // Non-neon line colour is constant across the row (depends only on fog).
            let row_col = Color::Rgb((palette.0 as f32 * bright) as u8,
                                     (palette.1 as f32 * bright) as u8,
                                     (palette.2 as f32 * bright) as u8);
            // Catmull-Rom across the band profile, stroked with hidden-line removal.
            let mut prev: Option<(f32, f32)> = None;
            for i in 0..n - 1 {
                let (y0, y1, y2, y3) =
                    (ys[i.saturating_sub(1)], ys[i], ys[i + 1], ys[(i + 2).min(n - 1)]);
                let (x0, dx) = (xs[i], xs[i + 1] - xs[i]);
                let (a0, da) = (av[i], av[i + 1] - av[i]);
                for s in 0..WAVE_STEPS {
                    let t = s as f32 / WAVE_STEPS as f32;
                    let x = x0 + dx * t;
                    let y = catmull_rom(y0, y1, y2, y3, t);
                    let xi = x.round() as i32;
                    if xi < 0 || xi >= cv.pw as i32 { prev = None; continue; }
                    if y < floor[xi as usize] {
                        let col = if app.wave_neon { neon_color(a0 + da * t, bright) } else { row_col };
                        if let Some((pxp, pyp)) = prev {
                            cv.line(pxp, pyp, x, y, col);
                        } else {
                            cv.set(xi, y as i32, col);
                        }
                        floor[xi as usize] = y;
                        prev = Some((x, y));
                    } else {
                        prev = None; // occluded; break the stroke
                    }
                }
            }
        }
    }
    cv.render(f, plot);

    // Footer: live level readout.
    let lvl = ((app.audio_master_rms[0] + app.audio_master_rms[1]) * 0.5).clamp(0.0, 1.0);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  ▮ {:>3}%   {}   c:colour n:neon t:tilt b:beat  ‹›:move tab",
                (lvl * 100.0) as u32,
                if app.playing { "▶ live" } else { "■ idle" }),
            Style::default().fg(Color::DarkGray),
        ))).style(Style::default().bg(PANEL)),
        Rect::new(inner.x, inner.y + plot.height, inner.width, 1),
    );
}

/// Pick the pattern under the matrix cursor, falling back to the first assigned.
fn focused_pattern<'a>(
    app: &App,
    proj: &'a seqterm_core::Project,
) -> Option<(String, &'a seqterm_core::Pattern)> {
    let (r, c) = app.matrix_state.cursor;
    let row_key = ((b'A' + r as u8) as char).to_string();
    let cur = proj.matrix.get(&row_key)
        .and_then(|row| row.get(c))
        .and_then(|s| s.as_ref())
        .and_then(|clip| clip.pattern_key.as_ref())
        .and_then(|k| proj.patterns.get(k).map(|p| (k.clone(), p)));
    cur.or_else(|| assigned_patterns(proj, app.matrix_rows, app.matrix_cols, false).into_iter().next())
}

/// METR tab — pulse-against-pulse subdivision tree: bar → beats (time-signature
/// numerator) → sub-pulses (Pat len ÷ beats). The branch the playhead is on
/// flashes on each onset, so you watch the pulse travel down the tree.
fn draw_metr_viz(f: &mut Frame, app: &App, area: Rect) {
    let active = app.matrix_section == 2;
    let block = viz_block(" METR · SUBDIVISION TREE ", active);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 8 || inner.height < 5 { return; }

    let proj = app.project.lock();
    let Some((key, pat)) = focused_pattern(app, &proj) else {
        f.render_widget(
            Paragraph::new(Span::styled("  no patterns assigned to matrix",
                Style::default().fg(Color::DarkGray))).style(Style::default().bg(PANEL)),
            inner);
        return;
    };

    let beats = pat.time_sig_num.max(1) as usize;
    // Sub-pulses per beat from Pat len (length ÷ beats), capped for legibility.
    let spb = (pat.length / beats.max(1)).clamp(1, 8);
    let beat = app.transport_beat;
    let (pos, cur_beat, fresh) = beat_phase(beat, beats, app.playing);
    let sub_pos = (pos - pos.floor()) * spb as f32; // 0..spb within current beat
    let cur_sub = (sub_pos.floor() as usize).min(spb.saturating_sub(1));

    let plot = Rect::new(inner.x, inner.y, inner.width, inner.height - 1);
    let mut cv = Canvas::new(plot.width as usize, plot.height as usize);
    let (pw, ph) = (cv.pw as f32, cv.ph as f32);

    let y_root = 3.0;
    let y_beat = ph * 0.42;
    let y_sub = ph - 3.0;
    let root_x = pw / 2.0;

    let beat_x = |b: usize| (b as f32 + 0.5) * pw / beats as f32;
    let sub_x = |b: usize, s: usize| {
        let span = pw / beats as f32;
        b as f32 * span + (s as f32 + 0.5) * span / spb as f32
    };

    let dim = Color::Rgb(70, 90, 120);
    // Root node.
    cv.blob(root_x, y_root, Color::Rgb(150, 165, 195));
    // Branches.
    for b in 0..beats {
        let bx = beat_x(b);
        let active_beat = b == cur_beat;
        let beat_col = if active_beat && fresh > 0.04 {
            flash((240, 180, 40), fresh)
        } else if b == 0 { HV_AMBER } else { dim };
        cv.line(root_x, y_root, bx, y_beat, if active_beat { beat_col } else { dim });
        cv.blob(bx, y_beat, beat_col);

        for s in 0..spb {
            let sx = sub_x(b, s);
            let active_sub = active_beat && s == cur_sub;
            // Light the just-passed sub-pulse; brightness decays within the beat.
            let sub_fresh = if active_sub { (1.0 - (sub_pos - sub_pos.floor())).powi(2) } else { 0.0 };
            let sub_col = if active_sub && app.playing && sub_fresh > 0.04 {
                flash((120, 200, 160), sub_fresh)
            } else {
                Color::Rgb(80, 100, 130)
            };
            cv.line(bx, y_beat, sx, y_sub, if active_sub { sub_col } else { dim });
        }
    }
    cv.render(f, plot);

    // Leaves of the tree rendered as `|` characters, one per sub-pulse, aligned
    // under each branch. Active pulse flashes; downbeats amber, rest dim.
    {
        let w = plot.width as usize;
        let mut chars = vec![' '; w];
        let mut styles = vec![Style::default().fg(dim); w];
        for b in 0..beats {
            for s in 0..spb {
                let col = (sub_x(b, s) / 2.0) as usize;
                if col < w {
                    chars[col] = '|';
                    let active_sub = b == cur_beat && s == cur_sub && app.playing;
                    styles[col] = if active_sub {
                        Style::default().fg(Color::Rgb(120, 220, 170)).add_modifier(Modifier::BOLD)
                    } else if s == 0 {
                        Style::default().fg(HV_AMBER)
                    } else {
                        Style::default().fg(Color::Rgb(110, 140, 175))
                    };
                }
            }
        }
        let spans: Vec<Span> = chars.into_iter().zip(styles)
            .map(|(c, st)| Span::styled(c.to_string(), st)).collect();
        f.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(PANEL)),
            Rect::new(inner.x, inner.y + plot.height.saturating_sub(1), inner.width, 1));
    }

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {} · {}/{} · len {} → {}×{} pulses  (cursor selects)",
                &key[..key.len().min(6)], pat.time_sig_num, pat.time_sig_den,
                pat.length, beats, spb),
            Style::default().fg(Color::DarkGray),
        ))).style(Style::default().bg(PANEL)),
        Rect::new(inner.x, inner.y + plot.height, inner.width, 1),
    );
}

/// SHAPES tab — superimposed polygons (chambercode "polyshapr"): every assigned
/// pattern is drawn concentrically on one shared centre, sides = time-signature
/// numerator, the perimeter subdivided into Pat-len step ticks. A dot orbits
/// each polygon; vertices flash on the beat.
fn draw_polyshape_viz(f: &mut Frame, app: &App, area: Rect) {
    let active = app.matrix_section == 2;
    let block = viz_block(" SHAPES · SUPERIMPOSED ", active);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 10 || inner.height < 6 { return; }

    let proj = app.project.lock();
    let pats = assigned_patterns(&proj, app.matrix_rows, app.matrix_cols, true);
    if pats.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("  no active patterns",
                Style::default().fg(Color::DarkGray))).style(Style::default().bg(PANEL)),
            inner);
        return;
    }
    let beat = app.transport_beat;

    // Reserve a couple of footer rows for the legend.
    let legend_rows = 2u16.min(inner.height.saturating_sub(4));
    let plot = Rect::new(inner.x, inner.y, inner.width, inner.height - legend_rows);
    let mut cv = Canvas::new(plot.width as usize, plot.height as usize);
    let (cx, cy) = (cv.pw as f32 / 2.0, cv.ph as f32 / 2.0);
    let r_max = (cv.pw as f32 / 2.0).min(cv.ph as f32 / 2.0) - 1.0;

    let count = pats.len();
    for (i, (_key, pat)) in pats.iter().enumerate() {
        let n = pat.time_sig_num.max(1).min(16) as usize;
        // Concentric radii: each pattern gets its own ring.
        let radius = r_max * (0.35 + 0.65 * (i + 1) as f32 / count as f32);
        let base = SHAPE_COLORS[i % SHAPE_COLORS.len()];
        let (pos, cur_beat, fresh) = beat_phase(beat, n, app.playing);
        // Independent spin: one full revolution per pattern cycle, so polygons with
        // more sides turn slower. All pass the top (12 o'clock) at their cycle start,
        // syncing whenever their downbeats coincide (shared common points).
        let rot = std::f32::consts::TAU * (pos / n as f32);
        let vert = |k: usize| -> (f32, f32) {
            let a = -std::f32::consts::FRAC_PI_2 + rot + std::f32::consts::TAU * (k as f32 / n as f32);
            (cx + radius * a.cos(), cy + radius * a.sin())
        };
        // Edges.
        for k in 0..n {
            let (x0, y0) = vert(k);
            let (x1, y1) = vert((k + 1) % n);
            cv.line(x0, y0, x1, y1, Color::Rgb(base.0 / 3 + 30, base.1 / 3 + 30, base.2 / 3 + 30));
        }
        // Pat-len subdivision ticks around the perimeter.
        let len = pat.length.max(n);
        for s in 0..len {
            let fseg = s as f32 / len as f32 * n as f32;
            let k0 = fseg.floor() as usize % n;
            let frac = fseg - fseg.floor();
            let (ax, ay) = vert(k0);
            let (bx, by) = vert((k0 + 1) % n);
            cv.set((ax + (bx - ax) * frac) as i32, (ay + (by - ay) * frac) as i32,
                Color::Rgb(base.0 / 2, base.1 / 2, base.2 / 2));
        }
        // Vertices: downbeat + onset flash.
        for k in 0..n {
            let (vx, vy) = vert(k);
            let col = if k == cur_beat && fresh > 0.04 { flash(base, fresh) }
                else if k == 0 { flash(base, 0.25) }
                else { Color::Rgb(base.0, base.1, base.2) };
            cv.blob(vx, vy, col);
        }
        // The top vertex (12 o'clock) is the cycle's leading edge — mark it bright.
        let (tx, ty) = (cx, cy - radius);
        cv.blob(tx, ty, flash(base, 0.5));
    }
    cv.render(f, plot);

    // Legend: KEY n/d ×len per pattern, coloured.
    for (li, lr) in (0..legend_rows).enumerate() {
        let mut spans: Vec<Span> = Vec::new();
        for (i, (key, pat)) in pats.iter().enumerate() {
            if i % legend_rows.max(1) as usize != li { continue; }
            let c = SHAPE_COLORS[i % SHAPE_COLORS.len()];
            spans.push(Span::styled(
                format!(" {} {}/{}·{} ", &key[..key.len().min(4)], pat.time_sig_num, pat.time_sig_den, pat.length),
                Style::default().fg(Color::Rgb(c.0, c.1, c.2))));
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(PANEL)),
            Rect::new(inner.x, inner.y + plot.height + lr, inner.width, 1));
    }
}

/// Set a glyph on the CURVES character grid (bounds-checked).
#[inline]
fn grid_put(g: &mut [(char, Color)], w: usize, h: usize, col: i32, row: i32, ch: char, color: Color) {
    if col < 0 || row < 0 || col as usize >= w || row as usize >= h { return; }
    g[row as usize * w + col as usize] = (ch, color);
}

/// CURVES tab — scrolling melodic SCORE for the active patterns. All patterns
/// share one vertical pitch axis (so their lines sit close and overlap), and the
/// score scrolls horizontally past a fixed playhead column: it advances left as
/// playback moves. Each note transition is drawn with slope glyphs (`/ \ ─ |`) so
/// the melodic motion reads clearly; a `●` marks every note and lights up bright
/// the moment the playhead crosses it.
fn draw_harmonograph_viz(f: &mut Frame, app: &App, area: Rect) {
    let active = app.matrix_section == 2;
    let block = viz_block(" CURVES · SCORE ", active);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 10 || inner.height < 6 { return; }

    let proj = app.project.lock();
    let pats = assigned_patterns(&proj, app.matrix_rows, app.matrix_cols, true);
    if pats.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("  no active patterns",
                Style::default().fg(Color::DarkGray))).style(Style::default().bg(PANEL)),
            inner);
        return;
    }

    // Global pitch range across ALL active patterns → one shared vertical axis.
    let (mut lo, mut hi) = (u8::MAX, 0u8);
    for (_k, pat) in &pats {
        for s in 0..pat.length {
            if let Some(m) = pat.steps.get(s).and_then(|note| note.to_midi()) {
                lo = lo.min(m);
                hi = hi.max(m);
            }
        }
    }
    if hi == 0 {
        f.render_widget(
            Paragraph::new(Span::styled("  active patterns have no notes",
                Style::default().fg(Color::DarkGray))).style(Style::default().bg(PANEL)),
            inner);
        return;
    }

    let legend_rows = 1u16.min(inner.height.saturating_sub(4));
    let plot = Rect::new(inner.x, inner.y, inner.width, inner.height - legend_rows);
    let w = plot.width as usize;
    let h = plot.height as usize;
    let mut g: Vec<(char, Color)> = vec![(' ', PANEL); w * h];

    let span = (hi.saturating_sub(lo)).max(1) as f64;
    let row_of = |m: u8| -> f64 {
        // High pitch → top row. Small margins so peaks/troughs stay on-screen.
        let t = (m as f64 - lo as f64) / span;
        (h as f64 - 1.0) * (1.0 - t) * 0.92 + (h as f64 - 1.0) * 0.04
    };

    // Fixed playhead column ~2/3 across; the score scrolls under it.
    let px = (w as f64 * 0.66) as i32;
    const VIEW_BEATS: f64 = 8.0;
    let bpc = VIEW_BEATS / w as f64; // beats per column
    let ph_beat = app.transport_beat;
    let win_lo = ph_beat - px as f64 * bpc;
    let win_hi = ph_beat + (w as i32 - px) as f64 * bpc;
    let col_of = |beat: f64| -> f64 { px as f64 + (beat - ph_beat) / bpc };

    // Playhead vertical guide (drawn first so notes/line overlay it).
    let cursor_col = Color::Rgb(90, 95, 120);
    for row in 0..h as i32 {
        grid_put(&mut g, w, h, px, row, '│', cursor_col);
    }

    for (i, (_key, pat)) in pats.iter().enumerate() {
        let step_b = pat.step_beats().to_f64();
        if step_b <= 0.0 { continue; }
        let len = pat.length.max(1);
        let cycle = len as f64 * step_b;
        let base = SHAPE_COLORS[i % SHAPE_COLORS.len()];
        let bcol = Color::Rgb(base.0, base.1, base.2);

        // Note occurrences (beat, midi) visible in the window — the pattern loops,
        // so the line is continuous in both directions.
        let mut ev: Vec<(f64, u8)> = Vec::new();
        for s in 0..len {
            if let Some(m) = pat.steps.get(s).and_then(|note| note.to_midi()) {
                let b0 = s as f64 * step_b;
                let kmin = ((win_lo - b0) / cycle).ceil() as i64;
                let kmax = ((win_hi - b0) / cycle).floor() as i64;
                for k in kmin..=kmax { ev.push((b0 + k as f64 * cycle, m)); }
            }
        }
        if ev.is_empty() { continue; }
        ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        // Connect consecutive notes with slope glyphs (/ \ ─ |).
        for win in ev.windows(2) {
            let (b0, m0) = win[0];
            let (b1, m1) = win[1];
            let (c0, r0) = (col_of(b0), row_of(m0));
            let (c1, r1) = (col_of(b1), row_of(m1));
            let cs = c0.round() as i32;
            let ce = c1.round() as i32;
            if ce <= cs { continue; }
            let mut prev_row = r0.round() as i32;
            for c in cs..=ce {
                let t = (c - cs) as f64 / (ce - cs).max(1) as f64;
                let rf = r0 + (r1 - r0) * t;
                let rr = rf.round() as i32;
                let drow = rr - prev_row;
                let glyph = if drow <= -1 { '/' } else if drow >= 1 { '\\' } else { '─' };
                // Bridge steep jumps with verticals so the line stays connected.
                if drow.abs() > 1 {
                    let (a, b) = (prev_row.min(rr), prev_row.max(rr));
                    for r in a..=b { grid_put(&mut g, w, h, c, r, '│', bcol); }
                } else {
                    grid_put(&mut g, w, h, c, rr, glyph, bcol);
                }
                prev_row = rr;
            }
        }

        // Note circles; light up bright when the playhead is crossing them.
        for &(b, m) in &ev {
            let c = col_of(b).round() as i32;
            let r = row_of(m).round() as i32;
            let under = (b - ph_beat).abs() < bpc; // within one column of the cursor
            let col = if under && app.playing {
                flash(base, 1.0)
            } else if under {
                Color::White
            } else {
                bcol
            };
            grid_put(&mut g, w, h, c, r, if under { '◉' } else { '●' }, col);
        }
    }

    // Blit the grid.
    for row in 0..h {
        let spans: Vec<Span> = (0..w).map(|c| {
            let (ch, col) = g[row * w + c];
            Span::styled(ch.to_string(), Style::default().fg(col).bg(PANEL))
        }).collect();
        f.render_widget(Paragraph::new(Line::from(spans)),
            Rect::new(plot.x, plot.y + row as u16, plot.width, 1));
    }

    // Legend: KEY ♪notes per pattern, coloured.
    for (li, lr) in (0..legend_rows).enumerate() {
        let mut spans: Vec<Span> = Vec::new();
        for (i, (key, pat)) in pats.iter().enumerate() {
            if i % legend_rows.max(1) as usize != li { continue; }
            let c = SHAPE_COLORS[i % SHAPE_COLORS.len()];
            let notes = (0..pat.length).filter(|&s| pat.steps.get(s).map(|note| !note.is_empty()).unwrap_or(false)).count();
            spans.push(Span::styled(
                format!(" {} ♪{} ", &key[..key.len().min(4)], notes),
                Style::default().fg(Color::Rgb(c.0, c.1, c.2))));
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(PANEL)),
            Rect::new(inner.x, inner.y + plot.height + lr, inner.width, 1));
    }
}

/// Three selectable action buttons (CLIP / CHANGE SOURCE / CHANGE BANK·PRESET).
/// Compact combined SOURCE panel for the TRACKER/P.ROLL → SOURCE section:
/// a one-line current-source summary followed by the 3 action buttons
/// (CLIP / CHANGE SOURCE / CHANGE BANK·PRESET) — one bordered panel instead of
/// two, so it takes much less vertical space. `app.matrix_action_cursor` selects
/// the focused button; edits the clip at the matrix cursor.
pub fn draw_tracker_source_panel(f: &mut Frame, app: &App, area: Rect, active: bool) {
    let (row, col) = app.matrix_state.cursor;
    let row_key = ((b'A' + row as u8) as char).to_string();

    // Current-source summary (single line). `midi_ch` is Some(channel 1-16) for
    // MIDI-driven sources (MIDI/SF2/Plugin), None for audio (no channel concept).
    let (badge, badge_col, detail, midi_ch) = {
        let proj = app.project.lock();
        let clip = proj.matrix.get(&row_key).and_then(|r| r.get(col)).and_then(|c| c.as_ref());
        let ch = clip.map(|c| c.midi_channel);
        match clip.map(|c| &c.source) {
            Some(PatternSource::Midi) | None => (
                "⇄ MIDI", Color::Rgb(99, 179, 237),
                clip.and_then(|c| c.midi_out.as_deref()).unwrap_or("(no port)").to_string(),
                ch,
            ),
            Some(PatternSource::Sf2 { bank, preset, preset_name, .. }) => (
                "♪ SF2", Color::Rgb(56, 200, 100),
                format!("B{} P{} {}", bank, preset, preset_name.chars().take(14).collect::<String>()),
                ch,
            ),
            Some(PatternSource::AudioFile { path, .. }) => (
                "▶ AUD", Color::Rgb(240, 136, 62),
                path.file_name().and_then(|n| n.to_str()).unwrap_or("?").chars().take(18).collect::<String>(),
                None,
            ),
            Some(PatternSource::Plugin { name, format, .. }) => (
                "◇ SYNTH", Color::Rgb(190, 140, 230),
                format!("{} [{}]", name.chars().take(14).collect::<String>(), format),
                ch,
            ),
        }
    };

    let block = Block::default()
        .title(format!(" SOURCE · {}{} ", row_key, col + 1))
        .title_style(Style::default().fg(HEADER))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if active { Color::Yellow } else { BORDER }))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 { return; }
    let max_x = inner.x + inner.width;
    let max_y = inner.y + inner.height;

    // Current-source summary line.
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {} ", badge),
                Style::default().fg(Color::Black).bg(badge_col).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {}", detail), Style::default().fg(Color::Rgb(170, 185, 215))),
        ])).style(Style::default().bg(PANEL)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // MIDI-channel stepper, right-aligned on the summary line: "◂ CH n ▸".
    // The two arrows are individually clickable; keyboard `[`/`]` also adjust it.
    // Skipped for audio sources (no MIDI channel).
    let mut chan_rects = [Rect::default(); 2];
    if let Some(ch) = midi_ch {
        let label = format!("CH {ch:>2}");
        // widths: "◂ " (2) + label + " ▸" (2)
        let total_w = 2 + label.chars().count() as u16 + 2;
        if inner.width > total_w + 2 {
            let cx = max_x - total_w; // right-aligned within the inner row
            let y = inner.y;
            let focused = active;
            let arrow_style = Style::default()
                .fg(if focused { Color::Yellow } else { Color::Rgb(150, 165, 195) })
                .bg(PANEL)
                .add_modifier(Modifier::BOLD);
            let lbl_style = Style::default()
                .fg(Color::Rgb(230, 230, 180)).bg(PANEL).add_modifier(Modifier::BOLD);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("◂ ", arrow_style),
                    Span::styled(label.clone(), lbl_style),
                    Span::styled(" ▸", arrow_style),
                ])).style(Style::default().bg(PANEL)),
                Rect::new(cx, y, total_w, 1),
            );
            // Clickable arrow regions: ◂ at cx (width 1), ▸ at the last column.
            chan_rects[0] = Rect::new(cx, y, 1, 1);
            chan_rects[1] = Rect::new(max_x - 1, y, 1, 1);
        }
    }
    app.source_chan_rects.set(chan_rects);

    // Action buttons as TRANSPORT-style boxes, two per row (2×2), flowing left to
    // right so the longer labels never overlap.
    let labels = ["▣ CLIP", "► CHANGE SOURCE", "◈ BANK/PRESET", "✎ EDIT"];
    let base_colors = [
        Color::Rgb(99, 179, 237),
        Color::Rgb(200, 180, 60),
        Color::Rgb(56, 200, 100),
        Color::Rgb(220, 130, 200),
    ];
    let cursor = app.matrix_action_cursor;
    let mut rects = [ratatui::layout::Rect::default(); 4];
    for grid_row in 0..2u16 {
        let y = inner.y + 1 + grid_row * 3; // row 0 = summary
        let mut x = inner.x;
        for gc in 0..2usize {
            let i = (grid_row as usize) * 2 + gc;
            let selected = active && cursor == i;
            let border = if selected { Color::Yellow } else { base_colors[i] };
            let face = if selected {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(base_colors[i]).bg(PANEL)
            };
            let w = crate::views::tracker::fx_button_box(f, x, y, max_x, max_y, labels[i], border, face);
            if w == 0 { break; }
            rects[i] = Rect::new(x, y, w, 3);
            x += w + 2; // gap between boxes
        }
    }
    app.matrix_action_btn_rects.set(rects);

    // ── Synth source: parameter knobs ─────────────────────────────────────────
    // When the clip's source is a plugin synth, show its general parameters as
    // editable knobs below the action buttons.
    let clip_key = format!("{}{}", row_key, col);
    if let Some(&rid) = app.synth_instances.get(&clip_key) {
        let pcount = app.plugin_registry.param_count(rid).min(8) as usize;
        let knob_top = inner.y + 7;
        if pcount > 0 && knob_top + 2 <= max_y {
            // Section header.
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    " PARAMETERS  (Tab=focus · ←→=adjust · scroll/drag)",
                    Style::default().fg(HEADER),
                ))).style(Style::default().bg(PANEL)),
                Rect::new(inner.x, knob_top, inner.width, 1),
            );
            let cell_w: u16 = 18;
            let per_row = ((inner.width / cell_w).max(1)) as usize;
            let mut knob_rects = [Rect::default(); 8];
            #[allow(clippy::needless_range_loop)] // p is also the parameter id
            for p in 0..pcount {
                let gr = (p / per_row) as u16;
                let gc = (p % per_row) as u16;
                let kx = inner.x + gc * cell_w;
                let ky = knob_top + 1 + gr * 2;
                if ky + 1 >= max_y || kx + cell_w > max_x { break; }
                let rect = Rect::new(kx, ky, cell_w.min(max_x - kx), 2);
                knob_rects[p] = rect;
                let val = app.plugin_registry.get_param(rid, p as u32);
                let pname = app.plugin_registry.param_name(rid, p as u32);
                let pdisp = app.plugin_registry.param_display(rid, p as u32);
                let focused = active && app.source_focus_knobs && app.source_knob_cursor == p;
                let name_style = if focused {
                    Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(190, 140, 230))
                };
                let name: String = pname.chars().take(cell_w as usize - 2).collect();
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(format!(" {name}"), name_style)))
                        .style(Style::default().bg(PANEL)),
                    Rect::new(kx, ky, rect.width, 1),
                );
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            format!(" {} ", crate::views::tracker::knob_indicator(val)),
                            Style::default().fg(if focused { Color::Yellow } else { Color::Rgb(120, 200, 160) }),
                        ),
                        Span::styled(
                            crate::views::tracker::knob_arc(val, 6),
                            Style::default().fg(Color::Rgb(120, 200, 160)),
                        ),
                        Span::styled(
                            format!(" {}", pdisp.chars().take(5).collect::<String>()),
                            Style::default().fg(Color::Rgb(170, 185, 215)),
                        ),
                    ])).style(Style::default().bg(PANEL)),
                    Rect::new(kx, ky + 1, rect.width, 1),
                );
            }
            app.source_knob_rects.set(knob_rects);
        }
    }
}

/// Drum pattern matrix: 16-pad × N-step interactive grid.
/// Retained for the dedicated drum workflow; no longer surfaced as a Matrix
/// sidebar tab (removed in favour of PANELS / HYBRID only).
#[allow(dead_code)]
fn draw_drum_panel(f: &mut Frame, app: &App, area: Rect) {
    const DRUM_NAMES: [&str; 16] = [
        "Kick", "Snare", "CHH", "OHH", "FTom", "LTom", "LMTom", "HiTom",
        "Clap", "Crash", "Ride", "Splash", "FFlTom", "Stick", "RdBell", "Cowbl",
    ];
    const PAD_COLORS: [Color; 3] = [
        Color::Rgb(56, 200, 100),   // Kick/Snare/HH group
        Color::Rgb(80, 160, 240),   // Toms group
        Color::Rgb(200, 150, 60),   // Misc group
    ];

    let focused = app.matrix_section == 4;
    let (cursor_row, cursor_col) = app.matrix_state.cursor;
    let row_key = ((b'A' + cursor_row as u8) as char).to_string();
    let (drum_pad_cursor, step_cursor) = app.drum_cursor;
    let step_scroll = app.drum_step_scroll;

    let (drum_map, pattern_steps, pat_len) = {
        let proj = app.project.lock();
        let dm = proj.channels.iter()
            .find(|c| c.midi_port.as_deref() == Some(row_key.as_str()))
            .map(|c| c.drum_map)
            .unwrap_or(seqterm_core::GM_DRUM_MAP);
        let pat_key = proj.matrix.get(&row_key)
            .and_then(|r| r.get(cursor_col))
            .and_then(|s| s.as_ref())
            .and_then(|c| c.pattern_key.clone());
        let (steps, len) = if let Some(k) = pat_key {
            if let Some(p) = proj.patterns.get(&k) {
                (p.steps.clone(), p.length)
            } else { (vec![], 0) }
        } else { (vec![], 0) };
        (dm, steps, len)
    };

    let border_color = if focused { Color::Rgb(31, 111, 235) } else { BORDER };
    let block = Block::default()
        .title(format!(" DRUM  {}{}  {} steps  Tab=focus  Space=toggle  e=euclid  x=clear ",
            row_key, cursor_col + 1, pat_len))
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(PANEL));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 3 {
        return;
    }

    if pat_len == 0 {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No drum pattern. Select a clip with a pattern,",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "  then Tab here to edit steps.",
                    Style::default().fg(Color::DarkGray),
                )),
            ]).style(Style::default().bg(PANEL)),
            inner,
        );
        return;
    }

    const LABEL_W: usize = 6;
    let step_area_w = (inner.width as usize).saturating_sub(LABEL_W);
    let step_w = 2usize; // 2 chars per step cell: "■ " or "· "
    let vis_steps = (step_area_w / step_w).max(1).min(pat_len);

    // Clamp scroll so cursor is always visible.
    let scroll = {
        let sc = step_scroll;
        let sc = if step_cursor < sc { step_cursor } else { sc };
        let sc = if step_cursor >= sc + vis_steps { step_cursor + 1 - vis_steps } else { sc };
        sc.min(pat_len.saturating_sub(vis_steps))
    };

    let mut lines: Vec<Line> = Vec::new();

    // Header: step numbers.
    let mut hdr_spans: Vec<Span> = vec![
        Span::styled(format!("{:width$}", "PAD", width = LABEL_W), Style::default().fg(BORDER)),
    ];
    for s in scroll..(scroll + vis_steps).min(pat_len) {
        let num = (s % 32) + 1;
        let is_beat = s % 4 == 0;
        let is_cur = focused && s == step_cursor;
        let style = if is_cur {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if is_beat {
            Style::default().fg(Color::Rgb(120, 140, 180))
        } else {
            Style::default().fg(BORDER)
        };
        hdr_spans.push(Span::styled(format!("{:<2}", num), style));
    }
    lines.push(Line::from(hdr_spans));

    // Pad rows.
    let visible_pads = (inner.height as usize).saturating_sub(2); // -1 header -1 hint
    for pad in 0..16usize.min(visible_pads) {
        let midi_note = drum_map[pad];
        let note_name = seqterm_core::Note::from_midi(midi_note, 100)
            .map(|n| n.note)
            .unwrap_or_default();

        let is_sel_row = focused && pad == drum_pad_cursor;
        let pad_color = PAD_COLORS[if pad < 4 { 0 } else if pad < 8 { 1 } else { 2 }];
        let label_style = if is_sel_row {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(pad_color)
        };

        let lbl = format!("{:2}:{}", midi_note % 100, &DRUM_NAMES[pad][..3.min(DRUM_NAMES[pad].len())]);
        let mut spans: Vec<Span> = vec![
            Span::styled(format!("{:<width$}", lbl, width = LABEL_W), label_style),
        ];

        for s in scroll..(scroll + vis_steps).min(pat_len) {
            let step = pattern_steps.get(s);
            let hit = step.map(|n| {
                n.note == note_name || n.chord_notes.contains(&note_name)
            }).unwrap_or(false);

            let is_cursor_cell = focused && pad == drum_pad_cursor && s == step_cursor;
            let cell = if hit { "■ " } else { "· " };
            let style = if is_cursor_cell {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else if hit {
                Style::default().fg(pad_color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(BORDER)
            };
            spans.push(Span::styled(cell, style));
        }
        lines.push(Line::from(spans));
    }

    // Hint line.
    let hint = if focused {
        format!("  hjkl=navigate  Space/Enter=toggle  e=euclid  x=clear  Esc=exit  step {}/{}",
            step_cursor + 1, pat_len)
    } else {
        "  Tab to focus  |  select drum clip to edit".to_string()
    };
    lines.push(Line::from(Span::styled(hint, Style::default().fg(BORDER))));

    f.render_widget(Paragraph::new(lines).style(Style::default().bg(PANEL)), inner);
}

fn draw_clip_grid(f: &mut Frame, app: &App, area: Rect) {
    const ROW_LBL: usize = 3;

    let proj = app.project.lock();
    let (cursor_row, cursor_col) = app.matrix_state.cursor;
    let n_rows = app.matrix_rows;
    let n_cols = app.matrix_cols;

    // Horizontal column scroll.
    // content_w = space available for columns after row-label and borders.
    // Each column takes (1 separator + cell_w) chars; the formula below finds
    // cell_w when all n_cols fit, and falls back to a minimum when they don't.
    const MIN_CELL_W: usize = 5;
    let available_w = (area.width as usize).saturating_sub(2); // block borders
    let content_w = available_w.saturating_sub(ROW_LBL + 1);   // row-label + trailing │
    let cell_w_all = if n_cols > 0 { content_w / n_cols } else { MIN_CELL_W + 1 };
    // If all columns fit at ≥ MIN_CELL_W, show all; otherwise scroll.
    let vis_cols = if cell_w_all > MIN_CELL_W {
        n_cols   // every column fits — no scroll
    } else {
        (content_w / (MIN_CELL_W + 1)).max(1).min(n_cols)
    };
    let col_scroll = {
        let old = app.matrix_col_scroll.get();
        let s = if cursor_col < old { cursor_col }
                else if cursor_col >= old + vis_cols { cursor_col + 1 - vis_cols }
                else { old };
        s.min(n_cols.saturating_sub(vis_cols))
    };
    app.matrix_col_scroll.set(col_scroll);
    // Render only the visible column window.
    let col_range = col_scroll..(col_scroll + vis_cols).min(n_cols);
    let n_visible_cols = col_range.len();
    let grid_active = app.matrix_section == 0;
    let tracker_key = app.tracker_state.pattern_key.as_deref();

    // Responsive square cells.
    let available_h = (area.height as usize).saturating_sub(2);
    let max_cell_h = if n_rows == 0 { 10 } else {
        (available_h.saturating_sub(3) / n_rows).saturating_sub(1).max(1)
    };
    let cell_h = max_cell_h;
    // cell_w derived from content_w / n_visible_cols, respecting min width.
    let max_cell_w = if n_visible_cols == 0 { MIN_CELL_W } else {
        (content_w / n_visible_cols).saturating_sub(1).max(MIN_CELL_W)
    };
    let cell_w = (cell_h * 2).min(max_cell_w).max(MIN_CELL_W);
    let n_font_chars = (cell_w / 4).min(5);
    let n_font_rows = cell_h.saturating_sub(1).min(3);

    let mut lines: Vec<Line> = Vec::new();

    // ── Column header (with scroll indicator when needed) ────────────────────
    {
        let mut hdr: Vec<Span> = vec![Span::raw(" ".repeat(ROW_LBL))];
        // Left scroll arrow when not at first column.
        if col_scroll > 0 {
            hdr.insert(0, Span::styled("◄", Style::default().fg(ACCENT)));
        }
        for col in col_range.clone() {
            let label = format!("{:^width$}", format!("{:02}", col + 1), width = cell_w);
            hdr.push(Span::raw(" ")); // align with │ border
            hdr.push(Span::styled(
                label,
                Style::default().fg(HEADER).add_modifier(Modifier::BOLD),
            ));
        }
        hdr.push(Span::raw(" ")); // align with trailing │
        // Right scroll arrow when there are more columns to the right.
        if col_scroll + vis_cols < n_cols {
            hdr.push(Span::styled("►", Style::default().fg(ACCENT)));
        }
        lines.push(Line::from(hdr));
    }

    // ── Matrix rows ───────────────────────────────────────────────────────────
    let bdr_default = Color::Indexed(240);
    let hovered = app.hovered_matrix_cell.get();

    // Returns border color for a horizontal segment: yellow if adjacent to hovered cell.
    let h_color = |loop_row: usize, c: usize| -> Color {
        if hovered == Some((loop_row, c)) || hovered.map(|(hr, hc)| hr + 1 == loop_row && hc == c).unwrap_or(false) {
            Color::Yellow
        } else {
            bdr_default
        }
    };
    // Returns border color for a vertical bar: yellow if left or right edge of hovered cell.
    let v_color = |loop_row: usize, c: usize| -> Color {
        if hovered.map(|(hr, hc)| hr == loop_row && (hc == c || hc + 1 == c)).unwrap_or(false) {
            Color::Yellow
        } else {
            bdr_default
        }
    };

    let h_seg = "─".repeat(cell_w);

    // Rectangular selection region (inclusive), or None when nothing is selected
    // beyond the cursor cell.
    let sel_region: Option<(usize, usize, usize, usize)> =
        app.matrix_state.selection_anchor.map(|(ar, ac)| {
            let (cr, cc) = app.matrix_state.cursor;
            (ar.min(cr), ar.max(cr), ac.min(cc), ac.max(cc))
        });

    for row in 0..n_rows {
        let row_label = (b'A' + row as u8) as char;
        let row_key   = row_label.to_string();
        let is_row_cursor = cursor_row == row;

        // Pre-compute display data for every cell in this row.
        // Each element: (bg, fg, cell_h content lines with vertical centering)
        let grabbed = app.matrix_state.grabbed_clip;
        let cell_data: Vec<(Color, Color, Vec<String>)> = col_range.clone().map(|col| {
            let is_cursor = is_row_cursor && cursor_col == col;
            let is_selected = sel_region
                .map(|(r0, r1, c0, c1)| row >= r0 && row <= r1 && col >= c0 && col <= c1)
                .unwrap_or(false) && !is_cursor;
            let is_grabbed_src = grabbed.map(|(gr, gc)| gr == row && gc == col).unwrap_or(false);
            let is_drop_target = grabbed.is_some() && is_cursor;
            let clip = proj.matrix.get(&row_key)
                .and_then(|r| r.get(col))
                .and_then(|c| c.as_ref());

            let has_clip   = clip.is_some();
            let is_enabled = clip.map(|c| c.enabled).unwrap_or(false);
            let is_disabled = has_clip && !is_enabled;
            let pat_key    = clip.and_then(|c| c.pattern_key.as_deref()).unwrap_or("");
            let source     = clip.map(|c| &c.source);

            // True when this clip has a configured MIDI output that is no longer available.
            let is_route_fail = is_enabled && clip
                .and_then(|c| c.midi_out.as_deref())
                .map(|out| app.unavailable_midi_routes.contains(out))
                .unwrap_or(false);

            let is_tracker_active = !pat_key.is_empty()
                && tracker_key.map(|k| k == pat_key).unwrap_or(false);

            let is_hit = app.playing && is_enabled && !pat_key.is_empty() && {
                proj.patterns.get(pat_key).map(|p| {
                    let pos = app.current_step % p.length.max(1);
                    p.steps.get(pos).map(|n| !n.is_empty()).unwrap_or(false)
                }).unwrap_or(false)
            };

            let bg = if is_grabbed_src {
                Color::Rgb(180, 90, 20)   // orange  – this clip is held for move
            } else if is_drop_target {
                Color::Rgb(20, 150, 100)  // teal    – valid drop zone (cursor)
            } else if is_selected {
                Color::Rgb(60, 70, 110)   // muted indigo – inside selection region
            } else if is_enabled && is_hit {
                C_CURRENT           // white       – note fires while playing
            } else if is_route_fail && is_cursor {
                C_ROUTE_CUR         // bright amber – cursor on route-fail clip
            } else if is_route_fail {
                C_ROUTE_FAIL        // dark amber  – MIDI destination gone
            } else if is_enabled && is_cursor {
                C_ACTIVE            // light gray  – cursor on enabled clip
            } else if is_enabled && is_tracker_active {
                Color::Indexed(19)  // medium blue – open in tracker
            } else if is_enabled {
                C_ENABLED           // dark blue   – clip present, idle
            } else if is_disabled && is_cursor {
                C_DIS_CURSOR        // medium gray – cursor on disabled clip
            } else if is_disabled {
                C_DISABLED          // dark gray   – disabled clip, idle
            } else if is_cursor {
                C_INACTIVE          // medium gray – cursor on empty slot
            } else {
                C_INACT_DIM         // very dark   – empty, not selected
            };

            let fg = if is_hit || (is_cursor && is_enabled && !is_route_fail) {
                C_TEXT_DARK         // near-black on white/light bg
            } else if is_route_fail {
                C_TEXT_DARK         // near-black on amber bg
            } else if is_enabled {
                C_TEXT_BLUE         // light text on dark blue bg
            } else if is_disabled {
                C_TEXT_GRAY         // light-gray text on dark gray
            } else {
                C_TEXT_MED          // dim text for empty cells
            };

            // Label line (line 0): position reference + source icon + tracker marker
            let label_line = {
                let pos = format!("{}{:02}", row_label, col + 1);
                let marker = if is_grabbed_src { "↑" }
                             else if is_drop_target { "↓" }
                             else if is_tracker_active { "▸" }
                             else { " " };
                let src_icon = match source {
                    Some(PatternSource::Sf2 { .. }) => "♪",
                    Some(PatternSource::AudioFile { .. }) => "▶",
                    _ => " ",
                };
                format!("{:<width$}", format!("{}{}{}", marker, src_icon, pos), width = cell_w)
            };

            // ASCII font rows (lines 1-3): SF2/AudioFile clips show their file name instead.
            let font = if has_clip {
                let display_key = match source {
                    Some(PatternSource::Sf2 { preset_name, .. }) if !preset_name.is_empty() => {
                        preset_name.chars().take(n_font_chars).collect::<String>().to_uppercase()
                    }
                    Some(PatternSource::AudioFile { path, .. }) => {
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("AUDIO")
                            .chars().take(n_font_chars).collect::<String>().to_uppercase()
                    }
                    _ => pat_key.chars().take(n_font_chars).collect::<String>().to_uppercase(),
                };
                ascii_rows(&display_key, cell_w)
            } else {
                [" ".repeat(cell_w), " ".repeat(cell_w), " ".repeat(cell_w)]
            };

            // Top-align: label first, font rows below, optional waveform, empty padding.
            let content_h = 1 + n_font_rows;
            let pad_bot = cell_h.saturating_sub(content_h);
            let empty = " ".repeat(cell_w);
            let mut content: Vec<String> = Vec::with_capacity(cell_h);
            content.push(label_line);
            for i in 0..n_font_rows { content.push(font[i].clone()); }
            // For AudioFile clips with spare rows: show amplitude waveform.
            if pad_bot > 0 {
                let wave_row = if let Some(PatternSource::AudioFile { path, .. }) = source {
                    if let Some(peaks) = app.waveform_cache.get(path) {
                        waveform_bar(peaks, cell_w)
                    } else {
                        // Pending scan: show a dim scanning indicator.
                        "·".repeat(cell_w)
                    }
                } else {
                    empty.clone()
                };
                content.push(wave_row);
                for _ in 0..(pad_bot - 1) { content.push(empty.clone()); }
            }

            (bg, fg, content)
        }).collect();

        // ── Separator / top border ────────────────────────────────────────────
        {
            let (l, m, r) = if row == 0 { ("┌", "┬", "┐") } else { ("├", "┼", "┤") };
            let lbl_style = Style::default()
                .fg(if is_row_cursor { Color::Yellow } else { HEADER })
                .add_modifier(Modifier::BOLD);
            let mut sep: Vec<Span> = vec![
                Span::styled(format!("{} ", row_label), lbl_style),
                Span::raw(" "),
                Span::styled(l.to_string(), Style::default().fg(h_color(row, 0))),
            ];
            for col in col_range.clone() {
                sep.push(Span::styled(h_seg.clone(), Style::default().fg(h_color(row, col))));
                if col < n_cols - 1 {
                    // Junction: yellow if adjacent to either the left (col) or right (col+1) hovered column.
                    let jc = if h_color(row, col) == Color::Yellow || h_color(row, col + 1) == Color::Yellow {
                        Color::Yellow
                    } else {
                        bdr_default
                    };
                    sep.push(Span::styled(m.to_string(), Style::default().fg(jc)));
                }
            }
            sep.push(Span::styled(r.to_string(), Style::default().fg(h_color(row, n_cols.saturating_sub(1)))));
            lines.push(Line::from(sep));
        }

        // ── Content lines ─────────────────────────────────────────────────────
        for h in 0..cell_h {
            let mut spans: Vec<Span> = vec![Span::raw(" ".repeat(ROW_LBL))];
            for col in col_range.clone() {
                let (bg, fg, ref content) = cell_data[col];
                spans.push(Span::styled("│", Style::default().fg(v_color(row, col))));
                spans.push(Span::styled(
                    content[h].clone(),
                    Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
                ));
            }
            spans.push(Span::styled("│", Style::default().fg(v_color(row, n_cols))));
            lines.push(Line::from(spans));
        }
    }

    // ── Bottom border ─────────────────────────────────────────────────────────
    {
        // Bottom border is the "bottom edge" of matrix row n_rows-1.
        let bot_row = n_rows;
        let mut bot: Vec<Span> = vec![Span::raw(" ".repeat(ROW_LBL))];
        bot.push(Span::styled("└".to_string(), Style::default().fg(h_color(bot_row, 0))));
        for col in col_range.clone() {
            bot.push(Span::styled(h_seg.clone(), Style::default().fg(h_color(bot_row, col))));
            if col < n_cols - 1 {
                let jc = if h_color(bot_row, col) == Color::Yellow || h_color(bot_row, col + 1) == Color::Yellow {
                    Color::Yellow
                } else {
                    bdr_default
                };
                bot.push(Span::styled("┴".to_string(), Style::default().fg(jc)));
            }
        }
        bot.push(Span::styled("┘".to_string(), Style::default().fg(h_color(bot_row, n_cols.saturating_sub(1)))));
        lines.push(Line::from(bot));
    }

    // ── Hint row ──────────────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        if grid_active {
            "  e=enable  Enter=open  Del=remove  hjkl=navigate"
        } else {
            "  SPACE=play  s=stop  r=rec  Tab=transport"
        },
        Style::default().fg(if grid_active { Color::Yellow } else { Color::DarkGray }),
    )));

    // ── Render ────────────────────────────────────────────────────────────────
    let title = format!(
        " MATRIX {}×{} :: BPM {} :: {} ",
        n_rows,
        n_cols,
        app.bpm as u32,
        if app.playing { "▶ PLAYING" } else { "■ STOPPED" }
    );
    let border_col = if grid_active { Color::Yellow } else { BORDER };

    // Publish cell geometry so the click handler can map pixel→cell without recalculating.
    app.matrix_cell_size.set((cell_w, cell_h));

    let p = Paragraph::new(lines).block(
        Block::default()
            .title(title)
            .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}

fn draw_transport_buttons(f: &mut Frame, app: &App, area: Rect) {
    let ta = app.matrix_section == 1; // transport active
    let tc = app.transport_cursor;

    let tap_recently = !app.tap_times.is_empty();

    // Play/Pause button: green=playing, yellow=paused, dim=stopped.
    let play_col = if app.playing { Color::Green }
                   else if app.paused { Color::Yellow }
                   else { Color::Rgb(20, 80, 30) };
    let stop_col   = Color::Rgb(80, 80, 95);
    let rewind_col = Color::Rgb(60, 80, 120);
    let tap_col  = if tap_recently  { Color::White } else { Color::Rgb(80, 80, 90) };

    let play_state   = Style::default().fg(play_col).add_modifier(
        if app.playing || app.paused { Modifier::BOLD } else { Modifier::empty() });
    let stop_state   = Style::default().fg(stop_col);
    let rewind_state = Style::default().fg(rewind_col);
    let tap_state    = Style::default().fg(tap_col).add_modifier(
        if tap_recently { Modifier::BOLD } else { Modifier::empty() });

    let border_s = |idx: usize, col: Color, bold: bool| -> Style {
        if ta && tc == idx {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if app.hovered_transport_btn == Some(idx as u8) {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if bold {
            Style::default().fg(col).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(col)
        }
    };

    let play_border   = border_s(0, play_col,   app.playing || app.paused);
    let stop_border   = border_s(1, stop_col,   false);
    let rewind_border = border_s(2, rewind_col, false);
    let tap_border    = border_s(3, tap_col,    tap_recently);

    // BPM box: now at index 4 (play=0 stop=1 rwd=2 tap=3 bpm=4).
    let bpm_col = if ta && tc == 4 { Color::Yellow }
        else if app.hovered_transport_btn == Some(4) { Color::Cyan }
        else { ACCENT };
    let bpm_val = if ta && tc == 4 {
        Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    };

    // Matrix size row: tc=6=ROWS, tc=7=COLS.
    let rows = app.matrix_rows;
    let cols = app.matrix_cols;
    let hov = app.hovered_transport_btn;
    let lbl_s = |idx: usize| -> Style {
        if ta && tc == idx { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) }
        else if hov == Some(idx as u8) { Style::default().fg(Color::Cyan) }
        else { Style::default().fg(ACCENT) }
    };
    let val_s = |idx: usize| -> Style {
        if ta && tc == idx { Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD) }
        else if hov == Some(idx as u8) { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) }
        else { Style::default().fg(Color::White) }
    };

    let hint = if ta {
        match tc {
            0..=3 => "  Enter=trigger  ←→=navigate  Tab=back to grid",
            4     => "  ↑↓=BPM  ←→=navigate  Tab=back to grid",
            5     => "  ↑↓=ROWS  ←→=navigate  Tab=back to grid",
            _     => "  ↑↓=COLS  ←→=navigate  Tab=back to grid",
        }
    } else {
        "  SPACE=play/pause  s=stop  r=rec  hjkl=navigate  Enter=open  e=enable  Tab=transport"
    };

    // Play label: ▶ PLAY, ⏸ PAUSE, or ■ STOP depending on state.
    let play_label = if app.playing { "│⏸ PAUSE│" }
                     else if app.paused { "│▶ RESUM│" }
                     else { "│▶ PLAY │" };

    let lines = vec![
        Line::from(vec![
            Span::styled("╭───────╮", play_border),
            Span::raw(" "),
            Span::styled("╭──────╮", stop_border),
            Span::raw(" "),
            Span::styled("╭──────╮", rewind_border),
            Span::raw(" "),
            Span::styled("╭──────╮", tap_border),
            Span::raw(" "),
            Span::styled("╭─────────╮", Style::default().fg(bpm_col)),
        ]),
        Line::from(vec![
            Span::styled(play_label, play_state),
            Span::raw(" "),
            Span::styled("│■ STOP│", stop_state),
            Span::raw(" "),
            Span::styled("│◀◀ RWD│", rewind_state),
            Span::raw(" "),
            Span::styled("│  TAP │", tap_state),
            Span::raw(" "),
            Span::styled("│BPM: ", Style::default().fg(bpm_col)),
            Span::styled(format!("{:>4}│", app.bpm as u32), bpm_val),
        ]),
        Line::from(vec![
            Span::styled("╰───────╯", play_border),
            Span::raw(" "),
            Span::styled("╰──────╯", stop_border),
            Span::raw(" "),
            Span::styled("╰──────╯", rewind_border),
            Span::raw(" "),
            Span::styled("╰──────╯", tap_border),
            Span::raw(" "),
            Span::styled("╰─────────╯", Style::default().fg(bpm_col)),
        ]),
        Line::from(vec![
            Span::styled("MATRIX SIZE : ", lbl_s(5)),
            Span::styled(format!("{:>3}", rows), val_s(5)),
            Span::styled(" × ", Style::default().fg(ACCENT)),
            Span::styled(format!("{:>3}", cols), val_s(6)),
            Span::styled(
                format!("  = {} slots", rows * cols),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(Span::styled(
            hint,
            Style::default().fg(if ta { Color::Yellow } else { Color::DarkGray }),
        )),
    ];

    let border_col = if ta { Color::Yellow } else { BORDER };
    let p = Paragraph::new(lines).block(
        Block::default()
            .title(if ta { " TRANSPORT [ACTIVE] " } else { " TRANSPORT " })
            .title_style(Style::default().fg(HEADER))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}

fn draw_polymeter(f: &mut Frame, app: &App, area: Rect) {
    let proj = app.project.lock();
    let step = app.current_step;
    let poly_active = app.matrix_section == 2;

    // Label prefix: "► KEY   LEN " = 1+5+3+1 = 10 chars, plus borders = 12 total overhead.
    let bar_w = area.width.saturating_sub(12) as usize;

    // Collect only patterns that have a matrix cell assigned, in grid order (A01, A02, ...).
    let n_rows = app.matrix_rows;
    let n_cols = app.matrix_cols;
    let mut seen = std::collections::HashSet::new();
    let mut pat_list: Vec<(&String, &seqterm_core::Pattern)> = Vec::new();
    for row in 0..n_rows {
        let row_key = ((b'A' + row as u8) as char).to_string();
        if let Some(slots) = proj.matrix.get(&row_key) {
            for col in 0..n_cols.min(slots.len()) {
                if let Some(Some(clip)) = slots.get(col) {
                    if let Some(pk) = &clip.pattern_key {
                        if !seen.contains(pk) {
                            if let Some(pat) = proj.patterns.get(pk) {
                                seen.insert(pk.clone());
                                pat_list.push((pk, pat));
                            }
                        }
                    }
                }
            }
        }
    }

    // Visible rows available for patterns (subtract 2: info line + hint).
    let inner_h = area.height.saturating_sub(2) as usize;
    let reserved = 2usize;
    let visible_rows = inner_h.saturating_sub(reserved);

    // Clamp pat_scroll so cursor is always visible.
    let cursor = app.polymeter_cursor.min(pat_list.len().saturating_sub(1));
    let mut pat_scroll = app.polymeter_pat_scroll;
    if cursor < pat_scroll { pat_scroll = cursor; }
    if cursor >= pat_scroll + visible_rows.max(1) {
        pat_scroll = cursor + 1 - visible_rows.max(1);
    }

    let mut lines: Vec<Line> = Vec::new();

    // Pattern rows — each bar is compressed to show the full pattern length.
    let visible_pats = pat_list.iter().enumerate().skip(pat_scroll).take(visible_rows);
    for (abs_idx, (key, pat)) in visible_pats {
        let is_cursor = poly_active && abs_idx == cursor;
        let cur_pos = if pat.length > 0 { step % pat.length } else { 0 };

        // Compress full pattern into bar_w chars.
        // Each char i represents steps [ i*L/W .. (i+1)*L/W ).
        let bar: String = if bar_w == 0 || pat.length == 0 {
            String::new()
        } else {
            (0..bar_w).map(|i| {
                let s0 = i * pat.length / bar_w;
                let s1 = ((i + 1) * pat.length / bar_w).max(s0 + 1).min(pat.length);
                // Playhead lands in this bucket?
                if cur_pos >= s0 && cur_pos < s1 {
                    return '►';
                }
                // Any note in this bucket?
                let has_note = (s0..s1)
                    .any(|s| pat.steps.get(s).map(|n| !n.is_empty()).unwrap_or(false));
                if has_note { '█' } else { '·' }
            }).collect()
        };

        let on_beat = pat.steps.get(cur_pos).map(|n| !n.is_empty()).unwrap_or(false);
        let bar_col = if on_beat { Color::Green } else { Color::Rgb(80, 100, 130) };

        let (lbl_style, bar_style) = if is_cursor {
            (
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                Style::default().fg(Color::Yellow),
            )
        } else {
            (
                Style::default().fg(Color::White),
                Style::default().fg(bar_col),
            )
        };

        let prefix = if is_cursor { "►" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}{:<5}{:>3} ", prefix, &key[..key.len().min(5)], pat.length),
                lbl_style,
            ),
            Span::styled(bar, bar_style),
        ]));
    }

    // Info line: global step + bar + pattern count.
    lines.push(Line::from(vec![
        Span::styled(
            format!("step:{:>4}  bar:{:>3}  ", step % 16 + 1, app.current_bar + 1),
            Style::default().fg(ACCENT),
        ),
        Span::styled(
            if app.playing { "▶ PLAY" } else { "■ STOP" },
            Style::default().fg(if app.playing { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            format!("  [{} pats]", pat_list.len()),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // Hint line.
    lines.push(Line::from(Span::styled(
        if poly_active {
            "  ↑↓=select  Tab=next"
        } else {
            "  Tab=focus"
        },
        Style::default().fg(if poly_active { Color::Yellow } else { Color::DarkGray }),
    )));

    let border_col = if poly_active { Color::Yellow } else { BORDER };
    let p = Paragraph::new(lines).block(
        Block::default()
            .title(" POLYMETER VISUALIZER ")
            .title_style(Style::default().fg(HEADER))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_col))
            .style(Style::default().bg(PANEL)),
    );
    f.render_widget(p, area);
}



// ─── Hybrid View ─────────────────────────────────────────────────────────────

#[allow(dead_code)]
const HV_ACCENT: Color = Color::Rgb(31, 111, 235);
const HV_GREEN:  Color = Color::Rgb(56, 200, 100);
const HV_AMBER:  Color = Color::Rgb(240, 180,  40);
const HV_DIM:    Color = Color::Rgb(80, 90, 100);

/// Hybrid View panel — 3 sections stacked:
///   1. Active patterns  (progress bars + click to select clip)
///   2. Tracker monitor  (current-step following + click to navigate)
///   3. Voice activity   (VU bars + voice count)
#[allow(dead_code)]
pub fn draw_hybrid_panel(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),   // active patterns
            Constraint::Min(8),      // tracker monitor
            Constraint::Length(7),   // voice/channel activity
        ])
        .split(area);

    draw_hv_active_patterns(f, app, chunks[0]);
    draw_hv_tracker_monitor(f, app, chunks[1]);
    draw_hv_voice_activity(f, app, chunks[2]);
}

fn hv_block(title: &str) -> ratatui::widgets::Block<'_> {
    Block::default()
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(HV_AMBER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL))
}

// ── 1. Active patterns ────────────────────────────────────────────────────────

fn draw_hv_active_patterns(f: &mut Frame, app: &App, area: Rect) {
    let block = hv_block("ACTIVE PATTERNS");
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.hv_patterns_inner.set(inner);
    if inner.width < 4 || inner.height < 1 { return; }

    let proj = app.project.lock();
    let w = inner.width as usize;
    let bar_w = (w.saturating_sub(20)).max(4);

    let mut lines: Vec<Line> = Vec::new();

    // Collect clips sorted by row+col.
    let mut entries: Vec<(String, &str, usize, usize)> = Vec::new(); // (clip_key, pat_key, pos, len)
    for r in 0..app.matrix_rows {
        let row_key = ((b'A' + r as u8) as char).to_string();
        if let Some(slots) = proj.matrix.get(&row_key) {
            for (c, slot) in slots.iter().enumerate() {
                if let Some(clip) = slot {
                    if !clip.enabled { continue; }
                    if let Some(k) = clip.pattern_key.as_deref() {
                        if let Some(pat) = proj.patterns.get(k) {
                            let pos = if pat.length > 0 { app.current_step % pat.length } else { 0 };
                            entries.push((format!("{}{}", row_key, c + 1), k, pos, pat.length));
                        }
                    }
                }
            }
        }
    }

    for (i, (clip_key, pat_key, pos, len)) in entries.iter().take(inner.height as usize).enumerate() {
        let _ = i;
        let frac = if *len > 0 { *pos as f32 / *len as f32 } else { 0.0 };
        let filled = (frac * bar_w as f32).round() as usize;
        let empty  = bar_w.saturating_sub(filled);
        let bar: String = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
        let name = if pat_key.len() > 8 { &pat_key[..8] } else { pat_key };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<4} ", clip_key), Style::default().fg(HV_DIM)),
            Span::styled(format!("{:<9}", name),      Style::default().fg(Color::White)),
            Span::styled(bar,                          Style::default().fg(HV_GREEN)),
            Span::styled(
                format!(" {:>3}/{:<3}", pos, len),
                Style::default().fg(HV_DIM),
            ),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  — no active clips —",
            Style::default().fg(HV_DIM),
        )));
    }

    drop(proj);
    f.render_widget(
        Paragraph::new(lines),
        inner,
    );
}

// ── 2. Tracker monitor ────────────────────────────────────────────────────────

fn draw_hv_tracker_monitor(f: &mut Frame, app: &App, area: Rect) {
    let block = hv_block("TRACKER MONITOR");
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.hv_monitor_inner.set(inner);
    if inner.width < 8 || inner.height < 3 { return; }

    let (cursor_row, cursor_col) = app.matrix_state.cursor;
    let row_key = ((b'A' + cursor_row as u8) as char).to_string();
    let proj = app.project.lock();

    let pat_key = proj
        .matrix
        .get(&row_key)
        .and_then(|r| r.get(cursor_col))
        .and_then(|s| s.as_ref())
        .and_then(|c| c.pattern_key.as_deref());

    let Some(pat_key) = pat_key else {
        f.render_widget(
            Paragraph::new(Span::styled("  — select a clip —", Style::default().fg(HV_DIM))),
            inner,
        );
        return;
    };

    let Some(pat) = proj.patterns.get(pat_key) else { return; };

    let current = if pat.length > 0 { app.current_step % pat.length } else { 0 };
    // Reserve 1 row for header; the rest shows steps.
    let h = (inner.height as usize).saturating_sub(1);
    let context = h / 2;

    let start = current.saturating_sub(context);
    let end   = (start + h).min(pat.length);
    app.hv_monitor_start_step.set(start);

    let mut lines: Vec<Line> = Vec::new();

    // Header row
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<7}", pat_key), Style::default().fg(HV_AMBER).add_modifier(Modifier::BOLD)),
        Span::styled("NOTE VEL GATE", Style::default().fg(HV_DIM)),
    ]));

    for step in start..end {
        let note = &pat.steps[step];
        let is_cur = step == current && app.playing;
        let arrow = if is_cur { "▶" } else { " " };
        let (note_str, vel_str, gate_str) = if note.is_empty() {
            ("---".to_string(), "   ".to_string(), "   ".to_string())
        } else {
            (
                format!("{:<4}", &note.note),
                format!("{:>3}", note.velocity),
                format!("{:>3}", note.gate),
            )
        };
        let row_style = if is_cur {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else if note.is_empty() {
            Style::default().fg(HV_DIM)
        } else {
            Style::default().fg(HV_GREEN)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{} {:>3} ", arrow, step), row_style),
            Span::styled(note_str,  row_style),
            Span::styled(" ", Style::default()),
            Span::styled(vel_str,   row_style),
            Span::styled(" ", Style::default()),
            Span::styled(gate_str,  row_style),
        ]));
    }

    drop(proj);
    f.render_widget(Paragraph::new(lines), inner);
}

// ── 3. Voice / channel activity ───────────────────────────────────────────────

fn draw_hv_voice_activity(f: &mut Frame, app: &App, area: Rect) {
    let block = hv_block("VOICE ACTIVITY");
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 8 || inner.height < 2 { return; }

    let w = inner.width as usize;
    let bar_w = (w.saturating_sub(8)).max(4);

    let mut lines: Vec<Line> = Vec::new();

    // Voice count line.
    lines.push(Line::from(vec![
        Span::styled("VOICES: ", Style::default().fg(HV_DIM)),
        Span::styled(
            format!("{} / 256", app.active_voices),
            Style::default().fg(
                if app.active_voices > 200 { Color::Rgb(220, 60, 60) }
                else if app.active_voices > 128 { HV_AMBER }
                else { HV_GREEN }
            ).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Per-slot peak bars (only show active/non-zero slots, up to remaining height).
    let max_slots = (inner.height as usize).saturating_sub(1);
    let mut shown = 0;
    for (i, &peak) in app.audio_slot_peaks.iter().enumerate() {
        if shown >= max_slots { break; }
        if peak < 0.001 { continue; }
        let filled = ((peak.clamp(0.0, 1.0)) * bar_w as f32).round() as usize;
        let color = if peak > 0.9 { Color::Rgb(220, 60, 60) }
                    else if peak > 0.7 { HV_AMBER }
                    else { HV_GREEN };
        lines.push(Line::from(vec![
            Span::styled(format!("S{:02} ", i + 1), Style::default().fg(HV_DIM)),
            Span::styled("█".repeat(filled),  Style::default().fg(color)),
            Span::styled("░".repeat(bar_w.saturating_sub(filled)), Style::default().fg(HV_DIM)),
        ]));
        shown += 1;
    }

    if shown == 0 && app.active_voices == 0 {
        lines.push(Line::from(Span::styled(
            "  — idle —",
            Style::default().fg(HV_DIM),
        )));
    }

    f.render_widget(Paragraph::new(lines), inner);
}
