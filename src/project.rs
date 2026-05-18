//! Project detection from Claude Code session data
//!
//! Maps session.id to project name by scanning ~/.claude/projects/*/sessions-index.json

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Session entry from sessions-index.json
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionEntry {
    session_id: String,
    project_path: String,
    #[allow(dead_code)]
    git_branch: Option<String>,
}

/// Sessions index file structure
#[derive(Debug, Deserialize)]
struct SessionsIndex {
    entries: Vec<SessionEntry>,
}

/// Resolved project info
#[derive(Debug, Clone)]
pub struct ResolvedProject {
    pub name: String,
    #[allow(dead_code)]
    pub path: String,
}

/// Maps session.id to project name by scanning ~/.claude/projects/*/sessions-index.json
pub struct ProjectResolver {
    /// Map of session_id -> project info
    session_to_project: HashMap<String, ResolvedProject>,
    /// List of all unique project names
    all_projects: Vec<String>,
}

impl ProjectResolver {
    /// Create a new ProjectResolver by scanning Claude Code's project directories
    pub fn new() -> Self {
        let mut resolver = Self {
            session_to_project: HashMap::new(),
            all_projects: Vec::new(),
        };
        resolver.scan_projects();
        resolver
    }

    /// Scan ~/.claude/projects/ for sessions-index.json files and build the mapping
    fn scan_projects(&mut self) {
        let claude_projects_dir = match Self::claude_projects_dir() {
            Some(dir) => dir,
            None => {
                tracing::debug!("Could not find ~/.claude/projects directory");
                return;
            }
        };

        tracing::debug!("Scanning Claude projects at: {:?}", claude_projects_dir);

        // Read all directories in ~/.claude/projects/
        let entries = match std::fs::read_dir(&claude_projects_dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("Failed to read Claude projects directory: {}", e);
                return;
            }
        };

        let mut unique_projects: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            // Look for sessions-index.json in each project directory
            let index_path = path.join("sessions-index.json");
            if !index_path.exists() {
                continue;
            }

            // Parse sessions-index.json
            match Self::parse_sessions_index(&index_path) {
                Ok(index) => {
                    for session_entry in index.entries {
                        // Extract project name from project_path (last component)
                        let project_name = std::path::Path::new(&session_entry.project_path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();

                        unique_projects.insert(project_name.clone());

                        self.session_to_project.insert(
                            session_entry.session_id,
                            ResolvedProject {
                                name: project_name,
                                path: session_entry.project_path,
                            },
                        );
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        "Failed to parse sessions-index.json at {:?}: {}",
                        index_path,
                        e
                    );
                }
            }
        }

        // Store sorted list of unique projects
        self.all_projects = unique_projects.into_iter().collect();
        self.all_projects.sort();

        tracing::info!(
            "ProjectResolver loaded {} sessions across {} projects",
            self.session_to_project.len(),
            self.all_projects.len()
        );
    }

    /// Parse a sessions-index.json file
    fn parse_sessions_index(path: &PathBuf) -> anyhow::Result<SessionsIndex> {
        let content = std::fs::read_to_string(path)?;
        let index: SessionsIndex = serde_json::from_str(&content)?;
        Ok(index)
    }

    /// Get the Claude projects directory path (~/.claude/projects/)
    fn claude_projects_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".claude").join("projects"))
    }

    /// Resolve a session.id to its project name
    pub fn resolve(&self, session_id: &str) -> Option<&ResolvedProject> {
        self.session_to_project.get(session_id)
    }

    /// Get all unique project names
    #[allow(dead_code)]
    pub fn get_all_projects(&self) -> &[String] {
        &self.all_projects
    }

    /// Refresh the project mappings by re-scanning
    #[allow(dead_code)]
    pub fn refresh(&mut self) {
        self.session_to_project.clear();
        self.all_projects.clear();
        self.scan_projects();
    }

    /// Get the number of sessions loaded
    #[allow(dead_code)]
    pub fn session_count(&self) -> usize {
        self.session_to_project.len()
    }

    /// Get the number of projects loaded
    #[allow(dead_code)]
    pub fn project_count(&self) -> usize {
        self.all_projects.len()
    }
}

impl Default for ProjectResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_parse_sessions_index() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("-Users-test-myproject");
        fs::create_dir_all(&project_dir).unwrap();

        create_test_sessions_index(&project_dir, &[("session-123", "/Users/test/myproject")]);

        let index = ProjectResolver::parse_sessions_index(&project_dir.join("sessions-index.json"))
            .unwrap();
        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.entries[0].session_id, "session-123");
        assert_eq!(index.entries[0].project_path, "/Users/test/myproject");
    }

    #[test]
    fn test_project_name_extraction() {
        // Test that project name is extracted from project_path
        let path = "/Users/it-support/Desktop/dev/agenttop";
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        assert_eq!(name, "agenttop");
    }

    #[test]
    fn test_resolver_default() {
        // Just exercise the constructor; values depend on the host's ~/.claude/projects dir.
        let resolver = ProjectResolver::default();
        let _ = resolver.session_count();
        let _ = resolver.project_count();
    }
}
