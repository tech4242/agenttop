//! State-transition tests for `tui::app::App`.
//!
//! Targets the un-tested branches of cycle_*, toggle_*, and select_*
//! methods. Each tests is self-contained — uses in-memory storage so no
//! filesystem or network state leaks across runs.

use agenttop::scraper::{LiveSession, ScraperSnapshot, SessionStatus};
use agenttop::storage::{LogEvent, StorageHandle};
use agenttop::tui::app::{App, FocusPanel, ProjectFilter, SortColumn, TimeFilter};
use chrono::Utc;
use std::collections::HashMap;

fn make_app() -> App {
    let storage = StorageHandle::new_in_memory().unwrap();
    App::new(storage)
}

fn tool_event(name: &str, success: bool, duration: u64) -> LogEvent {
    let mut a = HashMap::new();
    a.insert("tool_name".into(), name.into());
    a.insert("success".into(), success.to_string());
    a.insert("duration_ms".into(), duration.to_string());
    LogEvent {
        timestamp: Utc::now(),
        event_name: Some("tool_result".into()),
        body: None,
        attributes: a,
    }
}

fn fake_live_session(agent: &'static str, project: &str, session_id: &str) -> LiveSession {
    LiveSession {
        agent_id: agent,
        pid: 1234,
        session_id: session_id.into(),
        cwd: format!("/Users/test/{}", project),
        project_name: project.into(),
        started_at_ms: 0,
        status: SessionStatus::Thinking,
        model: "claude-opus-4-5".into(),
        context_percent: Some(0.42),
        context_window: Some(200_000),
        latest_context_tokens: 84_000,
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

// ---------- toggle_sort cycle ----------

#[test]
fn toggle_sort_cycles_all_five_columns() {
    let mut app = make_app();
    assert_eq!(app.sort_by, SortColumn::Calls);
    app.toggle_sort();
    assert_eq!(app.sort_by, SortColumn::LastCall);
    app.toggle_sort();
    assert_eq!(app.sort_by, SortColumn::AvgDuration);
    app.toggle_sort();
    assert_eq!(app.sort_by, SortColumn::Name);
    app.toggle_sort();
    assert_eq!(app.sort_by, SortColumn::Type);
    app.toggle_sort();
    assert_eq!(app.sort_by, SortColumn::Calls, "must wrap back to Calls");
}

// ---------- toggle_time_filter cycle ----------

#[test]
fn toggle_time_filter_cycles_through_all_windows() {
    let mut app = make_app();
    assert_eq!(app.time_filter, TimeFilter::AllTime);
    app.toggle_time_filter();
    assert_eq!(app.time_filter, TimeFilter::LastHour);
    app.toggle_time_filter();
    assert_eq!(app.time_filter, TimeFilter::Last24Hours);
    app.toggle_time_filter();
    assert_eq!(app.time_filter, TimeFilter::Last7Days);
    app.toggle_time_filter();
    assert_eq!(
        app.time_filter,
        TimeFilter::AllTime,
        "must wrap from Last7Days back to AllTime"
    );
}

// ---------- toggle_pause / detail ----------

#[test]
fn toggle_pause_flips_state() {
    let mut app = make_app();
    assert!(!app.paused);
    app.toggle_pause();
    assert!(app.paused);
    app.toggle_pause();
    assert!(!app.paused);
}

#[test]
fn toggle_detail_and_close_detail() {
    let mut app = make_app();
    assert!(!app.show_detail);
    app.toggle_detail();
    assert!(app.show_detail);
    app.close_detail();
    assert!(!app.show_detail);
}

// ---------- selection wrap-around (Tools focus, default) ----------

#[test]
fn select_next_wraps_at_end() {
    let storage = StorageHandle::new_in_memory().unwrap();
    storage.record_log_events(vec![
        tool_event("Bash", true, 100),
        tool_event("Edit", true, 50),
        tool_event("Read", true, 25),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(150));

    let mut app = App::new(storage);
    app.refresh().unwrap();
    assert_eq!(app.tool_metrics.len(), 3);

    app.selected_index = app.tool_metrics.len() - 1;
    app.select_next();
    assert_eq!(app.selected_index, 0, "must wrap to first");
}

#[test]
fn select_previous_wraps_at_zero() {
    let storage = StorageHandle::new_in_memory().unwrap();
    storage.record_log_events(vec![
        tool_event("Bash", true, 100),
        tool_event("Edit", true, 50),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(150));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    app.selected_index = 0;
    app.select_previous();
    assert_eq!(
        app.selected_index,
        app.tool_metrics.len() - 1,
        "must wrap to last"
    );
}

#[test]
fn select_does_not_panic_on_empty_metrics() {
    let mut app = make_app();
    // Empty tool_metrics + empty live_sessions.
    app.select_next();
    app.select_previous();
    assert_eq!(app.selected_index, 0);
}

// ---------- focus switching ----------

#[test]
fn toggle_focus_stays_on_tools_when_no_live_sessions() {
    let mut app = make_app();
    assert_eq!(app.effective_focus(), FocusPanel::Tools);
    app.toggle_focus();
    assert_eq!(
        app.effective_focus(),
        FocusPanel::Tools,
        "no live sessions -> can't switch to Live"
    );
}

#[test]
fn toggle_focus_cycles_tools_and_live_when_sessions_present() {
    let mut app = make_app();
    app.scraper_snapshot = ScraperSnapshot {
        live_sessions: vec![fake_live_session("claude_code", "proj", "s1")],
        ..ScraperSnapshot::default()
    };

    assert_eq!(app.effective_focus(), FocusPanel::Tools);
    app.toggle_focus();
    assert_eq!(app.effective_focus(), FocusPanel::Live);
    app.toggle_focus();
    assert_eq!(app.effective_focus(), FocusPanel::Tools);
}

#[test]
fn effective_focus_falls_back_when_focus_live_but_sessions_disappear() {
    let mut app = make_app();
    app.scraper_snapshot = ScraperSnapshot {
        live_sessions: vec![fake_live_session("claude_code", "proj", "s1")],
        ..ScraperSnapshot::default()
    };
    app.toggle_focus();
    assert_eq!(app.effective_focus(), FocusPanel::Live);

    // Sessions vanish (e.g. scraper tick returned empty list).
    app.scraper_snapshot = ScraperSnapshot::default();
    assert_eq!(
        app.effective_focus(),
        FocusPanel::Tools,
        "focus must fall back to Tools when live list is empty"
    );
}

// ---------- live session selection ----------

#[test]
fn select_next_navigates_live_sessions_when_focused() {
    let mut app = make_app();
    app.scraper_snapshot = ScraperSnapshot {
        live_sessions: vec![
            fake_live_session("claude_code", "p1", "s1"),
            fake_live_session("claude_code", "p2", "s2"),
            fake_live_session("claude_code", "p3", "s3"),
        ],
        ..ScraperSnapshot::default()
    };
    app.toggle_focus(); // -> Live

    assert_eq!(app.live_selected_index, 0);
    app.select_next();
    assert_eq!(app.live_selected_index, 1);
    app.select_next();
    assert_eq!(app.live_selected_index, 2);
    app.select_next();
    assert_eq!(app.live_selected_index, 0, "wraps");

    app.select_previous();
    assert_eq!(app.live_selected_index, 2, "wraps backwards");
}

#[test]
fn selected_live_session_returns_correct_entry() {
    let mut app = make_app();
    app.scraper_snapshot = ScraperSnapshot {
        live_sessions: vec![
            fake_live_session("claude_code", "p1", "s1"),
            fake_live_session("claude_code", "p2", "s2"),
        ],
        ..ScraperSnapshot::default()
    };
    app.live_selected_index = 1;
    let selected = app.selected_live_session().unwrap();
    assert_eq!(selected.session_id, "s2");
}

// ---------- agent cycling ----------

#[test]
fn cycle_agent_walks_detected_list() {
    let mut app = make_app();
    app.add_detected_agent("claude_code");
    app.add_detected_agent("codex");
    app.add_detected_agent("gemini_cli");

    assert_eq!(app.current_agent(), Some("claude_code"));
    app.cycle_agent();
    assert_eq!(app.current_agent(), Some("codex"));
    app.cycle_agent();
    assert_eq!(app.current_agent(), Some("gemini_cli"));
    app.cycle_agent();
    assert_eq!(app.current_agent(), Some("claude_code"), "wraps");
}

#[test]
fn cycle_agent_noop_when_empty() {
    let mut app = make_app();
    app.cycle_agent();
    assert_eq!(app.current_agent(), None);
}

#[test]
fn add_detected_agent_is_idempotent() {
    let mut app = make_app();
    app.add_detected_agent("claude_code");
    app.add_detected_agent("claude_code");
    app.add_detected_agent("claude_code");
    assert_eq!(app.detected_agents.len(), 1);
}

// ---------- project cycling ----------

#[test]
fn cycle_project_walks_all_then_each_project() {
    use agenttop::storage::ProjectInfo;
    let mut app = make_app();
    app.detected_projects = vec![
        ProjectInfo {
            name: "alpha".into(),
            event_count: 10,
            first_seen: None,
            last_seen: None,
        },
        ProjectInfo {
            name: "beta".into(),
            event_count: 5,
            first_seen: None,
            last_seen: None,
        },
    ];

    assert!(matches!(app.project_filter, ProjectFilter::All));
    app.cycle_project();
    assert!(matches!(&app.project_filter, ProjectFilter::Project(n) if n == "alpha"));
    app.cycle_project();
    assert!(matches!(&app.project_filter, ProjectFilter::Project(n) if n == "beta"));
    app.cycle_project();
    assert!(matches!(app.project_filter, ProjectFilter::All), "wraps");
}

#[test]
fn cycle_project_noop_when_no_projects() {
    let mut app = make_app();
    app.cycle_project();
    assert!(matches!(app.project_filter, ProjectFilter::All));
}

// ---------- pause prevents refresh from clobbering state ----------

#[test]
fn refresh_is_noop_when_paused() {
    let storage = StorageHandle::new_in_memory().unwrap();
    storage.record_log_events(vec![tool_event("Bash", true, 100)]);
    std::thread::sleep(std::time::Duration::from_millis(150));

    let mut app = App::new(storage);
    app.toggle_pause();

    let before = app.tool_metrics.len();
    app.refresh().unwrap();
    let after = app.tool_metrics.len();
    assert_eq!(before, after, "paused refresh must not touch state");
}

// ---------- time-filter labels ----------

#[test]
fn time_filter_labels_are_stable() {
    assert_eq!(TimeFilter::AllTime.label(), "All-time");
    assert_eq!(TimeFilter::LastHour.label(), "Last 1h");
    assert_eq!(TimeFilter::Last24Hours.label(), "Last 24h");
    assert_eq!(TimeFilter::Last7Days.label(), "Last 7d");
}

#[test]
fn time_filter_since_only_set_for_windowed_variants() {
    assert!(TimeFilter::AllTime.since().is_none());
    assert!(TimeFilter::LastHour.since().is_some());
    assert!(TimeFilter::Last24Hours.since().is_some());
    assert!(TimeFilter::Last7Days.since().is_some());
}
