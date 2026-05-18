use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

const OTLP_ENDPOINT: &str = "http://localhost:4318";

/// File name (under `~/.claude/`) for the StatusLine hook script that
/// captures rate-limit info into the sidecar JSON.
pub const STATUSLINE_SCRIPT_NAME: &str = "agenttop-statusline.sh";

/// Sidecar JSON written by the StatusLine hook; read by the scraper.
pub const RATE_LIMIT_SIDECAR_NAME: &str = "agenttop-rate-limits.json";

/// Bash script body for the StatusLine hook. Reads Claude's session JSON
/// from stdin, extracts whatever rate-limit fields are present, and writes
/// the result to `~/.claude/agenttop-rate-limits.json`. Requires `jq`;
/// degrades to printing only the status line otherwise.
pub const STATUSLINE_SCRIPT_BODY: &str = r#"#!/bin/sh
# agenttop StatusLine hook — installed by `agenttop --setup claude`.
# Captures rate-limit data from Claude Code's status-line stdin into a
# sidecar file consumed by the agenttop TUI.

CLAUDE_DIR="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
SIDECAR="$CLAUDE_DIR/agenttop-rate-limits.json"
TMP="$(mktemp)"
cat > "$TMP"

if command -v jq >/dev/null 2>&1; then
    NOW="$(date +%s)"
    jq -c --argjson now "$NOW" '{
        five_hour_pct: (
            (.rate_limits.five_hour.pct // .rate_limits.fiveHourPct
             // .quotas.five_hour.used_pct // null)
        ),
        seven_day_pct: (
            (.rate_limits.seven_day.pct // .rate_limits.sevenDayPct
             // .quotas.seven_day.used_pct // null)
        ),
        five_hour_resets_at: (
            (.rate_limits.five_hour.resets_at // .rate_limits.fiveHourResetsAt
             // .quotas.five_hour.resets_at // null)
        ),
        seven_day_resets_at: (
            (.rate_limits.seven_day.resets_at // .rate_limits.sevenDayResetsAt
             // .quotas.seven_day.resets_at // null)
        ),
        updated_at: $now
    }' < "$TMP" > "$SIDECAR.tmp" 2>/dev/null && mv "$SIDECAR.tmp" "$SIDECAR"

    MODEL="$(jq -r '.model.display_name // .model // "?"' < "$TMP" 2>/dev/null)"
else
    MODEL="?"
fi

rm -f "$TMP"

# Status line shown by Claude (single line).
printf "agenttop | %s\n" "$MODEL"
"#;

#[allow(dead_code)]
pub fn claude_settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".claude").join("settings.json"))
}

/// Ensures Claude Code OTEL is configured correctly using the env block format.
/// This is the correct way to configure telemetry as of Claude Code 2025+.
/// Note: This function is kept for backwards compatibility. New code should use
/// the provider's ensure_configured() method instead.
#[allow(dead_code)]
pub fn ensure_otel_configured() -> Result<()> {
    let settings_path = claude_settings_path()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    if !settings_path.exists() {
        // Create directory if needed
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Create new settings file with OTEL enabled via env block.
        // OTEL_LOG_TOOL_DETAILS=1 opts in to per-MCP-server tool names (Claude Code 2.1.2+).
        let settings = serde_json::json!({
            "enableTelemetry": true,
            "env": {
                "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
                "OTEL_METRICS_EXPORTER": "otlp",
                "OTEL_LOGS_EXPORTER": "otlp",
                "OTEL_LOG_TOOL_DETAILS": "1",
                "OTEL_EXPORTER_OTLP_PROTOCOL": "http/protobuf",
                "OTEL_EXPORTER_OTLP_ENDPOINT": OTLP_ENDPOINT
            }
        });

        fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
        tracing::info!(
            "Created Claude Code settings with OTEL enabled at {:?}",
            settings_path
        );
        return Ok(());
    }

    // Read existing settings
    let content = fs::read_to_string(&settings_path).context("Failed to read Claude settings")?;

    let mut settings: serde_json::Value =
        serde_json::from_str(&content).context("Failed to parse Claude settings")?;

    let mut modified = false;

    // Check if enableTelemetry is set
    if settings.get("enableTelemetry") != Some(&serde_json::Value::Bool(true)) {
        settings["enableTelemetry"] = serde_json::Value::Bool(true);
        modified = true;
    }

    // Check if env block exists and has correct OTEL settings
    let env_block = settings.get("env");
    let needs_env_update = match env_block {
        None => true,
        Some(env) => {
            env.get("CLAUDE_CODE_ENABLE_TELEMETRY")
                .and_then(|v| v.as_str())
                != Some("1")
                || env.get("OTEL_METRICS_EXPORTER").and_then(|v| v.as_str()) != Some("otlp")
                || env.get("OTEL_LOGS_EXPORTER").and_then(|v| v.as_str()) != Some("otlp")
                || env.get("OTEL_LOG_TOOL_DETAILS").and_then(|v| v.as_str()) != Some("1")
                || env
                    .get("OTEL_EXPORTER_OTLP_ENDPOINT")
                    .and_then(|v| v.as_str())
                    != Some(OTLP_ENDPOINT)
        }
    };

    if needs_env_update {
        // Create or update env block
        if settings.get("env").is_none() {
            settings["env"] = serde_json::json!({});
        }

        let env = settings.get_mut("env").unwrap();
        env["CLAUDE_CODE_ENABLE_TELEMETRY"] = serde_json::Value::String("1".to_string());
        env["OTEL_METRICS_EXPORTER"] = serde_json::Value::String("otlp".to_string());
        env["OTEL_LOGS_EXPORTER"] = serde_json::Value::String("otlp".to_string());
        env["OTEL_LOG_TOOL_DETAILS"] = serde_json::Value::String("1".to_string());
        env["OTEL_EXPORTER_OTLP_PROTOCOL"] = serde_json::Value::String("http/protobuf".to_string());
        env["OTEL_EXPORTER_OTLP_ENDPOINT"] = serde_json::Value::String(OTLP_ENDPOINT.to_string());

        modified = true;
    }

    // Remove old-style telemetry block if present (migrate to env format)
    if settings.get("telemetry").is_some() && settings.as_object_mut().is_some() {
        settings.as_object_mut().unwrap().remove("telemetry");
        modified = true;
        tracing::info!("Migrated from old telemetry format to env block format");
    }

    if modified {
        // Backup existing settings
        let backup_path = settings_path.with_extension("json.bak");
        fs::copy(&settings_path, &backup_path)?;
        tracing::info!("Backed up settings to {:?}", backup_path);

        // Write updated settings
        fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
        tracing::info!("Updated Claude Code settings with OTEL env configuration");
    } else {
        tracing::debug!("Claude Code OTEL already configured correctly");
    }

    Ok(())
}

/// Install the StatusLine hook script and register it in Claude's
/// `settings.json`. Idempotent — running twice is a no-op once the hook is
/// already pointed at our script. Returns `Ok(true)` if any change was made.
pub fn install_statusline_hook() -> Result<bool> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    install_statusline_hook_in(&home.join(".claude"))
}

/// Variant that takes an explicit Claude config dir — used by tests and any
/// future code path that wants to install into `$CLAUDE_CONFIG_DIR`.
pub fn install_statusline_hook_in(claude_dir: &std::path::Path) -> Result<bool> {
    fs::create_dir_all(claude_dir)?;

    let script_path = claude_dir.join(STATUSLINE_SCRIPT_NAME);
    let mut script_changed = false;
    let existing = fs::read_to_string(&script_path).ok();
    if existing.as_deref() != Some(STATUSLINE_SCRIPT_BODY) {
        fs::write(&script_path, STATUSLINE_SCRIPT_BODY)
            .with_context(|| format!("write {}", script_path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms)?;
        }
        script_changed = true;
    }

    let settings_path = claude_dir.join("settings.json");
    let script_str = script_path.to_string_lossy().to_string();

    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path).context("read settings.json")?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let desired_status_line = serde_json::json!({
        "type": "command",
        "command": script_str,
    });

    let needs_settings_update = settings.get("statusLine") != Some(&desired_status_line);

    if needs_settings_update {
        if settings_path.exists() {
            let backup_path = settings_path.with_extension("json.bak");
            // Don't overwrite an existing backup — preserve the user's earliest pre-agenttop state.
            if !backup_path.exists() {
                let _ = fs::copy(&settings_path, &backup_path);
            }
        }
        settings["statusLine"] = desired_status_line;
        fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
        tracing::info!("Registered agenttop StatusLine hook in {:?}", settings_path);
    }

    Ok(script_changed || needs_settings_update)
}

#[allow(dead_code)]
pub fn is_otel_configured() -> bool {
    let Some(settings_path) = claude_settings_path() else {
        return false;
    };

    if !settings_path.exists() {
        return false;
    }

    let Ok(content) = fs::read_to_string(&settings_path) else {
        return false;
    };

    let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };

    // Check if env block has correct OTEL settings
    let env = match settings.get("env") {
        Some(e) => e,
        None => return false,
    };

    let telemetry_enabled = env
        .get("CLAUDE_CODE_ENABLE_TELEMETRY")
        .and_then(|v| v.as_str())
        == Some("1");

    let endpoint_correct = env
        .get("OTEL_EXPORTER_OTLP_ENDPOINT")
        .and_then(|v| v.as_str())
        == Some(OTLP_ENDPOINT);

    telemetry_enabled && endpoint_correct
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_statusline_hook_creates_script_and_patches_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");

        // First invocation: should create everything.
        let changed = install_statusline_hook_in(&claude_dir).unwrap();
        assert!(changed);

        let script_path = claude_dir.join(STATUSLINE_SCRIPT_NAME);
        assert!(script_path.exists());
        let body = fs::read_to_string(&script_path).unwrap();
        assert_eq!(body, STATUSLINE_SCRIPT_BODY);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&script_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "script must be executable");
        }

        let settings_path = claude_dir.join("settings.json");
        assert!(settings_path.exists());
        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        let status_line = settings.get("statusLine").expect("statusLine present");
        assert_eq!(
            status_line.get("command").and_then(|v| v.as_str()),
            Some(script_path.to_string_lossy().as_ref())
        );

        // Second invocation: should be a no-op (returns false).
        let changed_again = install_statusline_hook_in(&claude_dir).unwrap();
        assert!(!changed_again);
    }

    #[test]
    fn install_statusline_hook_preserves_existing_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();

        // Pre-existing user settings.
        let settings_path = claude_dir.join("settings.json");
        let original = serde_json::json!({
            "enableTelemetry": true,
            "env": { "FOO": "bar" }
        });
        fs::write(
            &settings_path,
            serde_json::to_string_pretty(&original).unwrap(),
        )
        .unwrap();

        install_statusline_hook_in(&claude_dir).unwrap();

        // Backup created.
        assert!(settings_path.with_extension("json.bak").exists());

        // Original keys preserved.
        let after: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(after.get("enableTelemetry"), Some(&serde_json::json!(true)));
        assert_eq!(
            after.get("env").and_then(|e| e.get("FOO")),
            Some(&serde_json::json!("bar"))
        );
        assert!(after.get("statusLine").is_some());
    }
}
