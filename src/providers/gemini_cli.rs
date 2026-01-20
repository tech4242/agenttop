//! Gemini CLI provider implementation

use super::{Provider, TOKEN_INPUT, TOKEN_OUTPUT};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

const OTLP_ENDPOINT: &str = "http://localhost:4318";

/// Built-in Gemini CLI tools
/// Note: Gemini CLI tool names may vary; these are the known ones
const BUILTIN_TOOLS: &[&str] = &[
    "read_file",
    "write_file",
    "edit_file",
    "run_shell_command",
    "search_files",
    "list_directory",
    "find_files",
    "glob_files",
    "web_search",
    "memory_tool",
];

/// Gemini CLI provider
pub struct GeminiCliProvider;

impl Provider for GeminiCliProvider {
    fn id(&self) -> &'static str {
        "gemini_cli"
    }

    fn name(&self) -> &'static str {
        "Gemini CLI"
    }

    fn metric_prefix(&self) -> &'static str {
        "gemini_cli"
    }

    fn builtin_tools(&self) -> &'static [&'static str] {
        BUILTIN_TOOLS
    }

    fn shorten_model_name(&self, name: &str) -> Option<String> {
        let n = name.to_lowercase();

        // Gemini 2.x models
        if n.contains("gemini-2.0") || n.contains("gemini-2-") {
            if n.contains("flash") {
                return Some("gemini-2.0-flash".to_string());
            }
            if n.contains("pro") {
                return Some("gemini-2.0-pro".to_string());
            }
            return Some("gemini-2.0".to_string());
        }

        // Check for just "gemini-2" without version specifier
        if n.contains("gemini-2") {
            return Some("gemini-2".to_string());
        }

        // Gemini 1.5 models
        if n.contains("gemini-1.5-pro") || n.contains("gemini-1-5-pro") {
            return Some("gemini-1.5-pro".to_string());
        }
        if n.contains("gemini-1.5-flash") || n.contains("gemini-1-5-flash") {
            return Some("gemini-1.5-flash".to_string());
        }
        if n.contains("gemini-1.5") {
            return Some("gemini-1.5".to_string());
        }

        // Gemini 1.0 models
        if n.contains("gemini-1.0") || n.contains("gemini-pro") {
            return Some("gemini-1.0".to_string());
        }

        // Generic gemini match
        if n.contains("gemini") {
            return Some("gemini".to_string());
        }

        None // Not a Gemini model
    }

    fn normalize_token_type(&self, token_type: &str) -> Option<&'static str> {
        // Gemini uses gen_ai semantic conventions
        match token_type {
            "input" | "prompt" | "input_tokens" | "prompt_tokens" => Some(TOKEN_INPUT),
            "output" | "completion" | "output_tokens" | "completion_tokens" => Some(TOKEN_OUTPUT),
            _ => None,
        }
    }

    fn settings_path(&self) -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".gemini").join("settings.json"))
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
                "Created Gemini CLI settings with OTEL enabled at {:?}",
                settings_path
            );
            return Ok(true);
        }

        // Read existing settings
        let content =
            fs::read_to_string(&settings_path).context("Failed to read Gemini settings")?;

        let mut settings: serde_json::Value =
            serde_json::from_str(&content).context("Failed to parse Gemini settings")?;

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
            tracing::info!("Updated Gemini CLI settings with OTEL configuration");
            return Ok(true);
        }

        tracing::debug!("Gemini CLI OTEL already configured correctly");
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shorten_model_name() {
        let provider = GeminiCliProvider;

        // Gemini 2 models
        assert_eq!(
            provider.shorten_model_name("gemini-2.0-flash-exp"),
            Some("gemini-2.0-flash".to_string())
        );
        assert_eq!(
            provider.shorten_model_name("gemini-2.0-pro"),
            Some("gemini-2.0-pro".to_string())
        );
        assert_eq!(
            provider.shorten_model_name("gemini-2"),
            Some("gemini-2".to_string())
        );

        // Gemini 1.5 models
        assert_eq!(
            provider.shorten_model_name("gemini-1.5-pro-latest"),
            Some("gemini-1.5-pro".to_string())
        );
        assert_eq!(
            provider.shorten_model_name("gemini-1.5-flash-001"),
            Some("gemini-1.5-flash".to_string())
        );

        // Gemini 1.0 models
        assert_eq!(
            provider.shorten_model_name("gemini-pro"),
            Some("gemini-1.0".to_string())
        );

        // Not a Gemini model
        assert_eq!(provider.shorten_model_name("gpt-4o"), None);
        assert_eq!(provider.shorten_model_name("claude-opus-4"), None);
    }

    #[test]
    fn test_normalize_token_type() {
        let provider = GeminiCliProvider;

        assert_eq!(provider.normalize_token_type("input"), Some(TOKEN_INPUT));
        assert_eq!(provider.normalize_token_type("prompt"), Some(TOKEN_INPUT));
        assert_eq!(provider.normalize_token_type("output"), Some(TOKEN_OUTPUT));
        assert_eq!(
            provider.normalize_token_type("completion"),
            Some(TOKEN_OUTPUT)
        );
        assert_eq!(provider.normalize_token_type("cacheRead"), None);
    }

    #[test]
    fn test_builtin_tools() {
        let provider = GeminiCliProvider;
        let tools = provider.builtin_tools();

        assert!(tools.contains(&"read_file"));
        assert!(tools.contains(&"write_file"));
        assert!(tools.contains(&"run_shell_command"));
        assert!(!tools.contains(&"Read"));
        assert!(!tools.contains(&"shell"));
    }
}
