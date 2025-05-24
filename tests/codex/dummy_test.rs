// CLASSIFICATION: COMMUNITY
// Filename: dummy_test.rs v0.2
// Date Modified: 2025-05-27
// Author: Lukas Bower

//! Integration tests for the README_Codex.md and basic Codex CLI functionality.

use std::fs;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_codex_exists_and_non_empty() {
        let path = Path::new("docs/community/README_Codex.md");
        assert!(path.exists(), "README_Codex.md must exist at docs/community/README_Codex.md");
        let content = fs::read_to_string(path).expect("Failed to read README_Codex.md");
        assert!(!content.trim().is_empty(), "README_Codex.md should not be empty");
    }

    #[test]
    fn no_todo_placeholders_in_readme_codex() {
        let content = fs::read_to_string("docs/community/README_Codex.md").unwrap();
        assert!(!content.contains("TODO"), "README_Codex.md should not contain TODO placeholders");
    }

    #[test]
    fn cli_python_script_exists() {
        let path = Path::new("cli/cohcli.py");
        assert!(path.exists(), "cli/cohcli.py must exist");
    }

    #[test]
    fn cohcli_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata("cli/cohcli.py").expect("Failed to get metadata for cohcli.py");
        let perms = metadata.permissions();
        assert!(perms.mode() & 0o111 != 0, "cohcli.py should be executable");
    }
}
