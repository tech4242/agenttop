//! GitHub Copilot Chat provider (VSCode; native OTLP since Feb 2026)

use super::{Provider, TOKEN_INPUT, TOKEN_OUTPUT};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

const OTLP_ENDPOINT: &str = "http://localhost:4318";

pub struct CopilotChatProvider;

impl Provider for CopilotChatProvider {
    fn id(&self) -> &'static str {
        "copilot_chat"
    }

    fn name(&self) -> &'static str {
        "GitHub Copilot Chat"
    }

    fn metric_prefix(&self) -> &'static str {
        // Copilot Chat emits under OTel GenAI Semantic Conventions.
        "gen_ai"
    }

    fn builtin_tools(&self) -> &'static [&'static str] {
        // Tool names not yet documented by Microsoft; rely on service.name detection.
        &[]
    }

    fn shorten_model_name(&self, _name: &str) -> Option<String> {
        None
    }

    fn normalize_token_type(&self, token_type: &str) -> Option<&'static str> {
        // OTel GenAI Semantic Conventions use these names on `gen_ai.client.token.usage`.
        match token_type {
            "input" | "prompt" => Some(TOKEN_INPUT),
            "output" | "completion" => Some(TOKEN_OUTPUT),
            _ => None,
        }
    }

    fn service_name(&self) -> Option<&'static str> {
        Some("copilot-chat")
    }

    fn settings_path(&self) -> Option<PathBuf> {
        vscode_user_settings_path()
    }

    fn ensure_configured(&self) -> Result<bool> {
        let Some(settings_path) = self.settings_path() else {
            return Ok(false);
        };

        if !settings_path.exists() {
            tracing::warn!(
                "VSCode settings.json not found at {:?}; install VSCode and run Copilot Chat at least once before configuring",
                settings_path
            );
            return Ok(false);
        }

        let content =
            fs::read_to_string(&settings_path).context("Failed to read VSCode settings.json")?;
        let mut settings: serde_json::Value = serde_json::from_str(&content)
            .context("Failed to parse VSCode settings.json (may contain trailing commas; edit manually if so)")?;

        let needs_update = settings.get("github.copilot.chat.otel.enabled")
            != Some(&serde_json::Value::Bool(true))
            || settings
                .get("github.copilot.chat.otel.otlpEndpoint")
                .and_then(|v| v.as_str())
                != Some(OTLP_ENDPOINT);

        if !needs_update {
            tracing::debug!("Copilot Chat OTLP already configured");
            return Ok(false);
        }

        let backup_path = settings_path.with_extension("json.bak");
        fs::copy(&settings_path, &backup_path)?;
        tracing::info!("Backed up VSCode settings to {:?}", backup_path);

        if let Some(obj) = settings.as_object_mut() {
            obj.insert(
                "github.copilot.chat.otel.enabled".to_string(),
                serde_json::Value::Bool(true),
            );
            obj.insert(
                "github.copilot.chat.otel.otlpEndpoint".to_string(),
                serde_json::Value::String(OTLP_ENDPOINT.to_string()),
            );
        }

        fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
        tracing::info!(
            "Updated VSCode settings.json with Copilot Chat OTLP at {:?}",
            settings_path
        );
        Ok(true)
    }

    fn setup_instructions(&self) -> Option<&'static str> {
        Some(
            "Copilot Chat reads OTLP config from VSCode settings.json. agenttop\n\
             auto-writes the keys with `agenttop --setup copilot`; reload VSCode\n\
             after running it. If captured prompt/response content is desired, also\n\
             set \"github.copilot.chat.otel.captureContent\": true (opt-in).",
        )
    }
}

fn vscode_user_settings_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;

    if cfg!(target_os = "macos") {
        Some(
            home.join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("settings.json"),
        )
    } else if cfg!(target_os = "windows") {
        dirs::config_dir().map(|c| c.join("Code").join("User").join("settings.json"))
    } else {
        Some(
            home.join(".config")
                .join("Code")
                .join("User")
                .join("settings.json"),
        )
    }
}
