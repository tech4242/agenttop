use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::storage::{SessionMetrics, StorageHandle, TokenMetrics, ToolMetrics};

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
    pub selected_index: usize,
    pub sort_by: SortColumn,
    pub sort_ascending: bool,
    pub paused: bool,
    pub show_detail: bool,
    pub last_refresh: DateTime<Utc>,
}

impl App {
    pub fn new(storage: StorageHandle) -> Self {
        Self {
            storage,
            tool_metrics: Vec::new(),
            token_metrics: TokenMetrics::default(),
            session_metrics: SessionMetrics::default(),
            selected_index: 0,
            sort_by: SortColumn::Calls,
            sort_ascending: false,
            paused: false,
            show_detail: false,
            last_refresh: Utc::now(),
        }
    }

    pub fn refresh(&mut self) -> Result<()> {
        if self.paused {
            return Ok(());
        }

        self.tool_metrics = self.storage.get_tool_metrics()?;
        self.token_metrics = self.storage.get_token_metrics()?;
        self.session_metrics = self.storage.get_session_metrics()?;
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
        match self.sort_by {
            SortColumn::Calls => {
                self.tool_metrics.sort_by(|a, b| {
                    if ascending {
                        a.call_count.cmp(&b.call_count)
                    } else {
                        b.call_count.cmp(&a.call_count)
                    }
                });
            }
            SortColumn::LastCall => {
                self.tool_metrics.sort_by(|a, b| {
                    if ascending {
                        a.last_call.cmp(&b.last_call)
                    } else {
                        b.last_call.cmp(&a.last_call)
                    }
                });
            }
            SortColumn::AvgDuration => {
                self.tool_metrics.sort_by(|a, b| {
                    let ord = a
                        .avg_duration_ms
                        .partial_cmp(&b.avg_duration_ms)
                        .unwrap_or(std::cmp::Ordering::Equal);
                    if ascending { ord } else { ord.reverse() }
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

    pub fn session_duration(&self) -> chrono::Duration {
        Utc::now() - self.session_metrics.start_time
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

    pub fn productivity_multiplier(&self) -> f64 {
        // Rough estimate: lines of code / hours worked
        let hours = self.session_duration().num_seconds() as f64 / 3600.0;
        if hours < 0.01 {
            return 0.0;
        }
        self.session_metrics.lines_of_code.abs() as f64 / hours
    }
}
