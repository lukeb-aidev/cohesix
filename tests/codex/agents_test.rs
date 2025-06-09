// CLASSIFICATION: COMMUNITY
// Filename: agents_test.rs v1.0
// Date Modified: 2025-05-27
// Author: Lukas Bower

//! Integration tests for the AGENTS.md file used by Codex agents.

use std::fs;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_file_exists_and_non_empty() {
        let path = Path::new("docs/community/architecture/AGENTS.md");
        assert!(path.exists(), "AGENTS.md must exist at docs/community/architecture/AGENTS.md");
        let content = fs::read_to_string(path).expect("Failed to read AGENTS.md");
        assert!(!content.trim().is_empty(), "AGENTS.md should not be empty");
    }

    #[test]
    fn no_todo_placeholders_in_agents_file() {
        let content = fs::read_to_string("docs/community/architecture/AGENTS.md").unwrap();
        assert!(!content.contains("TODO"), "AGENTS.md should not contain TODO placeholders");
    }

    #[test]
    fn contains_agent_entries() {
        // Expect at least one markdown list entry for an agent
        let content = fs::read_to_string("docs/community/architecture/AGENTS.md").unwrap();
        let agent_entries = content.lines()
            .filter(|line| line.trim_start().starts_with("- "))
            .count();
        assert!(agent_entries > 0, "AGENTS.md should list at least one agent entry");
    }
}
