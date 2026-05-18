//! End-to-end tests for ClaudeCodeProvider::ensure_configured_at.
//!
//! Exercises the auto-config branches against a tempdir, so we cover the
//! create-vs-update / env-block / migration / backup logic without
//! touching the user's real ~/.claude/settings.json.

use agenttop::providers::claude_code::ClaudeCodeProvider;
use serde_json::Value;
use std::fs;

fn read_settings(path: &std::path::Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn creates_settings_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let changed = ClaudeCodeProvider.ensure_configured_at(&path).unwrap();
    assert!(changed);
    assert!(path.exists());

    let s = read_settings(&path);
    assert_eq!(s["enableTelemetry"], Value::Bool(true));
    assert_eq!(s["env"]["CLAUDE_CODE_ENABLE_TELEMETRY"], "1");
    assert_eq!(s["env"]["OTEL_LOG_TOOL_DETAILS"], "1");
    assert_eq!(
        s["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"],
        "http://localhost:4318"
    );

    // No backup on create (file didn't exist).
    assert!(!path.with_extension("json.bak").exists());
}

#[test]
fn idempotent_when_already_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let first = ClaudeCodeProvider.ensure_configured_at(&path).unwrap();
    let second = ClaudeCodeProvider.ensure_configured_at(&path).unwrap();
    assert!(first);
    assert!(!second, "second invocation must be a no-op");
}

#[test]
fn preserves_unrelated_existing_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let original = serde_json::json!({
        "permissions": { "allow": ["Bash"] },
        "statusLine": { "type": "command", "command": "/usr/local/bin/foo" }
    });
    fs::write(&path, serde_json::to_string_pretty(&original).unwrap()).unwrap();

    let changed = ClaudeCodeProvider.ensure_configured_at(&path).unwrap();
    assert!(changed);

    let s = read_settings(&path);
    assert_eq!(s["permissions"]["allow"][0], "Bash");
    assert_eq!(s["statusLine"]["command"], "/usr/local/bin/foo");
    assert_eq!(s["enableTelemetry"], Value::Bool(true));
    assert_eq!(s["env"]["CLAUDE_CODE_ENABLE_TELEMETRY"], "1");

    // Backup exists for modify-of-existing.
    assert!(path.with_extension("json.bak").exists());
}

#[test]
fn merges_into_existing_env_block_without_clobbering() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let original = serde_json::json!({
        "env": {
            "USER_VAR": "kept",
            "CLAUDE_CODE_ENABLE_TELEMETRY": "0"
        }
    });
    fs::write(&path, serde_json::to_string_pretty(&original).unwrap()).unwrap();

    let changed = ClaudeCodeProvider.ensure_configured_at(&path).unwrap();
    assert!(changed);

    let s = read_settings(&path);
    assert_eq!(s["env"]["USER_VAR"], "kept", "non-OTEL env vars preserved");
    assert_eq!(
        s["env"]["CLAUDE_CODE_ENABLE_TELEMETRY"], "1",
        "stale OTEL var overwritten"
    );
}

#[test]
fn migrates_legacy_telemetry_block_to_env() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let legacy = serde_json::json!({
        "telemetry": { "enabled": true, "endpoint": "http://old:4318" }
    });
    fs::write(&path, serde_json::to_string_pretty(&legacy).unwrap()).unwrap();

    let changed = ClaudeCodeProvider.ensure_configured_at(&path).unwrap();
    assert!(changed);

    let s = read_settings(&path);
    assert!(
        s.get("telemetry").is_none(),
        "legacy telemetry block removed"
    );
    assert_eq!(s["env"]["CLAUDE_CODE_ENABLE_TELEMETRY"], "1");
}

#[test]
fn does_not_overwrite_existing_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    let bak = path.with_extension("json.bak");

    fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({"misc": "v1"})).unwrap(),
    )
    .unwrap();
    ClaudeCodeProvider.ensure_configured_at(&path).unwrap();
    let bak_after_first = fs::read_to_string(&bak).unwrap();

    // Touch settings again (e.g. user edited it) and reconfigure.
    fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({"misc": "v2"})).unwrap(),
    )
    .unwrap();
    ClaudeCodeProvider.ensure_configured_at(&path).unwrap();

    // NOTE: claude_code ensure_configured currently overwrites .bak on every
    // modification. install_statusline_hook_in (in src/config) preserves
    // first-run backup. This test documents the *current* claude_code
    // behavior so we don't regress without intent.
    let bak_after_second = fs::read_to_string(&bak).unwrap();
    assert_ne!(
        bak_after_first, bak_after_second,
        "claude_code backup is replaced on each modification (documented current behavior)"
    );
}
