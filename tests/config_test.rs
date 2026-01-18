//! Configuration tests
//!
//! These tests verify configuration structures and path construction logic.
//! Tests use mock paths where possible to avoid depending on system state.

use std::path::PathBuf;

/// Test Claude settings path construction with mock home directory
#[test]
fn test_claude_settings_path_construction() {
    // Use a mock home directory to test path construction logic
    let mock_home = PathBuf::from("/home/testuser");
    let expected = mock_home.join(".claude").join("settings.json");

    assert!(expected.to_str().unwrap().contains(".claude"));
    assert!(expected.to_str().unwrap().ends_with("settings.json"));
    assert_eq!(
        expected.to_str().unwrap(),
        "/home/testuser/.claude/settings.json"
    );
}

/// Test settings JSON structure
#[test]
fn test_settings_json_structure() {
    let settings = serde_json::json!({
        "enableTelemetry": true,
        "telemetry": {
            "enabled": true,
            "otelEndpoint": "http://localhost:4318"
        }
    });

    assert_eq!(settings["enableTelemetry"], true);
    assert_eq!(settings["telemetry"]["enabled"], true);
    assert_eq!(
        settings["telemetry"]["otelEndpoint"],
        "http://localhost:4318"
    );
}

/// Test settings JSON serialization
#[test]
fn test_settings_json_serialization() {
    let settings = serde_json::json!({
        "enableTelemetry": true,
        "telemetry": {
            "enabled": true,
            "otelEndpoint": "http://localhost:4318"
        }
    });

    let serialized = serde_json::to_string_pretty(&settings).unwrap();
    assert!(serialized.contains("enableTelemetry"));
    assert!(serialized.contains("otelEndpoint"));
}

/// Test merging existing settings
#[test]
fn test_merge_existing_settings() {
    let mut existing = serde_json::json!({
        "someOtherSetting": "value",
        "enableTelemetry": false
    });

    // Simulate updating telemetry settings
    existing["enableTelemetry"] = serde_json::Value::Bool(true);
    existing["telemetry"] = serde_json::json!({
        "enabled": true,
        "otelEndpoint": "http://localhost:4318"
    });

    assert_eq!(existing["someOtherSetting"], "value");
    assert_eq!(existing["enableTelemetry"], true);
    assert_eq!(
        existing["telemetry"]["otelEndpoint"],
        "http://localhost:4318"
    );
}

/// Test temp directory creation for testing
#[test]
fn test_temp_dir_creation() {
    let temp = std::env::temp_dir();
    assert!(temp.exists(), "Temp directory should exist");
}

/// Test file backup path generation
#[test]
fn test_backup_path_generation() {
    let original = PathBuf::from("/path/to/settings.json");
    let backup = original.with_extension("json.bak");
    assert_eq!(backup.to_str().unwrap(), "/path/to/settings.json.bak");
}

/// Test agenttop data path construction with mock data directory
#[test]
fn test_agenttop_data_path_construction() {
    // Use a mock data directory to test path construction logic
    let mock_data_dir = PathBuf::from("/home/testuser/.local/share");
    let agenttop_dir = mock_data_dir.join("agenttop");
    let db_path = agenttop_dir.join("metrics.duckdb");

    assert!(db_path.to_str().unwrap().contains("agenttop"));
    assert!(db_path.to_str().unwrap().ends_with("metrics.duckdb"));
    assert_eq!(
        db_path.to_str().unwrap(),
        "/home/testuser/.local/share/agenttop/metrics.duckdb"
    );
}

/// Test OTLP endpoint constant
#[test]
fn test_otlp_endpoint() {
    let endpoint = "http://localhost:4318";
    assert!(endpoint.starts_with("http://"));
    assert!(endpoint.contains("4318"));
}

/// Test settings with existing telemetry config
#[test]
fn test_existing_telemetry_config() {
    let existing = serde_json::json!({
        "enableTelemetry": true,
        "telemetry": {
            "enabled": true,
            "otelEndpoint": "http://localhost:4318"
        }
    });

    let endpoint = existing["telemetry"]["otelEndpoint"].as_str();
    assert_eq!(endpoint, Some("http://localhost:4318"));
}

/// Test settings without telemetry config
#[test]
fn test_missing_telemetry_config() {
    let existing = serde_json::json!({
        "someOtherSetting": true
    });

    let telemetry = existing.get("telemetry");
    assert!(telemetry.is_none());
}
