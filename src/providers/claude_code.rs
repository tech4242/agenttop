//! Claude Code provider implementation

use super::{Provider, TOKEN_CACHE_READ, TOKEN_CACHE_WRITE, TOKEN_INPUT, TOKEN_OUTPUT};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

const OTLP_ENDPOINT: &str = "http://localhost:4318";

/// Built-in Claude Code tools
const BUILTIN_TOOLS: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash",
    "Glob",
    "Grep",
    "Task",
    "TodoRead",
    "TodoWrite",
    "WebFetch",
    "WebSearch",
    "Agent",
    "Skill",
    "AskUser",
    "AskUserQuestion",
    "MultiEdit",
    "NotebookEdit",
    "KillShell",
    "EnterPlanMode",
    "ExitPlanMode",
    "TaskOutput",
];

/// Claude Code provider
pub struct ClaudeCodeProvider;

impl Provider for ClaudeCodeProvider {
    fn id(&self) -> &'static str {
        "claude_code"
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn metric_prefix(&self) -> &'static str {
        "claude_code"
    }

    fn builtin_tools(&self) -> &'static [&'static str] {
        BUILTIN_TOOLS
    }

    fn shorten_model_name(&self, name: &str) -> Option<String> {
        let n = name.to_lowercase();

        // Claude Opus models
        if n.contains("opus") {
            if n.contains("4.5") || n.contains("4-5") {
                return Some("opus-4.5".to_string());
            }
            if let Some(ver) = extract_version(&n) {
                return Some(format!("opus-{}", ver));
            }
            return Some("opus".to_string());
        }

        // Claude Sonnet models
        if n.contains("sonnet") {
            if let Some(ver) = extract_version(&n) {
                return Some(format!("sonnet-{}", ver));
            }
            return Some("sonnet".to_string());
        }

        // Claude Haiku models
        if n.contains("haiku") {
            if let Some(ver) = extract_version(&n) {
                return Some(format!("haiku-{}", ver));
            }
            return Some("haiku".to_string());
        }

        None // Not a Claude model
    }

    fn normalize_token_type(&self, token_type: &str) -> Option<&'static str> {
        match token_type {
            "input" | "input_tokens" => Some(TOKEN_INPUT),
            "output" | "output_tokens" => Some(TOKEN_OUTPUT),
            "cacheRead" | "cache_read" | "cache_hit" => Some(TOKEN_CACHE_READ),
            "cacheCreation" | "cache_creation" | "cache_write" => Some(TOKEN_CACHE_WRITE),
            _ => None,
        }
    }

    fn settings_path(&self) -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".claude").join("settings.json"))
    }

    fn ensure_configured(&self) -> Result<bool> {
        let settings_path = self
            .settings_path()
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
            return Ok(true);
        }

        // Read existing settings
        let content =
            fs::read_to_string(&settings_path).context("Failed to read Claude settings")?;

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
            env["OTEL_EXPORTER_OTLP_PROTOCOL"] =
                serde_json::Value::String("http/protobuf".to_string());
            env["OTEL_EXPORTER_OTLP_ENDPOINT"] =
                serde_json::Value::String(OTLP_ENDPOINT.to_string());

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
            return Ok(true);
        }

        tracing::debug!("Claude Code OTEL already configured correctly");
        Ok(false)
    }
}

/// Extract version number from model name (e.g., "4" from "claude-sonnet-4-20250514")
fn extract_version(name: &str) -> Option<&str> {
    // Look for patterns like "-4-" or "-3-" or "-4.5-" or "-3.5-" or "-3-5-"
    for pattern in ["-4.5-", "-4-5-", "-4-", "-3.5-", "-3-5-", "-3-", "-5-"] {
        if name.contains(pattern) {
            // Return normalized version
            return Some(match pattern {
                "-4.5-" | "-4-5-" => "4.5",
                "-4-" => "4",
                "-3.5-" | "-3-5-" => "3.5",
                "-3-" => "3",
                "-5-" => "5",
                _ => pattern.trim_matches('-'),
            });
        }
    }
    // Check for version at end like "-4" or "-3"
    if name.ends_with("-4") || name.ends_with("-5") || name.ends_with("-3") {
        return name.rsplit('-').next();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shorten_model_name() {
        let provider = ClaudeCodeProvider;

        // Opus models
        assert_eq!(
            provider.shorten_model_name("claude-opus-4-5-20250514"),
            Some("opus-4.5".to_string())
        );
        assert_eq!(
            provider.shorten_model_name("claude-opus-4-20250514"),
            Some("opus-4".to_string())
        );

        // Sonnet models
        assert_eq!(
            provider.shorten_model_name("claude-sonnet-4-20250514"),
            Some("sonnet-4".to_string())
        );
        assert_eq!(
            provider.shorten_model_name("claude-3-5-sonnet-20241022"),
            Some("sonnet-3.5".to_string())
        );

        // Haiku models
        assert_eq!(
            provider.shorten_model_name("claude-3-5-haiku-20241022"),
            Some("haiku-3.5".to_string())
        );
        assert_eq!(
            provider.shorten_model_name("claude-haiku-3-20240307"),
            Some("haiku-3".to_string())
        );

        // Not a Claude model
        assert_eq!(provider.shorten_model_name("gpt-4o"), None);
    }

    #[test]
    fn test_normalize_token_type() {
        let provider = ClaudeCodeProvider;

        assert_eq!(provider.normalize_token_type("input"), Some(TOKEN_INPUT));
        assert_eq!(provider.normalize_token_type("output"), Some(TOKEN_OUTPUT));
        assert_eq!(
            provider.normalize_token_type("cacheRead"),
            Some(TOKEN_CACHE_READ)
        );
        assert_eq!(
            provider.normalize_token_type("cacheCreation"),
            Some(TOKEN_CACHE_WRITE)
        );
        assert_eq!(provider.normalize_token_type("unknown"), None);
    }

    #[test]
    fn test_builtin_tools() {
        let provider = ClaudeCodeProvider;
        let tools = provider.builtin_tools();

        assert!(tools.contains(&"Read"));
        assert!(tools.contains(&"Bash"));
        assert!(tools.contains(&"Edit"));
        assert!(tools.contains(&"TodoWrite"));
        assert!(!tools.contains(&"shell"));
    }
}
