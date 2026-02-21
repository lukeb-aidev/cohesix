// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Ensure cohsh-core doc snippets match fixtures and docs.
// Author: Lukas Bower

use std::fs;
use std::path::PathBuf;

use cohsh_core::docs::{render_console_grammar_doc, render_ticket_policy_doc};
use sha2::{Digest, Sha256};

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn read_fixture_hash(path: &PathBuf) -> String {
    let contents = fs::read_to_string(path).expect("fixture missing");
    contents
        .lines()
        .map(str::trim)
        .rfind(|line| line.len() == 64 && line.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or_else(|| panic!("fixture hash missing: {}", path.display()))
        .to_string()
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[test]
fn grammar_snippet_hash_matches_fixture() {
    let rendered = render_console_grammar_doc();
    let expected = read_fixture_hash(&fixture_path("grammar.sha256"));
    let actual = hash_bytes(rendered.as_bytes());
    assert_eq!(actual, expected, "grammar snippet hash drift");

    let docs_path = repo_root()
        .join("docs")
        .join("snippets")
        .join("cohsh_grammar.md");
    let docs_bytes = fs::read(&docs_path).expect("cohsh grammar snippet missing");
    let docs_hash = hash_bytes(&docs_bytes);
    assert_eq!(docs_hash, expected, "docs snippet hash drift");
}

#[test]
fn ticket_policy_snippet_hash_matches_fixture() {
    let rendered = render_ticket_policy_doc();
    let expected = read_fixture_hash(&fixture_path("ticket_policy.sha256"));
    let actual = hash_bytes(rendered.as_bytes());
    assert_eq!(actual, expected, "ticket policy snippet hash drift");

    let docs_path = repo_root()
        .join("docs")
        .join("snippets")
        .join("cohsh_ticket_policy.md");
    let docs_bytes = fs::read(&docs_path).expect("cohsh ticket policy snippet missing");
    let docs_hash = hash_bytes(&docs_bytes);
    assert_eq!(docs_hash, expected, "docs snippet hash drift");
}
