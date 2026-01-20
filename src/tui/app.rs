use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::providers::PROVIDER_REGISTRY;
use crate::storage::{ApiMetrics, SessionMetrics, StorageHandle, TokenMetrics, ToolMetrics};

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
}

pub struct App {
    storage: StorageHandle,
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
}

impl App {
    pub fn new(storage: StorageHandle) -> Self {
        Self {
            storage,
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
        }
    }

    pub fn refresh(&mut self) -> Result<()> {
        if self.paused {
            return Ok(());
        }

        self.tool_metrics = self.storage.get_tool_metrics(self.time_filter.since())?;
        self.token_metrics = self.storage.get_token_metrics(self.time_filter.since())?;
        self.session_metrics = self.storage.get_session_metrics(self.time_filter.since())?;
        self.api_metrics = self.storage.get_api_metrics(self.time_filter.since())?;
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

        // Now add all detected agents
        for agent_id in new_agents {
            self.add_detected_agent(agent_id);
        }

        // Sort the tools
        self.sort_tools();

        // Ensure selected index is valid
        if !self.tool_metrics.is_empty() && self.selected_index >= self.tool_metrics.len() {
            self.selected_index = self.tool_metrics.len() - 1;
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
        }
    }

    pub fn toggle_sort(&mut self) {
        self.sort_by = match self.sort_by {
            SortColumn::Calls => SortColumn::LastCall,
            SortColumn::LastCall => SortColumn::AvgDuration,
            SortColumn::AvgDuration => SortColumn::Name,
            SortColumn::Name => SortColumn::Calls,
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
        if !self.tool_metrics.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.tool_metrics.len();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.tool_metrics.is_empty() {
            self.selected_index = if self.selected_index == 0 {
                self.tool_metrics.len() - 1
            } else {
                self.selected_index - 1
            };
        }
    }

    pub fn reset_stats(&mut self) {
        // Clear old data and reset selection
        self.selected_index = 0;
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

    pub fn builtin_tools(&self) -> Vec<&ToolMetrics> {
        self.tool_metrics
            .iter()
            .filter(|t| t.is_builtin())
            .collect()
    }

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
}
