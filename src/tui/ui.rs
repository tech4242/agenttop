use chrono::Utc;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Sparkline, Table, TableState},
};

use super::app::{App, ProjectFilter};
use crate::providers::PROVIDER_REGISTRY;
use crate::scraper::{HostMetrics, LiveSession, OrphanPort, RateLimitInfo, SessionStatus};

pub fn draw(f: &mut Frame, app: &App) {
    let has_mcp_tools = !app.mcp_tools().is_empty();
    let live_panel_height = live_panel_height(app);

    // Build a layout that always has header + metrics + footer; everything
    // between is optional.
    let mut constraints: Vec<Constraint> = vec![
        Constraint::Length(3), // header
        Constraint::Length(3), // metrics bar
    ];
    if live_panel_height > 0 {
        constraints.push(Constraint::Length(live_panel_height));
    }
    if has_mcp_tools {
        constraints.push(Constraint::Ratio(1, 2)); // built-in tools
        constraints.push(Constraint::Ratio(1, 2)); // MCP tools
    } else {
        constraints.push(Constraint::Min(8)); // built-in tools
    }
    constraints.push(Constraint::Length(1)); // footer

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    let mut idx = 0;
    draw_header(f, app, chunks[idx]);
    idx += 1;
    draw_metrics_bar(f, app, chunks[idx]);
    idx += 1;
    if live_panel_height > 0 {
        draw_live_panel(f, app, chunks[idx]);
        idx += 1;
    }
    draw_builtin_tool_table(f, app, chunks[idx]);
    idx += 1;
    if has_mcp_tools {
        draw_mcp_table(f, app, chunks[idx]);
        idx += 1;
    }
    draw_footer(f, chunks[idx]);

    if app.show_detail {
        draw_detail_popup(f, app);
    }
}

/// Height of the live-state panel (sessions + quotas + orphan ports). Returns
/// 0 when there's nothing to show so we don't claim empty terminal space.
///
/// IMPORTANT: capped at MAX_LIVE_PANEL_HEIGHT so we don't starve the
/// tools / MCP tables below. The live panel scrolls or truncates internally
/// when there are more sessions than fit — better than pushing the OTLP-side
/// data off-screen.
fn live_panel_height(app: &App) -> u16 {
    const MAX_LIVE_PANEL_HEIGHT: u16 = 10;

    let s = &app.scraper_snapshot;
    let has_sessions = !s.live_sessions.is_empty();
    let has_rate_limits = !s.rate_limits.is_empty();
    let has_orphans = !s.orphan_ports.is_empty();
    if !has_sessions && !has_rate_limits && !has_orphans {
        return 0;
    }
    // One row per visible session (cap at 3 in the table; subagents fold
    // into the same row as a comma-joined summary, so no extra row needed).
    let visible_sessions = s.live_sessions.len().min(3) as u16;
    let sessions_height = if has_sessions {
        visible_sessions.saturating_add(3) // top border + header + bottom border
    } else {
        0
    };
    let quota_height = if has_rate_limits { 3 } else { 0 };
    let orphans_height = if has_orphans { 3 } else { 0 };

    // Layout puts quota + sessions on the same row split horizontally — so
    // we take the max of session/quota for that row, plus orphans below.
    let main_row = sessions_height.max(quota_height);
    (main_row + orphans_height).min(MAX_LIVE_PANEL_HEIGHT)
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let paused = if app.paused { " [PAUSED]" } else { "" };
    let title = format!(" agenttop{}", paused);

    let active_time = app.format_active_time();
    let filter_label = app.time_filter.label();

    let mut header_spans = Vec::new();

    match &app.project_filter {
        ProjectFilter::All => {
            if !app.detected_projects.is_empty() {
                header_spans.push(Span::styled(
                    "Project: ",
                    Style::default().fg(Color::DarkGray),
                ));
                header_spans.push(Span::styled("all", Style::default().fg(Color::Cyan)));
                header_spans.push(Span::raw("  "));
            }
        }
        ProjectFilter::Project(name) => {
            header_spans.push(Span::styled(
                "Project: ",
                Style::default().fg(Color::DarkGray),
            ));
            header_spans.push(Span::styled(name, Style::default().fg(Color::Cyan)));
            header_spans.push(Span::raw("  "));
        }
    }

    if let Some(agent_id) = app.current_agent() {
        let agent_name = PROVIDER_REGISTRY
            .get(agent_id)
            .map(|p| p.name())
            .unwrap_or(agent_id);
        header_spans.push(Span::styled(
            "Agent: ",
            Style::default().fg(Color::DarkGray),
        ));
        header_spans.push(Span::styled(agent_name, Style::default().fg(Color::Cyan)));
        header_spans.push(Span::raw("  "));
    }

    if active_time != "-" {
        header_spans.push(Span::styled(
            "Active: ",
            Style::default().fg(Color::DarkGray),
        ));
        header_spans.push(Span::styled(active_time, Style::default().fg(Color::Cyan)));
        header_spans.push(Span::raw("  "));
    }

    if let Some(summary) = app.format_compaction_summary() {
        header_spans.push(Span::styled(summary, Style::default().fg(Color::Yellow)));
        header_spans.push(Span::raw("  "));
    }

    // Host vitals strip (only when sysinfo has reported something non-zero).
    let host = app.scraper_snapshot.host_metrics;
    if host_has_data(&host) {
        for span in format_host_strip(&host) {
            header_spans.push(span);
        }
        header_spans.push(Span::raw("  "));
    }

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

fn host_has_data(host: &HostMetrics) -> bool {
    host.cpu_pct > 0.0 || host.mem_pct > 0.0 || host.load1 > 0.0
}

fn format_host_strip(host: &HostMetrics) -> Vec<Span<'static>> {
    let cpu_color = pct_color(host.cpu_pct);
    let mem_color = pct_color(host.mem_pct);
    vec![
        Span::styled("CPU ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:.0}%", host.cpu_pct), Style::default().fg(cpu_color)),
        Span::raw(" "),
        Span::styled("MEM ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:.0}%", host.mem_pct), Style::default().fg(mem_color)),
        Span::raw(" "),
        Span::styled("LOAD ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.2}", host.load1),
            Style::default().fg(Color::LightBlue),
        ),
    ]
}

fn pct_color(pct: f64) -> Color {
    if pct >= 90.0 {
        Color::Red
    } else if pct >= 70.0 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn draw_metrics_bar(f: &mut Frame, app: &App, area: Rect) {
    // Split the metrics bar horizontally so we can put a sparkline on the
    // right without disrupting the token/API text layout on the left.
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(22)])
        .split(area);

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
            format!(
                "{:.1}K",
                app.token_metrics.cache_read_tokens as f64 / 1000.0
            ),
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
            metrics_spans.push(Span::styled(
                "Commits: ",
                Style::default().fg(Color::DarkGray),
            ));
            metrics_spans.push(Span::styled(
                commits.to_string(),
                Style::default().fg(Color::Yellow),
            ));
        }
    }

    let metrics_line = Line::from(metrics_spans);

    let api_calls = app.api_metrics.total_calls;
    let api_errors = app.api_metrics.total_errors;
    let api_latency = app.format_api_latency();

    let mut api_spans = vec![
        Span::raw(" API     "),
        Span::styled("Calls: ", Style::default().fg(Color::DarkGray)),
        Span::styled(api_calls.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled("Avg: ", Style::default().fg(Color::DarkGray)),
        Span::styled(api_latency, Style::default().fg(Color::LightBlue)),
    ];

    if api_errors > 0 {
        api_spans.push(Span::raw("  "));
        api_spans.push(Span::styled(
            "Errors: ",
            Style::default().fg(Color::DarkGray),
        ));
        api_spans.push(Span::styled(
            api_errors.to_string(),
            Style::default().fg(Color::Red),
        ));
    }

    if !app.api_metrics.models.is_empty() {
        api_spans.push(Span::raw("  "));
        api_spans.push(Span::styled(
            "Models: ",
            Style::default().fg(Color::DarkGray),
        ));

        let mut models: Vec<_> = app.api_metrics.models.iter().collect();
        models.sort_by(|a, b| b.1.cmp(a.1));

        let model_strs: Vec<String> = models
            .iter()
            .take(3)
            .map(|(name, count)| {
                let short_name = PROVIDER_REGISTRY.shorten_model_name(name);
                format!("{} ({})", short_name, count)
            })
            .collect();

        api_spans.push(Span::styled(
            model_strs.join(", "),
            Style::default().fg(Color::Yellow),
        ));
    }

    api_spans.push(Span::raw("  │  "));
    api_spans.push(Span::styled(
        "Tools: ",
        Style::default().fg(Color::DarkGray),
    ));
    api_spans.push(Span::styled(
        total_calls.to_string(),
        Style::default().fg(Color::Cyan),
    ));

    let api_line = Line::from(api_spans);

    let left_block = Block::default().borders(Borders::LEFT);
    let left_paragraph = Paragraph::new(vec![metrics_line, api_line]).block(left_block);
    f.render_widget(left_paragraph, chunks[0]);

    // Right side: token-rate sparkline (tokens/sec over last 5 min).
    let series: Vec<u64> = app
        .token_rate_series
        .iter()
        .map(|v| v.max(0.0) as u64)
        .collect();
    let peak = series.iter().copied().max().unwrap_or(0);
    let title = if peak > 0 {
        format!(" Tok/s peak {} ", peak)
    } else {
        " Tok/s ".to_string()
    };
    let sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .title(title),
        )
        .data(&series)
        .style(Style::default().fg(Color::Cyan))
        .bar_set(symbols::bar::NINE_LEVELS);
    f.render_widget(sparkline, chunks[1]);
}

fn draw_live_panel(f: &mut Frame, app: &App, area: Rect) {
    let s = &app.scraper_snapshot;
    let has_sessions = !s.live_sessions.is_empty();
    let has_rate_limits = !s.rate_limits.is_empty();
    let has_orphans = !s.orphan_ports.is_empty();

    // Vertical split: main row (sessions | quotas) and (optional) orphans
    // strip below.
    let vertical = if has_orphans {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3)])
            .split(area)
    };

    let main_row = vertical[0];

    // Horizontal split within main row.
    let horizontal = if has_sessions && has_rate_limits {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(60), Constraint::Length(40)])
            .split(main_row)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20)])
            .split(main_row)
    };

    let mut h_idx = 0;
    if has_sessions {
        draw_live_sessions(f, &s.live_sessions, horizontal[h_idx]);
        h_idx += 1;
    }
    if has_rate_limits && h_idx < horizontal.len() {
        draw_quota_panel(f, &s.rate_limits, horizontal[h_idx]);
    }

    if has_orphans && vertical.len() > 1 {
        draw_orphan_ports(f, &s.orphan_ports, vertical[1]);
    }
}

fn draw_live_sessions(f: &mut Frame, sessions: &[LiveSession], area: Rect) {
    let header_cells = [
        "AGENT", "PROJECT", "STATUS", "MODEL", "CTX%", "TOKENS", "MEM", "TASK",
    ]
    .iter()
    .map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    });
    let header = Row::new(header_cells).height(1);

    let mut rows = Vec::new();
    for session in sessions.iter().take(3) {
        let model_short = PROVIDER_REGISTRY.shorten_model_name(&session.model);
        let ctx_str = match session.context_percent {
            Some(p) => format!("{:.0}%", p * 100.0),
            None => "—".to_string(),
        };
        let ctx_color = match session.context_percent {
            Some(p) if p >= 0.9 => Color::Red,
            Some(p) if p >= 0.8 => Color::Yellow,
            Some(_) => Color::Green,
            None => Color::DarkGray,
        };
        let tokens = humanize_u64(
            session.input_tokens
                + session.output_tokens
                + session.cache_read_tokens
                + session.cache_creation_tokens,
        );
        let mem_str = format!("{} MB", session.mem_mb);
        // Inline subagent summary so we don't need a second row per session.
        let mut task = if session.current_task.is_empty() {
            "—".to_string()
        } else {
            session.current_task.clone()
        };
        if !session.subagents.is_empty() {
            let labels: Vec<String> = session
                .subagents
                .iter()
                .take(3)
                .map(|sa| format!("{}({})", sa.name, humanize_u64(sa.tokens)))
                .collect();
            task = format!("{}  · sub: {}", task, labels.join(", "));
        }

        let status_color = status_color(session.status);
        rows.push(Row::new(vec![
            Cell::from(session.agent_id.replace('_', " ")),
            Cell::from(session.project_name.clone()),
            Cell::from(session.status.label()).style(Style::default().fg(status_color)),
            Cell::from(model_short),
            Cell::from(ctx_str).style(Style::default().fg(ctx_color)),
            Cell::from(tokens),
            Cell::from(mem_str),
            Cell::from(task),
        ]));
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(12), // AGENT
            Constraint::Length(18), // PROJECT
            Constraint::Length(11), // STATUS
            Constraint::Length(10), // MODEL
            Constraint::Length(5),  // CTX%
            Constraint::Length(8),  // TOKENS
            Constraint::Length(8),  // MEM
            Constraint::Min(20),    // TASK
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Live sessions ")
            .border_style(Style::default().fg(Color::Green)),
    );

    f.render_widget(table, area);
}

fn status_color(s: SessionStatus) -> Color {
    match s {
        SessionStatus::Thinking => Color::Cyan,
        SessionStatus::Executing => Color::Green,
        SessionStatus::Waiting => Color::DarkGray,
        SessionStatus::RateLimited => Color::Red,
        SessionStatus::Done => Color::DarkGray,
    }
}

fn draw_quota_panel(f: &mut Frame, rate_limits: &[RateLimitInfo], area: Rect) {
    let mut lines = Vec::new();
    for rl in rate_limits {
        let header = Span::styled(
            format!("{} ", capitalize(&rl.source)),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        lines.push(Line::from(vec![header]));

        if let Some(pct) = rl.five_hour_pct {
            lines.push(Line::from(quota_row("5h", pct, rl.five_hour_resets_at)));
        }
        if let Some(pct) = rl.seven_day_pct {
            lines.push(Line::from(quota_row("7d", pct, rl.seven_day_resets_at)));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Quota ")
        .border_style(Style::default().fg(Color::Yellow));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn quota_row(label: &str, pct: f64, resets_at: Option<u64>) -> Vec<Span<'static>> {
    let pct = pct.clamp(0.0, 100.0);
    let color = pct_color(pct);
    let bar = quota_bar(pct, 10);
    let reset = match resets_at {
        Some(ts) => format!("  resets in {}", humanize_eta(ts)),
        None => String::new(),
    };
    vec![
        Span::raw(format!(" {}  ", label)),
        Span::styled(bar, Style::default().fg(color)),
        Span::raw(format!("  {:.0}%", pct)),
        Span::styled(reset, Style::default().fg(Color::DarkGray)),
    ]
}

fn quota_bar(pct: f64, width: usize) -> String {
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn humanize_eta(unix_secs: u64) -> String {
    let now = chrono::Utc::now().timestamp() as u64;
    if unix_secs <= now {
        return "now".to_string();
    }
    let secs = unix_secs - now;
    if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d{}h", secs / 86400, (secs % 86400) / 3600)
    }
}

fn draw_orphan_ports(f: &mut Frame, orphans: &[OrphanPort], area: Rect) {
    let mut spans: Vec<Span<'static>> = vec![Span::styled(
        " Orphan ports: ",
        Style::default().fg(Color::Yellow),
    )];
    for (i, o) in orphans.iter().take(8).enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            format!(":{}", o.port),
            Style::default().fg(Color::Red),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("{}({})", o.command, o.pid),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Orphans ")
        .border_style(Style::default().fg(Color::Red));
    f.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

fn humanize_u64(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
    }
}

fn draw_builtin_tool_table(f: &mut Frame, app: &App, area: Rect) {
    let header_cells = [
        "TOOL", "CALLS", "ERR", "APR%", "AVG", "RANGE", "LAST", "FREQ",
    ]
    .iter()
    .map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    });
    let header = Row::new(header_cells).height(1);

    let now = Utc::now();
    let builtin_tools = app.builtin_tools();

    let max_calls = builtin_tools
        .iter()
        .map(|t| t.call_count)
        .max()
        .unwrap_or(1);

    let rows: Vec<Row> = builtin_tools
        .iter()
        .enumerate()
        .map(|(i, tool)| {
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

            let avg_str = if tool.avg_duration_ms < 1000.0 {
                format!("{}ms", tool.avg_duration_ms as u64)
            } else {
                format!("{:.1}s", tool.avg_duration_ms / 1000.0)
            };

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

            let bar_width = 10;
            let filled = ((tool.call_count as f64 / max_calls as f64) * bar_width as f64) as usize;
            let empty = bar_width - filled;
            let freq_bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

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

            let error_style = if tool.error_count > 0 {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };

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
            Constraint::Min(14),
            Constraint::Length(6),
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Length(5),
            Constraint::Length(10),
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

    let header_cells = [
        "TOOL", "CALLS", "ERR", "APR%", "AVG", "RANGE", "LAST", "FREQ",
    ]
    .iter()
    .map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    });
    let header = Row::new(header_cells).height(1);

    let now = Utc::now();
    let max_calls = mcp_tools.iter().map(|t| t.call_count).max().unwrap_or(1);

    let rows: Vec<Row> = mcp_tools
        .iter()
        .map(|tool| {
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

            let avg_str = if tool.avg_duration_ms < 1000.0 {
                format!("{}ms", tool.avg_duration_ms as u64)
            } else {
                format!("{:.1}s", tool.avg_duration_ms / 1000.0)
            };

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

            let bar_width = 10;
            let filled = ((tool.call_count as f64 / max_calls as f64) * bar_width as f64) as usize;
            let empty = bar_width - filled;
            let freq_bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

            let indicator = if tool
                .last_call
                .map(|l| (now - l).num_seconds() < 2)
                .unwrap_or(false)
            {
                "▶ "
            } else {
                "  "
            };

            let error_style = if tool.error_count > 0 {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };

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
            Constraint::Min(14),
            Constraint::Length(6),
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Length(5),
            Constraint::Length(10),
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
    let footer = Line::from(vec![Span::styled(
        " [q]uit [s]ort [p]ause [d]etail [t]ime [r] project [a]gent",
        Style::default().fg(Color::DarkGray),
    )]);

    let paragraph = Paragraph::new(footer);
    f.render_widget(paragraph, area);
}

fn draw_detail_popup(f: &mut Frame, app: &App) {
    let Some(tool) = app.selected_tool() else {
        return;
    };

    let area = centered_rect(60, 60, f.area());
    f.render_widget(Clear, area);

    let success_rate = if tool.call_count > 0 {
        (tool.success_count as f64 / tool.call_count as f64) * 100.0
    } else {
        100.0
    };

    let format_duration = |ms: f64| -> String {
        if ms < 1000.0 {
            format!("{:.0}ms", ms)
        } else {
            format!("{:.1}s", ms / 1000.0)
        }
    };

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

    if let Some(last_error) = app.get_selected_tool_last_error() {
        content.push(Line::from(""));
        content.push(Line::from(vec![Span::styled(
            "Last Error: ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]));
        let error_display = if last_error.len() > 120 {
            format!("{}...", &last_error[..117])
        } else {
            last_error
        };
        content.push(Line::from(vec![Span::styled(
            error_display,
            Style::default().fg(Color::Red),
        )]));
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
