//! Live opencode (sst/opencode) sessions, scraped from its local SQLite DB.
//!
//! opencode persists conversations in `~/.local/share/opencode/opencode.db`.
//! We open it read-only and surface live sessions next to Claude Code in
//! the Live panel. PID liveness is matched by `cwd` against the running
//! process tree (sysinfo) — same approach abtop uses.
//!
//! Why local-scrape instead of OTLP: opencode upstream doesn't ship native
//! OTLP yet (just a community plugin, `DEVtheOPS/opencode-plugin-otel`),
//! so 99% of users get no telemetry. The DB is always there.
//!
//! Schema reverse-engineered from opencode 0.x:
//!   - `session`   (id, title, directory, version, time_created, time_updated, project_id)
//!   - `project`   (id, name, ...)
//!   - `message`   (id, session_id, data JSON, time_created)
//!
//! `message.data` is a JSON blob containing role, tokens.input/output/cache,
//! modelID, providerID.
//!
//! The schema isn't documented and may drift across opencode versions. We
//! wrap the query in defensive error handling so a schema change downgrades
//! to "no opencode sessions" rather than crashing the whole scraper.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::process::ProcessScanner;
use super::{LiveSession, SessionStatus};

const SESSION_QUERY: &str = r#"
SELECT
    s.id,
    COALESCE(s.title, '') as title,
    COALESCE(s.directory, '') as directory,
    s.time_updated,
    COALESCE(SUM(json_extract(m.data, '$.tokens.input')),  0) as input_tokens,
    COALESCE(SUM(json_extract(m.data, '$.tokens.output')), 0) as output_tokens,
    COALESCE(SUM(json_extract(m.data, '$.tokens.cache.read')),  0) as cache_read,
    COALESCE(SUM(json_extract(m.data, '$.tokens.cache.write')), 0) as cache_write,
    (
        SELECT json_extract(m2.data, '$.modelID')
        FROM message m2
        WHERE m2.session_id = s.id
          AND json_extract(m2.data, '$.role') = 'assistant'
        ORDER BY m2.time_created DESC LIMIT 1
    ) as model
FROM session s
LEFT JOIN message m
    ON m.session_id = s.id
    AND json_extract(m.data, '$.role') = 'assistant'
GROUP BY s.id
ORDER BY s.time_updated DESC
LIMIT 20
"#;

#[derive(Debug, Clone)]
struct DbRow {
    id: String,
    #[allow(dead_code)]
    title: String,
    directory: String,
    time_updated_ms: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read: u64,
    cache_write: u64,
    model: Option<String>,
}

/// Scan opencode's SQLite database and return live sessions whose `cwd`
/// matches a running opencode process. Returns an empty vec when the DB
/// doesn't exist (opencode not installed) or any query step fails.
pub fn scan(sys: &ProcessScanner) -> Vec<LiveSession> {
    let Some(db_path) = default_db_path() else {
        return Vec::new();
    };
    scan_at(sys, &db_path)
}

pub(crate) fn scan_at(sys: &ProcessScanner, db_path: &Path) -> Vec<LiveSession> {
    if !db_path.exists() {
        return Vec::new();
    }

    let rows = match read_sessions(db_path) {
        Ok(rows) => rows,
        Err(e) => {
            tracing::debug!("opencode DB read failed: {} (schema drift?)", e);
            return Vec::new();
        }
    };

    // Index running opencode processes by cwd. cwd may not be available on
    // all platforms; missing cwd just means we won't match that process.
    let opencode_pids: Vec<u32> = sys
        .snapshot()
        .values()
        .filter(|p| p.cmd.contains("opencode") || p.name.contains("opencode"))
        .map(|p| p.pid)
        .collect();
    let mut pid_by_cwd: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for pid in &opencode_pids {
        if let Some(proc) = sys.get(*pid)
            && let Some(cwd) = proc.cwd.as_deref()
        {
            pid_by_cwd.insert(cwd.to_string(), *pid);
        }
    }

    let mut out = Vec::new();
    for row in rows {
        // Only surface sessions whose directory matches a live process —
        // otherwise we'd show every historical opencode session forever.
        let Some(&pid) = pid_by_cwd.get(&row.directory) else {
            continue;
        };
        let proc = match sys.get(pid) {
            Some(p) => p,
            None => continue,
        };

        let project_name = Path::new(&row.directory)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| row.directory.clone());

        out.push(LiveSession {
            agent_id: "opencode",
            pid,
            session_id: row.id,
            cwd: row.directory,
            project_name,
            started_at_ms: row.time_updated_ms,
            // Status from a passive scrape is necessarily Waiting — we
            // don't have a stream of events to tell us "right now the
            // model is generating." If the user wants live status they
            // need the community OTLP plugin.
            status: SessionStatus::Waiting,
            model: row.model.unwrap_or_default(),
            // opencode's tokens.* fields are already cumulative — no model
            // window detection (provider-dependent), so leave context_*
            // empty.
            context_percent: None,
            context_window: None,
            latest_context_tokens: 0,
            current_task: String::new(),
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            cache_read_tokens: row.cache_read,
            cache_creation_tokens: row.cache_write,
            mem_mb: proc.rss_kb / 1024,
            children: Vec::new(),
            subagents: Vec::new(),
        });
    }
    out
}

fn default_db_path() -> Option<PathBuf> {
    // XDG-style: $XDG_DATA_HOME or ~/.local/share/opencode/opencode.db.
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))?;
    Some(base.join("opencode").join("opencode.db"))
}

fn read_sessions(db_path: &Path) -> rusqlite::Result<Vec<DbRow>> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // Make the connection extra-safe: opencode runs concurrently and may
    // be writing. SQLite WAL mode is normally enabled by opencode itself.
    let _ = conn.execute_batch("PRAGMA query_only = ON;");

    let mut stmt = conn.prepare(SESSION_QUERY)?;
    let rows = stmt.query_map([], |row| {
        Ok(DbRow {
            id: row.get::<_, String>(0)?,
            title: row.get::<_, String>(1).unwrap_or_default(),
            directory: row.get::<_, String>(2).unwrap_or_default(),
            time_updated_ms: row.get::<_, i64>(3).unwrap_or(0).max(0) as u64,
            input_tokens: row.get::<_, i64>(4).unwrap_or(0).max(0) as u64,
            output_tokens: row.get::<_, i64>(5).unwrap_or(0).max(0) as u64,
            cache_read: row.get::<_, i64>(6).unwrap_or(0).max(0) as u64,
            cache_write: row.get::<_, i64>(7).unwrap_or(0).max(0) as u64,
            model: row.get::<_, Option<String>>(8).unwrap_or(None),
        })
    })?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tempfile::tempdir;

    /// Build a minimal opencode-shape SQLite DB with one assistant message
    /// per session. Schema mirrors opencode 0.x (verified via abtop).
    fn make_fixture_db(dir: &Path) -> PathBuf {
        let path = dir.join("opencode.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                title TEXT,
                directory TEXT,
                version TEXT,
                time_created INTEGER,
                time_updated INTEGER,
                project_id TEXT
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                data TEXT NOT NULL,
                time_created INTEGER NOT NULL
            );
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES (?, ?, ?, ?, ?, ?, NULL)",
            params![
                "sess-1",
                "Building agenttop",
                "/tmp/agenttop-test-cwd",
                "0.1.0",
                1700000000_i64,
                1700001000_i64,
            ],
        )
        .unwrap();
        // One assistant message with tokens.
        let assistant_data = serde_json::json!({
            "role": "assistant",
            "modelID": "claude-sonnet-4-5",
            "tokens": {
                "input": 1500,
                "output": 800,
                "cache": { "read": 12000, "write": 200 }
            }
        });
        conn.execute(
            "INSERT INTO message VALUES (?, ?, ?, ?)",
            params![
                "msg-1",
                "sess-1",
                assistant_data.to_string(),
                1700000500_i64
            ],
        )
        .unwrap();
        path
    }

    #[test]
    fn returns_empty_when_db_missing() {
        let sys = ProcessScanner::new();
        let result = scan_at(&sys, Path::new("/nonexistent/opencode.db"));
        assert!(result.is_empty());
    }

    #[test]
    fn returns_empty_when_no_running_opencode_process() {
        // DB has a session for /tmp/agenttop-test-cwd, but there's no
        // running opencode process whose cwd matches → no live session.
        let tmp = tempdir().unwrap();
        let db_path = make_fixture_db(tmp.path());
        let sys = ProcessScanner::new();
        let result = scan_at(&sys, &db_path);
        assert!(
            result.is_empty(),
            "no matching live process should mean no live session"
        );
    }

    #[test]
    fn reads_session_and_aggregates_tokens() {
        // We can't easily fake a running opencode process from a unit test,
        // so this test goes through `read_sessions` directly to verify the
        // SQL aggregation logic. The PID-matching step is tested by the
        // returns_empty_when_no_running_opencode_process case.
        let tmp = tempdir().unwrap();
        let db_path = make_fixture_db(tmp.path());
        let rows = read_sessions(&db_path).unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.id, "sess-1");
        assert_eq!(r.directory, "/tmp/agenttop-test-cwd");
        assert_eq!(r.input_tokens, 1500);
        assert_eq!(r.output_tokens, 800);
        assert_eq!(r.cache_read, 12000);
        assert_eq!(r.cache_write, 200);
        assert_eq!(r.model.as_deref(), Some("claude-sonnet-4-5"));
    }

    #[test]
    fn ignores_user_role_messages_in_token_sum() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("opencode.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE session (
                id TEXT PRIMARY KEY, title TEXT, directory TEXT, version TEXT,
                time_created INTEGER, time_updated INTEGER, project_id TEXT
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                data TEXT NOT NULL, time_created INTEGER NOT NULL
            );
            INSERT INTO session VALUES ('s1', '', '/x', '0', 0, 0, NULL);
            INSERT INTO message VALUES ('m1', 's1', '{"role":"user","tokens":{"input":999}}', 1);
            INSERT INTO message VALUES ('m2', 's1', '{"role":"assistant","tokens":{"input":100,"output":50}}', 2);
            "#,
        )
        .unwrap();
        drop(conn);

        let rows = read_sessions(&path).unwrap();
        assert_eq!(rows.len(), 1);
        // user.tokens.input (999) must NOT appear in the assistant-only sum.
        assert_eq!(rows[0].input_tokens, 100);
        assert_eq!(rows[0].output_tokens, 50);
    }

    #[test]
    fn schema_drift_returns_empty_rather_than_crashing() {
        // Build a DB with a totally different shape — read_sessions should
        // error cleanly and scan_at should return empty, not panic.
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("opencode.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE unrelated (id INTEGER); INSERT INTO unrelated VALUES (1);",
        )
        .unwrap();
        drop(conn);

        // read_sessions returns Err.
        let err = read_sessions(&path);
        assert!(err.is_err());

        // scan_at swallows it.
        let sys = ProcessScanner::new();
        let result = scan_at(&sys, &path);
        assert!(result.is_empty());
    }
}
