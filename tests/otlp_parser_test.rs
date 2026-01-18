//! OTLP parser tests
//!
//! These tests verify OTLP JSON and protobuf parsing, including edge cases
//! discovered during development like string-encoded numbers.

use agenttop::otlp::parser::{ParsedMetric, parse_logs, parse_metrics};

// =============================================================================
// JSON String Number Handling Tests
// =============================================================================
// OTLP JSON encoding uses strings for large numbers to avoid precision loss.
// These tests verify our parser handles both string and numeric formats.

/// Test that timeUnixNano can be parsed as a string (OTLP JSON format)
#[test]
fn test_parse_time_unix_nano_as_string() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "timeUnixNano": "1705600000000000000",
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Read"}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(events.len(), 1);
    // Verify timestamp was parsed correctly (2024-01-18 roughly)
    assert!(events[0].timestamp.timestamp() > 1705000000);
}

/// Test that timeUnixNano can be parsed as a number
#[test]
fn test_parse_time_unix_nano_as_number() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "timeUnixNano": 1705600000000000000,
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Read"}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(events.len(), 1);
}

/// Test that intValue can be parsed as a string (OTLP JSON format)
#[test]
fn test_parse_int_value_as_string() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Bash"}},
                        {"key": "duration_ms", "value": {"intValue": "12345"}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].attributes.get("duration_ms"),
        Some(&"12345".to_string())
    );
}

/// Test that intValue can be parsed as a number
#[test]
fn test_parse_int_value_as_number() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Bash"}},
                        {"key": "duration_ms", "value": {"intValue": 12345}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].attributes.get("duration_ms"),
        Some(&"12345".to_string())
    );
}

// =============================================================================
// Event Name Format Tests
// =============================================================================
// Claude Code may send events with different prefixes. Our query-time filtering
// uses LIKE '%tool_result' to match any prefix.

/// Test parsing event with 'tool_result' event name (no prefix)
#[test]
fn test_parse_event_name_no_prefix() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Read"}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(events[0].event_name, Some("tool_result".to_string()));
}

/// Test parsing event with 'claude_code.tool_result' event name (with prefix)
#[test]
fn test_parse_event_name_with_prefix() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "claude_code.tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Write"}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(
        events[0].event_name,
        Some("claude_code.tool_result".to_string())
    );
}

/// Test that non-tool_result events are also stored (filtering happens at query time)
#[test]
fn test_all_events_stored_not_filtered() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "tool_result"}}]},
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "api_request"}}]},
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "tool_decision"}}]},
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "session_start"}}]}
                ]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    // All 4 events should be stored - no filtering at parse time
    assert_eq!(events.len(), 4);
}

// =============================================================================
// Attribute Preservation Tests
// =============================================================================
// All attributes should be preserved in the HashMap for query-time access.

/// Test that all attribute types are preserved
#[test]
fn test_all_attribute_types_preserved() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "string_attr", "value": {"stringValue": "hello"}},
                        {"key": "int_attr", "value": {"intValue": 42}},
                        {"key": "double_attr", "value": {"doubleValue": 3.14}},
                        {"key": "bool_attr", "value": {"boolValue": true}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(events.len(), 1);

    let attrs = &events[0].attributes;
    assert_eq!(attrs.get("string_attr"), Some(&"hello".to_string()));
    assert_eq!(attrs.get("int_attr"), Some(&"42".to_string()));
    assert_eq!(attrs.get("double_attr"), Some(&"3.14".to_string()));
    assert_eq!(attrs.get("bool_attr"), Some(&"true".to_string()));
}

/// Test success attribute as boolean true
#[test]
fn test_success_as_bool_true() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "success", "value": {"boolValue": true}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(
        events[0].attributes.get("success"),
        Some(&"true".to_string())
    );
}

/// Test success attribute as boolean false
#[test]
fn test_success_as_bool_false() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "success", "value": {"boolValue": false}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(
        events[0].attributes.get("success"),
        Some(&"false".to_string())
    );
}

/// Test success attribute as string "true"
#[test]
fn test_success_as_string_true() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "success", "value": {"stringValue": "true"}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(
        events[0].attributes.get("success"),
        Some(&"true".to_string())
    );
}

/// Test success attribute as string "false"
#[test]
fn test_success_as_string_false() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "success", "value": {"stringValue": "false"}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(
        events[0].attributes.get("success"),
        Some(&"false".to_string())
    );
}

// =============================================================================
// Metrics Parsing Tests
// =============================================================================

/// Test parsing token metrics with string intValue
#[test]
fn test_parse_token_metrics_string_int() {
    let json = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "claude_code.token.usage",
                    "sum": {
                        "dataPoints": [{
                            "asInt": "5000",
                            "attributes": [{"key": "type", "value": {"stringValue": "input"}}]
                        }]
                    }
                }]
            }]
        }]
    }"#;

    let metrics = parse_metrics(json.as_bytes()).unwrap();
    assert_eq!(metrics.len(), 1);
    match &metrics[0] {
        ParsedMetric::TokenUsage { token_type, count } => {
            assert_eq!(token_type, "input");
            assert_eq!(*count, 5000);
        }
        _ => panic!("Expected TokenUsage metric"),
    }
}

/// Test parsing cost metrics
#[test]
fn test_parse_cost_metrics() {
    let json = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "claude_code.cost.usage",
                    "sum": {
                        "dataPoints": [{"asDouble": 0.0523}]
                    }
                }]
            }]
        }]
    }"#;

    let metrics = parse_metrics(json.as_bytes()).unwrap();
    assert_eq!(metrics.len(), 1);
    match &metrics[0] {
        ParsedMetric::CostUsage { cost_usd } => {
            assert!((*cost_usd - 0.0523).abs() < 0.0001);
        }
        _ => panic!("Expected CostUsage metric"),
    }
}

/// Test parsing session metrics (lines of code, commits, PRs)
#[test]
fn test_parse_session_metrics_json() {
    let json = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [
                    {"name": "claude_code.lines_of_code.count", "sum": {"dataPoints": [{"asInt": 150}]}},
                    {"name": "claude_code.commit.count", "sum": {"dataPoints": [{"asInt": 3}]}}
                ]
            }]
        }]
    }"#;

    let metrics = parse_metrics(json.as_bytes()).unwrap();
    assert_eq!(metrics.len(), 2);
}

// =============================================================================
// Error Handling Tests
// =============================================================================

/// Test graceful handling of invalid protobuf data
#[test]
fn test_invalid_protobuf_graceful() {
    let garbage = b"not valid protobuf or json data";
    let events = parse_logs(garbage).unwrap();
    // Should return empty vec, not error
    assert!(events.is_empty());
}

/// Test graceful handling of empty data
#[test]
fn test_empty_data_graceful() {
    let empty = b"";
    let events = parse_logs(empty).unwrap();
    assert!(events.is_empty());
}

/// Test missing optional fields don't cause errors
#[test]
fn test_missing_optional_fields() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}}
                    ]
                }]
            }]
        }]
    }"#;

    let events = parse_logs(json.as_bytes()).unwrap();
    assert_eq!(events.len(), 1);
    // No timestamp provided, should use current time
    assert!(events[0].timestamp.timestamp() > 0);
    // No body provided
    assert!(events[0].body.is_none());
}

// =============================================================================
// Original JSON Structure Tests (kept for compatibility)
// =============================================================================

/// Test parsing empty metrics
#[test]
fn test_parse_empty_metrics() {
    let json = r#"{"resourceMetrics": []}"#;
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    assert!(parsed["resourceMetrics"].as_array().unwrap().is_empty());
}

/// Test parsing multiple token types
#[test]
fn test_parse_multiple_token_types() {
    let json = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [
                    {
                        "name": "claude_code.token.usage",
                        "sum": {
                            "dataPoints": [
                                {"asInt": 1000, "attributes": [{"key": "type", "value": {"stringValue": "input"}}]},
                                {"asInt": 500, "attributes": [{"key": "type", "value": {"stringValue": "output"}}]},
                                {"asInt": 2000, "attributes": [{"key": "type", "value": {"stringValue": "cacheRead"}}]}
                            ]
                        }
                    }
                ]
            }]
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    let data_points =
        &parsed["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["sum"]["dataPoints"];
    assert_eq!(data_points.as_array().unwrap().len(), 3);
}

/// Test parsing cost metrics
#[test]
fn test_parse_cost_metric() {
    let json = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "claude_code.cost.usage",
                    "sum": {
                        "dataPoints": [{"asDouble": 0.0523}]
                    }
                }]
            }]
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    let cost = parsed["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["sum"]["dataPoints"]
        [0]["asDouble"]
        .as_f64()
        .unwrap();
    assert!((cost - 0.0523).abs() < 0.0001);
}

/// Test parsing session metrics
#[test]
fn test_parse_session_metrics() {
    let json = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [
                    {"name": "claude_code.lines_of_code.count", "sum": {"dataPoints": [{"asInt": 150}]}},
                    {"name": "claude_code.commit.count", "sum": {"dataPoints": [{"asInt": 3}]}},
                    {"name": "claude_code.pull_request.count", "sum": {"dataPoints": [{"asInt": 1}]}}
                ]
            }]
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    let metrics = parsed["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
        .as_array()
        .unwrap();
    assert_eq!(metrics.len(), 3);
}

/// Test parsing gauge metrics (alternative to sum)
#[test]
fn test_parse_gauge_metric() {
    let json = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "claude_code.token.usage",
                    "gauge": {
                        "dataPoints": [{"asInt": 750, "attributes": [{"key": "type", "value": {"stringValue": "input"}}]}]
                    }
                }]
            }]
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    assert!(parsed["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["gauge"].is_object());
}

/// Test parsing tool events with all fields
#[test]
fn test_parse_tool_event_complete() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "timeUnixNano": 1705000000000000000,
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "claude_code.tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Edit"}},
                        {"key": "success", "value": {"boolValue": true}},
                        {"key": "duration_ms", "value": {"intValue": 45}},
                        {"key": "decision", "value": {"stringValue": "approved"}}
                    ]
                }]
            }]
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    let attrs = parsed["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
        .as_array()
        .unwrap();
    assert_eq!(attrs.len(), 5);
}

/// Test parsing failed tool event
#[test]
fn test_parse_tool_event_failure() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "claude_code.tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Bash"}},
                        {"key": "success", "value": {"boolValue": false}},
                        {"key": "duration_ms", "value": {"intValue": 1200}},
                        {"key": "error", "value": {"stringValue": "Command failed with exit code 1"}}
                    ]
                }]
            }]
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    let success = parsed["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["key"] == "success")
        .unwrap()["value"]["boolValue"]
        .as_bool()
        .unwrap();
    assert!(!success);
}

/// Test parsing multiple log records
#[test]
fn test_parse_multiple_log_records() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "claude_code.tool_result"}}, {"key": "tool_name", "value": {"stringValue": "Read"}}]},
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "claude_code.tool_result"}}, {"key": "tool_name", "value": {"stringValue": "Write"}}]},
                    {"attributes": [{"key": "event.name", "value": {"stringValue": "claude_code.tool_result"}}, {"key": "tool_name", "value": {"stringValue": "Grep"}}]}
                ]
            }]
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    let records = parsed["resourceLogs"][0]["scopeLogs"][0]["logRecords"]
        .as_array()
        .unwrap();
    assert_eq!(records.len(), 3);
}

/// Test parsing non-tool events (should be filtered)
#[test]
fn test_parse_non_tool_event() {
    let json = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "claude_code.api_request"}},
                        {"key": "model", "value": {"stringValue": "claude-3-opus"}}
                    ]
                }]
            }]
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    let event_name = parsed["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["key"] == "event.name")
        .unwrap()["value"]["stringValue"]
        .as_str()
        .unwrap();
    assert_eq!(event_name, "claude_code.api_request");
}

/// Test malformed JSON handling
#[test]
fn test_malformed_json() {
    let json = r#"{"resourceMetrics": [{"incomplete"#;
    let result: Result<serde_json::Value, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

/// Test empty attributes
#[test]
fn test_empty_attributes() {
    let json = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "claude_code.token.usage",
                    "sum": {
                        "dataPoints": [{"asInt": 100, "attributes": []}]
                    }
                }]
            }]
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
    let attrs = parsed["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["sum"]["dataPoints"]
        [0]["attributes"]
        .as_array()
        .unwrap();
    assert!(attrs.is_empty());
}
