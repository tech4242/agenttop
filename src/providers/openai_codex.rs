//! OpenAI Codex CLI provider implementation

use super::{Provider, TOKEN_INPUT, TOKEN_OUTPUT};
use std::path::PathBuf;

/// Built-in OpenAI Codex CLI tools
const BUILTIN_TOOLS: &[&str] = &[
    "shell",
    "read_file",
    "write_file",
    "edit_file",
    "search",
    "list_files",
    "run_command",
    "apply_patch",
];

/// OpenAI Codex CLI provider
///
/// Note: Codex uses TOML config format (~/.codex/config.toml), so auto-configuration
/// is not implemented to avoid adding a TOML dependency. Manual setup is documented
/// in README.
pub struct OpenAICodexProvider;

impl Provider for OpenAICodexProvider {
    fn id(&self) -> &'static str {
        "openai_codex"
    }

    fn name(&self) -> &'static str {
        "OpenAI Codex"
    }

    fn metric_prefix(&self) -> &'static str {
        "codex"
    }

    fn builtin_tools(&self) -> &'static [&'static str] {
        BUILTIN_TOOLS
    }

    fn shorten_model_name(&self, name: &str) -> Option<String> {
        let n = name.to_lowercase();

        // GPT-4o models
        if n.contains("gpt-4o") {
            return Some("gpt-4o".to_string());
        }

        // GPT-4 turbo models
        if n.contains("gpt-4-turbo") {
            return Some("gpt-4-turbo".to_string());
        }

        // GPT-4 models
        if n.contains("gpt-4") {
            return Some("gpt-4".to_string());
        }

        // GPT-3.5 models
        if n.contains("gpt-3.5") || n.contains("gpt-3") {
            return Some("gpt-3.5".to_string());
        }

        // o1 models
        if n.contains("o1-preview") {
            return Some("o1-preview".to_string());
        }
        if n.contains("o1-mini") {
            return Some("o1-mini".to_string());
        }
        if n.contains("o1") {
            return Some("o1".to_string());
        }

        // o3 models
        if n.contains("o3-mini") {
            return Some("o3-mini".to_string());
        }
        if n.contains("o3") {
            return Some("o3".to_string());
        }

        None // Not an OpenAI model
    }

    fn normalize_token_type(&self, token_type: &str) -> Option<&'static str> {
        match token_type {
            "prompt_tokens" => Some(TOKEN_INPUT),
            "completion_tokens" => Some(TOKEN_OUTPUT),
            _ => None,
        }
    }

    fn settings_path(&self) -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".codex").join("config.toml"))
    }

    // No ensure_configured() - TOML format requires manual setup (documented in README)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shorten_model_name() {
        let provider = OpenAICodexProvider;

        // GPT-4o models
        assert_eq!(
            provider.shorten_model_name("gpt-4o-2024-05-13"),
            Some("gpt-4o".to_string())
        );
        assert_eq!(
            provider.shorten_model_name("gpt-4o-mini"),
            Some("gpt-4o".to_string())
        );

        // GPT-4 turbo models
        assert_eq!(
            provider.shorten_model_name("gpt-4-turbo-preview"),
            Some("gpt-4-turbo".to_string())
        );

        // GPT-4 models
        assert_eq!(
            provider.shorten_model_name("gpt-4-0613"),
            Some("gpt-4".to_string())
        );

        // GPT-3.5 models
        assert_eq!(
            provider.shorten_model_name("gpt-3.5-turbo"),
            Some("gpt-3.5".to_string())
        );

        // o1 models
        assert_eq!(
            provider.shorten_model_name("o1-preview"),
            Some("o1-preview".to_string())
        );
        assert_eq!(
            provider.shorten_model_name("o1-mini"),
            Some("o1-mini".to_string())
        );

        // o3 models
        assert_eq!(
            provider.shorten_model_name("o3-mini"),
            Some("o3-mini".to_string())
        );

        // Not an OpenAI model
        assert_eq!(provider.shorten_model_name("claude-opus-4"), None);
    }

    #[test]
    fn test_normalize_token_type() {
        let provider = OpenAICodexProvider;

        assert_eq!(
            provider.normalize_token_type("prompt_tokens"),
            Some(TOKEN_INPUT)
        );
        assert_eq!(
            provider.normalize_token_type("completion_tokens"),
            Some(TOKEN_OUTPUT)
        );
        assert_eq!(provider.normalize_token_type("input"), None);
        assert_eq!(provider.normalize_token_type("cacheRead"), None);
    }

    #[test]
    fn test_builtin_tools() {
        let provider = OpenAICodexProvider;
        let tools = provider.builtin_tools();

        assert!(tools.contains(&"shell"));
        assert!(tools.contains(&"read_file"));
        assert!(tools.contains(&"write_file"));
        assert!(tools.contains(&"apply_patch"));
        assert!(!tools.contains(&"Read"));
    }
}
