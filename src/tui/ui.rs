use chrono::Utc;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};

use super::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header with session info
            Constraint::Length(3), // Metrics bar (tokens + tools summary)
            Constraint::Min(8),    // Tool table
            Constraint::Length(1), // Footer (hotkeys only)
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_metrics_bar(f, app, chunks[1]);
    draw_tool_table(f, app, chunks[2]);
    draw_footer(f, chunks[3]);

    // Draw detail popup if active
    if app.show_detail {
        draw_detail_popup(f, app);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let paused = if app.paused { " [PAUSED]" } else { "" };
    let title = format!(" agenttop{}", paused);

    let filter_label = format!("[{}]", app.time_filter.label());
    let header_content = Line::from(vec![
        Span::styled(
            format!("{:>width$}", filter_label, width = area.width.saturating_sub(4) as usize),
            Style::default().fg(Color::Yellow),
        ),
    ]);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(header_content).block(block);
    f.render_widget(paragraph, area);
}

fn draw_metrics_bar(f: &mut Frame, app: &App, area: Rect) {
    let cache_hit = app.cache_hit_rate();
    let success_rate = app.overall_success_rate();
    let avg_duration = app.average_tool_duration();
    let total_calls = app.total_tool_calls();

    // Format average duration
    let avg_str = if avg_duration < 1000.0 {
        format!("{}ms", avg_duration as u64)
    } else {
        format!("{:.1}s", avg_duration / 1000.0)
    };

    let metrics_line = Line::from(vec![
        Span::raw(" Tokens  "),
        Span::styled("In: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.1}K", app.token_metrics.input_tokens as f64 / 1000.0),
            Style::default().fg(Color::Blue),
        ),
        Span::raw("  "),
        Span::styled("Out: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.1}K", app.token_metrics.output_tokens as f64 / 1000.0),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  "),
        Span::styled("Cache: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.1}K", app.token_metrics.cache_read_tokens as f64 / 1000.0),
            Style::default().fg(Color::Magenta),
        ),
        Span::raw(" ("),
        Span::styled(
            format!("{:.0}% hit", cache_hit),
            Style::default().fg(if cache_hit > 80.0 {
                Color::Green
            } else if cache_hit > 50.0 {
                Color::Yellow
            } else {
                Color::Red
            }),
        ),
        Span::raw(")"),
    ]);

    let tools_line = Line::from(vec![
        Span::raw(" Tools   "),
        Span::styled("Calls: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            total_calls.to_string(),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::styled("Success: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.1}%", success_rate),
            Style::default().fg(if success_rate > 95.0 {
                Color::Green
            } else if success_rate > 80.0 {
                Color::Yellow
            } else {
                Color::Red
            }),
        ),
        Span::raw("  "),
        Span::styled("Avg: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            avg_str,
            Style::default().fg(Color::Blue),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT);

    let paragraph = Paragraph::new(vec![metrics_line, tools_line]).block(block);
    f.render_widget(paragraph, area);
}

fn draw_tool_table(f: &mut Frame, app: &App, area: Rect) {
    let header_cells = ["TOOL", "CALLS", "SUCCESS", "AVG", "LAST", "STATUS"].iter().map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    });
    let header = Row::new(header_cells).height(1);

    let now = Utc::now();

    let rows: Vec<Row> = app
        .tool_metrics
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            // Calculate success rate for this tool
            let success_rate = if tool.call_count > 0 {
                (tool.success_count as f64 / tool.call_count as f64) * 100.0
            } else {
                100.0
            };

            // Calculate time since last call
            let last_str = match tool.last_call {
                Some(last) => {
                    let diff = now - last;
                    if diff.num_seconds() < 60 {
                        format!("{}s", diff.num_seconds())
                    } else if diff.num_minutes() < 60 {
                        format!("{}m", diff.num_minutes())
                    } else {
                        format!("{}h", diff.num_hours())
                    }
                }
                None => "-".to_string(),
            };

            // Format average duration
            let avg_str = if tool.avg_duration_ms < 1000.0 {
                format!("{}ms", tool.avg_duration_ms as u64)
            } else {
                format!("{:.1}s", tool.avg_duration_ms / 1000.0)
            };

            // Create status bar (relative call frequency like htop CPU bars)
            let max_calls = app
                .tool_metrics
                .iter()
                .map(|t| t.call_count)
                .max()
                .unwrap_or(1);
            let bar_width = 10;
            let filled = ((tool.call_count as f64 / max_calls as f64) * bar_width as f64) as usize;
            let empty = bar_width - filled;
            let status_bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

            // Currently executing indicator
            let indicator = if tool
                .last_call
                .map(|l| (now - l).num_seconds() < 2)
                .unwrap_or(false)
            {
                "▶ "
            } else {
                "  "
            };

            let style = if i == app.selected_index {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            // Success rate color
            let success_style = if success_rate > 95.0 {
                Style::default().fg(Color::Green)
            } else if success_rate > 80.0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Red)
            };

            Row::new(vec![
                Cell::from(format!("{}{}", indicator, tool.tool_name)),
                Cell::from(tool.call_count.to_string()),
                Cell::from(format!("{:.0}%", success_rate)).style(success_style),
                Cell::from(avg_str),
                Cell::from(last_str),
                Cell::from(status_bar).style(Style::default().fg(Color::Cyan)),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(16),    // TOOL
            Constraint::Length(7),  // CALLS
            Constraint::Length(9),  // SUCCESS
            Constraint::Length(8),  // AVG
            Constraint::Length(6),  // LAST
            Constraint::Length(12), // STATUS
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Tools ")
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = TableState::default();
    state.select(Some(app.selected_index));

    f.render_stateful_widget(table, area, &mut state);
}

fn draw_footer(f: &mut Frame, area: Rect) {
    let footer = Line::from(vec![
        Span::styled(
            " [q]uit [s]ort [p]ause [d]etail [t]ime [r]eset",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(footer);
    f.render_widget(paragraph, area);
}

fn draw_detail_popup(f: &mut Frame, app: &App) {
    let Some(tool) = app.selected_tool() else {
        return;
    };

    let area = centered_rect(60, 50, f.area());

    // Clear the area
    f.render_widget(Clear, area);

    let success_rate = if tool.call_count > 0 {
        (tool.success_count as f64 / tool.call_count as f64) * 100.0
    } else {
        100.0
    };

    let content = vec![
        Line::from(vec![
            Span::styled("Tool: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&tool.tool_name),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("Total Calls: "),
            Span::styled(
                tool.call_count.to_string(),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::raw("Successful: "),
            Span::styled(
                tool.success_count.to_string(),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::raw("Errors: "),
            Span::styled(
                tool.error_count.to_string(),
                Style::default().fg(if tool.error_count > 0 {
                    Color::Red
                } else {
                    Color::Green
                }),
            ),
        ]),
        Line::from(vec![
            Span::raw("Success Rate: "),
            Span::styled(
                format!("{:.1}%", success_rate),
                Style::default().fg(if success_rate > 95.0 {
                    Color::Green
                } else if success_rate > 80.0 {
                    Color::Yellow
                } else {
                    Color::Red
                }),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("Avg Duration: "),
            Span::styled(
                format!("{:.0}ms", tool.avg_duration_ms),
                Style::default().fg(Color::Blue),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Press ESC or Enter to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let paragraph = Paragraph::new(content).block(
        Block::default()
            .title(format!(" {} Details ", tool.tool_name))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );

    f.render_widget(paragraph, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
