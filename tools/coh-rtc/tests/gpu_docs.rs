// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Guard that GPU breadcrumb docs match coh-rtc output.
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
    let start = contents.find(start_marker).expect("start marker missing") + start_marker.len();
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
fn interfaces_gpu_breadcrumbs_snippet_matches_codegen() {
    let docs = generated_docs();
    let interfaces_path = repo_path("docs/INTERFACES.md");
    let contents = fs::read_to_string(&interfaces_path).expect("read interfaces");
    let extracted = extract_snippet(
        &contents,
        "<!-- coh-rtc:gpu-breadcrumbs:start -->",
        "<!-- coh-rtc:gpu-breadcrumbs:end -->",
    );
    assert_eq!(extracted, docs.gpu_breadcrumbs_md.trim());
}
