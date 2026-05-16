use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::project::ProjectResolver;
use crate::providers::PROVIDER_REGISTRY;
use crate::scraper::{Scraper, ScraperSnapshot};

fn humanize_age(d: chrono::Duration) -> String {
    let secs = d.num_seconds().max(0);
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

fn humanize_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        n.to_string()
    }
}
use crate::storage::{
    ApiMetrics, CompactionStats, ProjectInfo, SessionMetrics, StorageHandle, TokenMetrics,
    ToolMetrics,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeFilter {
    LastHour,
    Last24Hours,
    Last7Days,
    #[default]
    AllTime,
}

impl TimeFilter {
    pub fn label(&self) -> &'static str {
        match self {
            TimeFilter::LastHour => "Last 1h",
            TimeFilter::Last24Hours => "Last 24h",
            TimeFilter::Last7Days => "Last 7d",
            TimeFilter::AllTime => "All-time",
        }
    }

    pub fn since(&self) -> Option<DateTime<Utc>> {
        match self {
            TimeFilter::LastHour => Some(Utc::now() - chrono::Duration::hours(1)),
            TimeFilter::Last24Hours => Some(Utc::now() - chrono::Duration::hours(24)),
            TimeFilter::Last7Days => Some(Utc::now() - chrono::Duration::days(7)),
            TimeFilter::AllTime => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Calls,
    LastCall,
    AvgDuration,
    Name,
    /// Group built-in tools first, then MCP tools grouped by server name.
    Type,
}

/// Which panel keyboard navigation (j/k, up/down) operates on. `Tab` cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusPanel {
    #[default]
    Tools,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProjectFilter {
    #[default]
    All,
    Project(String),
}

pub struct App {
    storage: StorageHandle,
    /// Project resolver for mapping session.id to project names
    project_resolver: ProjectResolver,
    /// Cross-tick scraper for live process/file state (sessions, ports, host).
    scraper: Scraper,
    /// Latest snapshot from the scraper, refreshed each tick.
    pub scraper_snapshot: ScraperSnapshot,
    /// Token-rate over the last 5 minutes, bucketed for the sparkline.
    pub token_rate_series: Vec<f64>,
    pub tool_metrics: Vec<ToolMetrics>,
    pub token_metrics: TokenMetrics,
    pub session_metrics: SessionMetrics,
    pub api_metrics: ApiMetrics,
    pub selected_index: usize,
    pub sort_by: SortColumn,
    pub sort_ascending: bool,
    pub paused: bool,
    pub show_detail: bool,
    pub last_refresh: DateTime<Utc>,
    pub time_filter: TimeFilter,
    /// Detected agents from OTLP data (e.g., ["claude_code", "gemini_cli"])
    pub detected_agents: Vec<String>,
    /// Currently selected agent index (for filtering display)
    pub selected_agent_index: usize,
    /// Detected projects resolved from session data
    pub detected_projects: Vec<ProjectInfo>,
    /// Current project filter
    pub project_filter: ProjectFilter,
    /// Compaction stats from claude_code.compaction events
    pub compaction_stats: CompactionStats,
    /// Which panel j/k navigates. Tab cycles. Auto-falls-back to Tools when
    /// there are no live sessions to focus.
    pub focus: FocusPanel,
    /// Selected row in the live-sessions table.
    pub live_selected_index: usize,
}

impl App {
    pub fn new(storage: StorageHandle) -> Self {
        Self {
            storage,
            project_resolver: ProjectResolver::new(),
            scraper: Scraper::new(),
            scraper_snapshot: ScraperSnapshot::default(),
            token_rate_series: Vec::new(),
            tool_metrics: Vec::new(),
            token_metrics: TokenMetrics::default(),
            session_metrics: SessionMetrics::default(),
            api_metrics: ApiMetrics::default(),
            selected_index: 0,
            sort_by: SortColumn::Calls,
            sort_ascending: false,
            paused: false,
            show_detail: false,
            last_refresh: Utc::now(),
            time_filter: TimeFilter::default(),
            detected_agents: Vec::new(),
            selected_agent_index: 0,
            detected_projects: Vec::new(),
            project_filter: ProjectFilter::default(),
            compaction_stats: CompactionStats::default(),
            focus: FocusPanel::default(),
            live_selected_index: 0,
        }
    }

    pub fn refresh(&mut self) -> Result<()> {
        if self.paused {
            return Ok(());
        }

        // Scrape live process/file state — independent of OTLP, runs every tick.
        self.scraper_snapshot = self.scraper.tick();
        // Token-rate sparkline: last 5 minutes bucketed into 60 points (~5s each).
        self.token_rate_series = self
            .storage
            .get_token_rate_series(300, 60)
            .unwrap_or_default();

        self.tool_metrics = self.storage.get_tool_metrics(self.time_filter.since())?;
        self.token_metrics = self.storage.get_token_metrics(self.time_filter.since())?;
        self.session_metrics = self.storage.get_session_metrics(self.time_filter.since())?;
        self.api_metrics = self.storage.get_api_metrics(self.time_filter.since())?;

        // Get distinct sessions and map to projects using ProjectResolver
        let sessions = self
            .storage
            .get_distinct_sessions(self.time_filter.since())?;

        // Aggregate sessions by project name
        use std::collections::HashMap;
        let mut project_aggregates: HashMap<String, ProjectInfo> = HashMap::new();

        for session in sessions {
            // Resolve session to project name, or use truncated session ID as fallback
            let project_name = self
                .project_resolver
                .resolve(&session.session_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| {
                    // Fallback: use first 8 chars of session ID
                    format!("session:{}", &session.session_id[..8.min(session.session_id.len())])
                });

            let entry = project_aggregates.entry(project_name.clone()).or_insert(ProjectInfo {
                name: project_name,
                event_count: 0,
                first_seen: session.first_seen,
                last_seen: session.last_seen,
            });

            entry.event_count += session.event_count;

            // Update first_seen to earliest
            if let (Some(existing), Some(new)) = (entry.first_seen, session.first_seen) {
                if new < existing {
                    entry.first_seen = Some(new);
                }
            } else if entry.first_seen.is_none() {
                entry.first_seen = session.first_seen;
            }

            // Update last_seen to latest
            if let (Some(existing), Some(new)) = (entry.last_seen, session.last_seen) {
                if new > existing {
                    entry.last_seen = Some(new);
                }
            } else if entry.last_seen.is_none() {
                entry.last_seen = session.last_seen;
            }
        }

        // Convert to sorted vector (by event_count descending)
        let mut projects: Vec<ProjectInfo> = project_aggregates.into_values().collect();
        projects.sort_by(|a, b| b.event_count.cmp(&a.event_count));
        self.detected_projects = projects;

        self.compaction_stats = self
            .storage
            .get_compaction_stats(self.time_filter.since())
            .unwrap_or_default();

        self.last_refresh = Utc::now();

        // Detect agents from tool usage and model names
        // Collect agent IDs first to avoid borrow issues
        let mut new_agents: Vec<&'static str> = Vec::new();

        for tool in &self.tool_metrics {
            if let Some(provider) = PROVIDER_REGISTRY.provider_for_tool(&tool.tool_name) {
                new_agents.push(provider.id());
            }
        }

        for model_name in self.api_metrics.models.keys() {
            for provider in PROVIDER_REGISTRY.providers() {
                if provider.shorten_model_name(model_name).is_some() {
                    new_agents.push(provider.id());
                    break;
                }
            }
        }

        // Detect agents from OTel `service.name` resource attribute. This catches
        // Cline / Copilot Chat / opencode where tool names and model patterns
        // overlap with other providers.
        if let Ok(service_names) = self
            .storage
            .get_distinct_service_names(self.time_filter.since())
        {
            for service_name in service_names {
                if let Some(provider) = PROVIDER_REGISTRY.find_by_service_name(&service_name) {
                    new_agents.push(provider.id());
                }
            }
        }

        // Now add all detected agents
        for agent_id in new_agents {
            self.add_detected_agent(agent_id);
        }

        // Sort the tools
        self.sort_tools();

        // Ensure selected indices are valid for both panels.
        if !self.tool_metrics.is_empty() && self.selected_index >= self.tool_metrics.len() {
            self.selected_index = self.tool_metrics.len() - 1;
        }
        let live_len = self.scraper_snapshot.live_sessions.len();
        if live_len > 0 && self.live_selected_index >= live_len {
            self.live_selected_index = live_len - 1;
        } else if live_len == 0 {
            self.live_selected_index = 0;
        }

        Ok(())
    }

    fn sort_tools(&mut self) {
        let ascending = self.sort_ascending;
        // All sorts use tool_name as secondary key for stability
        match self.sort_by {
            SortColumn::Calls => {
                self.tool_metrics.sort_by(|a, b| {
                    let primary = if ascending {
                        a.call_count.cmp(&b.call_count)
                    } else {
                        b.call_count.cmp(&a.call_count)
                    };
                    primary.then_with(|| a.tool_name.cmp(&b.tool_name))
                });
            }
            SortColumn::LastCall => {
                self.tool_metrics.sort_by(|a, b| {
                    let primary = if ascending {
                        a.last_call.cmp(&b.last_call)
                    } else {
                        b.last_call.cmp(&a.last_call)
                    };
                    primary.then_with(|| a.tool_name.cmp(&b.tool_name))
                });
            }
            SortColumn::AvgDuration => {
                self.tool_metrics.sort_by(|a, b| {
                    let primary = a
                        .avg_duration_ms
                        .partial_cmp(&b.avg_duration_ms)
                        .unwrap_or(std::cmp::Ordering::Equal);
                    let primary = if ascending {
                        primary
                    } else {
                        primary.reverse()
                    };
                    primary.then_with(|| a.tool_name.cmp(&b.tool_name))
                });
            }
            SortColumn::Name => {
                self.tool_metrics.sort_by(|a, b| {
                    if ascending {
                        a.tool_name.cmp(&b.tool_name)
                    } else {
                        b.tool_name.cmp(&a.tool_name)
                    }
                });
            }
            SortColumn::Type => {
                // Built-ins first (lexicographic by name), then MCP grouped
                // by server name, then by tool name within each server.
                self.tool_metrics.sort_by(|a, b| {
                    let a_builtin = a.is_builtin();
                    let b_builtin = b.is_builtin();
                    a_builtin
                        .cmp(&b_builtin)
                        .reverse() // true (builtin) sorts first
                        .then_with(|| a.display_name().cmp(&b.display_name()))
                });
            }
        }
    }

    pub fn toggle_sort(&mut self) {
        // Type is appended at the end of the existing cycle so older tests
        // that assumed Calls → LastCall → AvgDuration → Name still pass.
        self.sort_by = match self.sort_by {
            SortColumn::Calls => SortColumn::LastCall,
            SortColumn::LastCall => SortColumn::AvgDuration,
            SortColumn::AvgDuration => SortColumn::Name,
            SortColumn::Name => SortColumn::Type,
            SortColumn::Type => SortColumn::Calls,
        };
        self.sort_tools();
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn toggle_detail(&mut self) {
        self.show_detail = !self.show_detail;
    }

    pub fn close_detail(&mut self) {
        self.show_detail = false;
    }

    pub fn select_next(&mut self) {
        match self.effective_focus() {
            FocusPanel::Tools => {
                if !self.tool_metrics.is_empty() {
                    self.selected_index = (self.selected_index + 1) % self.tool_metrics.len();
                }
            }
            FocusPanel::Live => {
                let len = self.scraper_snapshot.live_sessions.len();
                if len > 0 {
                    self.live_selected_index = (self.live_selected_index + 1) % len;
                }
            }
        }
    }

    pub fn select_previous(&mut self) {
        match self.effective_focus() {
            FocusPanel::Tools => {
                if !self.tool_metrics.is_empty() {
                    self.selected_index = if self.selected_index == 0 {
                        self.tool_metrics.len() - 1
                    } else {
                        self.selected_index - 1
                    };
                }
            }
            FocusPanel::Live => {
                let len = self.scraper_snapshot.live_sessions.len();
                if len > 0 {
                    self.live_selected_index = if self.live_selected_index == 0 {
                        len - 1
                    } else {
                        self.live_selected_index - 1
                    };
                }
            }
        }
    }

    /// Cycle focus between Live and Tools, but auto-fallback to Tools when
    /// there are no live sessions.
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPanel::Tools if !self.scraper_snapshot.live_sessions.is_empty() => FocusPanel::Live,
            _ => FocusPanel::Tools,
        };
    }

    /// Resolves the user's `focus` field against actual data presence — if
    /// `focus = Live` but there are no sessions, fall back to Tools so
    /// navigation keeps working.
    pub fn effective_focus(&self) -> FocusPanel {
        if self.focus == FocusPanel::Live && self.scraper_snapshot.live_sessions.is_empty() {
            FocusPanel::Tools
        } else {
            self.focus
        }
    }

    pub fn selected_live_session(&self) -> Option<&crate::scraper::LiveSession> {
        self.scraper_snapshot
            .live_sessions
            .get(self.live_selected_index)
    }

    pub fn selected_tool(&self) -> Option<&ToolMetrics> {
        self.tool_metrics.get(self.selected_index)
    }

    #[allow(dead_code)]
    pub fn total_tokens(&self) -> u64 {
        self.token_metrics.input_tokens
            + self.token_metrics.output_tokens
            + self.token_metrics.cache_read_tokens
            + self.token_metrics.cache_creation_tokens
    }

    pub fn toggle_time_filter(&mut self) {
        self.time_filter = match self.time_filter {
            TimeFilter::LastHour => TimeFilter::Last24Hours,
            TimeFilter::Last24Hours => TimeFilter::Last7Days,
            TimeFilter::Last7Days => TimeFilter::AllTime,
            TimeFilter::AllTime => TimeFilter::LastHour,
        };
    }

    pub fn cache_reuse_rate(&self) -> f64 {
        let total_input = self.token_metrics.input_tokens + self.token_metrics.cache_read_tokens;
        if total_input == 0 {
            return 0.0;
        }
        (self.token_metrics.cache_read_tokens as f64 / total_input as f64) * 100.0
    }

    /// Kept for the existing test suite; the unified TUI table now renders
    /// builtin + MCP together with a TYPE column, so this isn't used by
    /// production code.
    #[allow(dead_code)]
    pub fn builtin_tools(&self) -> Vec<&ToolMetrics> {
        self.tool_metrics
            .iter()
            .filter(|t| t.is_builtin())
            .collect()
    }

    #[allow(dead_code)]
    pub fn mcp_tools(&self) -> Vec<&ToolMetrics> {
        self.tool_metrics.iter().filter(|t| t.is_mcp()).collect()
    }

    pub fn total_tool_calls(&self) -> u64 {
        self.tool_metrics.iter().map(|t| t.call_count).sum()
    }

    /// Get the last error message for the selected tool (if any)
    pub fn get_selected_tool_last_error(&self) -> Option<String> {
        let tool = self.selected_tool()?;
        if tool.error_count == 0 {
            return None;
        }
        self.storage
            .get_last_tool_error(&tool.tool_name)
            .ok()
            .flatten()
    }

    /// Format active time as human-readable string (e.g., "1h 23m")
    pub fn format_active_time(&self) -> String {
        let secs = self.session_metrics.active_time_secs;
        if secs == 0 {
            return "-".to_string();
        }
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        if hours > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}m", mins)
        }
    }

    /// Format the compaction summary for the header. Returns `None` when no
    /// compaction events are present in the current time window.
    pub fn format_compaction_summary(&self) -> Option<String> {
        if self.compaction_stats.count == 0 {
            return None;
        }

        let mut parts = vec![format!("Compactions: {}", self.compaction_stats.count)];
        let mut detail = Vec::new();

        if let Some(seen) = self.compaction_stats.last_seen {
            detail.push(format!("last {}", humanize_age(Utc::now() - seen)));
        }

        if let (Some(pre), Some(post)) = (
            self.compaction_stats.last_pre_tokens,
            self.compaction_stats.last_post_tokens,
        ) {
            let saved = pre.saturating_sub(post);
            if saved > 0 {
                detail.push(format!("-{}", humanize_tokens(saved)));
            }
        }

        if !detail.is_empty() {
            parts.push(format!("({})", detail.join(", ")));
        }

        Some(parts.join(" "))
    }

    /// Format API latency as human-readable string
    pub fn format_api_latency(&self) -> String {
        let ms = self.api_metrics.avg_latency_ms;
        if ms == 0.0 {
            return "-".to_string();
        }
        if ms < 1000.0 {
            format!("{}ms", ms as u64)
        } else {
            format!("{:.1}s", ms / 1000.0)
        }
    }

    /// Get the currently selected agent ID
    pub fn current_agent(&self) -> Option<&str> {
        self.detected_agents
            .get(self.selected_agent_index)
            .map(|s| s.as_str())
    }

    /// Cycle through detected agents
    pub fn cycle_agent(&mut self) {
        if !self.detected_agents.is_empty() {
            self.selected_agent_index =
                (self.selected_agent_index + 1) % self.detected_agents.len();
        }
    }

    /// Add a detected agent if not already in the list
    pub fn add_detected_agent(&mut self, agent_id: &str) {
        if !self.detected_agents.contains(&agent_id.to_string()) {
            self.detected_agents.push(agent_id.to_string());
        }
    }

    /// Cycle through detected projects: All -> Project1 -> Project2 -> ... -> All
    pub fn cycle_project(&mut self) {
        if self.detected_projects.is_empty() {
            self.project_filter = ProjectFilter::All;
            return;
        }

        self.project_filter = match &self.project_filter {
            ProjectFilter::All => {
                // Go to first project
                ProjectFilter::Project(self.detected_projects[0].name.clone())
            }
            ProjectFilter::Project(current) => {
                // Find current project index and go to next, or wrap to All
                let current_idx = self
                    .detected_projects
                    .iter()
                    .position(|p| &p.name == current);

                match current_idx {
                    Some(idx) if idx + 1 < self.detected_projects.len() => {
                        ProjectFilter::Project(self.detected_projects[idx + 1].name.clone())
                    }
                    _ => ProjectFilter::All,
                }
            }
        };
    }
}
