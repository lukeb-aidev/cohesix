// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate CAS bundle helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cas_tool::{
    build_bundle, chunk_payload, load_delta_base, write_bundle, CasTemplateConfig,
};
use ed25519_dalek::{Signature, SigningKey};
use signature::Verifier;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn template() -> CasTemplateConfig {
    CasTemplateConfig {
        chunk_bytes: 16,
        delta_allowed: true,
        signing_required: true,
    }
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
    let bundle = build_bundle("10", payload, &template(), None, Some(key_bytes))
        .expect("build bundle");
    let signature = bundle
        .manifest
        .signature
        .expect("signature missing");
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();
    let payload = bundle
        .manifest
        .signature_payload()
        .expect("payload");
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
    let delta_bundle = build_bundle("101", delta_payload, &template, Some(base), None)
        .expect("delta bundle");

    let mut hasher = Sha256::new();
    hasher.update(base_payload);
    hasher.update(delta_payload);
    let digest = hasher.finalize();
    assert_eq!(digest.as_slice(), delta_bundle.manifest.payload_sha256);
    assert!(delta_bundle.manifest.delta.is_some());
}
