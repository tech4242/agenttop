//! Static lookup table from model identifier to its context window size.
//!
//! Used by the scraper to compute context-window utilization without making
//! any API call. The table is intentionally permissive: we match on substring
//! to absorb model-name variants (`claude-opus-4-5-20250514`, `opus-4.5`,
//! `claude-opus-4-5-1m`, …). Unknown models return `None`; callers should
//! render a "—" rather than guess.

/// Returns the model's context window in tokens, if known.
pub fn context_window_for(model_id: &str) -> Option<u64> {
    let n = model_id.to_lowercase();

    // Claude — Opus 4.x has both a 200k and a 1M variant
    if n.contains("opus") {
        if n.contains("1m") || n.contains("-1m") {
            return Some(1_000_000);
        }
        return Some(200_000);
    }
    if n.contains("sonnet") {
        if n.contains("1m") {
            return Some(1_000_000);
        }
        return Some(200_000);
    }
    if n.contains("haiku") {
        return Some(200_000);
    }

    // OpenAI Codex / GPT family
    if n.contains("gpt-5") || n.contains("o5") {
        return Some(400_000);
    }
    if n.contains("gpt-4.1") {
        return Some(1_000_000);
    }
    if n.contains("gpt-4o") || n.contains("o3") || n.contains("o4") {
        return Some(128_000);
    }

    // Google Gemini
    if n.contains("gemini-2") || n.contains("gemini-3") {
        return Some(2_000_000);
    }
    if n.contains("gemini") {
        return Some(1_000_000);
    }

    // Qwen
    if n.contains("qwen") {
        return Some(128_000);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_opus_default() {
        assert_eq!(
            context_window_for("claude-opus-4-5-20250514"),
            Some(200_000)
        );
    }

    #[test]
    fn claude_opus_1m_variant() {
        assert_eq!(context_window_for("claude-opus-4-7-1m"), Some(1_000_000));
    }

    #[test]
    fn claude_sonnet() {
        assert_eq!(context_window_for("claude-sonnet-4-6"), Some(200_000));
    }

    #[test]
    fn claude_haiku() {
        assert_eq!(context_window_for("claude-haiku-4-5"), Some(200_000));
    }

    #[test]
    fn openai_gpt5() {
        assert_eq!(context_window_for("gpt-5-mini"), Some(400_000));
    }

    #[test]
    fn gemini_25() {
        assert_eq!(context_window_for("gemini-2.5-pro"), Some(2_000_000));
    }

    #[test]
    fn qwen() {
        assert_eq!(context_window_for("qwen-coder-32b"), Some(128_000));
    }

    #[test]
    fn unknown_model_returns_none() {
        assert_eq!(context_window_for("some-random-model"), None);
        assert_eq!(context_window_for(""), None);
    }
}
