use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use seqterm_core::Pattern;

const PANEL: Color = Color::Rgb(22, 27, 34);
const BORDER: Color = Color::Rgb(48, 54, 61);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const HEADER: Color = Color::Rgb(240, 136, 62);

/// A standalone piano-roll widget for embedding in views.
pub struct PianoRollWidget<'a> {
    pub pattern: &'a Pattern,
    pub current_step: usize,
    pub playing: bool,
    pub title: &'a str,
}

impl Widget for PianoRollWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // 25 notes: C5 down to C3.
        let notes = [
            ("C5", false), ("B4", false), ("A#4", true), ("A4", false), ("G#4", true),
            ("G4", false), ("F#4", true), ("F4", false), ("E4", false), ("D#4", true),
            ("D4", false), ("C#4", true), ("C4", false), ("B3", false), ("A#3", true),
            ("A3", false), ("G#3", true), ("G3", false), ("F#3", true), ("F3", false),
            ("E3", false), ("D#3", true), ("D3", false), ("C#3", true), ("C3", false),
        ];

        let note_count = notes.len();
        let step_count = self.pattern.length.min(area.width.saturating_sub(6) as usize);

        let mut lines: Vec<Line> = Vec::new();

        for (row, (note_name, is_black)) in notes.iter().enumerate() {
            let key_style = if *is_black {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };

            let mut spans = vec![
                Span::styled(format!("{:<4}", note_name), key_style),
                Span::styled("|", Style::default().fg(BORDER)),
            ];

            for step in 0..step_count {
                let note = self.pattern.steps.get(step).cloned().unwrap_or_default();
                let is_step = self.playing && step == self.current_step;

                let ch = if !note.is_empty() {
                    // Simplified: just mark the note row corresponding to velocity.
                    let _note_row_hit = row < (note_count / 2); // placeholder
                    if is_step { '▶' } else { '█' }
                } else if is_step {
                    '│'
                } else {
                    '·'
                };

                let style = if is_step {
                    Style::default().fg(Color::Green)
                } else if !note.is_empty() {
                    Style::default().fg(ACCENT)
                } else {
                    Style::default().fg(BORDER)
                };

                spans.push(Span::styled(ch.to_string(), style));
            }

            lines.push(Line::from(spans));
        }

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .title_style(Style::default().fg(HEADER))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(PANEL));

        let para = Paragraph::new(lines).block(block);
        para.render(area, buf);
    }
}
