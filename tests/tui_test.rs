//! TUI integration tests
//!
//! These tests verify that the TUI App correctly interacts with the storage
//! layer and can render data properly.

use agenttop::storage::{LogEvent, StorageHandle};
use agenttop::tui::app::{App, SortColumn};
use chrono::Utc;
use ratatui::{Terminal, backend::TestBackend};
use std::collections::HashMap;

/// Helper to create a test log event
fn make_tool_event(tool_name: &str, success: bool, duration_ms: u64) -> LogEvent {
    let mut attrs = HashMap::new();
    attrs.insert("tool_name".to_string(), tool_name.to_string());
    attrs.insert("success".to_string(), success.to_string());
    attrs.insert("duration_ms".to_string(), duration_ms.to_string());
    LogEvent {
        timestamp: Utc::now(),
        event_name: Some("tool_result".to_string()),
        body: None,
        attributes: attrs,
    }
}

/// Test that App can be created with in-memory storage
#[test]
fn test_app_creation() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let app = App::new(storage);

    assert!(app.tool_metrics.is_empty());
    assert_eq!(app.selected_index, 0);
    assert_eq!(app.sort_by, SortColumn::Calls);
    assert!(!app.paused);
    assert!(!app.show_detail);
}

/// Test that App can refresh and load data from storage
#[test]
fn test_app_refresh_loads_data() {
    let storage = StorageHandle::new_in_memory().unwrap();

    // Insert test data
    storage.record_log_events(vec![
        make_tool_event("Read", true, 50),
        make_tool_event("Read", true, 75),
        make_tool_event("Write", true, 100),
        make_tool_event("Bash", false, 500),
    ]);

    // Wait for storage to process
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Create app and refresh
    let mut app = App::new(storage);
    app.refresh().unwrap();

    // Should have 3 tools
    assert_eq!(app.tool_metrics.len(), 3);

    // Find Read tool (should have 2 calls)
    let read = app.tool_metrics.iter().find(|t| t.tool_name == "Read");
    assert!(read.is_some());
    assert_eq!(read.unwrap().call_count, 2);
}

/// Test that App correctly computes total tokens
#[test]
fn test_app_total_tokens() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_token_usage("input", 1000);
    storage.record_token_usage("output", 500);
    storage.record_token_usage("cacheRead", 2000);
    storage.record_token_usage("cacheCreation", 100);

    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    assert_eq!(app.total_tokens(), 3600);
}

/// Test that App correctly computes cache hit rate
#[test]
fn test_app_cache_hit_rate() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_token_usage("input", 1000);
    storage.record_token_usage("cacheRead", 4000);

    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    // 4000 / (1000 + 4000) = 80%
    assert!((app.cache_hit_rate() - 80.0).abs() < 0.1);
}

/// Test that App sorting works correctly
#[test]
fn test_app_sorting() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_log_events(vec![
        make_tool_event("AAA", true, 50),
        make_tool_event("BBB", true, 100),
        make_tool_event("BBB", true, 100),
        make_tool_event("CCC", true, 200),
        make_tool_event("CCC", true, 200),
        make_tool_event("CCC", true, 200),
    ]);

    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    // Default sort is by calls descending
    assert_eq!(app.tool_metrics[0].tool_name, "CCC");
    assert_eq!(app.tool_metrics[1].tool_name, "BBB");
    assert_eq!(app.tool_metrics[2].tool_name, "AAA");

    // Toggle to sort by name
    app.toggle_sort(); // LastCall
    app.toggle_sort(); // AvgDuration
    app.toggle_sort(); // Name

    // Now sorted by name descending (Z-A)
    assert_eq!(app.tool_metrics[0].tool_name, "CCC");
    assert_eq!(app.tool_metrics[2].tool_name, "AAA");
}

/// Test navigation works correctly
#[test]
fn test_app_navigation() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_log_events(vec![
        make_tool_event("Tool1", true, 50),
        make_tool_event("Tool2", true, 100),
        make_tool_event("Tool3", true, 150),
    ]);

    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    assert_eq!(app.selected_index, 0);

    app.select_next();
    assert_eq!(app.selected_index, 1);

    app.select_next();
    assert_eq!(app.selected_index, 2);

    // Wraps around
    app.select_next();
    assert_eq!(app.selected_index, 0);

    // Go back
    app.select_previous();
    assert_eq!(app.selected_index, 2);
}

/// Test pause functionality
#[test]
fn test_app_pause() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_log_events(vec![make_tool_event("Tool1", true, 50)]);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage.clone());
    app.refresh().unwrap();

    assert_eq!(app.tool_metrics.len(), 1);

    // Pause the app
    app.toggle_pause();
    assert!(app.paused);

    // Add more data
    storage.record_log_events(vec![make_tool_event("Tool2", true, 100)]);
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Refresh should not update while paused
    app.refresh().unwrap();
    assert_eq!(app.tool_metrics.len(), 1); // Still 1, not 2

    // Unpause
    app.toggle_pause();
    assert!(!app.paused);

    // Now refresh should update
    app.refresh().unwrap();
    assert_eq!(app.tool_metrics.len(), 2);
}

/// Test detail view toggle
#[test]
fn test_app_detail_view() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_log_events(vec![
        make_tool_event("Bash", true, 100),
        make_tool_event("Bash", false, 200),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    assert!(!app.show_detail);

    app.toggle_detail();
    assert!(app.show_detail);

    let selected = app.selected_tool().unwrap();
    assert_eq!(selected.tool_name, "Bash");
    assert_eq!(selected.call_count, 2);
    assert_eq!(selected.success_count, 1);
    assert_eq!(selected.error_count, 1);

    app.close_detail();
    assert!(!app.show_detail);
}

/// Test total tool calls calculation
#[test]
fn test_app_total_tool_calls() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_log_events(vec![
        make_tool_event("Read", true, 50),
        make_tool_event("Read", true, 50),
        make_tool_event("Write", true, 100),
        make_tool_event("Bash", true, 200),
        make_tool_event("Bash", true, 200),
        make_tool_event("Bash", true, 200),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    assert_eq!(app.total_tool_calls(), 6);
}

/// Test session metrics are loaded
#[test]
fn test_app_session_metrics() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_session_metric("lines_of_code", 500);
    storage.record_session_metric("commit", 5);
    storage.record_session_metric("pull_request", 2);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    assert_eq!(app.session_metrics.lines_of_code, 500);
    assert_eq!(app.session_metrics.commit_count, 5);
    assert_eq!(app.session_metrics.pr_count, 2);
}

/// Test overall success rate calculation
#[test]
fn test_app_overall_success_rate() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_log_events(vec![
        make_tool_event("Read", true, 50),
        make_tool_event("Read", true, 50),
        make_tool_event("Read", false, 50),
        make_tool_event("Write", true, 100),
        make_tool_event("Write", true, 100),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    // 4 successes out of 5 calls = 80%
    assert!((app.overall_success_rate() - 80.0).abs() < 0.1);
}

/// Test average tool duration calculation
#[test]
fn test_app_average_tool_duration() {
    let storage = StorageHandle::new_in_memory().unwrap();

    // Tool1: 2 calls @ 100ms avg = 200ms total
    // Tool2: 3 calls @ 200ms avg = 600ms total
    // Total: 5 calls, 800ms total = 160ms avg
    storage.record_log_events(vec![
        make_tool_event("Tool1", true, 100),
        make_tool_event("Tool1", true, 100),
        make_tool_event("Tool2", true, 200),
        make_tool_event("Tool2", true, 200),
        make_tool_event("Tool2", true, 200),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    // Weighted average: (2*100 + 3*200) / 5 = 160ms
    assert!((app.average_tool_duration() - 160.0).abs() < 0.1);
}

/// Test overall success rate with no tools returns 100%
#[test]
fn test_app_overall_success_rate_empty() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let app = App::new(storage);

    assert!((app.overall_success_rate() - 100.0).abs() < 0.1);
}

/// Test average tool duration with no tools returns 0
#[test]
fn test_app_average_tool_duration_empty() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let app = App::new(storage);

    assert!((app.average_tool_duration() - 0.0).abs() < 0.1);
}

/// Test cost is loaded correctly
#[test]
fn test_app_cost() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_cost(0.05);
    storage.record_cost(0.03);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    assert!((app.token_metrics.total_cost_usd - 0.08).abs() < 0.001);
}

// =============================================================================
// UI Rendering Tests
// =============================================================================

/// Test that UI can render with empty data without crashing
#[test]
fn test_ui_renders_empty_state() {
    let storage = StorageHandle::new_in_memory().unwrap();
    let app = App::new(storage);

    // Create a test terminal
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Render the UI - this should not panic
    terminal.draw(|f| agenttop::tui::ui::draw(f, &app)).unwrap();

    // Verify something was rendered
    let buffer = terminal.backend().buffer();
    assert!(buffer.area.width > 0);
    assert!(buffer.area.height > 0);
}

/// Test that UI can render with real data
#[test]
fn test_ui_renders_with_data() {
    let storage = StorageHandle::new_in_memory().unwrap();

    // Insert various test data
    storage.record_log_events(vec![
        make_tool_event("Read", true, 50),
        make_tool_event("Read", true, 75),
        make_tool_event("Write", true, 100),
        make_tool_event("Write", false, 200),
        make_tool_event("Bash", true, 500),
        make_tool_event("Bash", true, 750),
        make_tool_event("Bash", false, 1000),
    ]);
    storage.record_token_usage("input", 5000);
    storage.record_token_usage("output", 2500);
    storage.record_token_usage("cacheRead", 10000);
    storage.record_cost(0.15);
    storage.record_session_metric("lines_of_code", 250);
    storage.record_session_metric("commit", 3);

    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    // Create a test terminal
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    // Render the UI
    terminal.draw(|f| agenttop::tui::ui::draw(f, &app)).unwrap();

    // Convert buffer to string for inspection
    let buffer = terminal.backend().buffer();
    let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();

    // Verify key elements are rendered
    assert!(content.contains("agenttop"), "Should show app name");
    assert!(content.contains("Tokens"), "Should show token gauge");
    assert!(content.contains("Tools"), "Should show tools section");
    assert!(content.contains("Bash"), "Should show Bash tool");
    assert!(content.contains("Read"), "Should show Read tool");
    assert!(content.contains("Write"), "Should show Write tool");
}

/// Test that UI renders detail popup correctly
#[test]
fn test_ui_renders_detail_popup() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_log_events(vec![
        make_tool_event("Bash", true, 100),
        make_tool_event("Bash", true, 150),
        make_tool_event("Bash", false, 200),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();
    app.toggle_detail(); // Show detail popup

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|f| agenttop::tui::ui::draw(f, &app)).unwrap();

    let buffer = terminal.backend().buffer();
    let content: String = buffer.content.iter().map(|cell| cell.symbol()).collect();

    // Verify detail popup content
    assert!(content.contains("Details"), "Should show details title");
    assert!(content.contains("Total Calls"), "Should show total calls");
    assert!(
        content.contains("Successful"),
        "Should show successful count"
    );
    assert!(content.contains("Errors"), "Should show error count");
}

/// Test UI with large terminal size
#[test]
fn test_ui_renders_large_terminal() {
    let storage = StorageHandle::new_in_memory().unwrap();

    // Add many tools to test scrolling/layout
    for i in 0..20 {
        storage.record_log_events(vec![make_tool_event(
            &format!("Tool{:02}", i),
            true,
            50 + i * 10,
        )]);
    }
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    // Large terminal
    let backend = TestBackend::new(200, 50);
    let mut terminal = Terminal::new(backend).unwrap();

    // Should not panic
    terminal.draw(|f| agenttop::tui::ui::draw(f, &app)).unwrap();
}

/// Test UI with small terminal size
#[test]
fn test_ui_renders_small_terminal() {
    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_log_events(vec![
        make_tool_event("Read", true, 50),
        make_tool_event("Write", true, 100),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut app = App::new(storage);
    app.refresh().unwrap();

    // Small terminal (minimum usable size)
    let backend = TestBackend::new(60, 15);
    let mut terminal = Terminal::new(backend).unwrap();

    // Should not panic even on small terminal
    terminal.draw(|f| agenttop::tui::ui::draw(f, &app)).unwrap();
}
