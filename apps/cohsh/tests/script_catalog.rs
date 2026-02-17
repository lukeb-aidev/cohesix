// Copyright © 2026 Lukas Bower
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
        "CMD:bind".to_owned(),
        "CMD:cat".to_owned(),
        "CMD:detach".to_owned(),
        "CMD:echo".to_owned(),
        "CMD:help".to_owned(),
        "CMD:kill".to_owned(),
        "CMD:lifecycle".to_owned(),
        "CMD:log".to_owned(),
        "CMD:pool".to_owned(),
        "CMD:ls".to_owned(),
        "CMD:ping".to_owned(),
        "CMD:quit".to_owned(),
        "CMD:spawn".to_owned(),
        "CMD:telemetry".to_owned(),
        "CMD:tail".to_owned(),
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
        "busy_backpressure.coh:0bfb61ee8f7a5eb87b9401e6e85d197b6ea2e134cf0c3b1c940a09afec8f7cd6"
            .to_owned(),
        "cas_roundtrip.coh:f6afb2dc79c414d04cc33f9a6d29da2f665959d2436c8d24baaa11f14b2ca98f"
            .to_owned(),
        "host_absent.coh:6c69c9db1a503fed9fb69636c4028b26432711eb62054e0d115a0f1cb6e89354"
            .to_owned(),
        "host_sidecar_mock.coh:9532e7596d512c9089359e569090316ebf54a03c5cb0b35ab26dd5b22acbf3cb"
            .to_owned(),
        "lifecycle_basic.coh:0cf1a06fa6e52b5e1bc516d83d979559083b69d03af703bb83b90c1643ad0eff"
            .to_owned(),
        "lifecycle_drain_spool.coh:3d31fbdd784b5c59b3acfc7485261426f47cfeab8cc845263a23e3708772c6fe"
            .to_owned(),
        "lifecycle_reboot_resume.coh:45471021214d5945361560b849611cc115bf714ed97822d3b583ed2a191290c0"
            .to_owned(),
        "model_cas_bind.coh:56bb0c3908dc28d2f04381977d9dcaf9e98a179cca8904c88e9ee89a031c7311"
            .to_owned(),
        "observe_watch.coh:cb6ae987049eb345c4dd7501fc657bfe6bd99c6f235593a5aab82e85ce3d4a9e"
            .to_owned(),
        "peft_roundtrip.coh:3657c33c1e8e8660fad19e774495622bbe7620bfe7da684f09ed75e25ca5bcea"
            .to_owned(),
        "policy_gate.coh:6d7ad6b827641b2578843e3db0bedcd9ffcf911f48520a6e0478d468860f239f"
            .to_owned(),
        "replay_journal.coh:1ff3cc6ac006c47fe8a6f914fb5b079d352a41691c2b5da34cf6441fba4d5a8f"
            .to_owned(),
        "rest_control_plane_smoke.coh:d9caa417b846e6af631a608d9f3b61dcc35d88f55ebe66a60ab5c75a72e6eb84"
            .to_owned(),
        "root_cut_basic.coh:13b16b64658f4042e13e69cadf7e8a51dae210a5f3e69f373dda08d819f59d8c"
            .to_owned(),
        "run_demo.coh:0ab45aa7b6b1446fa2d043ff70377bb86685120c5f09d26fe715f3fdd39ad4de"
            .to_owned(),
        "session_lifecycle.coh:6f80125a34b1b2bf7959f76615cf4cc349d1414957627310c238e9231623413b"
            .to_owned(),
        "session_pool.coh:ba523237c1933fbce09df879e871e4269013b74b5b8f8a046adbd2de00e7395e"
            .to_owned(),
        "shard_1k.coh:d4ce5a6d7a8dff0b2d26382cdec8edc2486ac20b6766b6a51f009e811691620a"
            .to_owned(),
        "sidecar_integration.coh:4cb5dcf403712defa9016f4bcc4bf27b891df6a1dce646dc304acf4b5f7cad74"
            .to_owned(),
        "smp_parity.coh:168c1785b657bd41644d1dd479619bac6f0ef6d82a9da774a4f7e08077420729"
            .to_owned(),
        "tcp_basic.coh:619970b6ff14332bbef80f704c117b4471653bb75f7a6187b27d93fbc16415a7"
            .to_owned(),
        "telemetry_push_create.coh:d8612c5d812d4e5d20e6dd3973569194b201d4465eadab9a6e61deb203231dea"
            .to_owned(),
        "telemetry_ring.coh:92ca2e5604e7b123e1aa026a2ca7b27fe6f914cc8cb280faff6189e6c35e1513"
            .to_owned(),
    ]);
    assert_eq!(results, expected);
    assert_eq!(hashes.len(), results.len());
}
