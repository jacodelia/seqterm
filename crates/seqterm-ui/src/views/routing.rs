use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use seqterm_core::RoutingNode;

use crate::app::App;

const BG:     Color = Color::Rgb(13, 17, 23);
const BORDER: Color = Color::Rgb(48, 54, 61);
const ACCENT: Color = Color::Rgb(31, 111, 235);
const HEADER: Color = Color::Rgb(240, 136, 62);
const OK:     Color = Color::Rgb(56, 200, 100);
const SEL:    Color = Color::Rgb(50, 60, 75);

/// Draw the routing graph, marking it focused/unfocused via border color.
pub fn draw_routing_focused(f: &mut Frame, app: &App, area: Rect, focused: bool) {
    app.routing_graph_area.set(area);

    let hovered = app.routing_graph_hovered.get();
    let outer_border_color = if focused {
        ACCENT
    } else if hovered {
        Color::Yellow
    } else {
        BORDER
    };
    let outer = Block::default()
        .title(" ROUTING GRAPH ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(outer_border_color))
        .style(Style::default().bg(BG));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // Layout: node list (30%) | connection matrix (50%) | property panel (20%)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(28),
            Constraint::Percentage(52),
            Constraint::Percentage(20),
        ])
        .split(inner);

    let node_area  = cols[0];
    let matrix_area = cols[1];
    let prop_area  = cols[2];

    let proj = app.project.lock();
    let graph = &proj.routing;
    let sorted_ids = graph.sorted_ids();

    // ── Left: Node list ───────────────────────────────────────────────────────
    let node_focused = app.routing_state.section == 0;
    let node_block = Block::default()
        .title(" NODES ")
        .title_style(Style::default().fg(HEADER))
        .borders(Borders::ALL)
        .border_style(if node_focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(BORDER)
        })
        .style(Style::default().bg(BG));
    let node_inner = node_block.inner(node_area);
    app.routing_node_list_inner.set(node_inner);
    f.render_widget(node_block, node_area);

    let scroll = app.routing_state.scroll;
    let cursor = app.routing_state.node_cursor;
    let visible_h = node_inner.height as usize;

    let node_items: Vec<ListItem> = sorted_ids.iter().enumerate().skip(scroll)
        .take(visible_h)
        .map(|(i, &id)| {
            let node = &graph.nodes[&id];
            let is_sel = i == cursor;
            let style = if is_sel && node_focused {
                Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
            } else if is_sel {
                Style::default().fg(ACCENT).bg(SEL)
            } else {
                Style::default().fg(Color::White)
            };
            let kind_color = match node {
                RoutingNode::PatternOut { .. } => ACCENT,
                RoutingNode::MidiIn { .. }     => Color::Cyan,
                RoutingNode::MidiOut { .. }    => OK,
                RoutingNode::AudioBus { .. }   => Color::Yellow,
                RoutingNode::Send { .. }       => Color::Magenta,
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("[{}] ", node.kind_label().chars().next().unwrap_or('?').to_ascii_uppercase()),
                    Style::default().fg(kind_color),
                ),
                Span::styled(
                    node.label().chars().take(node_inner.width.saturating_sub(5) as usize).collect::<String>(),
                    style,
                ),
            ]))
        })
        .collect();

    f.render_widget(List::new(node_items).style(Style::default().bg(BG)), node_inner);

    // Scrollbar hint
    if sorted_ids.len() > visible_h {
        let pct = if sorted_ids.len() > 1 { scroll * 100 / (sorted_ids.len() - 1) } else { 0 };
        let bar = Paragraph::new(format!("{pct}%"))
            .style(Style::default().fg(BORDER));
        let bar_rect = Rect::new(
            node_inner.x + node_inner.width.saturating_sub(4),
            node_inner.y + node_inner.height.saturating_sub(1),
            4, 1,
        );
        f.render_widget(bar, bar_rect);
    }

    // ── Center: Connection matrix ─────────────────────────────────────────────
    let mat_focused = app.routing_state.section == 1;
    let mat_block = Block::default()
        .title(" CONNECTIONS (Enter=toggle  Delete=remove) ")
        .title_style(Style::default().fg(HEADER))
        .borders(Borders::ALL)
        .border_style(if mat_focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(BORDER)
        })
        .style(Style::default().bg(BG));
    let mat_inner = mat_block.inner(matrix_area);
    app.routing_matrix_inner.set(mat_inner);
    f.render_widget(mat_block, matrix_area);

    if sorted_ids.is_empty() {
        let hint = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("  No routing nodes yet.", Style::default().fg(Color::DarkGray))),
            Line::from(""),
            Line::from(Span::styled("  Nodes are auto-created from:", Style::default().fg(BORDER))),
            Line::from(Span::styled("  • Pattern outputs (load/import project)", Style::default().fg(BORDER))),
            Line::from(Span::styled("  • MIDI ports (detected at startup)", Style::default().fg(BORDER))),
            Line::from(""),
            Line::from(Span::styled("  Press 'a' to sync nodes from project.", Style::default().fg(ACCENT))),
        ])
        .style(Style::default().bg(BG))
        .wrap(Wrap { trim: false });
        f.render_widget(hint, mat_inner);
    } else {
        // Header row: target node names (abbreviated)
        let col_w = (mat_inner.width.saturating_sub(14) / sorted_ids.len().max(1) as u16).max(3).min(10);
        app.routing_matrix_col_w.set(col_w);
        let mut header_spans = vec![
            Span::styled(format!("{:14}", "FROM \\ TO"), Style::default().fg(HEADER)),
        ];
        for &id in &sorted_ids {
            let label = graph.nodes[&id].label();
            let short: String = label.chars().take(col_w as usize).collect();
            header_spans.push(Span::styled(
                format!("{:width$}", short, width = col_w as usize),
                Style::default().fg(Color::DarkGray),
            ));
        }
        let mut lines: Vec<Line> = vec![Line::from(header_spans)];

        let mat_h = mat_inner.height.saturating_sub(1) as usize;
        for (row_i, &from_id) in sorted_ids.iter().enumerate().skip(scroll).take(mat_h) {
            let from_label = graph.nodes[&from_id].label();
            let from_short: String = from_label.chars().take(12).collect();
            let is_row_sel = row_i == cursor;

            let row_label_style = if is_row_sel && mat_focused {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(HEADER)
            };
            let mut spans = vec![
                Span::styled(format!("{:14}", from_short), row_label_style),
            ];

            for (col_i, &to_id) in sorted_ids.iter().enumerate() {
                let connected = graph.has_edge(from_id, to_id);
                let is_cursor = is_row_sel && col_i == app.routing_state.col_cursor && mat_focused;
                let cell_str = if from_id == to_id {
                    format!("{:width$}", "·", width = col_w as usize)
                } else if connected {
                    format!("{:width$}", "●", width = col_w as usize)
                } else {
                    format!("{:width$}", "○", width = col_w as usize)
                };
                let cell_style = if is_cursor {
                    Style::default().fg(Color::Black).bg(ACCENT)
                } else if connected {
                    Style::default().fg(OK)
                } else {
                    Style::default().fg(BORDER)
                };
                spans.push(Span::styled(cell_str, cell_style));
            }
            lines.push(Line::from(spans));
        }

        f.render_widget(
            Paragraph::new(lines).style(Style::default().bg(BG)),
            mat_inner,
        );
    }

    // ── Right: Property panel ─────────────────────────────────────────────────
    let prop_block = Block::default()
        .title(" DETAILS ")
        .title_style(Style::default().fg(HEADER))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));
    let prop_inner = prop_block.inner(prop_area);
    f.render_widget(prop_block, prop_area);

    let mut prop_lines = vec![
        Line::from(Span::styled("LEGEND", Style::default().fg(HEADER).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ● ", Style::default().fg(OK)),
            Span::styled("connected", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  ○ ", Style::default().fg(BORDER)),
            Span::styled("no edge", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("  · ", Style::default().fg(BORDER)),
            Span::styled("self (N/A)", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(""),
        Line::from(Span::styled("NODE TYPES", Style::default().fg(HEADER).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![Span::styled("  [P] ", Style::default().fg(ACCENT)),       Span::styled("Pattern out", Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  [I] ", Style::default().fg(Color::Cyan)),   Span::styled("MIDI in",     Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  [O] ", Style::default().fg(OK)),            Span::styled("MIDI out",    Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  [B] ", Style::default().fg(Color::Yellow)), Span::styled("Audio bus",   Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  [S] ", Style::default().fg(Color::Magenta)),Span::styled("Send",        Style::default().fg(Color::White))]),
        Line::from(""),
        Line::from(Span::styled("KEYS", Style::default().fg(HEADER).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled("  hjkl  navigate",     Style::default().fg(Color::White))),
        Line::from(Span::styled("  Tab   switch panel", Style::default().fg(Color::White))),
        Line::from(Span::styled("  Enter toggle edge",  Style::default().fg(Color::White))),
        Line::from(Span::styled("  Del   del node",     Style::default().fg(Color::White))),
        Line::from(Span::styled("  a     sync nodes",   Style::default().fg(Color::White))),
        Line::from(Span::styled("  5     config view",  Style::default().fg(Color::White))),
    ];

    // Show selected node with ASCII connection arrows.
    if let Some(&sel_id) = sorted_ids.get(cursor) {
        if let Some(node) = graph.nodes.get(&sel_id) {
            let outgoing = graph.outgoing(sel_id);
            let incoming = graph.incoming(sel_id);
            prop_lines.push(Line::from(""));
            prop_lines.push(Line::from(Span::styled("SELECTED", Style::default().fg(HEADER).add_modifier(Modifier::BOLD))));
            prop_lines.push(Line::from(""));
            prop_lines.push(Line::from(Span::styled(
                format!("  {}", node.label()),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            prop_lines.push(Line::from(Span::styled(
                format!("  {}", node.kind_label()),
                Style::default().fg(Color::DarkGray),
            )));

            // ASCII arrows for outgoing connections.
            if !outgoing.is_empty() {
                prop_lines.push(Line::from(""));
                prop_lines.push(Line::from(Span::styled(
                    "  SENDS TO", Style::default().fg(HEADER),
                )));
                let last = outgoing.len() - 1;
                for (i, &to_id) in outgoing.iter().enumerate() {
                    let prefix = if i == last { "  └─► " } else { "  ├─► " };
                    let label = graph.nodes.get(&to_id).map(|n| n.label()).unwrap_or_else(|| "?".to_string());
                    prop_lines.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(BORDER)),
                        Span::styled(label, Style::default().fg(OK)),
                    ]));
                }
            }

            // ASCII arrows for incoming connections.
            if !incoming.is_empty() {
                prop_lines.push(Line::from(""));
                prop_lines.push(Line::from(Span::styled(
                    "  RECEIVES FROM", Style::default().fg(HEADER),
                )));
                let last = incoming.len() - 1;
                for (i, &from_id) in incoming.iter().enumerate() {
                    let prefix = if i == last { "  └── " } else { "  ├── " };
                    let label = graph.nodes.get(&from_id).map(|n| n.label()).unwrap_or_else(|| "?".to_string());
                    prop_lines.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(BORDER)),
                        Span::styled(label, Style::default().fg(Color::Cyan)),
                    ]));
                }
            }

            if outgoing.is_empty() && incoming.is_empty() {
                prop_lines.push(Line::from(Span::styled(
                    "  (no connections)", Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    drop(proj);

    f.render_widget(
        Paragraph::new(prop_lines)
            .style(Style::default().bg(BG))
            .wrap(Wrap { trim: false }),
        prop_inner,
    );

    // ── Bottom hint ───────────────────────────────────────────────────────────
    // (status_msg in transport bar carries the hint)
}
