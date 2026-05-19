use chrono::Utc;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Sparkline, Table, TableState},
};

use super::app::{App, FocusPanel, ProjectFilter};
use crate::providers::PROVIDER_REGISTRY;
use crate::scraper::{HostMetrics, LiveSession, OrphanPort, RateLimitInfo, SessionStatus};
use crate::storage::ToolMetrics;

pub fn draw(f: &mut Frame, app: &App) {
    let total_height = f.area().height;
    let live_panel_height = live_panel_height(app, total_height);

    let mut constraints: Vec<Constraint> = vec![
        Constraint::Length(3), // header
        Constraint::Length(3), // metrics bar
    ];
    if live_panel_height > 0 {
        constraints.push(Constraint::Length(live_panel_height));
    }
    constraints.push(Constraint::Min(8)); // unified tools table
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
    draw_tools_table(f, app, chunks[idx]);
    idx += 1;
    draw_footer(f, app, chunks[idx]);

    if app.show_detail {
        draw_detail_popup(f, app);
    }
}

/// Height of the live-state panel (sessions + detail strip + quotas +
/// orphan ports). Returns 0 when there's nothing to show.
///
/// Tries to fit all live sessions, plus a detail strip beneath the table
/// when a session is selected (showing children/subagents). Capped at ~60%
/// of terminal so the tools table always has space.
fn live_panel_height(app: &App, total_height: u16) -> u16 {
    let s = &app.scraper_snapshot;
    let has_sessions = !s.live_sessions.is_empty();
    let has_rate_limits = !s.rate_limits.is_empty();
    let has_orphans = !s.orphan_ports.is_empty();
    if !has_sessions && !has_rate_limits && !has_orphans {
        return 0;
    }
    let max_height = ((total_height as u32 * 6 / 10) as u16).max(8);
    let session_count = s.live_sessions.len() as u16;
    let sessions_height = if has_sessions {
        session_count.saturating_add(3) // top border + header row + bottom border
    } else {
        0
    };
    // Detail strip: 4 lines (chrome + 2 lines of children/subagents) when a
    // live session is selected and the panel is focused.
    let detail_height = if has_sessions { 4 } else { 0 };
    // Quota panel needs room for 2 chrome + header + 5h + 7d rows.
    let quota_height = if has_rate_limits { 5 } else { 0 };
    let orphans_height = if has_orphans { 3 } else { 0 };

    let main_row = sessions_height.max(quota_height);
    (main_row + detail_height + orphans_height).min(max_height)
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
        Span::styled(
            format!("{:.0}%", host.cpu_pct),
            Style::default().fg(cpu_color),
        ),
        Span::raw(" "),
        Span::styled("MEM ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.0}%", host.mem_pct),
            Style::default().fg(mem_color),
        ),
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
    let focused = app.effective_focus() == FocusPanel::Live;

    // Vertical: main row (sessions | quotas), detail strip when a session
    // is selected, and orphan strip if any.
    let mut v_constraints: Vec<Constraint> = vec![Constraint::Min(3)];
    if has_sessions {
        v_constraints.push(Constraint::Length(4)); // detail strip
    }
    if has_orphans {
        v_constraints.push(Constraint::Length(3));
    }
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints(v_constraints)
        .split(area);

    let main_row = vertical[0];

    // Horizontal: sessions on the left, quotas on the right.
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
        draw_live_sessions(f, app, &s.live_sessions, horizontal[h_idx], focused);
        h_idx += 1;
    }
    if has_rate_limits && h_idx < horizontal.len() {
        draw_quota_panel(f, &s.rate_limits, horizontal[h_idx]);
    }

    let mut v_idx = 1;
    if has_sessions {
        draw_session_detail(f, app, vertical[v_idx]);
        v_idx += 1;
    }
    if has_orphans && v_idx < vertical.len() {
        draw_orphan_ports(f, &s.orphan_ports, vertical[v_idx]);
    }
}

fn draw_live_sessions(
    f: &mut Frame,
    app: &App,
    sessions: &[LiveSession],
    area: Rect,
    focused: bool,
) {
    let header_cells = [
        "AGENT", "PROJECT", "STATUS", "MODEL", "CTX", "TOKENS", "MEM", "TASK",
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
    for session in sessions.iter() {
        let model_short = PROVIDER_REGISTRY.shorten_model_name(&session.model);
        // Format as "<used>/<window> <pct>%" so the raw number is visible
        // alongside the ratio. e.g. "120k/200k 60%". This matters because
        // the opus 200k-vs-1M variant isn't always identifiable from the
        // transcript model name.
        let ctx_str = match (session.context_percent, session.context_window) {
            (Some(p), Some(w)) => format!(
                "{}/{} {:.0}%",
                humanize_u64(session.latest_context_tokens),
                humanize_u64(w),
                p * 100.0
            ),
            _ => "—".to_string(),
        };
        let ctx_color = match session.context_percent {
            Some(p) if p >= 0.9 => Color::Red,
            Some(p) if p >= 0.8 => Color::Yellow,
            Some(_) => Color::Green,
            None => Color::DarkGray,
        };
        // Show input + output only. Including cache_read produces inflated
        // numbers (every turn replays the full cached context — a 50-turn
        // session can easily report 10M+ even though actual model work was
        // <1M tokens). Billing-wise, cache reads cost ~10% of input, so
        // omitting them gives a more honest "how much work has this
        // session done" number.
        let tokens = humanize_u64(session.input_tokens + session.output_tokens);
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

    // Bright border when focused so the user knows which panel j/k navigates.
    let border_color = if focused {
        Color::Green
    } else {
        Color::DarkGray
    };
    let title = if focused {
        " Live sessions  [j/k navigate · Tab → Tools] "
    } else {
        " Live sessions  [Tab to focus] "
    };
    let table = Table::new(
        rows,
        [
            Constraint::Length(12), // AGENT
            Constraint::Length(18), // PROJECT
            Constraint::Length(11), // STATUS
            Constraint::Length(10), // MODEL
            Constraint::Length(16), // CTX (used/window pct)
            Constraint::Length(8),  // TOKENS
            Constraint::Length(8),  // MEM
            Constraint::Min(20),    // TASK
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(border_color)),
    )
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );

    let mut state = TableState::default();
    if focused && !sessions.is_empty() {
        state.select(Some(app.live_selected_index));
    }
    f.render_stateful_widget(table, area, &mut state);
}

/// Detail strip beneath the live-sessions table. Shows children processes
/// and subagents for the currently selected session.
fn draw_session_detail(f: &mut Frame, app: &App, area: Rect) {
    let Some(session) = app.selected_live_session() else {
        let p = Paragraph::new(Line::from(Span::styled(
            "  (no live session selected — press Tab then j/k)",
            Style::default().fg(Color::DarkGray),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Selected session detail ")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(p, area);
        return;
    };

    // Children line: pid · cmd · mem · port
    let children_line = if session.children.is_empty() {
        Line::from(Span::styled(
            "  children: (none)",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        let mut spans: Vec<Span<'static>> = vec![Span::styled(
            "  children: ",
            Style::default().fg(Color::DarkGray),
        )];
        for (i, c) in session.children.iter().take(6).enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(
                format!("{}", c.pid),
                Style::default().fg(Color::Cyan),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::raw(c.command.clone()));
            spans.push(Span::styled(
                format!(" ({} MB)", c.mem_kb / 1024),
                Style::default().fg(Color::DarkGray),
            ));
            if let Some(port) = c.port {
                spans.push(Span::styled(
                    format!(" :{}", port),
                    Style::default().fg(Color::Magenta),
                ));
            }
        }
        if session.children.len() > 6 {
            spans.push(Span::styled(
                format!("  +{} more", session.children.len() - 6),
                Style::default().fg(Color::DarkGray),
            ));
        }
        Line::from(spans)
    };

    // Subagents line: name(tokens) [status]
    let subagents_line = if session.subagents.is_empty() {
        Line::from(Span::styled(
            "  subagents: (none)",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        let mut spans: Vec<Span<'static>> = vec![Span::styled(
            "  subagents: ",
            Style::default().fg(Color::DarkGray),
        )];
        for (i, sa) in session.subagents.iter().take(8).enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::raw(sa.name.clone()));
            spans.push(Span::styled(
                format!("({})", humanize_u64(sa.tokens)),
                Style::default().fg(Color::Cyan),
            ));
            if !sa.status.is_empty() {
                spans.push(Span::styled(
                    format!(" [{}]", sa.status),
                    Style::default().fg(Color::Yellow),
                ));
            }
        }
        Line::from(spans)
    };

    let p = Paragraph::new(vec![children_line, subagents_line]).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " Selected: {} · {} ",
                session.project_name, session.session_id
            ))
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(p, area);
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

/// Compute a short type label for the TYPE column. Built-in tools show
/// `"builtin"`; MCP tools show the server name (e.g. `"context7"`). Falls
/// back to `"mcp"` when the tool isn't a built-in but doesn't match the
/// canonical `mcp__server__tool` shape.
fn tool_type_label(tool: &ToolMetrics) -> &'static str {
    if tool.is_builtin() {
        "builtin"
    } else {
        // We can't return a borrowed slice tied to `parse_mcp_tool_name`'s
        // return because it owns its strings. Keep this static for now —
        // the server name itself shows in the tool name column thanks to
        // `display_name()` (e.g. "context7:resolve-library-id").
        "mcp"
    }
}

fn draw_tools_table(f: &mut Frame, app: &App, area: Rect) {
    let header_cells = [
        "TYPE", "TOOL", "CALLS", "ERR", "APR%", "AVG", "RANGE", "LAST", "FREQ",
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
    // Single unified list — built-in + MCP — with a TYPE column to
    // distinguish them. Order is whatever sort_tools last produced.
    let all_tools: Vec<&ToolMetrics> = app.tool_metrics.iter().collect();

    let max_calls = all_tools.iter().map(|t| t.call_count).max().unwrap_or(1);

    let rows: Vec<Row> = all_tools
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

            // For MCP tools, show "server:tool" in the TOOL column via
            // display_name(). The TYPE cell distinguishes builtin vs mcp.
            let display = tool.display_name();
            let type_label = tool_type_label(tool);
            let type_style = if tool.is_builtin() {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::Magenta)
            };
            let freq_style = if tool.is_builtin() {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::Magenta)
            };

            Row::new(vec![
                Cell::from(type_label).style(type_style),
                Cell::from(format!("{}{}", indicator, display)),
                Cell::from(tool.call_count.to_string()),
                Cell::from(tool.error_count.to_string()).style(error_style),
                Cell::from(apr_str).style(apr_style),
                Cell::from(avg_str),
                Cell::from(range_str),
                Cell::from(last_str),
                Cell::from(freq_bar).style(freq_style),
            ])
            .style(style)
        })
        .collect();

    let focused = app.effective_focus() == FocusPanel::Tools;
    let border_color = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let title = if focused {
        format!(
            " Tools ({})  [s sort · d detail · Tab → Live] ",
            app.tool_metrics.len()
        )
    } else {
        format!(" Tools ({})  [Tab to focus] ", app.tool_metrics.len())
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),  // TYPE
            Constraint::Min(20),    // TOOL
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
            .title(title)
            .border_style(Style::default().fg(border_color)),
    )
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = TableState::default();
    if focused {
        state.select(Some(app.selected_index));
    }

    f.render_stateful_widget(table, area, &mut state);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let focus_hint = match app.effective_focus() {
        FocusPanel::Tools => "Tools",
        FocusPanel::Live => "Live",
    };
    let footer = Line::from(vec![
        Span::styled(
            " [q]uit [s]ort [p]ause [d]etail [t]ime [r] project [a]gent [Tab] focus ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("· focus: {}", focus_hint),
            Style::default().fg(Color::Cyan),
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
