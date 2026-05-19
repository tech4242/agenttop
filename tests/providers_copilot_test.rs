//! End-to-end tests for CopilotChatProvider::ensure_configured_at.
//!
//! Note: unlike the other auto-configurable providers, Copilot Chat does
//! NOT create a fresh settings.json — VSCode must have written it first.
//! Behavior when missing: returns Ok(false) and logs a warning.

use agenttop::providers::copilot_chat::CopilotChatProvider;
use serde_json::Value;
use std::fs;

fn read_settings(path: &std::path::Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn returns_false_when_settings_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    // No file → can't bootstrap VSCode for the user; expected to no-op.
    let changed = CopilotChatProvider.ensure_configured_at(&path).unwrap();
    assert!(!changed);
    assert!(!path.exists());
}

#[test]
fn patches_existing_vscode_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let original = serde_json::json!({
        "editor.fontSize": 13,
        "workbench.colorTheme": "Default Dark+"
    });
    fs::write(&path, serde_json::to_string_pretty(&original).unwrap()).unwrap();

    let changed = CopilotChatProvider.ensure_configured_at(&path).unwrap();
    assert!(changed);

    let s = read_settings(&path);
    assert_eq!(s["editor.fontSize"], 13);
    assert_eq!(s["workbench.colorTheme"], "Default Dark+");
    assert_eq!(s["github.copilot.chat.otel.enabled"], Value::Bool(true));
    assert_eq!(
        s["github.copilot.chat.otel.otlpEndpoint"],
        "http://localhost:4318"
    );

    assert!(path.with_extension("json.bak").exists());
}

#[test]
fn idempotent_when_already_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({
            "github.copilot.chat.otel.enabled": true,
            "github.copilot.chat.otel.otlpEndpoint": "http://localhost:4318"
        }))
        .unwrap(),
    )
    .unwrap();

    let changed = CopilotChatProvider.ensure_configured_at(&path).unwrap();
    assert!(!changed);
    assert!(!path.with_extension("json.bak").exists());
}

#[test]
fn updates_when_endpoint_drifted() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({
            "github.copilot.chat.otel.enabled": true,
            "github.copilot.chat.otel.otlpEndpoint": "http://old:9999"
        }))
        .unwrap(),
    )
    .unwrap();

    let changed = CopilotChatProvider.ensure_configured_at(&path).unwrap();
    assert!(changed);

    let s = read_settings(&path);
    assert_eq!(
        s["github.copilot.chat.otel.otlpEndpoint"],
        "http://localhost:4318"
    );
}

#[test]
fn malformed_settings_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    fs::write(&path, "{not valid json}").unwrap();

    let result = CopilotChatProvider.ensure_configured_at(&path);
    assert!(result.is_err());
}
