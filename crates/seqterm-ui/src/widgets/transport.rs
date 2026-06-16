use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

const ACCENT: Color = Color::Rgb(31, 111, 235);
const BORDER: Color = Color::Rgb(48, 54, 61);
const PANEL: Color = Color::Rgb(22, 27, 34);

pub struct TransportBar<'a> {
    pub status_msg: &'a str,
    pub view_labels: &'a [&'a str],
    pub current_view: usize,
    pub xrun: u32,
    pub cpu: u8,
    /// True while audio capture to WAV is active.
    pub capturing: bool,
    /// True while MIDI clock sync is enabled.
    pub midi_clock_sync: bool,
    /// Description of the action the next Undo would revert (`None` = nothing).
    pub undo_hint: Option<&'a str>,
    /// Description of the action the next Redo would re-apply (`None` = nothing).
    pub redo_hint: Option<&'a str>,
    /// PATTERN-view MIDI monitor — most recent incoming note `(channel, note, vel)`.
    /// `None` hides the IN field (stale or not in PATTERN view).
    pub midi_in: Option<(u8, u8, u8)>,
    /// PATTERN-view MIDI monitor — most recent outgoing note `(channel, note, vel)`.
    pub midi_out: Option<(u8, u8, u8)>,
}

/// Format a MIDI note for the monitor: `ch·NoteName:vel`, e.g. `1·C4:100`.
/// Channel is shown 1-based.
fn fmt_midi(ch: u8, note: u8, vel: u8) -> String {
    let name = seqterm_core::Note::from_midi(note, vel)
        .map(|n| n.note)
        .unwrap_or_else(|_| note.to_string());
    format!("{}\u{00B7}{}:{}", ch as u16 + 1, name, vel)
}

impl Widget for TransportBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let xrun_style = if self.xrun > 0 {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let cpu_style = if self.cpu > 80 {
            Style::default().fg(Color::Red)
        } else if self.cpu > 50 {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Green)
        };

        // Build view tabs line.
        let mut tab_spans: Vec<Span> = vec![Span::styled("  ", Style::default())];
        for (i, &label) in self.view_labels.iter().enumerate() {
            if i == self.current_view {
                tab_spans.push(Span::styled(
                    format!("[{}]", label),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                tab_spans.push(Span::styled(
                    format!(" {} ", label),
                    Style::default().fg(ACCENT),
                ));
            }
            if i < self.view_labels.len() - 1 {
                tab_spans.push(Span::styled(
                    "│",
                    Style::default().fg(BORDER),
                ));
            }
        }

        // Transport line.
        let mut transport_spans = vec![
            Span::styled(format!(" CPU:{:>2}% ", self.cpu), cpu_style),
            Span::styled("│", Style::default().fg(BORDER)),
            Span::styled(format!(" XRUN:{:<4}", self.xrun), xrun_style),
            Span::styled("│", Style::default().fg(BORDER)),
        ];
        if self.capturing {
            transport_spans.push(Span::styled(
                " ● REC ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
            transport_spans.push(Span::styled("│", Style::default().fg(BORDER)));
        }
        if self.midi_clock_sync {
            transport_spans.push(Span::styled(
                " CLK ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));
            transport_spans.push(Span::styled("│", Style::default().fg(BORDER)));
        }
        transport_spans.push(Span::styled(
            format!(" {} ", self.status_msg),
            Style::default().fg(Color::White),
        ));
        // Undo / Redo availability (disabled = dim) — shown in the status line.
        transport_spans.push(Span::styled("│", Style::default().fg(BORDER)));
        match self.undo_hint {
            Some(d) => transport_spans.push(Span::styled(
                format!(" ↶ {} ", d), Style::default().fg(Color::Rgb(150, 170, 200)))),
            None => transport_spans.push(Span::styled(
                " ↶ — ", Style::default().fg(Color::DarkGray))),
        }
        match self.redo_hint {
            Some(d) => transport_spans.push(Span::styled(
                format!(" ↷ {} ", d), Style::default().fg(Color::Rgb(150, 170, 200)))),
            None => transport_spans.push(Span::styled(
                " ↷ — ", Style::default().fg(Color::DarkGray))),
        }
        let transport_line = Line::from(transport_spans);

        let tab_line = Line::from(tab_spans);

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(PANEL));

        let para = Paragraph::new(vec![tab_line, transport_line]).block(block);
        para.render(area, buf);

        // PATTERN MIDI monitor — flush-right overlay on the transport row, so the
        // (variable-length) status message never pushes it off-screen.
        if self.midi_in.is_some() || self.midi_out.is_some() {
            let mut spans: Vec<Span> = vec![
                Span::styled("│", Style::default().fg(BORDER)),
                Span::styled(" MIDI ", Style::default().fg(Color::DarkGray)),
            ];
            match self.midi_in {
                Some((ch, n, v)) => spans.push(Span::styled(
                    format!("◀ {} ", fmt_midi(ch, n, v)),
                    Style::default().fg(Color::Rgb(90, 200, 120)).add_modifier(Modifier::BOLD))),
                None => spans.push(Span::styled("◀ —:— ", Style::default().fg(Color::DarkGray))),
            }
            match self.midi_out {
                Some((ch, n, v)) => spans.push(Span::styled(
                    format!("▶ {} ", fmt_midi(ch, n, v)),
                    Style::default().fg(Color::Rgb(110, 170, 240)).add_modifier(Modifier::BOLD))),
                None => spans.push(Span::styled("▶ —:— ", Style::default().fg(Color::DarkGray))),
            }
            let line = Line::from(spans);
            let w = line.width() as u16;
            // Transport line is the row below the TOP border + tab line (area.y + 2).
            let row = area.y + 2;
            if w <= area.width && row < area.y + area.height {
                let mon_area = Rect { x: area.x + area.width - w, y: row, width: w, height: 1 };
                Paragraph::new(line).style(Style::default().bg(PANEL)).render(mon_area, buf);
            }
        }
    }
}
