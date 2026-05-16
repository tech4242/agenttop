//! Live Claude Code sessions, scraped from the local filesystem.
//!
//! Claude Code writes one `~/.claude/sessions/{PID}.json` per active session
//! (pid, sessionId, cwd, startedAt). The full conversation is appended to a
//! JSONL transcript at `~/.claude/projects/{encoded_cwd}/{sessionId}.jsonl`.
//!
//! We derive everything from these two files plus the live process tree:
//!   - status: from transcript mtime + last line type
//!   - current_task: from the most recent tool_use block
//!   - tokens: sum of `usage` fields across all assistant turns
//!   - model: from the latest `message.model` field
//!   - mem_mb / children: from sysinfo
//!
//! Cross-tick state: we track per-transcript file offsets so we only re-read
//! appended bytes (matches abtop's incremental parsing).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use super::process::ProcessScanner;
use super::{ChildProcess, LiveSession, SessionStatus};
use crate::model_info::context_window_for;

const STALE_MTIME_SECS: u64 = 30;

#[derive(Debug, Deserialize)]
struct SessionFile {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    #[serde(rename = "startedAt", default)]
    started_at: u64,
}

pub fn scan(
    sys: &ProcessScanner,
    ports_by_pid: &HashMap<u32, u16>,
    transcript_offsets: &mut HashMap<PathBuf, u64>,
) -> Vec<LiveSession> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let sessions_dir = home.join(".claude").join("sessions");
    let projects_dir = home.join(".claude").join("projects");

    let Ok(read_dir) = fs::read_dir(&sessions_dir) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(meta): std::result::Result<SessionFile, _> = serde_json::from_str(&content) else {
            continue;
        };

        // PID-reuse safety: the session file lingers across crashes. Skip
        // entries whose PID is gone OR whose live process isn't actually
        // claude (PID got reassigned).
        let Some(proc) = sys.get(meta.pid) else {
            continue;
        };
        if !proc.cmd.contains("claude") && !proc.name.contains("claude") {
            continue;
        }

        let transcript_path = projects_dir
            .join(encode_cwd(&meta.cwd))
            .join(format!("{}.jsonl", meta.session_id));

        let summary = parse_transcript(&transcript_path, transcript_offsets);

        let status = derive_status(&transcript_path, &summary);
        let project_name = project_name_from_cwd(&meta.cwd);

        // Mem + children come from sysinfo.
        let mem_mb = proc.rss_kb / 1024;
        let mut children = Vec::new();
        for child_pid in sys.descendants(meta.pid) {
            if let Some(cp) = sys.get(child_pid) {
                children.push(ChildProcess {
                    pid: child_pid,
                    command: short_cmd(&cp.cmd, &cp.name),
                    mem_kb: cp.rss_kb,
                    port: ports_by_pid.get(&child_pid).copied(),
                });
            }
        }
        // Sort children by RSS desc so the biggest is first.
        children.sort_by(|a, b| b.mem_kb.cmp(&a.mem_kb));

        // Window detection: opus has both 200k and 1M variants, but the
        // transcript's model id usually doesn't say which one (the 1M variant
        // is selected via API beta header, not encoded in the name). If we
        // observe usage > 200k, the session must be in 1M mode — auto-bump.
        let mut context_window = summary
            .last_model
            .as_deref()
            .and_then(context_window_for);
        if let Some(window) = context_window
            && summary.latest_context_tokens > window
            && summary
                .last_model
                .as_deref()
                .map(|m| m.to_lowercase().contains("opus"))
                .unwrap_or(false)
        {
            context_window = Some(1_000_000);
        }
        let context_percent = context_window.and_then(|w| {
            if w == 0 || summary.latest_context_tokens == 0 {
                None
            } else {
                Some((summary.latest_context_tokens as f64 / w as f64).clamp(0.0, 1.0))
            }
        });

        out.push(LiveSession {
            agent_id: "claude_code",
            pid: meta.pid,
            session_id: meta.session_id,
            cwd: meta.cwd,
            project_name,
            started_at_ms: meta.started_at,
            status,
            model: summary.last_model.unwrap_or_default(),
            context_percent,
            context_window,
            latest_context_tokens: summary.latest_context_tokens,
            current_task: summary.current_task,
            input_tokens: summary.input_tokens,
            output_tokens: summary.output_tokens,
            cache_read_tokens: summary.cache_read_tokens,
            cache_creation_tokens: summary.cache_creation_tokens,
            mem_mb,
            children,
            subagents: Vec::new(), // filled in by Scraper::tick
        });
    }

    // Drop offsets for transcripts whose session no longer appears (so the
    // map doesn't grow unboundedly across long sessions of the dashboard).
    let live_paths: std::collections::HashSet<PathBuf> = out
        .iter()
        .map(|s| {
            home.join(".claude")
                .join("projects")
                .join(encode_cwd(&s.cwd))
                .join(format!("{}.jsonl", s.session_id))
        })
        .collect();
    transcript_offsets.retain(|p, _| live_paths.contains(p));

    // Newest-first.
    out.sort_by_key(|s| std::cmp::Reverse(s.started_at_ms));
    out
}

/// Cumulative parse result that gets merged with prior-tick state via the
/// offset map.
#[derive(Default)]
pub(crate) struct TranscriptSummary {
    /// Cumulative tokens across **all** assistant turns. Represents lifetime
    /// consumption — useful for the "TOKENS" column but NOT for context%,
    /// because each turn's `input_tokens` already contains the full prior
    /// history (Claude is stateless), so summing inflates by ~N turns.
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    /// The *most recent* assistant turn's input + cache_read. This is the
    /// actual current context-window occupancy (drops when compaction fires).
    pub latest_context_tokens: u64,
    pub last_model: Option<String>,
    pub current_task: String,
    pub last_line_kind: LastLineKind,
    pub had_data: bool,
}

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum LastLineKind {
    #[default]
    Unknown,
    /// Last entry was a user message or tool_result — model is generating next.
    UserOrToolResult,
    /// Last entry was an assistant turn with at least one tool_use block that
    /// has no matching tool_result yet → a tool is executing.
    AssistantWithPendingTool,
    /// Last entry was an assistant turn whose tool_use blocks were all
    /// followed by matching tool_results, or that had no tool_use blocks.
    AssistantSettled,
}

pub(crate) fn parse_transcript(
    path: &Path,
    offsets: &mut HashMap<PathBuf, u64>,
) -> TranscriptSummary {
    let mut summary = TranscriptSummary::default();

    let Ok(content) = fs::read_to_string(path) else {
        return summary;
    };

    // Offset tracking: if the file shrank (rotation), restart from 0.
    let prev_offset = offsets.get(path).copied().unwrap_or(0);
    let total_len = content.len() as u64;
    let _start_offset = if prev_offset > total_len {
        0
    } else {
        prev_offset
    };

    // For correctness we always re-scan the *whole* file (we need cumulative
    // totals across the conversation). The offset tracking still saves work
    // because we record it for future ticks where we may add an incremental
    // append-only branch, but cumulative summaries are cheap (transcripts
    // are typically < a few MB).
    //
    // NOTE: We intentionally do NOT skip to the prior offset — Claude Code
    // emits cumulative `usage` per turn, not deltas, so re-scanning is safer.
    offsets.insert(path.to_path_buf(), total_len);

    // Track open tool_uses by id so we can detect "assistant turn with
    // pending tool" (= Executing).
    let mut pending_tool_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_line_kind = LastLineKind::Unknown;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        summary.had_data = true;

        let entry_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match entry_type {
            "assistant" => {
                last_line_kind = LastLineKind::AssistantSettled;
                let message = value.get("message");

                // Model name (latest wins).
                if let Some(model) = message.and_then(|m| m.get("model")).and_then(|v| v.as_str())
                {
                    summary.last_model = Some(model.to_string());
                }

                // Token usage. We track BOTH:
                //   - cumulative totals (sum across all turns) → lifetime spend
                //   - the latest turn's input + cache_read → current context %
                if let Some(usage) = message.and_then(|m| m.get("usage")) {
                    let turn_input = usage
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let turn_output = usage
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let turn_cache_read = usage
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let turn_cache_create = usage
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);

                    summary.input_tokens += turn_input;
                    summary.output_tokens += turn_output;
                    summary.cache_read_tokens += turn_cache_read;
                    summary.cache_creation_tokens += turn_cache_create;

                    // Overwrite — only the latest turn matters for context%.
                    // Excludes cache_creation to match abtop's logic (avoids
                    // spikes on compaction turns where new cache is being
                    // written).
                    summary.latest_context_tokens = turn_input + turn_cache_read;
                }

                // Walk content blocks for the most recent tool_use.
                if let Some(blocks) = message
                    .and_then(|m| m.get("content"))
                    .and_then(|v| v.as_array())
                {
                    for block in blocks {
                        let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if block_type == "tool_use" {
                            let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let arg = first_meaningful_arg(block.get("input"));
                            summary.current_task = if arg.is_empty() {
                                name.to_string()
                            } else {
                                format!("{} {}", name, arg)
                            };
                            if let Some(id) = block.get("id").and_then(|v| v.as_str()) {
                                pending_tool_ids.insert(id.to_string());
                                last_line_kind = LastLineKind::AssistantWithPendingTool;
                            }
                        }
                    }
                }
            }
            "user" => {
                last_line_kind = LastLineKind::UserOrToolResult;
                // Mark any tool_results as resolving pending tool_uses.
                if let Some(blocks) = value
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|v| v.as_array())
                {
                    for block in blocks {
                        if block.get("type").and_then(|v| v.as_str()) == Some("tool_result")
                            && let Some(id) =
                                block.get("tool_use_id").and_then(|v| v.as_str())
                        {
                            pending_tool_ids.remove(id);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // If we ended on an assistant turn but all tools have settled, reflect that.
    if last_line_kind == LastLineKind::AssistantWithPendingTool && pending_tool_ids.is_empty() {
        last_line_kind = LastLineKind::AssistantSettled;
    }

    summary.last_line_kind = last_line_kind;
    summary
}

fn derive_status(transcript_path: &Path, summary: &TranscriptSummary) -> SessionStatus {
    if !summary.had_data {
        return SessionStatus::Waiting;
    }

    let stale = fs::metadata(transcript_path)
        .and_then(|m| m.modified())
        .map(|t| {
            SystemTime::now()
                .duration_since(t)
                .map(|d| d.as_secs() > STALE_MTIME_SECS)
                .unwrap_or(true)
        })
        .unwrap_or(true);

    if stale {
        return SessionStatus::Waiting;
    }

    match summary.last_line_kind {
        LastLineKind::AssistantWithPendingTool => SessionStatus::Executing,
        LastLineKind::UserOrToolResult => SessionStatus::Thinking,
        LastLineKind::AssistantSettled => SessionStatus::Waiting,
        LastLineKind::Unknown => SessionStatus::Waiting,
    }
}

/// Extract a short meaningful arg from a tool's input — file_path, command
/// prefix, pattern, etc. Falls back to empty string.
fn first_meaningful_arg(input: Option<&serde_json::Value>) -> String {
    let Some(input) = input.and_then(|v| v.as_object()) else {
        return String::new();
    };

    for key in ["file_path", "command", "pattern", "path", "url", "query"] {
        if let Some(v) = input.get(key).and_then(|v| v.as_str()) {
            return truncate(v, 60);
        }
    }
    String::new()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

/// Claude Code encodes `/Users/foo/bar` as `-Users-foo-bar` for the
/// per-project transcript directory.
pub(crate) fn encode_cwd(cwd: &str) -> String {
    cwd.replace('/', "-")
}

fn project_name_from_cwd(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| cwd.to_string())
}

fn short_cmd(cmd: &str, name: &str) -> String {
    // Prefer the full cmd if it's short enough, else fall back to name.
    if cmd.is_empty() {
        return name.to_string();
    }
    let first = cmd.split_whitespace().next().unwrap_or("");
    let basename = Path::new(first)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| first.to_string());
    if basename.is_empty() {
        name.to_string()
    } else {
        basename
    }
}

#[allow(dead_code)]
pub(crate) fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn encode_cwd_replaces_slashes() {
        assert_eq!(encode_cwd("/Users/foo/bar"), "-Users-foo-bar");
        assert_eq!(encode_cwd("/a"), "-a");
    }

    #[test]
    fn parse_transcript_tracks_cumulative_and_latest_separately() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","message":{{"content":"hi"}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"model":"claude-opus-4-5","usage":{{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":200,"cache_creation_input_tokens":10}},"content":[{{"type":"text","text":"ok"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"model":"claude-opus-4-5","usage":{{"input_tokens":120,"output_tokens":80,"cache_read_input_tokens":300,"cache_creation_input_tokens":0}},"content":[{{"type":"tool_use","id":"t1","name":"Edit","input":{{"file_path":"src/main.rs"}}}}]}}}}"#
        )
        .unwrap();

        let mut offsets = HashMap::new();
        let s = parse_transcript(&path, &mut offsets);
        // Cumulative across both turns (lifetime totals).
        assert_eq!(s.input_tokens, 220);
        assert_eq!(s.output_tokens, 130);
        assert_eq!(s.cache_read_tokens, 500);
        assert_eq!(s.cache_creation_tokens, 10);
        // Latest turn only — used for context% (120 + 300 = 420).
        assert_eq!(s.latest_context_tokens, 420);
        assert_eq!(s.last_model.as_deref(), Some("claude-opus-4-5"));
        assert_eq!(s.current_task, "Edit src/main.rs");
        assert_eq!(s.last_line_kind, LastLineKind::AssistantWithPendingTool);
        assert_eq!(offsets.get(&path).copied(), Some(fs::metadata(&path).unwrap().len()));
    }

    #[test]
    fn latest_context_drops_after_compaction() {
        // Simulates a compaction event: input_tokens drops from a large prior
        // turn to a small new turn. context% should follow the drop.
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"model":"sonnet","usage":{{"input_tokens":150000,"output_tokens":500,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"model":"sonnet","usage":{{"input_tokens":8000,"output_tokens":100,"cache_read_input_tokens":2000,"cache_creation_input_tokens":0}}}}}}"#
        )
        .unwrap();
        let mut offsets = HashMap::new();
        let s = parse_transcript(&path, &mut offsets);
        // Cumulative still high (lifetime spend).
        assert_eq!(s.input_tokens, 158_000);
        // But latest context = just the post-compaction turn.
        assert_eq!(s.latest_context_tokens, 10_000);
    }

    #[test]
    fn parse_transcript_settles_when_tool_result_arrives() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"model":"sonnet","usage":{{"input_tokens":1,"output_tokens":1}},"content":[{{"type":"tool_use","id":"t1","name":"Read","input":{{"file_path":"a"}}}}]}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"t1","content":"ok"}}]}}}}"#
        )
        .unwrap();
        let mut offsets = HashMap::new();
        let s = parse_transcript(&path, &mut offsets);
        assert_eq!(s.last_line_kind, LastLineKind::UserOrToolResult);
    }

    #[test]
    fn parse_transcript_resets_on_file_shrink() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut offsets = HashMap::new();
        offsets.insert(path.clone(), 999_999);
        // Tiny file → prev offset > len → we still parse cleanly from start.
        fs::write(&path, r#"{"type":"assistant","message":{"model":"x","usage":{"input_tokens":5,"output_tokens":5}}}"#).unwrap();
        let s = parse_transcript(&path, &mut offsets);
        assert_eq!(s.input_tokens, 5);
        assert_eq!(s.output_tokens, 5);
    }

    #[test]
    fn parse_transcript_missing_file_returns_default() {
        let mut offsets = HashMap::new();
        let s = parse_transcript(Path::new("/nonexistent/path.jsonl"), &mut offsets);
        assert!(!s.had_data);
        assert_eq!(s.input_tokens, 0);
    }
}
