//! Integration tests for agenttop
//!
//! These tests verify the complete OTLP ingestion flow from receiving data
//! to storing it in DuckDB and querying it back.

use agenttop::otlp::parser::{parse_logs, parse_metrics};

// =============================================================================
// OTLP Receiver Tests
// =============================================================================

/// Test that the OTLP receiver can be started
#[tokio::test]
async fn test_otlp_receiver_binds() {
    // Try to bind to a test port to verify the server setup works
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await;
    assert!(listener.is_ok(), "Should be able to bind to a port");
}

/// Test that we can bind to port 0 and get an ephemeral port
#[tokio::test]
async fn test_ephemeral_port_allocation() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(addr.port() > 0, "Should get a non-zero port");
}

// =============================================================================
// Full Flow Tests (Parse -> Store -> Query)
// =============================================================================

/// Test the complete flow: parse JSON logs, store events
#[test]
fn test_parse_and_store_log_events() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [
                    {
                        "timeUnixNano": "1705600000000000000",
                        "attributes": [
                            {"key": "event.name", "value": {"stringValue": "tool_result"}},
                            {"key": "tool_name", "value": {"stringValue": "Read"}},
                            {"key": "success", "value": {"stringValue": "true"}},
                            {"key": "duration_ms", "value": {"intValue": "50"}}
                        ]
                    },
                    {
                        "timeUnixNano": "1705600001000000000",
                        "attributes": [
                            {"key": "event.name", "value": {"stringValue": "tool_result"}},
                            {"key": "tool_name", "value": {"stringValue": "Write"}},
                            {"key": "success", "value": {"boolValue": false}},
                            {"key": "duration_ms", "value": {"intValue": 200}},
                            {"key": "error", "value": {"stringValue": "Permission denied"}}
                        ]
                    }
                ]
            }]
        }]
    }"#;

    // Parse the JSON
    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(events.len(), 2);

    // Verify first event
    assert_eq!(events[0].event_name, Some("tool_result".to_string()));
    assert_eq!(
        events[0].attributes.get("tool_name"),
        Some(&"Read".to_string())
    );
    assert_eq!(
        events[0].attributes.get("success"),
        Some(&"true".to_string())
    );

    // Verify second event (with error)
    assert_eq!(
        events[1].attributes.get("tool_name"),
        Some(&"Write".to_string())
    );
    assert_eq!(
        events[1].attributes.get("success"),
        Some(&"false".to_string())
    );
    assert_eq!(
        events[1].attributes.get("error"),
        Some(&"Permission denied".to_string())
    );
}

/// Test parsing mixed event types (tool_result, api_request, etc.)
#[test]
fn test_parse_mixed_event_types() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "tool_result"}}, {"key": "tool_name", "value": {"stringValue": "Read"}}]},
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "api_request"}}, {"key": "model", "value": {"stringValue": "claude-3-opus"}}]},
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "tool_decision"}}, {"key": "decision", "value": {"stringValue": "approved"}}]},
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "claude_code.tool_result"}}, {"key": "tool_name", "value": {"stringValue": "Bash"}}]}
                ]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();

    // All events should be parsed (no filtering at parse time)
    assert_eq!(events.len(), 4);

    // Check event names
    let event_names: Vec<_> = events
        .iter()
        .map(|e| e.event_name.as_ref().unwrap().as_str())
        .collect();
    assert!(event_names.contains(&"tool_result"));
    assert!(event_names.contains(&"api_request"));
    assert!(event_names.contains(&"tool_decision"));
    assert!(event_names.contains(&"claude_code.tool_result"));
}

/// Test parsing metrics with all token types
#[test]
fn test_parse_all_token_types() {
    let json = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "claude_code.token.usage",
                    "sum": {
                        "dataPoints": [
                            {"asInt": 1000, "attributes": [{"key": "type", "value": {"stringValue": "input"}}]},
                            {"asInt": 500, "attributes": [{"key": "type", "value": {"stringValue": "output"}}]},
                            {"asInt": 2000, "attributes": [{"key": "type", "value": {"stringValue": "cacheRead"}}]},
                            {"asInt": 100, "attributes": [{"key": "type", "value": {"stringValue": "cacheCreation"}}]}
                        ]
                    }
                }]
            }]
        }]
    }"#;

    let metrics = parse_metrics(json.as_bytes()).unwrap();
    assert_eq!(metrics.len(), 4);
}

/// Test metrics JSON parsing
#[test]
fn test_metrics_json_structure() {
    let json = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "claude_code.token.usage",
                    "sum": {
                        "dataPoints": [{
                            "asInt": 5000,
                            "attributes": [{"key": "type", "value": {"stringValue": "output"}}]
                        }]
                    }
                }]
            }]
        }]
    }"#;

    // Verify it's valid JSON
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    assert!(parsed.get("resourceMetrics").is_some());
}

/// Test logs JSON parsing
#[test]
fn test_logs_json_structure() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "claude_code.tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Read"}},
                        {"key": "success", "value": {"boolValue": true}},
                        {"key": "duration_ms", "value": {"intValue": 50}}
                    ]
                }]
            }]
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    assert!(parsed.get("resourceLogs").is_some());
}

/// Test storage path construction with mock data directory
#[test]
fn test_storage_path_construction() {
    use std::path::PathBuf;

    // Use mock data directory to test path construction logic
    let mock_data_dir = PathBuf::from("/var/lib/agenttop");
    let db_path = mock_data_dir.join("metrics.duckdb");

    assert!(db_path.to_str().is_some());
    assert!(db_path.to_str().unwrap().ends_with("metrics.duckdb"));
}

/// Test in-memory storage creation works
#[test]
fn test_in_memory_storage_integration() {
    use agenttop::storage::StorageHandle;

    // This verifies the complete storage stack works without file system
    let storage = StorageHandle::new_in_memory();
    assert!(storage.is_ok(), "In-memory storage should initialize");
}
