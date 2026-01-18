use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::storage::{StorageHandle, TokenMetrics, ToolMetrics};

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
    pub selected_index: usize,
    pub sort_by: SortColumn,
    pub sort_ascending: bool,
    pub paused: bool,
    pub show_detail: bool,
    pub last_refresh: DateTime<Utc>,
    pub time_filter: TimeFilter,
}

impl App {
    pub fn new(storage: StorageHandle) -> Self {
        Self {
            storage,
            tool_metrics: Vec::new(),
            token_metrics: TokenMetrics::default(),
            selected_index: 0,
            sort_by: SortColumn::Calls,
            sort_ascending: false,
            paused: false,
            show_detail: false,
            last_refresh: Utc::now(),
            time_filter: TimeFilter::default(),
        }
    }

    pub fn refresh(&mut self) -> Result<()> {
        if self.paused {
            return Ok(());
        }

        self.tool_metrics = self.storage.get_tool_metrics(self.time_filter.since())?;
        self.token_metrics = self.storage.get_token_metrics(self.time_filter.since())?;
        self.last_refresh = Utc::now();

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
                    let primary = if ascending { primary } else { primary.reverse() };
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

    pub fn cache_hit_rate(&self) -> f64 {
        let total_input = self.token_metrics.input_tokens + self.token_metrics.cache_read_tokens;
        if total_input == 0 {
            return 0.0;
        }
        (self.token_metrics.cache_read_tokens as f64 / total_input as f64) * 100.0
    }

    pub fn total_tool_calls(&self) -> u64 {
        self.tool_metrics.iter().map(|t| t.call_count).sum()
    }

    /// Calculate overall success rate across all tools (percentage)
    pub fn overall_success_rate(&self) -> f64 {
        let total_calls: u64 = self.tool_metrics.iter().map(|t| t.call_count).sum();
        if total_calls == 0 {
            return 100.0;
        }
        let total_success: u64 = self.tool_metrics.iter().map(|t| t.success_count).sum();
        (total_success as f64 / total_calls as f64) * 100.0
    }

    /// Calculate average tool duration across all tools (weighted by call count)
    pub fn average_tool_duration(&self) -> f64 {
        let total_calls: u64 = self.tool_metrics.iter().map(|t| t.call_count).sum();
        if total_calls == 0 {
            return 0.0;
        }
        let weighted_sum: f64 = self
            .tool_metrics
            .iter()
            .map(|t| t.avg_duration_ms * t.call_count as f64)
            .sum();
        weighted_sum / total_calls as f64
    }
}
