//! Storage tests
//!
//! These tests verify storage structures and calculations, as well as
//! query-time filtering behavior for tool_result events.

use chrono::{DateTime, Timelike, Utc};
use std::collections::HashMap;

// =============================================================================
// LogEvent Structure Tests
// =============================================================================

/// Test LogEvent structure with all fields
#[test]
fn test_log_event_structure() {
    use agenttop::storage::LogEvent;

    let mut attrs = HashMap::new();
    attrs.insert("tool_name".to_string(), "Read".to_string());
    attrs.insert("success".to_string(), "true".to_string());
    attrs.insert("duration_ms".to_string(), "50".to_string());

    let event = LogEvent {
        timestamp: Utc::now(),
        event_name: Some("tool_result".to_string()),
        body: None,
        attributes: attrs,
    };

    assert_eq!(event.event_name, Some("tool_result".to_string()));
    assert_eq!(event.attributes.get("tool_name"), Some(&"Read".to_string()));
    assert_eq!(event.attributes.get("success"), Some(&"true".to_string()));
}

/// Test LogEvent with prefixed event name
#[test]
fn test_log_event_prefixed_name() {
    use agenttop::storage::LogEvent;

    let event = LogEvent {
        timestamp: Utc::now(),
        event_name: Some("claude_code.tool_result".to_string()),
        body: None,
        attributes: HashMap::new(),
    };

    // Event names ending in tool_result should be matched by LIKE '%tool_result'
    let event_name = event.event_name.as_ref().unwrap();
    assert!(event_name.ends_with("tool_result"));
}

/// Test that all attribute types can be stored as strings
#[test]
fn test_log_event_all_attribute_types() {
    use agenttop::storage::LogEvent;

    let mut attrs = HashMap::new();
    // String values
    attrs.insert("string_val".to_string(), "hello".to_string());
    // Int values stored as strings
    attrs.insert("int_val".to_string(), "42".to_string());
    // Bool values stored as strings
    attrs.insert("bool_val".to_string(), "true".to_string());
    // Double values stored as strings
    attrs.insert("double_val".to_string(), "3.14".to_string());

    let event = LogEvent {
        timestamp: Utc::now(),
        event_name: Some("test_event".to_string()),
        body: Some("test body".to_string()),
        attributes: attrs,
    };

    assert_eq!(event.attributes.len(), 4);
    assert!(event.body.is_some());
}

// =============================================================================
// Query-Time Filtering Tests
// =============================================================================

/// Test event name matching pattern for tool_result
#[test]
fn test_event_name_like_pattern() {
    let patterns = vec![
        "tool_result",
        "claude_code.tool_result",
        "custom.prefix.tool_result",
    ];

    for pattern in patterns {
        // Simulating SQL LIKE '%tool_result' behavior
        assert!(
            pattern.ends_with("tool_result"),
            "Pattern '{}' should end with 'tool_result'",
            pattern
        );
    }
}

/// Test that non-tool_result events don't match the pattern
#[test]
fn test_event_name_non_matching() {
    let non_matching = vec![
        "api_request",
        "tool_decision",
        "session_start",
        "tool_result_modified", // Doesn't end with tool_result
    ];

    for pattern in non_matching {
        assert!(
            !pattern.ends_with("tool_result") || pattern != "tool_result",
            "Pattern '{}' should not match tool_result suffix",
            pattern
        );
    }
}

/// Test success value interpretation for SQL queries
#[test]
fn test_success_value_interpretation() {
    // These should be interpreted as true in SQL:
    // json_extract_string(attributes, '$.success') IN ('true', '1')
    let true_values = vec!["true", "1"];
    for val in true_values {
        assert!(
            val == "true" || val == "1",
            "Value '{}' should be truthy",
            val
        );
    }

    // These should be interpreted as false
    let false_values = vec!["false", "0", ""];
    for val in false_values {
        assert!(
            val != "true" && val != "1",
            "Value '{}' should be falsy",
            val
        );
    }
}

/// Test JSON attribute extraction simulation
#[test]
fn test_json_attribute_extraction() {
    // Simulate the JSON structure stored in DuckDB
    let json = r#"{"tool_name":"Read","success":"true","duration_ms":"50"}"#;
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();

    assert_eq!(parsed["tool_name"].as_str(), Some("Read"));
    assert_eq!(parsed["success"].as_str(), Some("true"));
    assert_eq!(parsed["duration_ms"].as_str(), Some("50"));
}

/// Test JSON with missing optional fields
#[test]
fn test_json_missing_optional_fields() {
    // Tool event with no error field
    let json = r#"{"tool_name":"Read","success":"true"}"#;
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();

    assert!(parsed["error"].is_null());
    assert_eq!(parsed["tool_name"].as_str(), Some("Read"));
}

// =============================================================================
// Original Structure Tests (kept for compatibility)
// =============================================================================

/// Test ToolMetrics structure
#[test]
fn test_tool_metrics_structure() {
    #[derive(Debug, Clone)]
    struct ToolMetrics {
        tool_name: String,
        call_count: u64,
        last_call: Option<DateTime<Utc>>,
        avg_duration_ms: f64,
        success_count: u64,
        error_count: u64,
    }

    let metrics = ToolMetrics {
        tool_name: "Bash".to_string(),
        call_count: 10,
        last_call: Some(Utc::now()),
        avg_duration_ms: 150.5,
        success_count: 9,
        error_count: 1,
    };

    assert_eq!(metrics.tool_name, "Bash");
    assert_eq!(metrics.call_count, 10);
    assert!(metrics.last_call.is_some());
    assert!((metrics.avg_duration_ms - 150.5).abs() < 0.01);
    assert_eq!(metrics.success_count, 9);
    assert_eq!(metrics.error_count, 1);
}

/// Test TokenMetrics structure
#[test]
fn test_token_metrics_structure() {
    #[derive(Debug, Clone, Default)]
    struct TokenMetrics {
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_creation_tokens: u64,
        total_cost_usd: f64,
    }

    let metrics = TokenMetrics {
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 2000,
        cache_creation_tokens: 100,
        total_cost_usd: 0.05,
    };

    assert_eq!(metrics.input_tokens, 1000);
    assert_eq!(metrics.output_tokens, 500);
    assert_eq!(metrics.cache_read_tokens, 2000);
    assert_eq!(metrics.cache_creation_tokens, 100);
    assert!((metrics.total_cost_usd - 0.05).abs() < 0.001);
}

/// Test SessionMetrics structure
#[test]
fn test_session_metrics_structure() {
    #[derive(Debug, Clone)]
    struct SessionMetrics {
        start_time: DateTime<Utc>,
        lines_of_code: i64,
        commit_count: u64,
        pr_count: u64,
        active_time_seconds: u64,
    }

    let metrics = SessionMetrics {
        start_time: Utc::now(),
        lines_of_code: 150,
        commit_count: 3,
        pr_count: 1,
        active_time_seconds: 3600,
    };

    assert_eq!(metrics.lines_of_code, 150);
    assert_eq!(metrics.commit_count, 3);
    assert_eq!(metrics.pr_count, 1);
    assert_eq!(metrics.active_time_seconds, 3600);
}

/// Test ToolEvent structure
#[test]
fn test_tool_event_structure() {
    #[derive(Debug, Clone)]
    struct ToolEvent {
        timestamp: DateTime<Utc>,
        tool_name: String,
        success: bool,
        duration_ms: u64,
        error: Option<String>,
    }

    let event = ToolEvent {
        timestamp: Utc::now(),
        tool_name: "Read".to_string(),
        success: true,
        duration_ms: 25,
        error: None,
    };

    assert_eq!(event.tool_name, "Read");
    assert!(event.success);
    assert_eq!(event.duration_ms, 25);
    assert!(event.error.is_none());
}

/// Test failed ToolEvent
#[test]
fn test_tool_event_failure() {
    #[derive(Debug, Clone)]
    struct ToolEvent {
        timestamp: DateTime<Utc>,
        tool_name: String,
        success: bool,
        duration_ms: u64,
        error: Option<String>,
    }

    let event = ToolEvent {
        timestamp: Utc::now(),
        tool_name: "Bash".to_string(),
        success: false,
        duration_ms: 1500,
        error: Some("Exit code 1".to_string()),
    };

    assert!(!event.success);
    assert!(event.error.is_some());
    assert_eq!(event.error.unwrap(), "Exit code 1");
}

/// Test total tokens calculation
#[test]
fn test_total_tokens_calculation() {
    let input = 1000u64;
    let output = 500u64;
    let cache_read = 2000u64;
    let cache_creation = 100u64;

    let total = input + output + cache_read + cache_creation;
    assert_eq!(total, 3600);
}

/// Test cache hit rate calculation
#[test]
fn test_cache_hit_rate_calculation() {
    let input_tokens = 1000u64;
    let cache_read_tokens = 4000u64;

    let total_input = input_tokens + cache_read_tokens;
    let hit_rate = (cache_read_tokens as f64 / total_input as f64) * 100.0;

    assert!((hit_rate - 80.0).abs() < 0.01);
}

/// Test cache hit rate with zero input
#[test]
fn test_cache_hit_rate_zero_input() {
    let input_tokens = 0u64;
    let cache_read_tokens = 0u64;

    let total_input = input_tokens + cache_read_tokens;
    let hit_rate = if total_input == 0 {
        0.0
    } else {
        (cache_read_tokens as f64 / total_input as f64) * 100.0
    };

    assert_eq!(hit_rate, 0.0);
}

/// Test productivity multiplier calculation
#[test]
fn test_productivity_multiplier() {
    let lines_of_code = 500i64;
    let hours = 2.0f64;

    let productivity = lines_of_code.abs() as f64 / hours;
    assert!((productivity - 250.0).abs() < 0.01);
}

/// Test session duration calculation
#[test]
fn test_session_duration() {
    let start = Utc::now() - chrono::Duration::hours(2);
    let now = Utc::now();
    let duration = now - start;

    assert!(duration.num_hours() >= 1);
    assert!(duration.num_minutes() >= 119);
}

/// Test SQL timestamp format
#[test]
fn test_timestamp_format() {
    let now = Utc::now();
    let formatted = now.to_rfc3339();

    assert!(formatted.contains("T"));
    assert!(formatted.contains("+") || formatted.contains("Z"));
}

/// Test timestamp parsing
#[test]
fn test_timestamp_parsing() {
    let timestamp_str = "2025-01-16T12:00:00+00:00";
    let parsed = DateTime::parse_from_rfc3339(timestamp_str);

    assert!(parsed.is_ok());
    let dt = parsed.unwrap().with_timezone(&Utc);
    assert_eq!(dt.hour(), 12);
}

/// Test negative lines of code
#[test]
fn test_negative_lines_of_code() {
    let lines_of_code = -50i64;
    assert!(lines_of_code < 0);
    assert_eq!(lines_of_code.abs(), 50);
}

// =============================================================================
// In-Memory Database Integration Tests
// =============================================================================
// These tests use in-memory DuckDB for proper test isolation without polluting
// the local database or requiring special CI configuration.

/// Test creating an in-memory storage handle
#[test]
fn test_in_memory_storage_creation() {
    use agenttop::storage::StorageHandle;

    let storage = StorageHandle::new_in_memory();
    assert!(storage.is_ok(), "Should create in-memory storage");
}

/// Test recording and retrieving log events
#[test]
fn test_record_and_retrieve_log_events() {
    use agenttop::storage::{LogEvent, StorageHandle};

    let storage = StorageHandle::new_in_memory().unwrap();

    // Create a tool_result event
    let mut attrs = HashMap::new();
    attrs.insert("tool_name".to_string(), "Read".to_string());
    attrs.insert("success".to_string(), "true".to_string());
    attrs.insert("duration_ms".to_string(), "50".to_string());

    let event = LogEvent {
        timestamp: Utc::now(),
        event_name: Some("tool_result".to_string()),
        body: None,
        attributes: attrs,
    };

    // Record the event
    storage.record_log_events(vec![event]);

    // Give the actor time to process
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Query tool metrics
    let metrics = storage.get_tool_metrics(None).unwrap();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].tool_name, "Read");
    assert_eq!(metrics[0].call_count, 1);
    assert_eq!(metrics[0].success_count, 1);
    assert_eq!(metrics[0].error_count, 0);
}

/// Test recording multiple tool events
#[test]
fn test_multiple_tool_events() {
    use agenttop::storage::{LogEvent, StorageHandle};

    let storage = StorageHandle::new_in_memory().unwrap();

    // Create multiple events
    let events: Vec<LogEvent> = vec![
        ("Read", "true", "50"),
        ("Read", "true", "30"),
        ("Write", "false", "100"),
        ("Bash", "true", "200"),
    ]
    .into_iter()
    .map(|(tool, success, duration)| {
        let mut attrs = HashMap::new();
        attrs.insert("tool_name".to_string(), tool.to_string());
        attrs.insert("success".to_string(), success.to_string());
        attrs.insert("duration_ms".to_string(), duration.to_string());
        LogEvent {
            timestamp: Utc::now(),
            event_name: Some("tool_result".to_string()),
            body: None,
            attributes: attrs,
        }
    })
    .collect();

    storage.record_log_events(events);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let metrics = storage.get_tool_metrics(None).unwrap();
    assert_eq!(metrics.len(), 3); // Read, Write, Bash

    // Find Read metrics
    let read_metrics = metrics.iter().find(|m| m.tool_name == "Read").unwrap();
    assert_eq!(read_metrics.call_count, 2);
    assert_eq!(read_metrics.success_count, 2);

    // Find Write metrics (one failure)
    let write_metrics = metrics.iter().find(|m| m.tool_name == "Write").unwrap();
    assert_eq!(write_metrics.call_count, 1);
    assert_eq!(write_metrics.error_count, 1);
}

/// Test recording token usage
#[test]
fn test_token_usage_recording() {
    use agenttop::storage::StorageHandle;

    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_token_usage("input", 1000);
    storage.record_token_usage("output", 500);
    storage.record_token_usage("cacheRead", 2000);
    storage.record_token_usage("cacheCreation", 100);

    std::thread::sleep(std::time::Duration::from_millis(100));

    let metrics = storage.get_token_metrics(None).unwrap();
    assert_eq!(metrics.input_tokens, 1000);
    assert_eq!(metrics.output_tokens, 500);
    assert_eq!(metrics.cache_read_tokens, 2000);
    assert_eq!(metrics.cache_creation_tokens, 100);
}

/// Test recording cost
#[test]
fn test_cost_recording() {
    use agenttop::storage::StorageHandle;

    let storage = StorageHandle::new_in_memory().unwrap();

    storage.record_cost(0.05);
    storage.record_cost(0.03);

    std::thread::sleep(std::time::Duration::from_millis(100));

    let metrics = storage.get_token_metrics(None).unwrap();
    assert!((metrics.total_cost_usd - 0.08).abs() < 0.001);
}

/// Test prefixed event names are properly aggregated
#[test]
fn test_prefixed_event_names_aggregation() {
    use agenttop::storage::{LogEvent, StorageHandle};

    let storage = StorageHandle::new_in_memory().unwrap();

    // One event with plain name, one with prefix
    let events: Vec<LogEvent> = vec![("tool_result", "Read"), ("claude_code.tool_result", "Read")]
        .into_iter()
        .map(|(event_name, tool)| {
            let mut attrs = HashMap::new();
            attrs.insert("tool_name".to_string(), tool.to_string());
            attrs.insert("success".to_string(), "true".to_string());
            attrs.insert("duration_ms".to_string(), "50".to_string());
            LogEvent {
                timestamp: Utc::now(),
                event_name: Some(event_name.to_string()),
                body: None,
                attributes: attrs,
            }
        })
        .collect();

    storage.record_log_events(events);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let metrics = storage.get_tool_metrics(None).unwrap();
    let read_metrics = metrics.iter().find(|m| m.tool_name == "Read").unwrap();
    // Both events should be counted for Read tool
    assert_eq!(read_metrics.call_count, 2);
}

/// Test that non-tool_result events are stored but not in tool metrics
#[test]
fn test_non_tool_result_events_stored() {
    use agenttop::storage::{LogEvent, StorageHandle};

    let storage = StorageHandle::new_in_memory().unwrap();

    // Create non-tool_result events
    let events: Vec<LogEvent> = vec!["api_request", "session_start", "tool_decision"]
        .into_iter()
        .map(|event_name| {
            let mut attrs = HashMap::new();
            attrs.insert("model".to_string(), "claude-3-opus".to_string());
            LogEvent {
                timestamp: Utc::now(),
                event_name: Some(event_name.to_string()),
                body: None,
                attributes: attrs,
            }
        })
        .collect();

    storage.record_log_events(events);
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Tool metrics should be empty (no tool_result events)
    let metrics = storage.get_tool_metrics(None).unwrap();
    assert!(metrics.is_empty());
}

/// Test empty database returns empty/default metrics
#[test]
fn test_empty_database_metrics() {
    use agenttop::storage::StorageHandle;

    let storage = StorageHandle::new_in_memory().unwrap();

    let tool_metrics = storage.get_tool_metrics(None).unwrap();
    assert!(tool_metrics.is_empty());

    let token_metrics = storage.get_token_metrics(None).unwrap();
    assert_eq!(token_metrics.input_tokens, 0);
    assert_eq!(token_metrics.output_tokens, 0);
    assert_eq!(token_metrics.total_cost_usd, 0.0);
}

/// Test each test gets isolated database (parallel test safety)
#[test]
fn test_database_isolation_1() {
    use agenttop::storage::StorageHandle;

    let storage = StorageHandle::new_in_memory().unwrap();
    storage.record_token_usage("input", 999);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let metrics = storage.get_token_metrics(None).unwrap();
    assert_eq!(metrics.input_tokens, 999);
}

/// Test each test gets isolated database (parallel test safety)
#[test]
fn test_database_isolation_2() {
    use agenttop::storage::StorageHandle;

    let storage = StorageHandle::new_in_memory().unwrap();
    // This should NOT see data from test_database_isolation_1
    let metrics = storage.get_token_metrics(None).unwrap();
    assert_eq!(metrics.input_tokens, 0);
}

// =============================================================================
// Tool Classification Tests
// =============================================================================

/// Test that built-in tools are correctly classified
#[test]
fn test_tool_metrics_is_builtin() {
    use agenttop::storage::ToolMetrics;

    let builtin_tools = vec!["Read", "Write", "Edit", "Bash", "Glob", "Grep", "Task", "TodoWrite"];

    for tool_name in builtin_tools {
        let metrics = ToolMetrics {
            tool_name: tool_name.to_string(),
            call_count: 1,
            last_call: None,
            avg_duration_ms: 0.0,
            min_duration_ms: 0.0,
            max_duration_ms: 0.0,
            success_count: 1,
            error_count: 0,
        };
        assert!(metrics.is_builtin(), "{} should be classified as built-in", tool_name);
        assert!(!metrics.is_mcp(), "{} should NOT be classified as MCP", tool_name);
    }
}

/// Test that MCP tools are correctly classified
#[test]
fn test_tool_metrics_is_mcp() {
    use agenttop::storage::ToolMetrics;

    let mcp_tools = vec!["context7", "playwright", "my_custom_tool", "TestRead", "sqlite_query"];

    for tool_name in mcp_tools {
        let metrics = ToolMetrics {
            tool_name: tool_name.to_string(),
            call_count: 1,
            last_call: None,
            avg_duration_ms: 0.0,
            min_duration_ms: 0.0,
            max_duration_ms: 0.0,
            success_count: 1,
            error_count: 0,
        };
        assert!(metrics.is_mcp(), "{} should be classified as MCP", tool_name);
        assert!(!metrics.is_builtin(), "{} should NOT be classified as built-in", tool_name);
    }
}

/// Test get_session_metrics from storage
#[test]
fn test_get_session_metrics() {
    use agenttop::storage::StorageHandle;

    let storage = StorageHandle::new_in_memory().unwrap();

    // Record some session metrics
    storage.record_session_metric("lines_of_code", 150);
    storage.record_session_metric("lines_of_code", -30);
    storage.record_session_metric("commits", 2);
    storage.record_session_metric("commits", 1);

    std::thread::sleep(std::time::Duration::from_millis(100));

    let metrics = storage.get_session_metrics(None).unwrap();
    assert_eq!(metrics.lines_of_code, 120); // 150 + (-30) = 120
    assert_eq!(metrics.commit_count, 3);    // 2 + 1 = 3
}

/// Test get_session_metrics returns defaults when empty
#[test]
fn test_get_session_metrics_empty() {
    use agenttop::storage::StorageHandle;

    let storage = StorageHandle::new_in_memory().unwrap();

    let metrics = storage.get_session_metrics(None).unwrap();
    assert_eq!(metrics.lines_of_code, 0);
    assert_eq!(metrics.commit_count, 0);
}
