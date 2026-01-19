use anyhow::Result;
use chrono::{DateTime, Utc};
use duckdb::{Connection, params};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

/// Known built-in Claude Code tools. Tools not in this list are classified as MCP tools.
const BUILTIN_TOOLS: &[&str] = &[
    "Read", "Write", "Edit", "Bash", "Glob", "Grep", "Task",
    "TodoRead", "TodoWrite", "WebFetch", "WebSearch", "Agent",
    "Skill", "AskUser", "AskUserQuestion", "MultiEdit", "NotebookEdit",
    "KillShell", "EnterPlanMode", "ExitPlanMode", "TaskOutput",
];

/// Regex to parse MCP tool names in format: mcp__<server>__<tool> or mcp__plugin_<plugin>_<server>__<tool>
static MCP_TOOL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^mcp__(?:plugin_\w+_)?(\w+)__(.+)$").unwrap());

/// Parsed MCP tool name containing server and tool components
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolInfo {
    pub server_name: String,
    pub tool_name: String,
}

/// Parse an MCP tool name to extract server and tool name.
/// Returns None if the name doesn't match MCP format.
///
/// # Examples
/// ```ignore
/// parse_mcp_tool_name("mcp__context7__resolve-library-id")
///   => Some(McpToolInfo { server_name: "context7", tool_name: "resolve-library-id" })
/// parse_mcp_tool_name("mcp__plugin_foo_myserver__my-tool")
///   => Some(McpToolInfo { server_name: "myserver", tool_name: "my-tool" })
/// parse_mcp_tool_name("Read")
///   => None
/// ```
pub fn parse_mcp_tool_name(name: &str) -> Option<McpToolInfo> {
    MCP_TOOL_REGEX.captures(name).map(|caps| McpToolInfo {
        server_name: caps.get(1).unwrap().as_str().to_string(),
        tool_name: caps.get(2).unwrap().as_str().to_string(),
    })
}

/// Check if a tool name is an MCP tool (uses mcp__ prefix)
pub fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with("mcp__")
}

/// Get a display-friendly name for a tool.
/// For MCP tools, returns "server:tool" format.
/// For built-in tools, returns the name as-is.
pub fn get_tool_display_name(name: &str) -> String {
    if let Some(info) = parse_mcp_tool_name(name) {
        format!("{}:{}", info.server_name, info.tool_name)
    } else {
        name.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetrics {
    pub tool_name: String,
    pub call_count: u64,
    pub last_call: Option<DateTime<Utc>>,
    pub avg_duration_ms: f64,
    pub min_duration_ms: f64,
    pub max_duration_ms: f64,
    pub success_count: u64,
    pub error_count: u64,
    /// Number of calls that were approved (decision = 'approved' or 'auto_approved')
    pub approved_count: u64,
    /// Number of calls that were rejected (decision = 'rejected')
    pub rejected_count: u64,
}

impl ToolMetrics {
    pub fn is_builtin(&self) -> bool {
        BUILTIN_TOOLS.contains(&self.tool_name.as_str())
    }

    pub fn is_mcp(&self) -> bool {
        // Any tool not in the built-in list is considered MCP
        !self.is_builtin()
    }

    /// Calculate approval rate as a percentage (0-100).
    /// Returns 100.0 if no decision data is available (assumes all approved).
    pub fn approval_rate(&self) -> f64 {
        let total_decisions = self.approved_count + self.rejected_count;
        if total_decisions == 0 {
            // No decision data available, assume all approved
            100.0
        } else {
            (self.approved_count as f64 / total_decisions as f64) * 100.0
        }
    }

    /// Get the MCP server name if this tool uses the mcp__server__tool format
    pub fn mcp_server_name(&self) -> Option<String> {
        parse_mcp_tool_name(&self.tool_name).map(|info| info.server_name)
    }

    /// Get a display-friendly version of the tool name.
    /// For MCP tools with mcp__server__tool format, returns "server:tool".
    /// Otherwise returns the name as-is.
    pub fn display_name(&self) -> String {
        get_tool_display_name(&self.tool_name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_cost_usd: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SessionMetrics {
    pub lines_of_code: i64,
    pub commit_count: u64,
    pub active_time_secs: u64,
}

/// API request metrics aggregated from api_request events
#[derive(Debug, Clone, Default)]
pub struct ApiMetrics {
    pub total_calls: u64,
    pub total_errors: u64,
    pub avg_latency_ms: f64,
    pub models: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEvent {
    pub timestamp: DateTime<Utc>,
    pub tool_name: String,
    pub success: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// Raw log event that stores all OTLP log records without filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub timestamp: DateTime<Utc>,
    pub event_name: Option<String>,
    pub body: Option<String>,
    pub attributes: HashMap<String, String>,
}

// Commands that can be sent to the storage actor
#[allow(dead_code)]
enum StorageCommand {
    RecordToolEvent(ToolEvent),
    RecordLogEvents(Vec<LogEvent>),
    RecordTokenUsage { token_type: String, count: u64 },
    RecordCost(f64),
    RecordSessionMetric { name: String, value: i64 },
    GetToolMetrics {
        since: Option<DateTime<Utc>>,
        tx: mpsc::Sender<Result<Vec<ToolMetrics>>>,
    },
    GetTokenMetrics {
        since: Option<DateTime<Utc>>,
        tx: mpsc::Sender<Result<TokenMetrics>>,
    },
    GetLastToolError {
        tool_name: String,
        tx: mpsc::Sender<Result<Option<String>>>,
    },
    GetSessionMetrics {
        since: Option<DateTime<Utc>>,
        tx: mpsc::Sender<Result<SessionMetrics>>,
    },
    GetApiMetrics {
        since: Option<DateTime<Utc>>,
        tx: mpsc::Sender<Result<ApiMetrics>>,
    },
    Shutdown,
}

// Thread-safe handle to the storage actor
#[derive(Clone)]
pub struct StorageHandle {
    sender: mpsc::Sender<StorageCommand>,
}

impl StorageHandle {
    pub fn new() -> Result<Self> {
        Self::spawn_actor(Storage::new()?)
    }

    /// Create an in-memory storage handle for testing.
    /// The database is isolated and won't persist or affect other tests.
    #[allow(dead_code)]
    pub fn new_in_memory() -> Result<Self> {
        Self::spawn_actor(Storage::new_in_memory()?)
    }

    fn spawn_actor(storage: Storage) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();

        // Spawn the storage actor thread
        thread::spawn(move || {
            if let Err(e) = run_storage_actor(storage, receiver) {
                tracing::error!("Storage actor error: {}", e);
            }
        });

        Ok(Self { sender })
    }

    /// Record a tool event to the legacy tool_events table.
    /// Note: This method is kept for backward compatibility. New code should use
    /// record_log_events() which stores all OTLP logs without filtering.
    #[allow(dead_code)]
    pub fn record_tool_event(&self, event: ToolEvent) {
        let _ = self.sender.send(StorageCommand::RecordToolEvent(event));
    }

    pub fn record_log_events(&self, events: Vec<LogEvent>) {
        let _ = self.sender.send(StorageCommand::RecordLogEvents(events));
    }

    pub fn record_token_usage(&self, token_type: &str, count: u64) {
        let _ = self.sender.send(StorageCommand::RecordTokenUsage {
            token_type: token_type.to_string(),
            count,
        });
    }

    pub fn record_cost(&self, cost_usd: f64) {
        let _ = self.sender.send(StorageCommand::RecordCost(cost_usd));
    }

    pub fn record_session_metric(&self, name: &str, value: i64) {
        let _ = self.sender.send(StorageCommand::RecordSessionMetric {
            name: name.to_string(),
            value,
        });
    }

    pub fn get_tool_metrics(&self, since: Option<DateTime<Utc>>) -> Result<Vec<ToolMetrics>> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(StorageCommand::GetToolMetrics { since, tx })?;
        rx.recv()?
    }

    pub fn get_token_metrics(&self, since: Option<DateTime<Utc>>) -> Result<TokenMetrics> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(StorageCommand::GetTokenMetrics { since, tx })?;
        rx.recv()?
    }

    pub fn get_last_tool_error(&self, tool_name: &str) -> Result<Option<String>> {
        let (tx, rx) = mpsc::channel();
        self.sender.send(StorageCommand::GetLastToolError {
            tool_name: tool_name.to_string(),
            tx,
        })?;
        rx.recv()?
    }

    pub fn get_session_metrics(&self, since: Option<DateTime<Utc>>) -> Result<SessionMetrics> {
        let (tx, rx) = mpsc::channel();
        self.sender.send(StorageCommand::GetSessionMetrics { since, tx })?;
        rx.recv()?
    }

    pub fn get_api_metrics(&self, since: Option<DateTime<Utc>>) -> Result<ApiMetrics> {
        let (tx, rx) = mpsc::channel();
        self.sender.send(StorageCommand::GetApiMetrics { since, tx })?;
        rx.recv()?
    }
}

fn run_storage_actor(storage: Storage, receiver: mpsc::Receiver<StorageCommand>) -> Result<()> {
    for cmd in receiver {
        match cmd {
            StorageCommand::RecordToolEvent(event) => {
                if let Err(e) = storage.record_tool_event(&event) {
                    tracing::error!("Failed to record tool event: {}", e);
                }
            }
            StorageCommand::RecordLogEvents(events) => {
                if let Err(e) = storage.insert_log_events(&events) {
                    tracing::error!("Failed to record log events: {}", e);
                }
            }
            StorageCommand::RecordTokenUsage { token_type, count } => {
                if let Err(e) = storage.record_token_usage(&token_type, count) {
                    tracing::error!("Failed to record token usage: {}", e);
                }
            }
            StorageCommand::RecordCost(cost) => {
                if let Err(e) = storage.record_cost(cost) {
                    tracing::error!("Failed to record cost: {}", e);
                }
            }
            StorageCommand::RecordSessionMetric { name, value } => {
                if let Err(e) = storage.record_session_metric(&name, value) {
                    tracing::error!("Failed to record session metric: {}", e);
                }
            }
            StorageCommand::GetToolMetrics { since, tx } => {
                let _ = tx.send(storage.get_tool_metrics(since));
            }
            StorageCommand::GetTokenMetrics { since, tx } => {
                let _ = tx.send(storage.get_token_metrics(since));
            }
            StorageCommand::GetLastToolError { tool_name, tx } => {
                let _ = tx.send(storage.get_last_tool_error(&tool_name));
            }
            StorageCommand::GetSessionMetrics { since, tx } => {
                let _ = tx.send(storage.get_session_metrics(since));
            }
            StorageCommand::GetApiMetrics { since, tx } => {
                let _ = tx.send(storage.get_api_metrics(since));
            }
            StorageCommand::Shutdown => break,
        }
    }

    Ok(())
}

struct Storage {
    conn: Connection,
}

impl Storage {
    /// Create storage with the default database path
    fn new() -> Result<Self> {
        let db_path = Self::db_path()?;

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Create an in-memory storage instance (for testing)
    #[allow(dead_code)]
    fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    fn db_path() -> Result<PathBuf> {
        let data_dir = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;
        Ok(data_dir.join("agenttop").join("metrics.duckdb"))
    }

    fn init_schema(&self) -> Result<()> {
        // Note: Using BIGINT with GENERATED ALWAYS AS IDENTITY for auto-increment in DuckDB
        self.conn.execute_batch(
            r#"
            CREATE SEQUENCE IF NOT EXISTS tool_events_seq;
            CREATE TABLE IF NOT EXISTS tool_events (
                id BIGINT DEFAULT nextval('tool_events_seq') PRIMARY KEY,
                timestamp TIMESTAMP NOT NULL,
                tool_name VARCHAR NOT NULL,
                success BOOLEAN NOT NULL,
                duration_ms BIGINT NOT NULL,
                error VARCHAR
            );

            CREATE SEQUENCE IF NOT EXISTS log_events_seq;
            CREATE TABLE IF NOT EXISTS log_events (
                id BIGINT DEFAULT nextval('log_events_seq') PRIMARY KEY,
                timestamp TIMESTAMP NOT NULL,
                event_name VARCHAR,
                body TEXT,
                attributes JSON
            );

            CREATE SEQUENCE IF NOT EXISTS token_usage_seq;
            CREATE TABLE IF NOT EXISTS token_usage (
                id BIGINT DEFAULT nextval('token_usage_seq') PRIMARY KEY,
                timestamp TIMESTAMP NOT NULL,
                token_type VARCHAR NOT NULL,
                count BIGINT NOT NULL
            );

            CREATE SEQUENCE IF NOT EXISTS cost_usage_seq;
            CREATE TABLE IF NOT EXISTS cost_usage (
                id BIGINT DEFAULT nextval('cost_usage_seq') PRIMARY KEY,
                timestamp TIMESTAMP NOT NULL,
                cost_usd DOUBLE NOT NULL
            );

            CREATE SEQUENCE IF NOT EXISTS session_metrics_seq;
            CREATE TABLE IF NOT EXISTS session_metrics (
                id BIGINT DEFAULT nextval('session_metrics_seq') PRIMARY KEY,
                timestamp TIMESTAMP NOT NULL,
                metric_name VARCHAR NOT NULL,
                value BIGINT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_tool_events_timestamp ON tool_events(timestamp);
            CREATE INDEX IF NOT EXISTS idx_tool_events_tool_name ON tool_events(tool_name);
            CREATE INDEX IF NOT EXISTS idx_log_events_timestamp ON log_events(timestamp);
            CREATE INDEX IF NOT EXISTS idx_log_events_event_name ON log_events(event_name);
            CREATE INDEX IF NOT EXISTS idx_token_usage_timestamp ON token_usage(timestamp);
            "#,
        )?;
        Ok(())
    }

    fn record_tool_event(&self, event: &ToolEvent) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tool_events (timestamp, tool_name, success, duration_ms, error) VALUES (?, ?, ?, ?, ?)",
            params![
                event.timestamp.to_rfc3339(),
                event.tool_name,
                event.success,
                event.duration_ms as i64,
                event.error,
            ],
        )?;
        Ok(())
    }

    fn insert_log_events(&self, events: &[LogEvent]) -> Result<()> {
        for event in events {
            let attributes_json = serde_json::to_string(&event.attributes)?;
            self.conn.execute(
                "INSERT INTO log_events (timestamp, event_name, body, attributes) VALUES (?, ?, ?, ?)",
                params![
                    event.timestamp.to_rfc3339(),
                    event.event_name,
                    event.body,
                    attributes_json,
                ],
            )?;
        }
        Ok(())
    }

    fn record_token_usage(&self, token_type: &str, count: u64) -> Result<()> {
        tracing::debug!("Token received: type={}, count={}", token_type, count);
        self.conn.execute(
            "INSERT INTO token_usage (timestamp, token_type, count) VALUES (?, ?, ?)",
            params![Utc::now().to_rfc3339(), token_type, count as i64],
        )?;
        Ok(())
    }

    fn record_cost(&self, cost_usd: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO cost_usage (timestamp, cost_usd) VALUES (?, ?)",
            params![Utc::now().to_rfc3339(), cost_usd],
        )?;
        Ok(())
    }

    fn record_session_metric(&self, metric_name: &str, value: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_metrics (timestamp, metric_name, value) VALUES (?, ?, ?)",
            params![Utc::now().to_rfc3339(), metric_name, value],
        )?;
        Ok(())
    }

    fn get_tool_metrics(&self, since: Option<DateTime<Utc>>) -> Result<Vec<ToolMetrics>> {
        // Query that combines both legacy tool_events and new log_events tables
        // The log_events query filters by event_name at query time (not ingestion)
        // This matches both "tool_result" and "claude_code.tool_result"
        let time_clause = since
            .map(|dt| format!("AND timestamp >= '{}'", dt.to_rfc3339()))
            .unwrap_or_default();

        let query = format!(
            r#"
            WITH combined_events AS (
                -- Legacy tool_events table (no decision tracking)
                SELECT
                    tool_name,
                    timestamp,
                    duration_ms,
                    success,
                    NULL as decision
                FROM tool_events
                WHERE 1=1 {time_clause}

                UNION ALL

                -- New log_events table with query-time filtering
                SELECT
                    COALESCE(json_extract_string(attributes, '$.tool_name'), 'unknown') as tool_name,
                    timestamp,
                    COALESCE(CAST(json_extract(attributes, '$.duration_ms') AS BIGINT), 0) as duration_ms,
                    CASE
                        WHEN json_extract_string(attributes, '$.success') IN ('true', '1') THEN true
                        WHEN json_extract(attributes, '$.success') = true THEN true
                        ELSE false
                    END as success,
                    json_extract_string(attributes, '$.decision') as decision
                FROM log_events
                WHERE event_name LIKE '%tool_result' {time_clause}
            )
            SELECT
                tool_name,
                COUNT(*) as call_count,
                CAST(MAX(timestamp) AS VARCHAR) as last_call,
                AVG(duration_ms) as avg_duration_ms,
                MIN(duration_ms) as min_duration_ms,
                MAX(duration_ms) as max_duration_ms,
                SUM(CASE WHEN success THEN 1 ELSE 0 END) as success_count,
                SUM(CASE WHEN NOT success THEN 1 ELSE 0 END) as error_count,
                SUM(CASE WHEN decision IN ('approved', 'auto_approved') THEN 1 ELSE 0 END) as approved_count,
                SUM(CASE WHEN decision = 'rejected' THEN 1 ELSE 0 END) as rejected_count
            FROM combined_events
            GROUP BY tool_name
            ORDER BY call_count DESC
            "#
        );

        let mut stmt = self.conn.prepare(&query)?;

        let rows = stmt.query_map([], |row| {
            let last_call_str: Option<String> = row.get(2)?;
            // DuckDB CAST(timestamp AS VARCHAR) produces "2026-01-18 21:03:57.123456"
            // We need to parse this format, not RFC3339
            let last_call = last_call_str.and_then(|s| {
                // Try RFC3339 first (for backwards compatibility with stored RFC3339 strings)
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
                    .or_else(|| {
                        // Try DuckDB's format: "2026-01-18 21:03:57.123456" or "2026-01-18 21:03:57"
                        chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f")
                            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S"))
                            .ok()
                            .map(|naive| naive.and_utc())
                    })
            });

            Ok(ToolMetrics {
                tool_name: row.get(0)?,
                call_count: row.get::<_, i64>(1)? as u64,
                last_call,
                avg_duration_ms: row.get(3)?,
                min_duration_ms: row.get(4)?,
                max_duration_ms: row.get(5)?,
                success_count: row.get::<_, i64>(6)? as u64,
                error_count: row.get::<_, i64>(7)? as u64,
                approved_count: row.get::<_, i64>(8)? as u64,
                rejected_count: row.get::<_, i64>(9)? as u64,
            })
        })?;

        let mut metrics = Vec::new();
        for row in rows {
            metrics.push(row?);
        }
        Ok(metrics)
    }

    fn get_token_metrics(&self, since: Option<DateTime<Utc>>) -> Result<TokenMetrics> {
        let time_clause = since
            .map(|dt| format!("WHERE timestamp >= '{}'", dt.to_rfc3339()))
            .unwrap_or_default();

        let query = format!(
            r#"
            SELECT
                token_type,
                SUM(count) as total
            FROM token_usage
            {time_clause}
            GROUP BY token_type
            "#
        );

        let mut stmt = self.conn.prepare(&query)?;

        let mut metrics = TokenMetrics::default();

        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?;

        for row in rows {
            let (token_type, count) = row?;
            match token_type.as_str() {
                "input" | "prompt_tokens" | "input_tokens" => metrics.input_tokens += count,
                "output" | "completion_tokens" | "output_tokens" => metrics.output_tokens += count,
                "cacheRead" | "cache_read" | "cache_hit" => metrics.cache_read_tokens += count,
                "cacheCreation" | "cache_creation" | "cache_write" => metrics.cache_creation_tokens += count,
                other => {
                    tracing::warn!("Unknown token type: {}", other);
                }
            }
        }

        // Get total cost
        let cost_clause = since
            .map(|dt| format!("WHERE timestamp >= '{}'", dt.to_rfc3339()))
            .unwrap_or_default();
        let cost_query = format!("SELECT COALESCE(SUM(cost_usd), 0) FROM cost_usage {cost_clause}");
        let cost: f64 = self.conn.query_row(&cost_query, [], |row| row.get(0))?;
        metrics.total_cost_usd = cost;

        Ok(metrics)
    }

    fn get_last_tool_error(&self, tool_name: &str) -> Result<Option<String>> {
        // Query for the last error from both legacy tool_events and log_events tables
        let query = r#"
            WITH errors AS (
                -- Legacy tool_events table
                SELECT timestamp, error as error_msg
                FROM tool_events
                WHERE tool_name = ? AND success = false AND error IS NOT NULL

                UNION ALL

                -- New log_events table
                SELECT timestamp, json_extract_string(attributes, '$.error') as error_msg
                FROM log_events
                WHERE event_name LIKE '%tool_result'
                  AND json_extract_string(attributes, '$.tool_name') = ?
                  AND json_extract_string(attributes, '$.success') NOT IN ('true', '1')
                  AND json_extract_string(attributes, '$.error') IS NOT NULL
            )
            SELECT error_msg
            FROM errors
            ORDER BY timestamp DESC
            LIMIT 1
        "#;

        let result: Result<String, _> = self.conn.query_row(query, params![tool_name, tool_name], |row| {
            row.get(0)
        });

        match result {
            Ok(msg) => Ok(Some(msg)),
            Err(duckdb::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn get_session_metrics(&self, since: Option<DateTime<Utc>>) -> Result<SessionMetrics> {
        let time_clause = since
            .map(|dt| format!("WHERE timestamp >= '{}'", dt.to_rfc3339()))
            .unwrap_or_default();

        let query = format!(
            r#"
            SELECT
                metric_name,
                SUM(value) as total
            FROM session_metrics
            {time_clause}
            GROUP BY metric_name
            "#
        );

        let mut stmt = self.conn.prepare(&query)?;
        let mut metrics = SessionMetrics::default();

        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        for row in rows {
            let (metric_name, value) = row?;
            match metric_name.as_str() {
                "lines_of_code" | "loc" => metrics.lines_of_code = value,
                "commits" | "commit_count" => metrics.commit_count = value as u64,
                "active_time" => metrics.active_time_secs = value as u64,
                _ => {}
            }
        }

        Ok(metrics)
    }

    /// Get API metrics from api_request and api_error events
    fn get_api_metrics(&self, since: Option<DateTime<Utc>>) -> Result<ApiMetrics> {
        let time_clause = since
            .map(|dt| format!("AND timestamp >= '{}'", dt.to_rfc3339()))
            .unwrap_or_default();

        // Query api_request events for call count, latency, and model breakdown
        let api_query = format!(
            r#"
            SELECT
                COUNT(*) as call_count,
                AVG(CAST(COALESCE(json_extract(attributes, '$.latency_ms'), json_extract(attributes, '$.duration_ms'), '0') AS DOUBLE)) as avg_latency,
                json_extract_string(attributes, '$.model') as model
            FROM log_events
            WHERE event_name LIKE '%api_request' {time_clause}
            GROUP BY model
            "#
        );

        let mut stmt = self.conn.prepare(&api_query)?;
        let mut metrics = ApiMetrics::default();

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)? as u64,
                row.get::<_, f64>(1).unwrap_or(0.0),
                row.get::<_, Option<String>>(2)?,
            ))
        })?;

        let mut total_latency_sum = 0.0;
        for row in rows {
            let (count, avg_latency, model) = row?;
            metrics.total_calls += count;
            total_latency_sum += avg_latency * count as f64;
            if let Some(m) = model {
                *metrics.models.entry(m).or_insert(0) += count;
            }
        }

        if metrics.total_calls > 0 {
            metrics.avg_latency_ms = total_latency_sum / metrics.total_calls as f64;
        }

        // Query api_error events for error count
        let error_query = format!(
            r#"
            SELECT COUNT(*) as error_count
            FROM log_events
            WHERE event_name LIKE '%api_error' {time_clause}
            "#
        );

        let error_count: i64 = self.conn.query_row(&error_query, [], |row| row.get(0))?;
        metrics.total_errors = error_count as u64;

        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mcp_tool_name_standard() {
        let result = parse_mcp_tool_name("mcp__context7__resolve-library-id");
        assert_eq!(
            result,
            Some(McpToolInfo {
                server_name: "context7".to_string(),
                tool_name: "resolve-library-id".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_mcp_tool_name_query_docs() {
        let result = parse_mcp_tool_name("mcp__context7__query-docs");
        assert_eq!(
            result,
            Some(McpToolInfo {
                server_name: "context7".to_string(),
                tool_name: "query-docs".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_mcp_tool_name_with_plugin() {
        let result = parse_mcp_tool_name("mcp__plugin_foo_myserver__my-tool");
        assert_eq!(
            result,
            Some(McpToolInfo {
                server_name: "myserver".to_string(),
                tool_name: "my-tool".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_mcp_tool_name_builtin_returns_none() {
        assert_eq!(parse_mcp_tool_name("Read"), None);
        assert_eq!(parse_mcp_tool_name("Write"), None);
        assert_eq!(parse_mcp_tool_name("Bash"), None);
    }

    #[test]
    fn test_parse_mcp_tool_name_invalid_format() {
        assert_eq!(parse_mcp_tool_name("mcp_tool"), None);
        assert_eq!(parse_mcp_tool_name("mcp__server"), None);
        assert_eq!(parse_mcp_tool_name("mcp__"), None);
    }

    #[test]
    fn test_is_mcp_tool_name() {
        assert!(is_mcp_tool_name("mcp__context7__resolve-library-id"));
        assert!(is_mcp_tool_name("mcp__server__tool"));
        assert!(!is_mcp_tool_name("Read"));
        assert!(!is_mcp_tool_name("mcp_tool"));
    }

    #[test]
    fn test_get_tool_display_name_mcp() {
        assert_eq!(
            get_tool_display_name("mcp__context7__resolve-library-id"),
            "context7:resolve-library-id"
        );
        assert_eq!(
            get_tool_display_name("mcp__context7__query-docs"),
            "context7:query-docs"
        );
    }

    #[test]
    fn test_get_tool_display_name_builtin() {
        assert_eq!(get_tool_display_name("Read"), "Read");
        assert_eq!(get_tool_display_name("Bash"), "Bash");
    }

    #[test]
    fn test_tool_metrics_is_mcp() {
        // Any non-builtin tool is considered MCP
        let mcp_tool = ToolMetrics {
            tool_name: "mcp__context7__query-docs".to_string(),
            call_count: 1,
            last_call: None,
            avg_duration_ms: 100.0,
            min_duration_ms: 50.0,
            max_duration_ms: 150.0,
            success_count: 1,
            error_count: 0,
            approved_count: 1,
            rejected_count: 0,
        };
        assert!(mcp_tool.is_mcp());
        assert!(!mcp_tool.is_builtin());
        assert_eq!(mcp_tool.mcp_server_name(), Some("context7".to_string()));
        assert_eq!(mcp_tool.display_name(), "context7:query-docs");

        // Generic MCP tool names (without mcp__ prefix) are also MCP
        let generic_mcp = ToolMetrics {
            tool_name: "context7".to_string(),
            call_count: 1,
            last_call: None,
            avg_duration_ms: 100.0,
            min_duration_ms: 50.0,
            max_duration_ms: 150.0,
            success_count: 1,
            error_count: 0,
            approved_count: 0,
            rejected_count: 0,
        };
        assert!(generic_mcp.is_mcp());
        assert!(!generic_mcp.is_builtin());
        assert_eq!(generic_mcp.display_name(), "context7"); // No transformation for non-standard format
    }

    #[test]
    fn test_tool_metrics_is_builtin() {
        let builtin_tool = ToolMetrics {
            tool_name: "Read".to_string(),
            call_count: 1,
            last_call: None,
            avg_duration_ms: 50.0,
            min_duration_ms: 25.0,
            max_duration_ms: 75.0,
            success_count: 1,
            error_count: 0,
            approved_count: 1,
            rejected_count: 0,
        };
        assert!(!builtin_tool.is_mcp());
        assert!(builtin_tool.is_builtin());
        assert_eq!(builtin_tool.mcp_server_name(), None);
        assert_eq!(builtin_tool.display_name(), "Read");
    }

    #[test]
    fn test_approval_rate() {
        // Tool with all approved
        let all_approved = ToolMetrics {
            tool_name: "Read".to_string(),
            call_count: 10,
            last_call: None,
            avg_duration_ms: 50.0,
            min_duration_ms: 25.0,
            max_duration_ms: 75.0,
            success_count: 10,
            error_count: 0,
            approved_count: 10,
            rejected_count: 0,
        };
        assert!((all_approved.approval_rate() - 100.0).abs() < 0.01);

        // Tool with some rejections
        let some_rejected = ToolMetrics {
            tool_name: "Bash".to_string(),
            call_count: 10,
            last_call: None,
            avg_duration_ms: 50.0,
            min_duration_ms: 25.0,
            max_duration_ms: 75.0,
            success_count: 8,
            error_count: 2,
            approved_count: 8,
            rejected_count: 2,
        };
        assert!((some_rejected.approval_rate() - 80.0).abs() < 0.01);

        // Tool with no decision data (should return 100%)
        let no_decisions = ToolMetrics {
            tool_name: "Edit".to_string(),
            call_count: 5,
            last_call: None,
            avg_duration_ms: 50.0,
            min_duration_ms: 25.0,
            max_duration_ms: 75.0,
            success_count: 5,
            error_count: 0,
            approved_count: 0,
            rejected_count: 0,
        };
        assert!((no_decisions.approval_rate() - 100.0).abs() < 0.01);
    }
}
