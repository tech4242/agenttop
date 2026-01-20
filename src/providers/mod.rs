//! Provider plugin architecture for agenttop
//!
//! This module provides a trait-based abstraction for supporting multiple AI coding agents.
//! Each provider defines its own:
//! - Metric/event prefix for auto-detection
//! - Built-in tools list
//! - Model name shortening logic
//! - Token type normalization

pub mod claude_code;
pub mod gemini_cli;
pub mod openai_codex;
pub mod qwen_code;

use anyhow::Result;
use once_cell::sync::Lazy;

/// Normalized token type names used internally
pub const TOKEN_INPUT: &str = "input";
pub const TOKEN_OUTPUT: &str = "output";
pub const TOKEN_CACHE_READ: &str = "cache_read";
pub const TOKEN_CACHE_WRITE: &str = "cache_write";

/// Trait for AI coding agent providers
pub trait Provider: Send + Sync {
    /// Unique ID (e.g., "claude_code")
    fn id(&self) -> &'static str;

    /// Display name (e.g., "Claude Code")
    fn name(&self) -> &'static str;

    /// OTLP metric/event prefix for auto-detection (e.g., "claude_code")
    fn metric_prefix(&self) -> &'static str;

    /// Built-in tools specific to this agent
    fn builtin_tools(&self) -> &'static [&'static str];

    /// Shorten model name for display. Returns None if not this provider's model.
    fn shorten_model_name(&self, name: &str) -> Option<String>;

    /// Normalize token type to internal format. Returns None if unknown.
    fn normalize_token_type(&self, token_type: &str) -> Option<&'static str>;

    /// Configure this provider's OTLP settings. Returns Ok(true) if configured.
    fn ensure_configured(&self) -> Result<bool> {
        Ok(false) // Default: no auto-config
    }

    /// Get the settings file path for this provider (if applicable)
    fn settings_path(&self) -> Option<std::path::PathBuf> {
        None
    }
}

/// Registry of all known providers
pub struct ProviderRegistry {
    providers: Vec<Box<dyn Provider>>,
}

impl ProviderRegistry {
    /// Create a new registry with all known providers
    pub fn new() -> Self {
        Self {
            providers: vec![
                Box::new(claude_code::ClaudeCodeProvider),
                Box::new(openai_codex::OpenAICodexProvider),
                Box::new(gemini_cli::GeminiCliProvider),
                Box::new(qwen_code::QwenCodeProvider),
            ],
        }
    }

    /// Get all registered providers
    pub fn providers(&self) -> &[Box<dyn Provider>] {
        &self.providers
    }

    /// Get a provider by ID
    pub fn get(&self, id: &str) -> Option<&dyn Provider> {
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .map(|p| p.as_ref())
    }

    /// Detect provider from metric/event name prefix
    pub fn detect_from_metric(&self, metric_name: &str) -> Option<&dyn Provider> {
        self.providers
            .iter()
            .find(|p| metric_name.starts_with(p.metric_prefix()))
            .map(|p| p.as_ref())
    }

    /// Try all providers to normalize a token type
    pub fn normalize_token_type(&self, token_type: &str) -> Option<&'static str> {
        for provider in &self.providers {
            if let Some(normalized) = provider.normalize_token_type(token_type) {
                return Some(normalized);
            }
        }
        None
    }

    /// Try all providers to shorten a model name
    pub fn shorten_model_name(&self, model_name: &str) -> String {
        for provider in &self.providers {
            if let Some(short) = provider.shorten_model_name(model_name) {
                return short;
            }
        }
        // Fallback: truncate to 12 chars
        if model_name.len() > 12 {
            format!("{}...", &model_name[..12])
        } else {
            model_name.to_string()
        }
    }

    /// Check if tool is builtin for any provider
    pub fn is_any_builtin_tool(&self, tool_name: &str) -> bool {
        self.providers
            .iter()
            .any(|p| p.builtin_tools().contains(&tool_name))
    }

    /// Get provider for a given builtin tool
    pub fn provider_for_tool(&self, tool_name: &str) -> Option<&dyn Provider> {
        self.providers
            .iter()
            .find(|p| p.builtin_tools().contains(&tool_name))
            .map(|p| p.as_ref())
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global provider registry instance
pub static PROVIDER_REGISTRY: Lazy<ProviderRegistry> = Lazy::new(ProviderRegistry::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_providers() {
        let registry = ProviderRegistry::new();
        assert!(!registry.providers().is_empty());
    }

    #[test]
    fn test_get_provider_by_id() {
        let registry = ProviderRegistry::new();
        assert!(registry.get("claude_code").is_some());
        assert!(registry.get("openai_codex").is_some());
        assert!(registry.get("gemini_cli").is_some());
        assert!(registry.get("qwen_code").is_some());
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn test_detect_from_metric() {
        let registry = ProviderRegistry::new();

        let claude = registry.detect_from_metric("claude_code.token.usage");
        assert!(claude.is_some());
        assert_eq!(claude.unwrap().id(), "claude_code");

        let codex = registry.detect_from_metric("codex.tool_result");
        assert!(codex.is_some());
        assert_eq!(codex.unwrap().id(), "openai_codex");

        let gemini = registry.detect_from_metric("gemini_cli.api_request");
        assert!(gemini.is_some());
        assert_eq!(gemini.unwrap().id(), "gemini_cli");

        let qwen = registry.detect_from_metric("qwen-code.tool_call");
        assert!(qwen.is_some());
        assert_eq!(qwen.unwrap().id(), "qwen_code");
    }

    #[test]
    fn test_is_any_builtin_tool() {
        let registry = ProviderRegistry::new();

        // Claude Code tools
        assert!(registry.is_any_builtin_tool("Read"));
        assert!(registry.is_any_builtin_tool("Bash"));
        assert!(registry.is_any_builtin_tool("Edit"));

        // OpenAI Codex tools
        assert!(registry.is_any_builtin_tool("shell"));
        assert!(registry.is_any_builtin_tool("read_file"));

        // Not a builtin tool
        assert!(!registry.is_any_builtin_tool("mcp__context7__query-docs"));
    }

    #[test]
    fn test_normalize_token_type() {
        let registry = ProviderRegistry::new();

        // Claude Code token types
        assert_eq!(registry.normalize_token_type("input"), Some(TOKEN_INPUT));
        assert_eq!(registry.normalize_token_type("output"), Some(TOKEN_OUTPUT));
        assert_eq!(
            registry.normalize_token_type("cacheRead"),
            Some(TOKEN_CACHE_READ)
        );
        assert_eq!(
            registry.normalize_token_type("cacheCreation"),
            Some(TOKEN_CACHE_WRITE)
        );

        // OpenAI token types
        assert_eq!(
            registry.normalize_token_type("prompt_tokens"),
            Some(TOKEN_INPUT)
        );
        assert_eq!(
            registry.normalize_token_type("completion_tokens"),
            Some(TOKEN_OUTPUT)
        );

        // Unknown
        assert_eq!(registry.normalize_token_type("unknown_type"), None);
    }

    #[test]
    fn test_shorten_model_name() {
        let registry = ProviderRegistry::new();

        // Claude models
        assert_eq!(
            registry.shorten_model_name("claude-opus-4-5-20250514"),
            "opus-4.5"
        );
        assert_eq!(
            registry.shorten_model_name("claude-sonnet-4-20250514"),
            "sonnet-4"
        );
        assert_eq!(
            registry.shorten_model_name("claude-3-5-haiku-20241022"),
            "haiku-3.5"
        );

        // OpenAI models
        assert_eq!(registry.shorten_model_name("gpt-4o-2024-05-13"), "gpt-4o");
        assert_eq!(
            registry.shorten_model_name("gpt-4-turbo-preview"),
            "gpt-4-turbo"
        );

        // Fallback
        assert_eq!(
            registry.shorten_model_name("some-very-long-unknown-model-name"),
            "some-very-lo..."
        );
    }
}
