use chrono::Utc;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};

use super::app::App;

/// Shorten model names for display (e.g., "claude-sonnet-4-20250514" -> "sonnet-4")
fn shorten_model_name(name: &str) -> String {
    // Try to extract the key part of the model name
    let name = name.to_lowercase();

    // Common patterns to simplify
    if name.contains("opus") {
        if name.contains("4.5") || name.contains("4-5") {
            return "opus-4.5".to_string();
        }
        if let Some(ver) = extract_version(&name) {
            return format!("opus-{}", ver);
        }
        return "opus".to_string();
    }
    if name.contains("sonnet") {
        if let Some(ver) = extract_version(&name) {
            return format!("sonnet-{}", ver);
        }
        return "sonnet".to_string();
    }
    if name.contains("haiku") {
        if let Some(ver) = extract_version(&name) {
            return format!("haiku-{}", ver);
        }
        return "haiku".to_string();
    }
    if name.contains("gpt-4") {
        return "gpt-4".to_string();
    }
    if name.contains("gpt-3") {
        return "gpt-3.5".to_string();
    }

    // Fallback: take first 12 chars
    if name.len() > 12 {
        format!("{}...", &name[..12])
    } else {
        name
    }
}

/// Extract version number from model name (e.g., "4" from "claude-sonnet-4-20250514")
fn extract_version(name: &str) -> Option<&str> {
    // Look for patterns like "-4-" or "-3-" or "-4.5-"
    for pattern in ["-4.5-", "-4-", "-3.5-", "-3-", "-5-"] {
        if name.contains(pattern) {
            return Some(pattern.trim_matches('-'));
        }
    }
    // Check for version at end like "-4" or "-3"
    if name.ends_with("-4") || name.ends_with("-5") || name.ends_with("-3") {
        return name.rsplit('-').next();
    }
    None
}

pub fn draw(f: &mut Frame, app: &App) {
    let has_mcp_tools = !app.mcp_tools().is_empty();

    let chunks = if has_mcp_tools {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Header with session info
                Constraint::Length(3),  // Metrics bar (tokens + tools summary)
                Constraint::Ratio(1, 2), // Built-in tools table (50%)
                Constraint::Ratio(1, 2), // MCP tools section (50%)
                Constraint::Length(1),  // Footer (hotkeys only)
            ])
            .split(f.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header with session info
                Constraint::Length(3), // Metrics bar (tokens + tools summary)
                Constraint::Min(8),    // Built-in tools table
                Constraint::Length(1), // Footer (hotkeys only)
            ])
            .split(f.area())
    };

    draw_header(f, app, chunks[0]);
    draw_metrics_bar(f, app, chunks[1]);
    draw_builtin_tool_table(f, app, chunks[2]);

    if has_mcp_tools {
        draw_mcp_table(f, app, chunks[3]);
        draw_footer(f, chunks[4]);
    } else {
        draw_footer(f, chunks[3]);
    }

    // Draw detail popup if active
    if app.show_detail {
        draw_detail_popup(f, app);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let paused = if app.paused { " [PAUSED]" } else { "" };
    let title = format!(" agenttop{}", paused);

    // Build header right side: active time, cost, time filter
    let active_time = app.format_active_time();
    let cost = app.token_metrics.total_cost_usd;
    let filter_label = app.time_filter.label();

    let mut header_spans = Vec::new();

    // Add active time if available
    if active_time != "-" {
        header_spans.push(Span::styled("Active: ", Style::default().fg(Color::DarkGray)));
        header_spans.push(Span::styled(active_time, Style::default().fg(Color::Cyan)));
        header_spans.push(Span::raw("  "));
    }

    // Add cost
    header_spans.push(Span::styled(
        format!("${:.2}", cost),
        Style::default().fg(Color::Yellow),
    ));
    header_spans.push(Span::raw("  "));

    // Add time filter
    header_spans.push(Span::styled(
        format!("[{}]", filter_label),
        Style::default().fg(Color::DarkGray),
    ));

    let header_content = Line::from(header_spans);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(header_content)
        .alignment(ratatui::layout::Alignment::Right)
        .block(block);
    f.render_widget(paragraph, area);
}

fn draw_metrics_bar(f: &mut Frame, app: &App, area: Rect) {
    let cache_reuse = app.cache_reuse_rate();
    let total_calls = app.total_tool_calls();

    let mut metrics_spans = vec![
        Span::raw(" Tokens  "),
        Span::styled("In: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.1}K", app.token_metrics.input_tokens as f64 / 1000.0),
            Style::default().fg(Color::LightBlue),
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
            format!("{:.0}% reuse", cache_reuse),
            Style::default().fg(if cache_reuse > 80.0 {
                Color::Green
            } else if cache_reuse > 50.0 {
                Color::Yellow
            } else {
                Color::Red
            }),
        ),
        Span::raw(")"),
    ];

    // Add LOC and Commits if available
    let loc = app.session_metrics.lines_of_code;
    let commits = app.session_metrics.commit_count;
    if loc != 0 || commits > 0 {
        metrics_spans.push(Span::raw("  "));
        if loc != 0 {
            metrics_spans.push(Span::styled("LOC: ", Style::default().fg(Color::DarkGray)));
            let loc_str = if loc >= 0 {
                format!("+{}", loc)
            } else {
                format!("{}", loc)
            };
            metrics_spans.push(Span::styled(
                loc_str,
                Style::default().fg(if loc >= 0 { Color::Green } else { Color::Red }),
            ));
        }
        if commits > 0 {
            if loc != 0 {
                metrics_spans.push(Span::raw("  "));
            }
            metrics_spans.push(Span::styled("Commits: ", Style::default().fg(Color::DarkGray)));
            metrics_spans.push(Span::styled(
                commits.to_string(),
                Style::default().fg(Color::Yellow),
            ));
        }
    }

    let metrics_line = Line::from(metrics_spans);

    // Second line: API summary and tool stats
    let api_calls = app.api_metrics.total_calls;
    let api_errors = app.api_metrics.total_errors;
    let api_latency = app.format_api_latency();

    let mut api_spans = vec![
        Span::raw(" API     "),
        Span::styled("Calls: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            api_calls.to_string(),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::styled("Avg: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            api_latency,
            Style::default().fg(Color::LightBlue),
        ),
    ];

    if api_errors > 0 {
        api_spans.push(Span::raw("  "));
        api_spans.push(Span::styled("Errors: ", Style::default().fg(Color::DarkGray)));
        api_spans.push(Span::styled(
            api_errors.to_string(),
            Style::default().fg(Color::Red),
        ));
    }

    // Add model breakdown if available
    if !app.api_metrics.models.is_empty() {
        api_spans.push(Span::raw("  "));
        api_spans.push(Span::styled("Models: ", Style::default().fg(Color::DarkGray)));

        // Sort models by count descending and format as "model (count)"
        let mut models: Vec<_> = app.api_metrics.models.iter().collect();
        models.sort_by(|a, b| b.1.cmp(a.1));

        let model_strs: Vec<String> = models
            .iter()
            .take(3) // Show top 3 models max
            .map(|(name, count)| {
                // Shorten model names (e.g., "claude-sonnet-4-20250514" -> "sonnet-4")
                let short_name = shorten_model_name(name);
                format!("{} ({})", short_name, count)
            })
            .collect();

        api_spans.push(Span::styled(
            model_strs.join(", "),
            Style::default().fg(Color::Yellow),
        ));
    }

    api_spans.push(Span::raw("  │  "));
    api_spans.push(Span::styled("Tools: ", Style::default().fg(Color::DarkGray)));
    api_spans.push(Span::styled(
        total_calls.to_string(),
        Style::default().fg(Color::Cyan),
    ));

    let api_line = Line::from(api_spans);

    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT);

    let paragraph = Paragraph::new(vec![metrics_line, api_line]).block(block);
    f.render_widget(paragraph, area);
}

fn draw_builtin_tool_table(f: &mut Frame, app: &App, area: Rect) {
    let header_cells = ["TOOL", "CALLS", "ERR", "APR%", "AVG", "RANGE", "LAST", "FREQ"].iter().map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    });
    let header = Row::new(header_cells).height(1);

    let now = Utc::now();
    let builtin_tools = app.builtin_tools();

    // Calculate max calls from built-in tools only for the frequency bar
    let max_calls = builtin_tools
        .iter()
        .map(|t| t.call_count)
        .max()
        .unwrap_or(1);

    let rows: Vec<Row> = builtin_tools
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            // Calculate time since last call
            let last_str = match tool.last_call {
                Some(last) => {
                    let diff = now - last;
                    let secs = diff.num_seconds();
                    if secs < 0 {
                        "-".to_string()
                    } else if secs < 60 {
                        format!("{}s", secs)
                    } else if secs < 3600 {
                        format!("{}m", secs / 60)
                    } else if secs < 86400 {
                        format!("{}h", secs / 3600)
                    } else {
                        format!("{}d", secs / 86400)
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

            // Format duration range (min-max)
            let format_duration = |ms: f64| -> String {
                if ms < 1000.0 {
                    format!("{}ms", ms as u64)
                } else {
                    format!("{:.1}s", ms / 1000.0)
                }
            };
            let range_str = format!(
                "{}-{}",
                format_duration(tool.min_duration_ms),
                format_duration(tool.max_duration_ms)
            );

            // Create frequency bar (relative call frequency like htop CPU bars)
            let bar_width = 10;
            let filled = ((tool.call_count as f64 / max_calls as f64) * bar_width as f64) as usize;
            let empty = bar_width - filled;
            let freq_bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

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

            // Error count style (red if > 0)
            let error_style = if tool.error_count > 0 {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };

            // Approval rate formatting
            let approval_rate = tool.approval_rate();
            let apr_str = format!("{:.0}%", approval_rate);
            let apr_style = if approval_rate >= 95.0 {
                Style::default().fg(Color::Green)
            } else if approval_rate >= 80.0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Red)
            };

            Row::new(vec![
                Cell::from(format!("{}{}", indicator, tool.tool_name)),
                Cell::from(tool.call_count.to_string()),
                Cell::from(tool.error_count.to_string()).style(error_style),
                Cell::from(apr_str).style(apr_style),
                Cell::from(avg_str),
                Cell::from(range_str),
                Cell::from(last_str),
                Cell::from(freq_bar).style(Style::default().fg(Color::Cyan)),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(14),    // TOOL
            Constraint::Length(6),  // CALLS
            Constraint::Length(4),  // ERR
            Constraint::Length(5),  // APR%
            Constraint::Length(7),  // AVG
            Constraint::Length(12), // RANGE
            Constraint::Length(5),  // LAST
            Constraint::Length(10), // FREQ
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

fn draw_mcp_table(f: &mut Frame, app: &App, area: Rect) {
    let mcp_tools = app.mcp_tools();

    if mcp_tools.is_empty() {
        return;
    }

    let header_cells = ["TOOL", "CALLS", "ERR", "APR%", "AVG", "RANGE", "LAST", "FREQ"].iter().map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    });
    let header = Row::new(header_cells).height(1);

    let now = Utc::now();

    // Calculate max calls from MCP tools only for the frequency bar
    let max_calls = mcp_tools
        .iter()
        .map(|t| t.call_count)
        .max()
        .unwrap_or(1);

    let rows: Vec<Row> = mcp_tools
        .iter()
        .map(|tool| {
            // Calculate time since last call
            let last_str = match tool.last_call {
                Some(last) => {
                    let diff = now - last;
                    let secs = diff.num_seconds();
                    if secs < 0 {
                        "-".to_string()
                    } else if secs < 60 {
                        format!("{}s", secs)
                    } else if secs < 3600 {
                        format!("{}m", secs / 60)
                    } else if secs < 86400 {
                        format!("{}h", secs / 3600)
                    } else {
                        format!("{}d", secs / 86400)
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

            // Format duration range (min-max)
            let format_duration = |ms: f64| -> String {
                if ms < 1000.0 {
                    format!("{}ms", ms as u64)
                } else {
                    format!("{:.1}s", ms / 1000.0)
                }
            };
            let range_str = format!(
                "{}-{}",
                format_duration(tool.min_duration_ms),
                format_duration(tool.max_duration_ms)
            );

            // Create frequency bar
            let bar_width = 10;
            let filled = ((tool.call_count as f64 / max_calls as f64) * bar_width as f64) as usize;
            let empty = bar_width - filled;
            let freq_bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

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

            // Error count style (red if > 0)
            let error_style = if tool.error_count > 0 {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };

            // Approval rate formatting
            let approval_rate = tool.approval_rate();
            let apr_str = format!("{:.0}%", approval_rate);
            let apr_style = if approval_rate >= 95.0 {
                Style::default().fg(Color::Green)
            } else if approval_rate >= 80.0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Red)
            };

            // Use display_name() for MCP tools to show "server:tool" format
            Row::new(vec![
                Cell::from(format!("{}{}", indicator, tool.display_name())),
                Cell::from(tool.call_count.to_string()),
                Cell::from(tool.error_count.to_string()).style(error_style),
                Cell::from(apr_str).style(apr_style),
                Cell::from(avg_str),
                Cell::from(range_str),
                Cell::from(last_str),
                Cell::from(freq_bar).style(Style::default().fg(Color::Magenta)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(14),    // TOOL
            Constraint::Length(6),  // CALLS
            Constraint::Length(4),  // ERR
            Constraint::Length(5),  // APR%
            Constraint::Length(7),  // AVG
            Constraint::Length(12), // RANGE
            Constraint::Length(5),  // LAST
            Constraint::Length(10), // FREQ
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" MCP Tools ")
            .border_style(Style::default().fg(Color::Magenta)),
    );

    f.render_widget(table, area);
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

    let area = centered_rect(60, 60, f.area());

    // Clear the area
    f.render_widget(Clear, area);

    let success_rate = if tool.call_count > 0 {
        (tool.success_count as f64 / tool.call_count as f64) * 100.0
    } else {
        100.0
    };

    // Format duration range
    let format_duration = |ms: f64| -> String {
        if ms < 1000.0 {
            format!("{:.0}ms", ms)
        } else {
            format!("{:.1}s", ms / 1000.0)
        }
    };

    // Use display_name() for MCP tools to show "server:tool" format
    let display_name = tool.display_name();
    let mut content = vec![
        Line::from(vec![
            Span::styled("Tool: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&display_name),
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
                format_duration(tool.avg_duration_ms),
                Style::default().fg(Color::LightBlue),
            ),
        ]),
        Line::from(vec![
            Span::raw("Min Duration: "),
            Span::styled(
                format_duration(tool.min_duration_ms),
                Style::default().fg(Color::LightBlue),
            ),
        ]),
        Line::from(vec![
            Span::raw("Max Duration: "),
            Span::styled(
                format_duration(tool.max_duration_ms),
                Style::default().fg(Color::LightBlue),
            ),
        ]),
    ];

    // Add last error if present
    if let Some(last_error) = app.get_selected_tool_last_error() {
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::styled("Last Error: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        ]));
        // Truncate error message if too long (max ~60 chars per line, 2 lines)
        let error_display = if last_error.len() > 120 {
            format!("{}...", &last_error[..117])
        } else {
            last_error
        };
        content.push(Line::from(vec![
            Span::styled(error_display, Style::default().fg(Color::Red)),
        ]));
    }

    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "Press ESC or Enter to close",
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(content).block(
        Block::default()
            .title(format!(" {} Details ", display_name))
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
