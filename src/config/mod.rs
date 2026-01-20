use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

const OTLP_ENDPOINT: &str = "http://localhost:4318";

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

        // Create new settings file with OTEL enabled via env block
        let settings = serde_json::json!({
            "enableTelemetry": true,
            "env": {
                "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
                "OTEL_METRICS_EXPORTER": "otlp",
                "OTEL_LOGS_EXPORTER": "otlp",
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
