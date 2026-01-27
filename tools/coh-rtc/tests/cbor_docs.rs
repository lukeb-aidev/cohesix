// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Guard that CBOR schema docs match coh-rtc output.
// Author: Lukas Bower

use coh_rtc::codegen::cbor::telemetry_cbor_snippet;
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

#[test]
fn interfaces_cbor_snippet_matches_codegen() {
    let snippet = telemetry_cbor_snippet().expect("generate snippet");
    let interfaces_path = repo_path("docs/INTERFACES.md");
    let contents = fs::read_to_string(&interfaces_path).expect("read interfaces");

    let start_marker = "<!-- coh-rtc:telemetry-cbor:start -->";
    let end_marker = "<!-- coh-rtc:telemetry-cbor:end -->";
    let start = contents.find(start_marker).expect("start marker missing") + start_marker.len();
    let end = contents[start..]
        .find(end_marker)
        .map(|idx| start + idx)
        .expect("end marker missing");
    let extracted = contents[start..end].trim();

    assert_eq!(extracted, snippet.markdown.trim());
}
