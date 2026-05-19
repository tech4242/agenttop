//! End-to-end tests for GeminiCliProvider::ensure_configured_at.

use agenttop::providers::gemini_cli::GeminiCliProvider;
use serde_json::Value;
use std::fs;

fn read_settings(path: &std::path::Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn creates_settings_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let changed = GeminiCliProvider.ensure_configured_at(&path).unwrap();
    assert!(changed);
    assert!(path.exists());

    let s = read_settings(&path);
    assert_eq!(s["telemetry"]["enabled"], Value::Bool(true));
    assert_eq!(s["telemetry"]["target"], "local");
    assert_eq!(s["telemetry"]["otlpEndpoint"], "http://localhost:4318");
    assert!(!path.with_extension("json.bak").exists());
}

#[test]
fn idempotent_when_already_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    assert!(GeminiCliProvider.ensure_configured_at(&path).unwrap());
    assert!(!GeminiCliProvider.ensure_configured_at(&path).unwrap());
}

#[test]
fn preserves_unrelated_existing_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let original = serde_json::json!({
        "theme": "dark",
        "model": "gemini-2.0-pro"
    });
    fs::write(&path, serde_json::to_string_pretty(&original).unwrap()).unwrap();

    let changed = GeminiCliProvider.ensure_configured_at(&path).unwrap();
    assert!(changed);

    let s = read_settings(&path);
    assert_eq!(s["theme"], "dark");
    assert_eq!(s["model"], "gemini-2.0-pro");
    assert_eq!(s["telemetry"]["enabled"], Value::Bool(true));
    assert!(path.with_extension("json.bak").exists());
}

#[test]
fn overwrites_stale_telemetry_block() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let original = serde_json::json!({
        "telemetry": {
            "enabled": true,
            "target": "remote",
            "otlpEndpoint": "http://wrong:9999"
        }
    });
    fs::write(&path, serde_json::to_string_pretty(&original).unwrap()).unwrap();

    let changed = GeminiCliProvider.ensure_configured_at(&path).unwrap();
    assert!(changed, "wrong endpoint must trigger an update");

    let s = read_settings(&path);
    assert_eq!(s["telemetry"]["target"], "local");
    assert_eq!(s["telemetry"]["otlpEndpoint"], "http://localhost:4318");
}

#[test]
fn returns_false_when_unchanged_after_first_run() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let initial = serde_json::json!({
        "telemetry": {
            "enabled": true,
            "target": "local",
            "otlpEndpoint": "http://localhost:4318",
            "otlpProtocol": "http"
        }
    });
    fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

    let changed = GeminiCliProvider.ensure_configured_at(&path).unwrap();
    assert!(!changed, "matching config must report no change");
    assert!(
        !path.with_extension("json.bak").exists(),
        "no backup when no modification"
    );
}
