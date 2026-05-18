//! Local file/process scraper — the non-OTLP side of agenttop.
//!
//! While the OTLP receiver gives us aggregate/historical metrics, the scraper
//! answers "what is each agent doing *right now*" — borrowed from abtop's
//! design but adapted to be cross-platform (sysinfo instead of /proc).
//!
//! Architecture:
//!   - The `Scraper` is owned by the TUI `App` and ticked alongside the
//!     storage refresh.
//!   - Each tick produces a `ScraperSnapshot` that the UI renders.
//!   - State carried across ticks: file offsets per transcript (incremental
//!     parsing), tracked ports (orphan detection), cached agent rate limits.
//!
//! Submodules:
//!   - `process` — sysinfo-backed process tree + RSS
//!   - `host` — system-wide CPU% / MEM% / load1
//!   - `ports` — listening ports + orphan tracking
//!   - `claude_sessions` — live Claude Code sessions from ~/.claude
//!   - `subagents` — Claude Code subagent state
//!   - `rate_limits` — sidecar file written by the StatusLine hook

use std::collections::HashMap;
use std::path::PathBuf;

pub mod claude_sessions;
pub mod host;
pub mod ports;
pub mod process;
pub mod rate_limits;
pub mod subagents;

/// Live status of an agent session, derived from transcript freshness and
/// rate-limit promotion. Mirrors abtop's `SessionStatus` so users moving
/// between the two tools see consistent labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// The model is generating its next response (last transcript line is a
    /// user/tool_result and recent).
    Thinking,
    /// A tool is executing (last assistant turn has an unmatched `tool_use`).
    Executing,
    /// Idle — waiting for the user or a permission prompt.
    Waiting,
    /// Promoted from Waiting when account-level rate limits are at 100%.
    RateLimited,
    /// The owning process is gone. Reserved for future use — current scrapers
    /// drop dead sessions from the live list immediately.
    #[allow(dead_code)]
    Done,
}

impl SessionStatus {
    pub fn label(self) -> &'static str {
        match self {
            SessionStatus::Thinking => "Thinking",
            SessionStatus::Executing => "Executing",
            SessionStatus::Waiting => "Waiting",
            SessionStatus::RateLimited => "RateLimited",
            SessionStatus::Done => "Done",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChildProcess {
    pub pid: u32,
    pub command: String,
    pub mem_kb: u64,
    pub port: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct SubAgent {
    pub name: String,
    pub status: String,
    pub tokens: u64,
}

#[derive(Debug, Clone)]
pub struct LiveSession {
    /// Which agent CLI: "claude_code", "codex", "gemini_cli", …
    pub agent_id: &'static str,
    /// Owning process id — kept for future kill/inspect actions.
    #[allow(dead_code)]
    pub pid: u32,
    pub session_id: String,
    pub cwd: String,
    pub project_name: String,
    /// Unix epoch ms — when the underlying session was created.
    pub started_at_ms: u64,
    pub status: SessionStatus,
    pub model: String,
    /// 0.0–1.0; `None` when the model is unrecognized or no tokens recorded yet.
    pub context_percent: Option<f64>,
    /// Total context window in tokens (lookup table, possibly auto-bumped to
    /// 1M for opus when observed usage exceeds the 200k default).
    pub context_window: Option<u64>,
    /// The most recent assistant turn's input + cache_read tokens — the
    /// actual current occupancy of the context window. Surfaced alongside
    /// `context_percent` so the UI can display "850k/1M" instead of just a %.
    pub latest_context_tokens: u64,
    /// Last tool invocation in human-readable form (e.g. `"Edit src/main.rs"`).
    pub current_task: String,
    /// Cumulative input tokens for the session (read from transcript).
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Cumulative cache replay across all turns. Not displayed today
    /// because every turn replays the full cached context — summing
    /// produces inflated multi-million numbers — but kept on the struct
    /// so a future "Cache: X / Y" breakdown can surface it.
    #[allow(dead_code)]
    pub cache_read_tokens: u64,
    #[allow(dead_code)]
    pub cache_creation_tokens: u64,
    /// RSS of the owning process in MB.
    pub mem_mb: u64,
    /// All descendant processes (with their RSS and any open port).
    pub children: Vec<ChildProcess>,
    /// Subagents (Claude Code only; empty otherwise).
    pub subagents: Vec<SubAgent>,
}

/// A port still bound to a live PID whose parent agent session has gone away.
#[derive(Debug, Clone)]
pub struct OrphanPort {
    pub port: u16,
    pub pid: u32,
    pub command: String,
    /// Session id that originally owned this port — for "blame". Currently
    /// not surfaced in the UI but tracked so we can show it in a future
    /// detail view.
    #[allow(dead_code)]
    pub origin_session_id: String,
}

/// Account-level rate-limit snapshot. Currently only Claude Code populates this.
#[derive(Debug, Clone, Default)]
pub struct RateLimitInfo {
    pub source: String,
    pub five_hour_pct: Option<f64>,
    pub five_hour_resets_at: Option<u64>,
    pub seven_day_pct: Option<f64>,
    pub seven_day_resets_at: Option<u64>,
    /// Unix epoch seconds — when the sidecar was last written. Tracked for
    /// staleness display in a future "data age" indicator.
    #[allow(dead_code)]
    pub updated_at: Option<u64>,
}

impl RateLimitInfo {
    pub fn is_at_limit(&self) -> bool {
        self.five_hour_pct.unwrap_or(0.0) >= 99.0 || self.seven_day_pct.unwrap_or(0.0) >= 99.0
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HostMetrics {
    pub cpu_pct: f64,
    pub mem_pct: f64,
    pub load1: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ScraperSnapshot {
    pub live_sessions: Vec<LiveSession>,
    pub orphan_ports: Vec<OrphanPort>,
    pub rate_limits: Vec<RateLimitInfo>,
    pub host_metrics: HostMetrics,
}

/// Scraper owns the cross-tick state that allows incremental parsing and
/// orphan-port detection. One instance per `App`.
pub struct Scraper {
    sys: process::ProcessScanner,
    host_sampler: host::HostSampler,
    /// Per-transcript file offsets so we only read appended bytes.
    transcript_offsets: HashMap<PathBuf, u64>,
    /// PIDs that hold a port → (port, owning session id at time of discovery).
    /// Used to flag orphans when the owning session disappears.
    tracked_port_children: HashMap<u32, (u16, String, String)>,
    /// Tick counter for heavy-refresh throttling (process tree, file scans).
    heavy_tick_count: u32,
    /// Tick counter for the slowest cycle (port scan).
    slow_tick_count: u32,
    /// Cached port snapshot from the last slow tick.
    cached_ports: HashMap<u32, u16>,
    /// Cached snapshot of everything that's expensive to compute. We replace
    /// this on heavy ticks; cheap host metrics are overlaid every tick.
    cached_snapshot: ScraperSnapshot,
}

// App.refresh() runs every ~100 ms. Heavy I/O (process tree, file reads,
// transcript parsing) only happens every HEAVY_TICK_EVERY refreshes — once
// per second — so the UI stays calm and CPU stays low. Host vitals are
// cheap and refresh every tick.
const HEAVY_TICK_EVERY: u32 = 10; // ~1 s
const SLOW_TICK_EVERY: u32 = 100; // ~10 s (ports, etc.)

impl Default for Scraper {
    fn default() -> Self {
        Self::new()
    }
}

impl Scraper {
    pub fn new() -> Self {
        Self {
            sys: process::ProcessScanner::new(),
            host_sampler: host::HostSampler::new(),
            transcript_offsets: HashMap::new(),
            tracked_port_children: HashMap::new(),
            heavy_tick_count: HEAVY_TICK_EVERY, // force a heavy tick on first call
            slow_tick_count: SLOW_TICK_EVERY,
            cached_ports: HashMap::new(),
            cached_snapshot: ScraperSnapshot::default(),
        }
    }

    pub fn tick(&mut self) -> ScraperSnapshot {
        // Host vitals are cheap — sample every tick so CPU/MEM/load feel live.
        let host_metrics = self.host_sampler.sample();

        let heavy_tick = self.heavy_tick_count >= HEAVY_TICK_EVERY;
        let slow_tick = self.slow_tick_count >= SLOW_TICK_EVERY;

        if !heavy_tick {
            self.heavy_tick_count += 1;
            // Reuse the cached snapshot — only the host vitals are fresh.
            let mut snap = self.cached_snapshot.clone();
            snap.host_metrics = host_metrics;
            return snap;
        }

        // We're doing the heavy work this tick.
        self.heavy_tick_count = 0;
        if slow_tick {
            self.slow_tick_count = 0;
        }
        self.slow_tick_count += 1;

        // 1. Refresh process tree.
        self.sys.refresh();

        // 2. Refresh ports on slow tick only (lsof is expensive).
        if slow_tick {
            self.cached_ports = ports::scan_listening_ports();
        }

        // 3. Scrape live Claude Code sessions (the only agent we can scrape
        //    directly today — others are OTLP-only).
        let mut live_sessions =
            claude_sessions::scan(&self.sys, &self.cached_ports, &mut self.transcript_offsets);

        // 4. Enrich with subagents (cheap — only reads files for live sessions).
        for session in &mut live_sessions {
            session.subagents = subagents::for_session(&session.session_id, &session.cwd);
        }

        // 5. Rate limits from the Claude StatusLine sidecar file.
        let rate_limits = rate_limits::read_all();

        // 6. Promote Waiting → RateLimited when at-limit.
        let any_at_limit = rate_limits.iter().any(|r| r.is_at_limit());
        if any_at_limit {
            for session in &mut live_sessions {
                if session.status == SessionStatus::Waiting && session.agent_id == "claude_code" {
                    session.status = SessionStatus::RateLimited;
                }
            }
        }

        // 7. Orphan-port detection — refresh tracked map from live sessions,
        //    flag PIDs that still hold a port but whose owning session is gone.
        let orphan_ports = self.detect_orphan_ports(&live_sessions);

        let snap = ScraperSnapshot {
            live_sessions,
            orphan_ports,
            rate_limits,
            host_metrics,
        };
        self.cached_snapshot = snap.clone();
        snap
    }

    fn detect_orphan_ports(&mut self, live_sessions: &[LiveSession]) -> Vec<OrphanPort> {
        let mut live_session_ids = std::collections::HashSet::new();
        for s in live_sessions {
            live_session_ids.insert(s.session_id.clone());
            for child in &s.children {
                if let Some(port) = child.port {
                    self.tracked_port_children.insert(
                        child.pid,
                        (port, child.command.clone(), s.session_id.clone()),
                    );
                }
            }
        }

        let (orphans, to_remove) =
            compute_orphans_and_evictions(&self.tracked_port_children, &live_session_ids, |pid| {
                self.sys.is_alive(pid)
            });
        for pid in to_remove {
            self.tracked_port_children.remove(&pid);
        }
        orphans
    }
}

/// Pure helper for orphan-port detection. Takes the tracked map, the set of
/// session ids currently alive, and a function that reports whether a given
/// PID is still alive. Returns (orphans, pids-to-evict-from-tracked).
///
/// Extracted so tests can drive it without instantiating a real
/// `ProcessScanner` (which depends on live sysinfo state).
fn compute_orphans_and_evictions(
    tracked: &HashMap<u32, (u16, String, String)>,
    live_session_ids: &std::collections::HashSet<String>,
    is_pid_alive: impl Fn(u32) -> bool,
) -> (Vec<OrphanPort>, Vec<u32>) {
    let mut orphans = Vec::new();
    let mut to_remove = Vec::new();
    for (&pid, (port, command, origin)) in tracked {
        if !is_pid_alive(pid) {
            to_remove.push(pid);
            continue;
        }
        if !live_session_ids.contains(origin) {
            orphans.push(OrphanPort {
                port: *port,
                pid,
                command: command.clone(),
                origin_session_id: origin.clone(),
            });
        }
    }
    (orphans, to_remove)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn tracked_entry(
        pid: u32,
        port: u16,
        command: &str,
        origin: &str,
    ) -> (u32, (u16, String, String)) {
        (pid, (port, command.to_string(), origin.to_string()))
    }

    #[test]
    fn rate_limit_info_is_at_limit_handles_boundaries() {
        let none = RateLimitInfo::default();
        assert!(!none.is_at_limit(), "no data → not at limit");

        let under = RateLimitInfo {
            five_hour_pct: Some(98.9),
            ..Default::default()
        };
        assert!(!under.is_at_limit(), "98.9% is not at limit");

        let at = RateLimitInfo {
            five_hour_pct: Some(99.0),
            ..Default::default()
        };
        assert!(at.is_at_limit(), "exactly 99% triggers");

        let over = RateLimitInfo {
            seven_day_pct: Some(100.0),
            ..Default::default()
        };
        assert!(over.is_at_limit(), "7d limit also counts");
    }

    #[test]
    fn session_status_label_round_trips() {
        // Lock down the user-visible strings.
        assert_eq!(SessionStatus::Thinking.label(), "Thinking");
        assert_eq!(SessionStatus::Executing.label(), "Executing");
        assert_eq!(SessionStatus::Waiting.label(), "Waiting");
        assert_eq!(SessionStatus::RateLimited.label(), "RateLimited");
        assert_eq!(SessionStatus::Done.label(), "Done");
    }

    #[test]
    fn compute_orphans_finds_port_with_dead_parent() {
        let tracked: HashMap<u32, (u16, String, String)> = [
            tracked_entry(100, 8000, "python", "session-A"),
            tracked_entry(101, 8001, "node", "session-B"),
        ]
        .into_iter()
        .collect();

        // Only session-B is currently alive. PIDs 100 and 101 are both alive.
        let live: HashSet<String> = ["session-B".to_string()].into_iter().collect();
        let (orphans, evict) = compute_orphans_and_evictions(&tracked, &live, |_pid| true);

        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].port, 8000);
        assert_eq!(orphans[0].pid, 100);
        assert_eq!(orphans[0].origin_session_id, "session-A");
        assert!(evict.is_empty(), "no PID died, nothing to evict");
    }

    #[test]
    fn compute_orphans_evicts_dead_pids() {
        let tracked: HashMap<u32, (u16, String, String)> =
            [tracked_entry(200, 9000, "python", "session-X")]
                .into_iter()
                .collect();
        let live: HashSet<String> = HashSet::new();

        // PID 200 is dead.
        let (orphans, evict) = compute_orphans_and_evictions(&tracked, &live, |pid| pid != 200);

        assert!(orphans.is_empty(), "dead PID is not an orphan — it's gone");
        assert_eq!(evict, vec![200], "must mark dead PID for eviction");
    }

    #[test]
    fn compute_orphans_skips_live_sessions() {
        let tracked: HashMap<u32, (u16, String, String)> =
            [tracked_entry(300, 7000, "ruby", "session-active")]
                .into_iter()
                .collect();
        let live: HashSet<String> = ["session-active".to_string()].into_iter().collect();

        let (orphans, evict) = compute_orphans_and_evictions(&tracked, &live, |_pid| true);
        assert!(
            orphans.is_empty(),
            "port belongs to a live session, not orphan"
        );
        assert!(evict.is_empty());
    }

    #[test]
    fn rate_limit_only_seven_day_at_limit() {
        let rl = RateLimitInfo {
            five_hour_pct: Some(0.0),
            seven_day_pct: Some(99.5),
            ..Default::default()
        };
        assert!(rl.is_at_limit());
    }
}
