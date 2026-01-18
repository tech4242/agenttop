use anyhow::Result;
use chrono::{TimeZone, Utc};
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::common::v1::AnyValue;
use opentelemetry_proto::tonic::common::v1::any_value::Value as AnyValueKind;
use prost::Message;
use serde::Deserialize;
use std::collections::HashMap;

use crate::storage::LogEvent;

#[derive(Debug, Clone)]
pub enum ParsedMetric {
    TokenUsage { token_type: String, count: u64 },
    CostUsage { cost_usd: f64 },
    SessionMetric { name: String, value: i64 },
}

// OTLP JSON structures for metrics (fallback)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpMetricsRequest {
    resource_metrics: Vec<ResourceMetrics>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceMetrics {
    scope_metrics: Vec<ScopeMetrics>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScopeMetrics {
    metrics: Vec<Metric>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metric {
    name: String,
    #[serde(default)]
    sum: Option<MetricSum>,
    #[serde(default)]
    gauge: Option<MetricGauge>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetricSum {
    data_points: Vec<DataPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetricGauge {
    data_points: Vec<DataPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataPoint {
    #[serde(default, deserialize_with = "deserialize_optional_string_or_i64")]
    as_int: Option<i64>,
    #[serde(default)]
    as_double: Option<f64>,
    #[serde(default)]
    attributes: Vec<Attribute>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Attribute {
    key: String,
    value: AttributeValue,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttributeValue {
    #[serde(default)]
    string_value: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_i64")]
    int_value: Option<i64>,
    #[serde(default)]
    double_value: Option<f64>,
    #[serde(default)]
    bool_value: Option<bool>,
}

/// Deserialize a field that can be either a string or i64 (OTLP JSON uses strings for large numbers)
fn deserialize_optional_string_or_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrI64 {
        String(String),
        I64(i64),
    }

    match Option::<StringOrI64>::deserialize(deserializer)? {
        Some(StringOrI64::String(s)) => s.parse().map(Some).map_err(D::Error::custom),
        Some(StringOrI64::I64(n)) => Ok(Some(n)),
        None => Ok(None),
    }
}

// OTLP JSON structures for logs (fallback)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OtlpLogsRequest {
    resource_logs: Vec<ResourceLogs>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceLogs {
    scope_logs: Vec<ScopeLogs>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScopeLogs {
    log_records: Vec<LogRecord>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogRecord {
    #[serde(default, deserialize_with = "deserialize_string_or_u64")]
    time_unix_nano: Option<u64>,
    #[serde(default)]
    body: Option<LogBody>,
    #[serde(default)]
    attributes: Vec<Attribute>,
}

/// Deserialize a field that can be either a string or u64 (OTLP JSON uses strings for large numbers)
fn deserialize_string_or_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrU64 {
        String(String),
        U64(u64),
    }

    match Option::<StringOrU64>::deserialize(deserializer)? {
        Some(StringOrU64::String(s)) => s.parse().map(Some).map_err(D::Error::custom),
        Some(StringOrU64::U64(n)) => Ok(Some(n)),
        None => Ok(None),
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogBody {
    #[serde(default)]
    string_value: Option<String>,
}

/// Extract string value from AnyValue
fn get_string_value(value: &AnyValue) -> Option<String> {
    match &value.value {
        Some(AnyValueKind::StringValue(s)) => Some(s.clone()),
        _ => None,
    }
}

// Note: The following helper functions (get_int_value, get_bool_value, get_bool_from_string_or_bool,
// get_double_value) have been removed as they are no longer needed. With the new architecture,
// we store all attributes as strings in a HashMap and do value conversion at query time instead.

/// Convert any AnyValue to a string representation for storing in HashMap
fn get_any_value_as_string(value: &AnyValue) -> Option<String> {
    match &value.value {
        Some(AnyValueKind::StringValue(s)) => Some(s.clone()),
        Some(AnyValueKind::IntValue(i)) => Some(i.to_string()),
        Some(AnyValueKind::DoubleValue(d)) => Some(d.to_string()),
        Some(AnyValueKind::BoolValue(b)) => Some(b.to_string()),
        _ => None,
    }
}

pub fn parse_metrics(data: &[u8]) -> Result<Vec<ParsedMetric>> {
    // Try protobuf first (Claude Code uses http/protobuf by default)
    if let Ok(request) = ExportMetricsServiceRequest::decode(data) {
        tracing::debug!("Successfully parsed metrics as protobuf");
        return parse_metrics_proto(request);
    }

    // Try JSON as fallback
    if let Ok(request) = serde_json::from_slice::<OtlpMetricsRequest>(data) {
        tracing::debug!("Successfully parsed metrics as JSON");
        return parse_metrics_json(request);
    }

    tracing::warn!(
        "Failed to parse metrics data ({} bytes) as protobuf or JSON",
        data.len()
    );
    Ok(vec![])
}

fn parse_metrics_proto(request: ExportMetricsServiceRequest) -> Result<Vec<ParsedMetric>> {
    let mut metrics = Vec::new();

    for resource in request.resource_metrics {
        for scope in resource.scope_metrics {
            for metric in scope.metrics {
                let name = &metric.name;

                // Get data points from sum or gauge
                let data_points: Vec<_> = metric
                    .data
                    .map(|d| match d {
                        opentelemetry_proto::tonic::metrics::v1::metric::Data::Sum(sum) => {
                            sum.data_points
                        }
                        opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(gauge) => {
                            gauge.data_points
                        }
                        _ => vec![],
                    })
                    .unwrap_or_default();

                for dp in data_points {
                    let parsed = match name.as_str() {
                        "claude_code.token.usage" => {
                            let token_type = dp
                                .attributes
                                .iter()
                                .find(|a| a.key == "type")
                                .and_then(|a| a.value.as_ref())
                                .and_then(get_string_value)
                                .unwrap_or_else(|| "unknown".to_string());

                            let count = match dp.value {
                                Some(
                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(i),
                                ) => i as u64,
                                Some(
                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(d),
                                ) => d as u64,
                                None => 0,
                            };

                            Some(ParsedMetric::TokenUsage { token_type, count })
                        }
                        "claude_code.cost.usage" => {
                            let cost_usd = match dp.value {
                                Some(
                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(d),
                                ) => d,
                                Some(
                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(i),
                                ) => i as f64,
                                None => 0.0,
                            };
                            Some(ParsedMetric::CostUsage { cost_usd })
                        }
                        n if n.starts_with("claude_code.") => {
                            let metric_name = n
                                .replace("claude_code.", "")
                                .replace(".count", "")
                                .replace(".total", "");

                            let value = match dp.value {
                                Some(
                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(i),
                                ) => i,
                                Some(
                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(d),
                                ) => d as i64,
                                None => 0,
                            };

                            Some(ParsedMetric::SessionMetric {
                                name: metric_name,
                                value,
                            })
                        }
                        _ => None,
                    };

                    if let Some(m) = parsed {
                        metrics.push(m);
                    }
                }
            }
        }
    }

    tracing::debug!("Parsed {} metrics from protobuf", metrics.len());
    Ok(metrics)
}

fn parse_metrics_json(request: OtlpMetricsRequest) -> Result<Vec<ParsedMetric>> {
    let mut metrics = Vec::new();

    for resource in request.resource_metrics {
        for scope in resource.scope_metrics {
            for metric in scope.metrics {
                let data_points = metric
                    .sum
                    .map(|s| s.data_points)
                    .or_else(|| metric.gauge.map(|g| g.data_points))
                    .unwrap_or_default();

                for dp in data_points {
                    let parsed = match metric.name.as_str() {
                        "claude_code.token.usage" => {
                            let token_type = dp
                                .attributes
                                .iter()
                                .find(|a| a.key == "type")
                                .and_then(|a| a.value.string_value.clone())
                                .unwrap_or_else(|| "unknown".to_string());

                            let count = dp.as_int.unwrap_or(0) as u64;
                            Some(ParsedMetric::TokenUsage { token_type, count })
                        }
                        "claude_code.cost.usage" => {
                            let cost_usd = dp.as_double.unwrap_or(0.0);
                            Some(ParsedMetric::CostUsage { cost_usd })
                        }
                        "claude_code.lines_of_code.count"
                        | "claude_code.commit.count"
                        | "claude_code.pull_request.count"
                        | "claude_code.active_time.total" => {
                            let name = metric
                                .name
                                .replace("claude_code.", "")
                                .replace(".count", "")
                                .replace(".total", "");
                            let value = dp.as_int.unwrap_or(0);
                            Some(ParsedMetric::SessionMetric { name, value })
                        }
                        _ => None,
                    };

                    if let Some(m) = parsed {
                        metrics.push(m);
                    }
                }
            }
        }
    }

    Ok(metrics)
}

/// Parse logs and return ALL log events without filtering.
/// Filtering by event type (e.g., tool_result) happens at query time, not ingestion time.
/// This approach matches the ai-observer reference implementation and allows us to:
/// 1. See exactly what events Claude Code sends
/// 2. Support any event.name format (tool_result, claude_code.tool_result, etc.)
/// 3. Debug issues more easily by inspecting raw log data
pub fn parse_logs(data: &[u8]) -> Result<Vec<LogEvent>> {
    // Try protobuf first (Claude Code uses http/protobuf by default)
    if let Ok(request) = ExportLogsServiceRequest::decode(data) {
        tracing::debug!("Successfully parsed logs as protobuf");
        return parse_logs_proto(request);
    }

    // Try JSON as fallback
    if let Ok(request) = serde_json::from_slice::<OtlpLogsRequest>(data) {
        tracing::debug!("Successfully parsed logs as JSON");
        return parse_logs_json(request);
    }

    tracing::warn!(
        "Failed to parse logs data ({} bytes) as protobuf or JSON",
        data.len()
    );
    Ok(vec![])
}

fn parse_logs_proto(request: ExportLogsServiceRequest) -> Result<Vec<LogEvent>> {
    let mut events = Vec::new();

    for resource in request.resource_logs {
        for scope in resource.scope_logs {
            for record in scope.log_records {
                // Extract event.name from attributes
                let event_name = record
                    .attributes
                    .iter()
                    .find(|a| a.key == "event.name")
                    .and_then(|a| a.value.as_ref())
                    .and_then(get_string_value);

                // Store ALL attributes as a HashMap for query-time filtering
                let attributes: HashMap<String, String> = record
                    .attributes
                    .iter()
                    .filter_map(|a| {
                        a.value
                            .as_ref()
                            .and_then(|v| get_any_value_as_string(v).map(|s| (a.key.clone(), s)))
                    })
                    .collect();

                // Extract body if present
                let body = record.body.as_ref().and_then(get_string_value);

                // Parse timestamp from time_unix_nano
                let timestamp = if record.time_unix_nano > 0 {
                    Utc.timestamp_nanos(record.time_unix_nano as i64)
                } else {
                    Utc::now()
                };

                events.push(LogEvent {
                    timestamp,
                    event_name,
                    body,
                    attributes,
                });
            }
        }
    }

    tracing::debug!("Parsed {} log events from protobuf", events.len());
    Ok(events)
}

fn parse_logs_json(request: OtlpLogsRequest) -> Result<Vec<LogEvent>> {
    let mut events = Vec::new();

    for resource in request.resource_logs {
        for scope in resource.scope_logs {
            for record in scope.log_records {
                // Extract event.name from attributes
                let event_name = record
                    .attributes
                    .iter()
                    .find(|a| a.key == "event.name")
                    .and_then(|a| a.value.string_value.clone());

                // Store ALL attributes as a HashMap for query-time filtering
                let attributes: HashMap<String, String> = record
                    .attributes
                    .iter()
                    .filter_map(|a| {
                        get_json_attribute_as_string(&a.value).map(|s| (a.key.clone(), s))
                    })
                    .collect();

                // Extract body if present
                let body = record.body.as_ref().and_then(|b| b.string_value.clone());

                // Parse timestamp
                let timestamp = record
                    .time_unix_nano
                    .map(|nanos| Utc.timestamp_nanos(nanos as i64))
                    .unwrap_or_else(Utc::now);

                events.push(LogEvent {
                    timestamp,
                    event_name,
                    body,
                    attributes,
                });
            }
        }
    }

    Ok(events)
}

/// Convert JSON AttributeValue to string
fn get_json_attribute_as_string(value: &AttributeValue) -> Option<String> {
    if let Some(s) = &value.string_value {
        return Some(s.clone());
    }
    if let Some(i) = value.int_value {
        return Some(i.to_string());
    }
    if let Some(d) = value.double_value {
        return Some(d.to_string());
    }
    if let Some(b) = value.bool_value {
        return Some(b.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_token_metrics_json() {
        let json = r#"{
            "resourceMetrics": [{
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "claude_code.token.usage",
                        "sum": {
                            "dataPoints": [{
                                "asInt": 1000,
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
                assert_eq!(*count, 1000);
            }
            _ => panic!("Expected TokenUsage metric"),
        }
    }

    #[test]
    fn test_parse_log_event_json_bool_success() {
        let json = r#"{
            "resourceLogs": [{
                "scopeLogs": [{
                    "logRecords": [{
                        "attributes": [
                            {"key": "event.name", "value": {"stringValue": "tool_result"}},
                            {"key": "tool_name", "value": {"stringValue": "Bash"}},
                            {"key": "success", "value": {"boolValue": true}},
                            {"key": "duration_ms", "value": {"intValue": 150}}
                        ]
                    }]
                }]
            }]
        }"#;

        let events = parse_logs(json.as_bytes()).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_name, Some("tool_result".to_string()));
        assert_eq!(
            events[0].attributes.get("tool_name"),
            Some(&"Bash".to_string())
        );
        assert_eq!(
            events[0].attributes.get("success"),
            Some(&"true".to_string())
        );
        assert_eq!(
            events[0].attributes.get("duration_ms"),
            Some(&"150".to_string())
        );
    }

    #[test]
    fn test_parse_log_event_json_string_success() {
        // Test with string success value (Claude Code's actual format)
        let json = r#"{
            "resourceLogs": [{
                "scopeLogs": [{
                    "logRecords": [{
                        "attributes": [
                            {"key": "event.name", "value": {"stringValue": "tool_result"}},
                            {"key": "tool_name", "value": {"stringValue": "Read"}},
                            {"key": "success", "value": {"stringValue": "true"}},
                            {"key": "duration_ms", "value": {"intValue": 50}}
                        ]
                    }]
                }]
            }]
        }"#;

        let events = parse_logs(json.as_bytes()).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_name, Some("tool_result".to_string()));
        assert_eq!(
            events[0].attributes.get("tool_name"),
            Some(&"Read".to_string())
        );
        assert_eq!(
            events[0].attributes.get("success"),
            Some(&"true".to_string())
        );
        assert_eq!(
            events[0].attributes.get("duration_ms"),
            Some(&"50".to_string())
        );
    }

    #[test]
    fn test_parse_log_event_json_string_success_false() {
        // Test with string "false" value
        let json = r#"{
            "resourceLogs": [{
                "scopeLogs": [{
                    "logRecords": [{
                        "attributes": [
                            {"key": "event.name", "value": {"stringValue": "tool_result"}},
                            {"key": "tool_name", "value": {"stringValue": "Glob"}},
                            {"key": "success", "value": {"stringValue": "false"}},
                            {"key": "duration_ms", "value": {"intValue": 200}},
                            {"key": "error", "value": {"stringValue": "File not found"}}
                        ]
                    }]
                }]
            }]
        }"#;

        let events = parse_logs(json.as_bytes()).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_name, Some("tool_result".to_string()));
        assert_eq!(
            events[0].attributes.get("tool_name"),
            Some(&"Glob".to_string())
        );
        assert_eq!(
            events[0].attributes.get("success"),
            Some(&"false".to_string())
        );
        assert_eq!(
            events[0].attributes.get("duration_ms"),
            Some(&"200".to_string())
        );
        assert_eq!(
            events[0].attributes.get("error"),
            Some(&"File not found".to_string())
        );
    }

    #[test]
    fn test_parse_log_event_stores_all_events() {
        // Test that we now store ALL events, not just tool_result
        let json = r#"{
            "resourceLogs": [{
                "scopeLogs": [{
                    "logRecords": [
                        {
                            "attributes": [
                                {"key": "event.name", "value": {"stringValue": "tool_result"}},
                                {"key": "tool_name", "value": {"stringValue": "Read"}}
                            ]
                        },
                        {
                            "attributes": [
                                {"key": "event.name", "value": {"stringValue": "api_request"}},
                                {"key": "model", "value": {"stringValue": "claude-3"}}
                            ]
                        },
                        {
                            "attributes": [
                                {"key": "event.name", "value": {"stringValue": "claude_code.tool_result"}},
                                {"key": "tool_name", "value": {"stringValue": "Bash"}}
                            ]
                        }
                    ]
                }]
            }]
        }"#;

        let events = parse_logs(json.as_bytes()).unwrap();
        // Now we should get ALL 3 events, not just tool_result ones
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_name, Some("tool_result".to_string()));
        assert_eq!(events[1].event_name, Some("api_request".to_string()));
        assert_eq!(
            events[2].event_name,
            Some("claude_code.tool_result".to_string())
        );
    }
}
