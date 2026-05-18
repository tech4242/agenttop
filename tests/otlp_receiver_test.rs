//! End-to-end tests for the OTLP HTTP receiver.
//!
//! Drives the axum router via `tower::ServiceExt::oneshot` so we cover the
//! full payload → parser → storage path without binding a real TCP socket.
//! Uses JSON OTLP payloads (the receiver falls back from protobuf to JSON
//! when prost decode fails, so JSON works for testing purposes).

use agenttop::otlp::build_router;
use agenttop::storage::StorageHandle;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

async fn post_json(storage: StorageHandle, path: &str, body: &str) -> (StatusCode, StorageHandle) {
    let app = build_router(storage.clone());
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    (status, storage)
}

#[tokio::test]
async fn logs_endpoint_persists_tool_result_event() {
    let storage = StorageHandle::new_in_memory().unwrap();

    let payload = r#"{
        "resourceLogs": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "claude-code"}}
                ]
            },
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Bash"}},
                        {"key": "success", "value": {"boolValue": true}},
                        {"key": "duration_ms", "value": {"intValue": 250}},
                        {"key": "decision_type", "value": {"stringValue": "accept"}},
                        {"key": "tool_use_id", "value": {"stringValue": "tuid-1"}}
                    ]
                }]
            }]
        }]
    }"#;

    let (status, storage) = post_json(storage, "/v1/logs", payload).await;
    assert_eq!(status, StatusCode::OK);

    std::thread::sleep(std::time::Duration::from_millis(150));

    let metrics = storage.get_tool_metrics(None).unwrap();
    let bash = metrics
        .iter()
        .find(|t| t.tool_name == "Bash")
        .expect("Bash tool_result must land in storage");
    assert_eq!(bash.call_count, 1);
    assert_eq!(bash.success_count, 1);
}

#[tokio::test]
async fn logs_endpoint_persists_tool_decision_event() {
    let storage = StorageHandle::new_in_memory().unwrap();

    let result_payload = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "tool_result"}},
                        {"key": "tool_name", "value": {"stringValue": "Bash"}},
                        {"key": "success", "value": {"stringValue": "true"}},
                        {"key": "duration_ms", "value": {"intValue": 100}},
                        {"key": "decision_type", "value": {"stringValue": "accept"}},
                        {"key": "tool_use_id", "value": {"stringValue": "tuid-accept"}}
                    ]
                }]
            }]
        }]
    }"#;
    let decision_payload = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [
                    {
                        "attributes": [
                            {"key": "event.name", "value": {"stringValue": "tool_decision"}},
                            {"key": "tool_name", "value": {"stringValue": "Bash"}},
                            {"key": "decision", "value": {"stringValue": "accept"}},
                            {"key": "tool_use_id", "value": {"stringValue": "tuid-accept"}}
                        ]
                    },
                    {
                        "attributes": [
                            {"key": "event.name", "value": {"stringValue": "tool_decision"}},
                            {"key": "tool_name", "value": {"stringValue": "Bash"}},
                            {"key": "decision", "value": {"stringValue": "reject"}},
                            {"key": "tool_use_id", "value": {"stringValue": "tuid-reject"}}
                        ]
                    }
                ]
            }]
        }]
    }"#;

    let (s1, storage) = post_json(storage, "/v1/logs", result_payload).await;
    assert_eq!(s1, StatusCode::OK);
    let (s2, storage) = post_json(storage, "/v1/logs", decision_payload).await;
    assert_eq!(s2, StatusCode::OK);

    std::thread::sleep(std::time::Duration::from_millis(150));

    let metrics = storage.get_tool_metrics(None).unwrap();
    let bash = metrics.iter().find(|t| t.tool_name == "Bash").unwrap();
    assert_eq!(bash.call_count, 1);
    assert_eq!(bash.approved_count, 1);
    assert_eq!(bash.rejected_count, 1);
    // 1 / (1+1) = 50%
    assert!((bash.approval_rate() - 50.0).abs() < 0.5);
}

#[tokio::test]
async fn metrics_endpoint_records_token_usage() {
    let storage = StorageHandle::new_in_memory().unwrap();

    let payload = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "claude_code.token.usage",
                    "sum": {
                        "dataPoints": [
                            {
                                "asInt": 1000,
                                "attributes": [{"key": "type", "value": {"stringValue": "input"}}]
                            },
                            {
                                "asInt": 500,
                                "attributes": [{"key": "type", "value": {"stringValue": "output"}}]
                            }
                        ]
                    }
                }]
            }]
        }]
    }"#;

    let (status, storage) = post_json(storage, "/v1/metrics", payload).await;
    assert_eq!(status, StatusCode::OK);

    std::thread::sleep(std::time::Duration::from_millis(150));

    let tokens = storage.get_token_metrics(None).unwrap();
    assert_eq!(tokens.input_tokens, 1000);
    assert_eq!(tokens.output_tokens, 500);
}

#[tokio::test]
async fn metrics_endpoint_records_cost() {
    let storage = StorageHandle::new_in_memory().unwrap();

    let payload = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "claude_code.cost.usage",
                    "sum": {
                        "dataPoints": [{"asDouble": 0.42}]
                    }
                }]
            }]
        }]
    }"#;

    let (status, storage) = post_json(storage, "/v1/metrics", payload).await;
    assert_eq!(status, StatusCode::OK);
    std::thread::sleep(std::time::Duration::from_millis(150));

    let tokens = storage.get_token_metrics(None).unwrap();
    assert!((tokens.total_cost_usd - 0.42).abs() < 0.001);
}

#[tokio::test]
async fn traces_endpoint_returns_ok_but_no_storage_change() {
    let storage = StorageHandle::new_in_memory().unwrap();
    // Traces are accepted but not currently processed.
    let (status, storage) = post_json(storage, "/v1/traces", "{}").await;
    assert_eq!(status, StatusCode::OK);
    std::thread::sleep(std::time::Duration::from_millis(50));

    assert!(storage.get_tool_metrics(None).unwrap().is_empty());
    assert_eq!(storage.get_token_metrics(None).unwrap().input_tokens, 0);
}

#[tokio::test]
async fn malformed_payload_returns_ok_with_empty_parse() {
    // The receiver currently swallows unparseable payloads (parse_logs
    // returns Ok(vec![])) and returns 200. This documents that behavior.
    let storage = StorageHandle::new_in_memory().unwrap();
    let (status, storage) = post_json(storage, "/v1/logs", "not valid json").await;
    assert_eq!(status, StatusCode::OK);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(storage.get_tool_metrics(None).unwrap().is_empty());
}

#[tokio::test]
async fn compaction_event_lands_in_compaction_stats() {
    let storage = StorageHandle::new_in_memory().unwrap();

    let payload = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "attributes": [
                        {"key": "event.name", "value": {"stringValue": "claude_code.compaction"}},
                        {"key": "pre_tokens", "value": {"intValue": 180000}},
                        {"key": "post_tokens", "value": {"intValue": 60000}}
                    ]
                }]
            }]
        }]
    }"#;

    let (status, storage) = post_json(storage, "/v1/logs", payload).await;
    assert_eq!(status, StatusCode::OK);
    std::thread::sleep(std::time::Duration::from_millis(150));

    let comp = storage.get_compaction_stats(None).unwrap();
    assert_eq!(comp.count, 1);
    assert_eq!(comp.last_pre_tokens, Some(180_000));
    assert_eq!(comp.last_post_tokens, Some(60_000));
}
