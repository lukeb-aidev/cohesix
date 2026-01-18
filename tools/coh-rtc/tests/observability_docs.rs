// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Guard that observability docs match coh-rtc output.
// Author: Lukas Bower

use coh_rtc::codegen::{hash_bytes, DocFragments};
use coh_rtc::ir::{load_manifest, serialize_manifest};
use std::fs;
use std::path::PathBuf;

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(path)
}

fn extract_snippet<'a>(contents: &'a str, start_marker: &str, end_marker: &str) -> &'a str {
    let start = contents
        .find(start_marker)
        .expect("start marker missing")
        + start_marker.len();
    let end = contents[start..]
        .find(end_marker)
        .map(|idx| start + idx)
        .expect("end marker missing");
    contents[start..end].trim()
}

fn generated_docs() -> DocFragments {
    let manifest_path = repo_path("configs/root_task.toml");
    let manifest = load_manifest(&manifest_path).expect("load manifest");
    let resolved = serialize_manifest(&manifest).expect("serialize manifest");
    let manifest_hash = hash_bytes(&resolved);
    DocFragments::from_manifest(&manifest, &manifest_hash)
}

#[test]
fn interfaces_observability_snippet_matches_codegen() {
    let docs = generated_docs();
    let interfaces_path = repo_path("docs/INTERFACES.md");
    let contents = fs::read_to_string(&interfaces_path).expect("read interfaces");
    let extracted = extract_snippet(
        &contents,
        "<!-- coh-rtc:observability-interfaces:start -->",
        "<!-- coh-rtc:observability-interfaces:end -->",
    );
    assert_eq!(extracted, docs.observability_interfaces_md.trim());
}

#[test]
fn security_observability_snippet_matches_codegen() {
    let docs = generated_docs();
    let security_path = repo_path("docs/SECURITY.md");
    let contents = fs::read_to_string(&security_path).expect("read security");
    let extracted = extract_snippet(
        &contents,
        "<!-- coh-rtc:observability-security:start -->",
        "<!-- coh-rtc:observability-security:end -->",
    );
    assert_eq!(extracted, docs.observability_security_md.trim());
}

#[test]
fn security_ticket_quota_snippet_matches_codegen() {
    let docs = generated_docs();
    let security_path = repo_path("docs/SECURITY.md");
    let contents = fs::read_to_string(&security_path).expect("read security");
    let extracted = extract_snippet(
        &contents,
        "<!-- coh-rtc:ticket-quotas:start -->",
        "<!-- coh-rtc:ticket-quotas:end -->",
    );
    assert_eq!(extracted, docs.ticket_quotas_md.trim());
}

#[test]
fn userland_ticket_quota_snippet_matches_codegen() {
    let docs = generated_docs();
    let userland_path = repo_path("docs/USERLAND_AND_CLI.md");
    let contents = fs::read_to_string(&userland_path).expect("read userland");
    let extracted = extract_snippet(
        &contents,
        "<!-- coh-rtc:ticket-quotas:start -->",
        "<!-- coh-rtc:ticket-quotas:end -->",
    );
    assert_eq!(extracted, docs.ticket_quotas_md.trim());
}
