// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate CAS bundle helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cas_tool::{
    build_bundle, chunk_payload, load_delta_base, load_template_config, load_upload_manifest,
    validate_manifest_capacity, write_bundle, CasTemplateConfig,
};
use cohesix_cas::{CasManifest, CAS_MANIFEST_MAX_CHUNKS, CAS_MANIFEST_SCHEMA};
use ed25519_dalek::{Signature, SigningKey};
use sha2::{Digest, Sha256};
use signature::Verifier;
use std::path::PathBuf;
use tempfile::TempDir;

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("apps directory")
        .parent()
        .expect("repository root")
        .join(path)
}

fn template() -> CasTemplateConfig {
    CasTemplateConfig {
        chunk_bytes: 16,
        max_chunks: CAS_MANIFEST_MAX_CHUNKS,
        delta_allowed: true,
        signing_required: true,
    }
}

#[test]
fn exact_manifest_chunk_capacity_is_accepted() {
    let payload = vec![0x5au8; 16 * CAS_MANIFEST_MAX_CHUNKS];
    let bundle = build_bundle("8", &payload, &template(), None, Some([9u8; 32]))
        .expect("exact-capacity bundle");
    assert_eq!(bundle.chunks.len(), CAS_MANIFEST_MAX_CHUNKS);
}

#[test]
fn dedicated_exact_eight_chunk_fixture_reaches_the_boundary() {
    let mut payload = std::fs::read(repo_path("tests/fixtures/cas/max_chunks_v1.txt"))
        .expect("read exact-capacity source fixture");
    let mut fixture_template = template();
    fixture_template.chunk_bytes = 128;
    let target_len = fixture_template.chunk_bytes * CAS_MANIFEST_MAX_CHUNKS;
    assert!(
        payload.len() <= target_len,
        "source fixture exceeds test capacity"
    );
    let tail_len = target_len - payload.len();
    payload.extend((0..tail_len).map(|index| (((index * 73 + 19) % 251) + 1) as u8));
    let bundle = build_bundle("8", &payload, &fixture_template, None, Some([9u8; 32]))
        .expect("build dedicated exact-capacity bundle");
    assert_eq!(bundle.manifest.payload_bytes, target_len as u64);
    assert_eq!(bundle.chunks.len(), CAS_MANIFEST_MAX_CHUNKS);
    let unique_digests = bundle
        .chunks
        .iter()
        .map(|chunk| chunk.digest)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(unique_digests.len(), CAS_MANIFEST_MAX_CHUNKS);
}

#[test]
fn over_manifest_chunk_capacity_is_rejected_before_bundle_creation() {
    let payload = vec![0x5au8; 16 * (CAS_MANIFEST_MAX_CHUNKS + 1)];
    let err = build_bundle("9", &payload, &template(), None, None)
        .expect_err("over-capacity bundle must fail");
    assert!(err
        .to_string()
        .contains("payload exceeds selected CAS manifest capacity"));
    assert!(err
        .root_cause()
        .to_string()
        .contains("CAS manifest capacity exceeded: payload_bytes=144 chunk_bytes=16 chunks=9 max_chunks=8 max_payload_bytes=128"));
}

#[test]
fn in_capacity_unsigned_bundle_still_requires_the_template_key() {
    let payload = vec![0x5au8; 16 * CAS_MANIFEST_MAX_CHUNKS];
    let err = build_bundle("8", &payload, &template(), None, None)
        .expect_err("in-capacity unsigned bundle must fail closed");
    assert_eq!(err.to_string(), "signing key required by template");
}

#[test]
fn direct_library_template_limit_bypass_is_rejected() {
    let mut invalid = template();
    invalid.max_chunks = CAS_MANIFEST_MAX_CHUNKS + 1;
    let payload = vec![0x5au8; 16 * (CAS_MANIFEST_MAX_CHUNKS + 1)];
    let err = build_bundle("9", &payload, &invalid, None, Some([9u8; 32]))
        .expect_err("direct library caller must not override manifest-v1 capacity");
    assert_eq!(
        err.to_string(),
        "template max_chunks 9 does not match manifest-v1 maximum 8"
    );
}

#[test]
fn foreign_manifest_chunk_capacity_is_rejected_locally() {
    let err = validate_manifest_capacity(
        1_152,
        128,
        CAS_MANIFEST_MAX_CHUNKS + 1,
        CAS_MANIFEST_MAX_CHUNKS,
    )
    .expect_err("foreign over-capacity manifest must fail");
    assert_eq!(
        err.to_string(),
        "CAS manifest capacity exceeded: payload_bytes=1152 chunk_bytes=128 chunks=9 max_chunks=8 max_payload_bytes=1024"
    );
}

#[test]
fn generated_template_limits_are_loaded_and_legacy_templates_default_safely() {
    let temp_dir = TempDir::new().expect("tempdir");
    let current_path = temp_dir.path().join("current.json");
    std::fs::write(
        &current_path,
        r#"{
            "chunk_bytes": 128,
            "delta": {},
            "limits": {"max_chunks": 8, "max_payload_bytes": 1024},
            "signature": null
        }"#,
    )
    .expect("write current template");
    let current = load_template_config(&current_path).expect("load current template");
    assert_eq!(current.max_chunks, CAS_MANIFEST_MAX_CHUNKS);

    let legacy_path = temp_dir.path().join("legacy.json");
    std::fs::write(
        &legacy_path,
        r#"{"chunk_bytes":128,"delta":null,"signature":null}"#,
    )
    .expect("write legacy template");
    let legacy = load_template_config(&legacy_path).expect("load legacy template");
    assert_eq!(legacy.max_chunks, CAS_MANIFEST_MAX_CHUNKS);
}

#[test]
fn template_limit_mismatch_is_rejected() {
    let temp_dir = TempDir::new().expect("tempdir");
    let path = temp_dir.path().join("invalid.json");
    std::fs::write(
        &path,
        r#"{
            "chunk_bytes": 128,
            "delta": null,
            "limits": {"max_chunks": 9, "max_payload_bytes": 1152},
            "signature": null
        }"#,
    )
    .expect("write invalid template");
    let err = load_template_config(&path).expect_err("over-limit template must fail");
    assert!(err
        .to_string()
        .contains("template max_chunks 9 does not match manifest-v1 maximum 8"));
}

#[test]
fn template_limit_below_manifest_contract_is_rejected() {
    let temp_dir = TempDir::new().expect("tempdir");
    let path = temp_dir.path().join("invalid.json");
    std::fs::write(
        &path,
        r#"{
            "chunk_bytes": 128,
            "delta": null,
            "limits": {"max_chunks": 7, "max_payload_bytes": 896},
            "signature": null
        }"#,
    )
    .expect("write invalid template");
    let err = load_template_config(&path).expect_err("under-limit template must fail");
    assert!(err
        .to_string()
        .contains("template max_chunks 7 does not match manifest-v1 maximum 8"));
}

#[test]
fn over_capacity_upload_manifest_is_rejected_before_chunk_reads() {
    let temp_dir = TempDir::new().expect("tempdir");
    let bundle_dir = temp_dir.path().join("bundle");
    std::fs::create_dir_all(bundle_dir.join("chunks")).expect("create empty chunks dir");
    let manifest = CasManifest {
        schema: CAS_MANIFEST_SCHEMA.to_owned(),
        epoch: "9".to_owned(),
        chunk_bytes: 128,
        payload_bytes: 128 * 9,
        payload_sha256: [0u8; 32],
        chunks: vec![[0u8; 32]; CAS_MANIFEST_MAX_CHUNKS + 1],
        delta: None,
        signature: None,
    };
    std::fs::write(
        bundle_dir.join("manifest.cbor"),
        manifest.encode_signed().expect("encode manifest"),
    )
    .expect("write manifest");

    let err = load_upload_manifest(&bundle_dir)
        .expect_err("over-capacity manifest must fail before missing chunks are read");
    assert!(err
        .to_string()
        .contains("bundle exceeds CAS manifest-v1 target capacity"));
    assert!(err
        .root_cause()
        .to_string()
        .contains("CAS manifest capacity exceeded: payload_bytes=1152 chunk_bytes=128 chunks=9 max_chunks=8 max_payload_bytes=1024"));
}

#[test]
fn chunk_payload_hashes() {
    let payload = b"0123456789abcdef0123456789abcdef";
    let chunks = chunk_payload(payload, 16).expect("chunk payload");
    assert_eq!(chunks.len(), 2);
    for chunk in chunks {
        let digest = Sha256::digest(&chunk.data);
        assert_eq!(digest.as_slice(), chunk.digest);
    }
}

#[test]
fn build_bundle_signs_manifest() {
    let payload = b"0123456789abcdef";
    let key_bytes = [9u8; 32];
    let bundle =
        build_bundle("10", payload, &template(), None, Some(key_bytes)).expect("build bundle");
    let signature = bundle.manifest.signature.expect("signature missing");
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();
    let payload = bundle.manifest.signature_payload().expect("payload");
    let signature = Signature::from_bytes(&signature);
    verifying_key
        .verify(&payload, &signature)
        .expect("signature verify");
}

#[test]
fn delta_bundle_hashes_with_base() {
    let base_payload = b"aaaaaaaaaaaaaaaa";
    let delta_payload = b"bbbbbbbbbbbbbbbb";
    let template = CasTemplateConfig {
        signing_required: false,
        ..template()
    };

    let base_bundle =
        build_bundle("100", base_payload, &template, None, None).expect("base bundle");
    let temp_dir = TempDir::new().expect("tempdir");
    let base_dir = temp_dir.path().join("base");
    write_bundle(&base_bundle, &base_dir).expect("write base bundle");

    let base = load_delta_base(&base_dir).expect("load base");
    let delta_bundle =
        build_bundle("101", delta_payload, &template, Some(base), None).expect("delta bundle");

    let mut hasher = Sha256::new();
    hasher.update(base_payload);
    hasher.update(delta_payload);
    let digest = hasher.finalize();
    assert_eq!(digest.as_slice(), delta_bundle.manifest.payload_sha256);
    assert!(delta_bundle.manifest.delta.is_some());
}
