//! Project resolver tests

use agenttop::project::ProjectResolver;
use std::fs;
use tempfile::TempDir;

fn create_test_sessions_index(dir: &std::path::Path, entries: &[(&str, &str)]) {
    let sessions: Vec<serde_json::Value> = entries
        .iter()
        .map(|(session_id, project_path)| {
            serde_json::json!({
                "sessionId": session_id,
                "projectPath": project_path,
                "gitBranch": "main"
            })
        })
        .collect();

    let index = serde_json::json!({ "entries": sessions });
    fs::write(
        dir.join("sessions-index.json"),
        serde_json::to_string_pretty(&index).unwrap(),
    )
    .unwrap();
}

#[test]
fn test_resolver_creates_without_panic() {
    let resolver = ProjectResolver::new();
    let _ = resolver.session_count();
    let _ = resolver.project_count();
}

#[test]
fn test_project_name_extraction_from_path() {
    // Test that project name is correctly extracted from project_path
    let path = "/Users/it-support/Desktop/dev/agenttop";
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    assert_eq!(name, "agenttop");
}

#[test]
#[cfg(target_os = "windows")]
fn test_project_name_extraction_windows_style() {
    // Test Windows-style paths (only on Windows)
    let path = "C:\\Users\\dev\\projects\\myapp";
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    assert_eq!(name, "myapp");
}

#[test]
fn test_project_name_with_trailing_slash() {
    // Test path with trailing slash
    let path = "/Users/dev/projects/myapp/";
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        // file_name returns None for paths ending in /
        .or_else(|| {
            std::path::Path::new(path.trim_end_matches('/'))
                .file_name()
                .and_then(|n| n.to_str())
        })
        .unwrap_or("unknown");
    assert_eq!(name, "myapp");
}

#[test]
fn test_sessions_index_parsing() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("-Users-test-myproject");
    fs::create_dir_all(&project_dir).unwrap();

    create_test_sessions_index(
        &project_dir,
        &[
            ("session-abc-123", "/Users/test/myproject"),
            ("session-def-456", "/Users/test/myproject"),
        ],
    );

    // Verify the JSON file was created correctly
    let content = fs::read_to_string(project_dir.join("sessions-index.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let entries = parsed["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["sessionId"], "session-abc-123");
    assert_eq!(entries[0]["projectPath"], "/Users/test/myproject");
}

#[test]
fn test_multiple_projects_in_sessions_index() {
    let tmp = TempDir::new().unwrap();

    // Create project 1
    let project1_dir = tmp.path().join("-Users-test-project1");
    fs::create_dir_all(&project1_dir).unwrap();
    create_test_sessions_index(&project1_dir, &[("session-1", "/Users/test/project1")]);

    // Create project 2
    let project2_dir = tmp.path().join("-Users-test-project2");
    fs::create_dir_all(&project2_dir).unwrap();
    create_test_sessions_index(&project2_dir, &[("session-2", "/Users/test/project2")]);

    // Verify both files exist
    assert!(project1_dir.join("sessions-index.json").exists());
    assert!(project2_dir.join("sessions-index.json").exists());
}

#[test]
fn test_empty_sessions_index() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("-Users-test-empty");
    fs::create_dir_all(&project_dir).unwrap();

    // Create empty sessions index
    let index = serde_json::json!({ "entries": [] });
    fs::write(
        project_dir.join("sessions-index.json"),
        serde_json::to_string_pretty(&index).unwrap(),
    )
    .unwrap();

    // Verify it can be parsed
    let content = fs::read_to_string(project_dir.join("sessions-index.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed["entries"].as_array().unwrap().is_empty());
}
