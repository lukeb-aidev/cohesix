// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Lock cohsh .coh grammar coverage for existing regression scripts.
// Author: Lukas Bower

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use cohesix_ticket::{Role, TicketKey, TicketToken, TicketVerb};
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
        "CMD:smp".to_owned(),
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
fn shard_regression_uses_distinct_admitted_role_slots() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cohsh/shard_1k.coh");
    let content = fs::read_to_string(path).expect("read shard regression script");
    let tokens =
        tokenize_script(BufReader::new(content.as_bytes())).expect("tokenize shard script");
    let spawns: Vec<&str> = tokens
        .iter()
        .map(String::as_str)
        .filter(|token| token.starts_with("spawn "))
        .collect();

    assert_eq!(
        spawns,
        vec!["spawn heartbeat ticks=10 ttl_s=60", "spawn lora"]
    );
    let rendered = tokens.join("\n");
    for path in [
        "/shard/13/worker/worker-1/telemetry",
        "/worker/worker-1/telemetry",
        "/shard/1c/worker/worker-2/telemetry",
        "/worker/worker-2/telemetry",
    ] {
        assert!(
            rendered.contains(path),
            "missing shard or alias path {path}"
        );
    }
}

#[test]
fn telemetry_regression_uses_explicit_bandwidth_fixture_before_workload() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cohsh/telemetry_ring.coh");
    let content = fs::read_to_string(path).expect("read telemetry regression script");
    let tokens =
        tokenize_script(BufReader::new(content.as_bytes())).expect("tokenize telemetry script");
    let key = TicketKey::from_secret("bootstrap");

    let (quota_idx, quota_token) = tokens
        .iter()
        .enumerate()
        .filter_map(|(idx, token)| {
            let encoded = token.strip_prefix("attach queen ")?;
            let decoded = TicketToken::decode(encoded, &key).ok()?;
            (decoded.claims().quotas.bandwidth_bytes == Some(8)).then_some((idx, decoded))
        })
        .next()
        .expect("explicit bandwidth fixture ticket");
    let claims = quota_token.claims();
    assert_eq!(claims.role, Role::Queen);
    assert_eq!(claims.quotas.bandwidth_bytes, Some(8));
    assert_eq!(claims.quotas.cursor_resumes, None);
    assert_eq!(claims.quotas.cursor_advances, None);
    assert_eq!(
        claims
            .scopes
            .iter()
            .map(|scope| (scope.path.as_str(), scope.verb, scope.rate_per_s))
            .collect::<Vec<_>>(),
        vec![
            ("/log", TicketVerb::Read, 0),
            ("/queen", TicketVerb::Write, 0),
            ("/shard", TicketVerb::Write, 0),
        ]
    );

    assert_eq!(tokens[quota_idx + 1], "EXPECT OK");
    assert_eq!(tokens[quota_idx + 2], "echo forbidden > /log/queen.log");
    assert_eq!(tokens[quota_idx + 3], "EXPECT ERR");
    assert_eq!(tokens[quota_idx + 4], "EXPECT SUBSTR EPERM");
    assert_eq!(tokens[quota_idx + 5], "cat /log/queen.log");
    assert_eq!(tokens[quota_idx + 6], "EXPECT ERR");
    assert_eq!(tokens[quota_idx + 7], "EXPECT SUBSTR ELIMIT");
    assert_eq!(tokens[quota_idx + 8], "detach");
    assert_eq!(tokens[quota_idx + 9], "EXPECT OK");

    let log_cat_positions = tokens
        .iter()
        .enumerate()
        .filter_map(|(idx, token)| (token == "cat /log/queen.log").then_some(idx))
        .collect::<Vec<_>>();
    assert_eq!(log_cat_positions, vec![quota_idx + 5]);
    let tail_idx = tokens
        .iter()
        .position(|token| token == "tail /log/queen.log")
        .expect("operational log tail");
    let spawn_idx = tokens
        .iter()
        .position(|token| token.starts_with("spawn heartbeat "))
        .expect("operational telemetry spawn");
    assert!(quota_idx < spawn_idx);
    assert!(spawn_idx < tail_idx);
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
        "cas_fixture_signature_rejected.coh:1191d3e9fd32a5af24c6382b79d7dd183c30aac503958250b3760cd55462e323"
            .to_owned(),
        "cas_roundtrip.coh:3c4f9c57ba91c2910b69155a36676b220c7aa889923dfadc7e554fbfcf88d427"
            .to_owned(),
        "converge_target_activity.coh:a013b830df74aec56efc723005c7391465fdefcdb219b9f75dc720b98f453404"
            .to_owned(),
        "converge_worker.coh:93a947c051021ea88e56af66758e0aa7fd2de168adc66d2107e04a0209a3abf1"
            .to_owned(),
        "host_absent.coh:f86f0aee6f7199034b7414d55788edbe2900ac8754f68296edf975816a7919df"
            .to_owned(),
        "host_sidecar_mock.coh:9532e7596d512c9089359e569090316ebf54a03c5cb0b35ab26dd5b22acbf3cb"
            .to_owned(),
        "lifecycle_basic.coh:0cf1a06fa6e52b5e1bc516d83d979559083b69d03af703bb83b90c1643ad0eff"
            .to_owned(),
        "lifecycle_drain_spool.coh:3d31fbdd784b5c59b3acfc7485261426f47cfeab8cc845263a23e3708772c6fe"
            .to_owned(),
        "lifecycle_reboot_resume.coh:45471021214d5945361560b849611cc115bf714ed97822d3b583ed2a191290c0"
            .to_owned(),
        "model_cas_bind.coh:b36d084b87963b584926e0ecbf5e1b652d8a991f85f6407f4c0a4bfbe9ae6fa5"
            .to_owned(),
        "observe_watch.coh:cb6ae987049eb345c4dd7501fc657bfe6bd99c6f235593a5aab82e85ce3d4a9e"
            .to_owned(),
        "peft_roundtrip.coh:3657c33c1e8e8660fad19e774495622bbe7620bfe7da684f09ed75e25ca5bcea"
            .to_owned(),
        "policy_gate.coh:e6fe5b2fa36a36d9805b893a441a9eccf596046b635c55c2977c65a628ab7ee6"
            .to_owned(),
        "replay_journal.coh:8152fc5c9a3cbbf80689267f0e4fc07c89b5c883279d38cb5a30621038b514cb"
            .to_owned(),
        "rest_control_plane_smoke.coh:d9caa417b846e6af631a608d9f3b61dcc35d88f55ebe66a60ab5c75a72e6eb84"
            .to_owned(),
        "root_cut_basic.coh:13b16b64658f4042e13e69cadf7e8a51dae210a5f3e69f373dda08d819f59d8c"
            .to_owned(),
        "run_demo.coh:0ab45aa7b6b1446fa2d043ff70377bb86685120c5f09d26fe715f3fdd39ad4de"
            .to_owned(),
        "session_lifecycle.coh:bb0ef5f00b4e0198b24c3a74df1860d779145625133c8c3832fa25a8cfc06b43"
            .to_owned(),
        "session_pool.coh:ba523237c1933fbce09df879e871e4269013b74b5b8f8a046adbd2de00e7395e"
            .to_owned(),
        "shard_1k.coh:18cb0d8b12f71488f3874650c0739beed554d7775998a47e56f3f5e374d84574"
            .to_owned(),
        "sidecar_integration.coh:7371003a707d038727841bc7e0e6d005767d048ecdd83806400d9687ad316aa3"
            .to_owned(),
        "smp_parity.coh:168c1785b657bd41644d1dd479619bac6f0ef6d82a9da774a4f7e08077420729"
            .to_owned(),
        "tcp_basic.coh:619970b6ff14332bbef80f704c117b4471653bb75f7a6187b27d93fbc16415a7"
            .to_owned(),
        "telemetry_push_create.coh:5fd750c00e702d1660c35a96b141f067dd5fa5de14f11720524f3a1ef0cc154c"
            .to_owned(),
        "telemetry_ring.coh:e2eb77dd05985279182a59c027132dcb84353b9d4cf81b5beb30dbf43f6c6698"
            .to_owned(),
        "worker_host_model.coh:971fad19faabe4ffd7f322f61944b9bb548810229d3617ab65090765a996122e"
            .to_owned(),
    ]);
    assert_eq!(results, expected);
    assert_eq!(hashes.len(), results.len());
}
