//! End-to-end tests for QwenCodeProvider::ensure_configured_at.

use agenttop::providers::qwen_code::QwenCodeProvider;
use serde_json::Value;
use std::fs;

fn read_settings(path: &std::path::Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn creates_settings_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    assert!(QwenCodeProvider.ensure_configured_at(&path).unwrap());

    let s = read_settings(&path);
    assert_eq!(s["telemetry"]["enabled"], Value::Bool(true));
    assert_eq!(s["telemetry"]["otlpEndpoint"], "http://localhost:4318");
}

#[test]
fn idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    assert!(QwenCodeProvider.ensure_configured_at(&path).unwrap());
    assert!(!QwenCodeProvider.ensure_configured_at(&path).unwrap());
}

#[test]
fn preserves_unrelated_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({
            "model": "qwen2.5-coder-32b"
        }))
        .unwrap(),
    )
    .unwrap();

    QwenCodeProvider.ensure_configured_at(&path).unwrap();

    let s = read_settings(&path);
    assert_eq!(s["model"], "qwen2.5-coder-32b");
    assert_eq!(s["telemetry"]["enabled"], Value::Bool(true));
}

#[test]
fn creates_backup_only_on_modification() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    QwenCodeProvider.ensure_configured_at(&path).unwrap(); // creates file
    assert!(
        !path.with_extension("json.bak").exists(),
        "no backup on initial create"
    );

    fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({
            "telemetry": { "enabled": false }
        }))
        .unwrap(),
    )
    .unwrap();

    QwenCodeProvider.ensure_configured_at(&path).unwrap(); // modifies
    assert!(
        path.with_extension("json.bak").exists(),
        "backup created on modification"
    );
}

#[test]
fn parse_error_propagates() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    fs::write(&path, "{not valid json").unwrap();

    let result = QwenCodeProvider.ensure_configured_at(&path);
    assert!(
        result.is_err(),
        "malformed settings.json should produce a clear error"
    );
}
