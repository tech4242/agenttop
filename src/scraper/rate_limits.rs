//! Reader for the sidecar JSON written by the StatusLine hook.
//!
//! The Claude Code StatusLine hook (installed by `agenttop --setup claude`)
//! writes `~/.claude/agenttop-rate-limits.json` on every status-bar refresh.
//! This module reads it and rejects data older than 10 minutes (stale).

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use super::RateLimitInfo;

use crate::config::RATE_LIMIT_SIDECAR_NAME;

const MAX_AGE_SECS: u64 = 600; // 10 minutes

#[derive(Debug, Deserialize)]
struct SidecarFile {
    #[serde(default)]
    five_hour_pct: Option<f64>,
    #[serde(default)]
    five_hour_resets_at: Option<u64>,
    #[serde(default)]
    seven_day_pct: Option<f64>,
    #[serde(default)]
    seven_day_resets_at: Option<u64>,
    #[serde(default)]
    updated_at: Option<u64>,
}

pub fn read_all() -> Vec<RateLimitInfo> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let path = home.join(".claude").join(RATE_LIMIT_SIDECAR_NAME);
    let Ok(content) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(parsed): std::result::Result<SidecarFile, _> = serde_json::from_str(&content) else {
        return Vec::new();
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Use updated_at if present, else fall back to mtime.
    let updated_at = parsed.updated_at.or_else(|| {
        fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
    });

    if let Some(ts) = updated_at
        && now.saturating_sub(ts) > MAX_AGE_SECS
    {
        return Vec::new();
    }

    if parsed.five_hour_pct.is_none() && parsed.seven_day_pct.is_none() {
        return Vec::new();
    }

    vec![RateLimitInfo {
        source: "claude".to_string(),
        five_hour_pct: parsed.five_hour_pct,
        five_hour_resets_at: parsed.five_hour_resets_at,
        seven_day_pct: parsed.seven_day_pct,
        seven_day_resets_at: parsed.seven_day_resets_at,
        updated_at,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_stale_data() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let stale = now.saturating_sub(MAX_AGE_SECS + 60);
        let json = serde_json::to_string(&serde_json::json!({
            "five_hour_pct": 50.0,
            "seven_day_pct": 20.0,
            "updated_at": stale,
        }))
        .unwrap();

        let parsed: SidecarFile = serde_json::from_str(&json).unwrap();
        let age = now - parsed.updated_at.unwrap();
        assert!(age > MAX_AGE_SECS);
    }

    #[test]
    fn parses_well_formed_sidecar() {
        let json = r#"{"five_hour_pct": 42.0, "seven_day_pct": 15.5}"#;
        let parsed: SidecarFile = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.five_hour_pct, Some(42.0));
        assert_eq!(parsed.seven_day_pct, Some(15.5));
    }

    #[test]
    fn ignores_unknown_fields() {
        let json = r#"{"five_hour_pct": 1.0, "extra": "ignored"}"#;
        let parsed: SidecarFile = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.five_hour_pct, Some(1.0));
    }
}
