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
        let transport_line = Line::from(vec![
            Span::styled(
                format!(" CPU:{:>2}% ", self.cpu),
                cpu_style,
            ),
            Span::styled("│", Style::default().fg(BORDER)),
            Span::styled(
                format!(" XRUN:{:<4}", self.xrun),
                xrun_style,
            ),
            Span::styled("│", Style::default().fg(BORDER)),
            Span::styled(
                format!(" {} ", self.status_msg),
                Style::default().fg(Color::White),
            ),
        ]);

        let tab_line = Line::from(tab_spans);

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(PANEL));

        let para = Paragraph::new(vec![tab_line, transport_line]).block(block);
        para.render(area, buf);
    }
}
