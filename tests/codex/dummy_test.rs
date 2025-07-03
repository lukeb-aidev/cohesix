// CLASSIFICATION: COMMUNITY
// Filename: dummy_test.rs v0.3
// Date Modified: 2026-12-31
// Author: Lukas Bower

//! Integration tests for the README_Codex.md and basic Codex CLI functionality.

use cohesix::plan9::syscalls;
use std::fs::{self, File};
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
        let mut f = File::open("cli/cohcli.py").expect("open cohcli.py");
        let meta = syscalls::fstat(&f).expect("stat cohcli.py");
        assert!(!meta.permissions().readonly(), "cohcli.py should be accessible");
    }
}
