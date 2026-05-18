//! Claude Code subagent enumeration.
//!
//! Claude Code stores subagent state in
//! `~/.claude/projects/{encoded_cwd}/{sessionId}/subagents/` — one JSONL per
//! subagent plus a `.meta.json` sibling describing it. We aggregate cumulative
//! token usage from each JSONL's `usage` lines.
//!
//! Returns an empty list when the directory doesn't exist (which is the case
//! for most sessions that never spawn a subagent).

use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::{SubAgent, claude_sessions};

#[derive(Debug, Deserialize)]
struct MetaFile {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

pub fn for_session(session_id: &str, cwd: &str) -> Vec<SubAgent> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let dir = home
        .join(".claude")
        .join("projects")
        .join(claude_sessions::encode_cwd(cwd))
        .join(session_id)
        .join("subagents");

    let Ok(read_dir) = fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut by_name: std::collections::HashMap<String, SubAgent> = std::collections::HashMap::new();

    for entry in read_dir.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };

        if file_name.ends_with(".meta.json") {
            let stem = file_name.trim_end_matches(".meta.json").to_string();
            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(meta) = serde_json::from_str::<MetaFile>(&content)
            {
                let entry = by_name.entry(stem.clone()).or_insert_with(|| SubAgent {
                    name: stem.clone(),
                    status: String::new(),
                    tokens: 0,
                });
                if let Some(n) = meta.name {
                    entry.name = n;
                }
                if let Some(s) = meta.status {
                    entry.status = s;
                }
            }
        } else if file_name.ends_with(".jsonl") {
            let stem = file_name.trim_end_matches(".jsonl").to_string();
            let tokens = sum_jsonl_usage(&path);
            let entry = by_name.entry(stem.clone()).or_insert_with(|| SubAgent {
                name: stem,
                status: String::new(),
                tokens: 0,
            });
            entry.tokens = tokens;
        }
    }

    let mut out: Vec<SubAgent> = by_name.into_values().collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.tokens));
    out
}

fn sum_jsonl_usage(path: &Path) -> u64 {
    let Ok(content) = fs::read_to_string(path) else {
        return 0;
    };
    let mut total = 0u64;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(usage) = value
            .get("message")
            .and_then(|m| m.get("usage"))
            .or_else(|| value.get("usage"))
        {
            total += usage
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            total += usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn returns_empty_when_missing() {
        let result = for_session("nonexistent-session", "/tmp/nonexistent-project");
        assert!(result.is_empty());
    }

    #[test]
    fn sums_usage_across_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agent.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"message":{{"usage":{{"input_tokens":10,"output_tokens":5}}}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"message":{{"usage":{{"input_tokens":20,"output_tokens":3}}}}}}"#
        )
        .unwrap();
        let total = sum_jsonl_usage(&path);
        assert_eq!(total, 38);
    }
}
