//! Qwen Code provider implementation

use super::{Provider, TOKEN_CACHE_READ, TOKEN_INPUT, TOKEN_OUTPUT};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

const OTLP_ENDPOINT: &str = "http://localhost:4318";

/// Built-in Qwen Code tools
/// Note: Qwen Code tool names may vary; these are estimated based on similar tools
const BUILTIN_TOOLS: &[&str] = &[
    "read_file",
    "write_file",
    "edit_file",
    "run_command",
    "search",
    "list_files",
    "create_file",
    "delete_file",
];

/// Qwen Code provider
pub struct QwenCodeProvider;

impl Provider for QwenCodeProvider {
    fn id(&self) -> &'static str {
        "qwen_code"
    }

    fn name(&self) -> &'static str {
        "Qwen Code"
    }

    fn metric_prefix(&self) -> &'static str {
        "qwen-code"
    }

    fn builtin_tools(&self) -> &'static [&'static str] {
        BUILTIN_TOOLS
    }

    fn shorten_model_name(&self, name: &str) -> Option<String> {
        let n = name.to_lowercase();

        if !n.contains("qwen") {
            return None;
        }

        // Qwen 2.5 Coder models
        if n.contains("2.5") || n.contains("2-5") {
            if n.contains("coder") {
                return Some("qwen-2.5-coder".to_string());
            }
            return Some("qwen-2.5".to_string());
        }

        // Qwen 2 models
        if n.contains("qwen2") || n.contains("qwen-2") {
            if n.contains("coder") {
                return Some("qwen-2-coder".to_string());
            }
            return Some("qwen-2".to_string());
        }

        // Qwen 1.5 models
        if n.contains("1.5") || n.contains("1-5") {
            return Some("qwen-1.5".to_string());
        }

        // Generic qwen
        Some("qwen".to_string())
    }

    fn normalize_token_type(&self, token_type: &str) -> Option<&'static str> {
        // Qwen has 5 token types: input, output, thought, cache, tool
        match token_type {
            "input" | "input_tokens" => Some(TOKEN_INPUT),
            "output" | "output_tokens" => Some(TOKEN_OUTPUT),
            "cache" | "cache_tokens" => Some(TOKEN_CACHE_READ),
            // "thought" and "tool" tokens are tracked separately if needed
            "thought" | "tool" => None,
            _ => None,
        }
    }

    fn settings_path(&self) -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".qwen").join("settings.json"))
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

            // Create new settings file with OTEL enabled
            let settings = serde_json::json!({
                "telemetry": {
                    "enabled": true,
                    "target": "local",
                    "otlpEndpoint": OTLP_ENDPOINT,
                    "otlpProtocol": "http"
                }
            });

            fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
            tracing::info!(
                "Created Qwen Code settings with OTEL enabled at {:?}",
                settings_path
            );
            return Ok(true);
        }

        // Read existing settings
        let content =
            fs::read_to_string(&settings_path).context("Failed to read Qwen Code settings")?;

        let mut settings: serde_json::Value =
            serde_json::from_str(&content).context("Failed to parse Qwen Code settings")?;

        let mut modified = false;

        // Check if telemetry block exists and has correct settings
        let telemetry = settings.get("telemetry");
        let needs_update = match telemetry {
            None => true,
            Some(t) => {
                t.get("enabled") != Some(&serde_json::Value::Bool(true))
                    || t.get("target").and_then(|v| v.as_str()) != Some("local")
                    || t.get("otlpEndpoint").and_then(|v| v.as_str()) != Some(OTLP_ENDPOINT)
            }
        };

        if needs_update {
            settings["telemetry"] = serde_json::json!({
                "enabled": true,
                "target": "local",
                "otlpEndpoint": OTLP_ENDPOINT,
                "otlpProtocol": "http"
            });
            modified = true;
        }

        if modified {
            // Backup existing settings
            let backup_path = settings_path.with_extension("json.bak");
            fs::copy(&settings_path, &backup_path)?;
            tracing::info!("Backed up settings to {:?}", backup_path);

            // Write updated settings
            fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
            tracing::info!("Updated Qwen Code settings with OTEL configuration");
            return Ok(true);
        }

        tracing::debug!("Qwen Code OTEL already configured correctly");
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shorten_model_name() {
        let provider = QwenCodeProvider;

        // Qwen 2.5 models
        assert_eq!(
            provider.shorten_model_name("qwen2.5-coder-32b-instruct"),
            Some("qwen-2.5-coder".to_string())
        );
        assert_eq!(
            provider.shorten_model_name("qwen-2.5-72b"),
            Some("qwen-2.5".to_string())
        );

        // Qwen 2 models
        assert_eq!(
            provider.shorten_model_name("qwen2-72b-instruct"),
            Some("qwen-2".to_string())
        );

        // Generic qwen
        assert_eq!(
            provider.shorten_model_name("qwen-coder"),
            Some("qwen".to_string())
        );

        // Not a Qwen model
        assert_eq!(provider.shorten_model_name("gpt-4o"), None);
        assert_eq!(provider.shorten_model_name("claude-opus-4"), None);
    }

    #[test]
    fn test_normalize_token_type() {
        let provider = QwenCodeProvider;

        assert_eq!(provider.normalize_token_type("input"), Some(TOKEN_INPUT));
        assert_eq!(provider.normalize_token_type("output"), Some(TOKEN_OUTPUT));
        assert_eq!(
            provider.normalize_token_type("cache"),
            Some(TOKEN_CACHE_READ)
        );
        // Thought and tool are not normalized (tracked separately if needed)
        assert_eq!(provider.normalize_token_type("thought"), None);
        assert_eq!(provider.normalize_token_type("tool"), None);
    }

    #[test]
    fn test_builtin_tools() {
        let provider = QwenCodeProvider;
        let tools = provider.builtin_tools();

        assert!(tools.contains(&"read_file"));
        assert!(tools.contains(&"write_file"));
        assert!(tools.contains(&"run_command"));
        assert!(!tools.contains(&"Read"));
        assert!(!tools.contains(&"shell"));
    }
}
