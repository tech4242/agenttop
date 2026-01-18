use anyhow::Result;
use chrono::{DateTime, Utc};
use duckdb::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetrics {
    pub tool_name: String,
    pub call_count: u64,
    pub last_call: Option<DateTime<Utc>>,
    pub avg_duration_ms: f64,
    pub success_count: u64,
    pub error_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub start_time: DateTime<Utc>,
    pub lines_of_code: i64,
    pub commit_count: u64,
    pub pr_count: u64,
    pub active_time_seconds: u64,
}

impl Default for SessionMetrics {
    fn default() -> Self {
        Self {
            start_time: Utc::now(),
            lines_of_code: 0,
            commit_count: 0,
            pr_count: 0,
            active_time_seconds: 0,
        }
    }
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
    GetToolMetrics(mpsc::Sender<Result<Vec<ToolMetrics>>>),
    GetTokenMetrics(mpsc::Sender<Result<TokenMetrics>>),
    GetSessionMetrics(mpsc::Sender<Result<SessionMetrics>>),
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

    pub fn get_tool_metrics(&self) -> Result<Vec<ToolMetrics>> {
        let (tx, rx) = mpsc::channel();
        self.sender.send(StorageCommand::GetToolMetrics(tx))?;
        rx.recv()?
    }

    pub fn get_token_metrics(&self) -> Result<TokenMetrics> {
        let (tx, rx) = mpsc::channel();
        self.sender.send(StorageCommand::GetTokenMetrics(tx))?;
        rx.recv()?
    }

    pub fn get_session_metrics(&self) -> Result<SessionMetrics> {
        let (tx, rx) = mpsc::channel();
        self.sender.send(StorageCommand::GetSessionMetrics(tx))?;
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
            StorageCommand::GetToolMetrics(tx) => {
                let _ = tx.send(storage.get_tool_metrics());
            }
            StorageCommand::GetTokenMetrics(tx) => {
                let _ = tx.send(storage.get_token_metrics());
            }
            StorageCommand::GetSessionMetrics(tx) => {
                let _ = tx.send(storage.get_session_metrics());
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

    fn get_tool_metrics(&self) -> Result<Vec<ToolMetrics>> {
        // Query that combines both legacy tool_events and new log_events tables
        // The log_events query filters by event_name at query time (not ingestion)
        // This matches both "tool_result" and "claude_code.tool_result"
        let mut stmt = self.conn.prepare(
            r#"
            WITH combined_events AS (
                -- Legacy tool_events table
                SELECT
                    tool_name,
                    timestamp,
                    duration_ms,
                    success
                FROM tool_events

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
                    END as success
                FROM log_events
                WHERE event_name LIKE '%tool_result'
            )
            SELECT
                tool_name,
                COUNT(*) as call_count,
                CAST(MAX(timestamp) AS VARCHAR) as last_call,
                AVG(duration_ms) as avg_duration_ms,
                SUM(CASE WHEN success THEN 1 ELSE 0 END) as success_count,
                SUM(CASE WHEN NOT success THEN 1 ELSE 0 END) as error_count
            FROM combined_events
            GROUP BY tool_name
            ORDER BY call_count DESC
            "#,
        )?;

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
                success_count: row.get::<_, i64>(4)? as u64,
                error_count: row.get::<_, i64>(5)? as u64,
            })
        })?;

        let mut metrics = Vec::new();
        for row in rows {
            metrics.push(row?);
        }
        Ok(metrics)
    }

    fn get_token_metrics(&self) -> Result<TokenMetrics> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT
                token_type,
                SUM(count) as total
            FROM token_usage
            GROUP BY token_type
            "#,
        )?;

        let mut metrics = TokenMetrics::default();

        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?;

        for row in rows {
            let (token_type, count) = row?;
            match token_type.as_str() {
                "input" => metrics.input_tokens = count,
                "output" => metrics.output_tokens = count,
                "cacheRead" => metrics.cache_read_tokens = count,
                "cacheCreation" => metrics.cache_creation_tokens = count,
                _ => {}
            }
        }

        // Get total cost
        let cost: f64 = self.conn.query_row(
            "SELECT COALESCE(SUM(cost_usd), 0) FROM cost_usage",
            [],
            |row| row.get(0),
        )?;
        metrics.total_cost_usd = cost;

        Ok(metrics)
    }

    fn get_session_metrics(&self) -> Result<SessionMetrics> {
        let mut metrics = SessionMetrics::default();

        // Get earliest timestamp as session start from both tables
        if let Ok(Some(start_str)) = self.conn.query_row(
            r#"
            SELECT MIN(ts) FROM (
                SELECT MIN(timestamp) as ts FROM tool_events
                UNION ALL
                SELECT MIN(timestamp) as ts FROM log_events
            )
            "#,
            [],
            |row| row.get::<_, Option<String>>(0),
        ) && let Ok(dt) = DateTime::parse_from_rfc3339(&start_str)
        {
            metrics.start_time = dt.with_timezone(&Utc);
        }

        // Get latest session metrics
        let mut stmt = self.conn.prepare(
            r#"
            SELECT metric_name, value
            FROM session_metrics
            WHERE id IN (
                SELECT MAX(id) FROM session_metrics GROUP BY metric_name
            )
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        for row in rows {
            let (metric_name, value) = row?;
            match metric_name.as_str() {
                // Support both old format (with _count suffix) and new format (without)
                "lines_of_code" => metrics.lines_of_code = value,
                "commit" | "commit_count" => metrics.commit_count = value as u64,
                "pull_request" | "pr_count" => metrics.pr_count = value as u64,
                "active_time" => metrics.active_time_seconds = value as u64,
                _ => {}
            }
        }

        Ok(metrics)
    }
}
