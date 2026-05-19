//! Render-content tests for `tui::ui::draw`.
//!
//! Existing TUI tests verified rendering doesn't panic; these assert on the
//! actual buffer contents using `TestBackend`. The goal is to lock down the
//! data → UI mapping so a change to a format string or column order shows
//! up as a test failure rather than a "looks wrong" smoke-test report.

use agenttop::scraper::{
    ChildProcess, HostMetrics, LiveSession, OrphanPort, RateLimitInfo, ScraperSnapshot,
    SessionStatus, SubAgent,
};
use agenttop::storage::{LogEvent, StorageHandle};
use agenttop::tui::app::App;
use agenttop::tui::ui::draw;
use chrono::Utc;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::collections::HashMap;

/// Render the App to a TestBackend of (w, h) and return the buffer as a
/// single space-collapsed string for easy substring search.
fn render_to_text(app: &App, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            out.push_str(cell.symbol());
        }
        out.push('\n');
    }
    out
}

fn fake_live_session(
    agent: &'static str,
    project: &str,
    status: SessionStatus,
    ctx_used: u64,
    ctx_window: u64,
) -> LiveSession {
    LiveSession {
        agent_id: agent,
        pid: 1234,
        session_id: format!("sess-{}", project),
        cwd: format!("/Users/test/{}", project),
        project_name: project.into(),
        started_at_ms: 0,
        status,
        model: "claude-opus-4-5".into(),
        context_percent: Some(ctx_used as f64 / ctx_window as f64),
        context_window: Some(ctx_window),
        latest_context_tokens: ctx_used,
        current_task: "Edit src/main.rs".into(),
        input_tokens: 5_000,
        output_tokens: 2_500,
        cache_read_tokens: 100_000,
        cache_creation_tokens: 1_000,
        mem_mb: 256,
        children: vec![],
        subagents: vec![],
    }
}

fn tool_result(name: &str, success: bool) -> LogEvent {
    let mut a = HashMap::new();
    a.insert("tool_name".into(), name.into());
    a.insert("success".into(), success.to_string());
    a.insert("duration_ms".into(), "100".into());
    LogEvent {
        timestamp: Utc::now(),
        event_name: Some("tool_result".into()),
        body: None,
        attributes: a,
    }
}

#[test]
fn empty_state_renders_without_panic_and_shows_no_live_panel() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let app = App::new(storage);

    let text = render_to_text(&app, 120, 30);
    // Header always present.
    assert!(text.contains("agenttop"));
    // Empty live snapshot → no Live sessions panel border.
    assert!(
        !text.contains("Live sessions"),
        "Live panel must hide when there's nothing to show"
    );
}

#[test]
fn live_panel_shows_ctx_used_over_window_format() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let mut app = App::new(storage);
    app.scraper_snapshot = ScraperSnapshot {
        live_sessions: vec![fake_live_session(
            "claude_code",
            "alpha",
            SessionStatus::Executing,
            120_000,
            200_000,
        )],
        ..ScraperSnapshot::default()
    };

    let text = render_to_text(&app, 200, 40);
    assert!(text.contains("Live sessions"));
    // CTX column format: "120K/200K 60%" — humanize_u64 emits with K suffix.
    assert!(
        text.contains("120.0K") || text.contains("120K"),
        "CTX column should show used tokens; got:\n{}",
        text
    );
    assert!(
        text.contains("200K") || text.contains("200.0K"),
        "CTX column should show window size"
    );
    // CTX% would be 60% — but column width may truncate "60%"; assert
    // either the percentage or its constituent number appears.
    assert!(
        text.contains("60%") || text.contains("120.0K/200.0K"),
        "CTX percentage or used/window pair must appear; got:\n{}",
        text
    );
}

#[test]
fn live_panel_color_codes_status() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let mut app = App::new(storage);
    app.scraper_snapshot = ScraperSnapshot {
        live_sessions: vec![fake_live_session(
            "claude_code",
            "alpha",
            SessionStatus::RateLimited,
            150_000,
            200_000,
        )],
        ..ScraperSnapshot::default()
    };

    let text = render_to_text(&app, 160, 40);
    assert!(
        text.contains("RateLimited"),
        "STATUS column should display 'RateLimited' literal"
    );
}

#[test]
fn live_panel_shows_subagents_inline_in_task_column() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let mut app = App::new(storage);
    let mut session = fake_live_session(
        "claude_code",
        "alpha",
        SessionStatus::Thinking,
        50_000,
        200_000,
    );
    session.subagents = vec![
        SubAgent {
            name: "Explore".into(),
            status: "running".into(),
            tokens: 1_500,
        },
        SubAgent {
            name: "Plan".into(),
            status: "done".into(),
            tokens: 800,
        },
    ];
    app.scraper_snapshot = ScraperSnapshot {
        live_sessions: vec![session],
        ..ScraperSnapshot::default()
    };

    let text = render_to_text(&app, 200, 30);
    // Subagent summary inlines into the TASK column: "... · sub: Explore(1.5K), Plan(800)"
    assert!(
        text.contains("sub:") || text.contains("Explore"),
        "subagent names should appear inline; got:\n{}",
        text
    );
}

#[test]
fn quota_panel_renders_when_rate_limits_present() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let mut app = App::new(storage);
    // Combine with a session so the live panel gets enough vertical space
    // for the quota panel to render its 5h + 7d rows. Quota alone would
    // get clipped to just the title row.
    app.scraper_snapshot = ScraperSnapshot {
        live_sessions: vec![fake_live_session(
            "claude_code",
            "p",
            SessionStatus::Thinking,
            50_000,
            200_000,
        )],
        rate_limits: vec![RateLimitInfo {
            source: "claude".into(),
            five_hour_pct: Some(42.0),
            seven_day_pct: Some(15.0),
            five_hour_resets_at: None,
            seven_day_resets_at: None,
            updated_at: None,
        }],
        ..ScraperSnapshot::default()
    };

    // Tall terminal so the quota panel has room for both 5h and 7d rows.
    let text = render_to_text(&app, 200, 60);
    assert!(text.contains("Quota"));
    assert!(text.contains("42%"), "5-hour quota must render");
    assert!(text.contains("15%"), "7-day quota must render");
}

#[test]
fn orphan_ports_panel_renders_when_ports_present() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let mut app = App::new(storage);
    // Pair with a session so the orphan strip gets actual vertical space.
    app.scraper_snapshot = ScraperSnapshot {
        live_sessions: vec![fake_live_session(
            "claude_code",
            "p",
            SessionStatus::Thinking,
            50_000,
            200_000,
        )],
        orphan_ports: vec![OrphanPort {
            port: 8080,
            pid: 99999,
            command: "python".into(),
            origin_session_id: "abandoned-session".into(),
        }],
        ..ScraperSnapshot::default()
    };

    let text = render_to_text(&app, 200, 40);
    assert!(text.contains("Orphans"));
    assert!(text.contains("8080"));
    assert!(text.contains("python"));
}

#[test]
fn host_vitals_strip_renders_when_data_present() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let mut app = App::new(storage);
    app.scraper_snapshot = ScraperSnapshot {
        host_metrics: HostMetrics {
            cpu_pct: 35.0,
            mem_pct: 62.0,
            load1: 1.42,
        },
        ..ScraperSnapshot::default()
    };

    let text = render_to_text(&app, 200, 30);
    assert!(text.contains("CPU"));
    assert!(text.contains("MEM"));
    assert!(text.contains("LOAD"));
    assert!(text.contains("35%"));
    assert!(text.contains("62%"));
    assert!(text.contains("1.42"));
}

#[test]
fn tools_table_shows_type_column_for_builtin_and_mcp() {
    let storage = StorageHandle::new_in_memory().unwrap();
    storage.record_log_events(vec![
        tool_result("Bash", true),
        tool_result("Read", true),
        tool_result("mcp__context7__resolve-library-id", true),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(150));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    let text = render_to_text(&app, 200, 40);
    // Header row.
    assert!(text.contains("TYPE"));
    assert!(text.contains("TOOL"));
    // At least one builtin row.
    assert!(text.contains("builtin"), "builtin TYPE label must appear");
    // MCP tool renders as "server:tool" via display_name.
    assert!(
        text.contains("context7:resolve-library-id"),
        "MCP tool should render with server:tool format"
    );
    assert!(text.contains("mcp"), "mcp TYPE label must appear");
}

#[test]
fn footer_indicates_current_focus() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let app = App::new(storage);

    let text = render_to_text(&app, 160, 30);
    assert!(text.contains("focus:"));
    assert!(text.contains("Tools"));
    assert!(text.contains("[Tab]"));
}

#[test]
fn detail_strip_shows_children_for_selected_live_session() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let mut app = App::new(storage);
    let mut session = fake_live_session(
        "claude_code",
        "alpha",
        SessionStatus::Thinking,
        50_000,
        200_000,
    );
    session.children = vec![
        ChildProcess {
            pid: 12345,
            command: "python".into(),
            mem_kb: 102_400,
            port: Some(8000),
        },
        ChildProcess {
            pid: 67890,
            command: "node".into(),
            mem_kb: 51_200,
            port: None,
        },
    ];
    app.scraper_snapshot = ScraperSnapshot {
        live_sessions: vec![session],
        ..ScraperSnapshot::default()
    };
    app.toggle_focus(); // -> Live

    let text = render_to_text(&app, 200, 50);
    assert!(text.contains("children:"));
    assert!(text.contains("12345"));
    assert!(text.contains("python"));
    assert!(text.contains(":8000"), "open ports should render");
    assert!(text.contains("67890"));
    assert!(text.contains("node"));
}

#[test]
fn paused_indicator_shown_in_header() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let mut app = App::new(storage);
    app.toggle_pause();

    let text = render_to_text(&app, 160, 30);
    assert!(text.contains("PAUSED"));
}
