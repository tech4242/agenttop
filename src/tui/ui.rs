use chrono::Utc;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Gauge, Paragraph, Row, Table, TableState},
};

use super::app::App;

const TOKEN_LIMIT: u64 = 500_000; // Default context window estimate

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(3), // Token bar
            Constraint::Length(2), // Token details
            Constraint::Min(10),   // Tool table
            Constraint::Length(1), // Footer
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_token_bar(f, app, chunks[1]);
    draw_token_details(f, app, chunks[2]);
    draw_tool_table(f, app, chunks[3]);
    draw_footer(f, app, chunks[4]);

    // Draw detail popup if active
    if app.show_detail {
        draw_detail_popup(f, app);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let duration = app.session_duration();
    let hours = duration.num_hours();
    let minutes = duration.num_minutes() % 60;

    let session_time = if hours > 0 {
        format!("{}h {:02}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    };

    let cost = format!("${:.2}", app.token_metrics.total_cost_usd);
    let paused = if app.paused { " [PAUSED]" } else { "" };

    let title = format!(" agenttop{} Session: {} | {} ", paused, session_time, cost);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(block, area);
}

fn draw_token_bar(f: &mut Frame, app: &App, area: Rect) {
    let total = app.total_tokens();
    let ratio = (total as f64 / TOKEN_LIMIT as f64).min(1.0);

    let label = format!("Tokens [{}K/{}K]", total / 1000, TOKEN_LIMIT / 1000);

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .gauge_style(
            Style::default()
                .fg(if ratio > 0.8 {
                    Color::Red
                } else if ratio > 0.6 {
                    Color::Yellow
                } else {
                    Color::Green
                })
                .bg(Color::DarkGray),
        )
        .ratio(ratio)
        .label(label);

    f.render_widget(gauge, area);
}

fn draw_token_details(f: &mut Frame, app: &App, area: Rect) {
    let cache_hit = app.cache_hit_rate();

    let details = Line::from(vec![
        Span::raw("        In: "),
        Span::styled(
            format!("{}K", app.token_metrics.input_tokens / 1000),
            Style::default().fg(Color::Blue),
        ),
        Span::raw(" | Out: "),
        Span::styled(
            format!("{}K", app.token_metrics.output_tokens / 1000),
            Style::default().fg(Color::Green),
        ),
        Span::raw(" | Cache: "),
        Span::styled(
            format!("{}K", app.token_metrics.cache_read_tokens / 1000),
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

    let paragraph = Paragraph::new(details)
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM));

    f.render_widget(paragraph, area);
}

fn draw_tool_table(f: &mut Frame, app: &App, area: Rect) {
    let header_cells = ["TOOL", "CALLS", "LAST", "AVG", "STATUS"].iter().map(|h| {
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

            // Create status bar
            let max_calls = app
                .tool_metrics
                .iter()
                .map(|t| t.call_count)
                .max()
                .unwrap_or(1);
            let bar_width = 20;
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

            Row::new(vec![
                Cell::from(format!("{}{}", indicator, tool.tool_name)),
                Cell::from(tool.call_count.to_string()),
                Cell::from(last_str),
                Cell::from(avg_str),
                Cell::from(status_bar).style(Style::default().fg(Color::Cyan)),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(20),    // TOOL
            Constraint::Length(8),  // CALLS
            Constraint::Length(6),  // LAST
            Constraint::Length(8),  // AVG
            Constraint::Length(22), // STATUS
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

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let productivity = app.productivity_multiplier();

    let footer = Line::from(vec![
        Span::raw(" Productivity: "),
        Span::styled(
            format!("{:.0}x", productivity),
            Style::default().fg(Color::Green),
        ),
        Span::raw(" | Lines: "),
        Span::styled(
            format!("{:+}", app.session_metrics.lines_of_code),
            Style::default().fg(if app.session_metrics.lines_of_code >= 0 {
                Color::Green
            } else {
                Color::Red
            }),
        ),
        Span::raw(" | Commits: "),
        Span::styled(
            app.session_metrics.commit_count.to_string(),
            Style::default().fg(Color::Blue),
        ),
        Span::raw(" | PRs: "),
        Span::styled(
            app.session_metrics.pr_count.to_string(),
            Style::default().fg(Color::Magenta),
        ),
        Span::raw(" | Tools: "),
        Span::styled(
            app.total_tool_calls().to_string(),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::styled(
            "[q]uit [s]ort [p]ause [d]etail [r]eset",
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
