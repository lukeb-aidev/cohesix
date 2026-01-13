// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Lock cohsh .coh grammar coverage for existing regression scripts.
// Author: Lukas Bower

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use cohsh::{tokenize_script, validate_script};
use sha2::{Digest, Sha256};

fn script_paths() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cohsh");
    let mut paths = Vec::new();
    for entry in fs::read_dir(root).expect("read scripts/cohsh") {
        let entry = entry.expect("read_dir entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("coh") {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

fn record_features(content: &str, features: &mut BTreeSet<String>) {
    for raw_line in content.lines() {
        let trimmed = raw_line.trim_end();
        let without_comment = trimmed
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(trimmed);
        let line = without_comment.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("EXPECT") {
            let selector = rest.trim();
            if selector.starts_with("OK") {
                features.insert("EXPECT OK".to_owned());
            } else if selector.starts_with("ERR") {
                features.insert("EXPECT ERR".to_owned());
            } else if selector.starts_with("SUBSTR") {
                features.insert("EXPECT SUBSTR".to_owned());
            } else if selector.starts_with("NOT") {
                features.insert("EXPECT NOT".to_owned());
            } else {
                features.insert("EXPECT <invalid>".to_owned());
            }
            continue;
        }
        if line.starts_with("WAIT") {
            features.insert("WAIT".to_owned());
            continue;
        }
        if let Some(cmd) = line.split_whitespace().next() {
            features.insert(format!("CMD:{cmd}"));
        }
    }
}

fn token_hash(content: &str) -> String {
    let tokens = tokenize_script(BufReader::new(content.as_bytes())).expect("tokenize script");
    let rendered = tokens.join("\n");
    let mut hasher = Sha256::new();
    hasher.update(rendered.as_bytes());
    hex::encode(hasher.finalize())
}

#[test]
fn parses_all_existing_scripts() {
    for path in script_paths() {
        let file = File::open(&path).expect("open script");
        validate_script(BufReader::new(file)).expect("script should parse");
    }
}

#[test]
fn script_feature_inventory_is_stable() {
    let mut features = BTreeSet::new();
    for path in script_paths() {
        let content = fs::read_to_string(&path).expect("read script");
        record_features(&content, &mut features);
    }
    let expected = BTreeSet::from([
        "CMD:attach".to_owned(),
        "CMD:cat".to_owned(),
        "CMD:detach".to_owned(),
        "CMD:echo".to_owned(),
        "CMD:help".to_owned(),
        "CMD:log".to_owned(),
        "CMD:ls".to_owned(),
        "CMD:quit".to_owned(),
        "CMD:spawn".to_owned(),
        "EXPECT ERR".to_owned(),
        "EXPECT OK".to_owned(),
        "EXPECT SUBSTR".to_owned(),
        "WAIT".to_owned(),
    ]);
    assert_eq!(features, expected);
}

#[test]
fn script_token_stream_is_stable() {
    let mut hashes = BTreeSet::new();
    let mut results = BTreeSet::new();
    for path in script_paths() {
        let content = fs::read_to_string(&path).expect("read script");
        let hash = token_hash(&content);
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap()
            .to_owned();
        results.insert(format!("{name}:{hash}"));
        hashes.insert(hash);
    }
    let expected = BTreeSet::from([
        "9p_batch.coh:e9bb34b9eb3c59c3122321f2ca9069bc5343eaae4d6764da27d699f84fa1d8fa"
            .to_owned(),
        "boot_v0.coh:8cead851b286c62cde383098a2da802c78d7d324d792ed43bad880ed8cbea3e4"
            .to_owned(),
        "cas_roundtrip.coh:aed77e239f8731255c2713647a5d849c57138651d4e4e6c9f3dd17dffcab15c9"
            .to_owned(),
        "host_absent.coh:2fa2a736b03716ca96080dc2be5d2bf59bb077f41c8ece646c86dcf3cd56901f"
            .to_owned(),
        "host_sidecar_mock.coh:9a3d2a016f7946f48a1a3dc10684d688d14ac774a1556d69ebe05b9cdaa2d633"
            .to_owned(),
        "observe_watch.coh:1155ef2d36586a464370b4e201c15268ac7bdcc526fee6ce0d09490e0302eddf"
            .to_owned(),
        "policy_gate.coh:6d7ad6b827641b2578843e3db0bedcd9ffcf911f48520a6e0478d468860f239f"
            .to_owned(),
        "tcp_basic.coh:619970b6ff14332bbef80f704c117b4471653bb75f7a6187b27d93fbc16415a7"
            .to_owned(),
        "telemetry_ring.coh:6402e4b80f374648c317da9d35a1ae2c7cda90719f4d7d68e82d0722c5d1bb7f"
            .to_owned(),
    ]);
    assert_eq!(results, expected);
    assert_eq!(hashes.len(), results.len());
}
