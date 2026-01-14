// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate CAS provider behavior in the NineDoor namespace.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_cas::{CasManifest, CAS_MANIFEST_SCHEMA};
use ed25519_dalek::{Signature, SigningKey};
use nine_door::{CasConfig, InProcessConnection, NineDoor, NineDoorError};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};
use sha2::{Digest, Sha256};
use signature::Signer;

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("create session");
    client.version(MAX_MSIZE).expect("version handshake");
    client.attach(1, cohesix_ticket::Role::Queen).expect("attach queen");
    client
}

fn write_path(
    client: &mut InProcessConnection,
    fid: u32,
    path: &[String],
    payload: &[u8],
) -> Result<(), NineDoorError> {
    client.walk(1, fid, path)?;
    client.open(fid, OpenMode::write_append())?;
    client.write(fid, payload)?;
    client.clunk(fid)?;
    Ok(())
}

#[test]
fn cas_upload_and_read_roundtrip() {
    let key_bytes = [7u8; 32];
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key().to_bytes();
    let cas = CasConfig::enabled(16, false, true, Some(verifying_key), false);
    let server = NineDoor::new_with_cas_config(cas);
    let mut client = attach_queen(&server);

    let payload = b"0123456789abcdef";
    let digest = Sha256::digest(payload);
    let mut digest_bytes = [0u8; 32];
    digest_bytes.copy_from_slice(&digest);
    let chunk_hex = hex::encode(digest_bytes);

    let manifest = build_signed_manifest("42", 16, payload, digest_bytes, &key_bytes);
    let manifest_cbor = manifest.encode_signed().expect("encode manifest");

    let chunk_path = vec![
        "updates".to_owned(),
        "42".to_owned(),
        "chunks".to_owned(),
        chunk_hex.clone(),
    ];
    write_path(&mut client, 2, &chunk_path, payload).expect("upload chunk");

    let manifest_path = vec![
        "updates".to_owned(),
        "42".to_owned(),
        "manifest.cbor".to_owned(),
    ];
    write_path(&mut client, 3, &manifest_path, &manifest_cbor).expect("upload manifest");

    client.walk(1, 4, &chunk_path).expect("walk chunk");
    client
        .open(4, OpenMode::read_only())
        .expect("open chunk");
    let read_back = client.read(4, 0, MAX_MSIZE).expect("read chunk");
    assert_eq!(read_back, payload);
}

#[test]
fn cas_missing_signature_rejected() {
    let key_bytes = [5u8; 32];
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key().to_bytes();
    let cas = CasConfig::enabled(16, false, true, Some(verifying_key), false);
    let server = NineDoor::new_with_cas_config(cas);
    let mut client = attach_queen(&server);

    let payload = b"0123456789abcdef";
    let digest = Sha256::digest(payload);
    let mut digest_bytes = [0u8; 32];
    digest_bytes.copy_from_slice(&digest);
    let chunk_hex = hex::encode(digest_bytes);

    let manifest = CasManifest {
        schema: CAS_MANIFEST_SCHEMA.to_owned(),
        epoch: "100".to_owned(),
        chunk_bytes: 16,
        payload_bytes: 16,
        payload_sha256: digest_bytes,
        chunks: vec![digest_bytes],
        delta: None,
        signature: None,
    };
    let manifest_cbor = manifest.encode_signed().expect("encode manifest");

    let chunk_path = vec![
        "updates".to_owned(),
        "100".to_owned(),
        "chunks".to_owned(),
        chunk_hex,
    ];
    write_path(&mut client, 2, &chunk_path, payload).expect("upload chunk");

    let manifest_path = vec![
        "updates".to_owned(),
        "100".to_owned(),
        "manifest.cbor".to_owned(),
    ];
    let err = write_path(&mut client, 3, &manifest_path, &manifest_cbor)
        .expect_err("manifest rejected");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Permission),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn cas_hash_mismatch_rejected() {
    let cas = CasConfig::enabled(16, false, false, None, false);
    let server = NineDoor::new_with_cas_config(cas);
    let mut client = attach_queen(&server);

    let payload = b"0123456789abcdef";
    let mut digest_bytes = [0u8; 32];
    digest_bytes.copy_from_slice(&Sha256::digest(payload));
    digest_bytes[0] ^= 0xff;
    let chunk_hex = hex::encode(digest_bytes);

    let chunk_path = vec![
        "updates".to_owned(),
        "55".to_owned(),
        "chunks".to_owned(),
        chunk_hex,
    ];
    let err = write_path(&mut client, 2, &chunk_path, payload)
        .expect_err("chunk rejected");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Invalid),
        other => panic!("unexpected error: {other:?}"),
    }
}

fn build_signed_manifest(
    epoch: &str,
    chunk_bytes: u32,
    payload: &[u8],
    digest: [u8; 32],
    key_bytes: &[u8; 32],
) -> CasManifest {
    let payload_digest = Sha256::digest(payload);
    let mut payload_sha256 = [0u8; 32];
    payload_sha256.copy_from_slice(&payload_digest);
    let mut manifest = CasManifest {
        schema: CAS_MANIFEST_SCHEMA.to_owned(),
        epoch: epoch.to_owned(),
        chunk_bytes,
        payload_bytes: chunk_bytes as u64,
        payload_sha256,
        chunks: vec![digest],
        delta: None,
        signature: None,
    };
    let signing_key = SigningKey::from_bytes(key_bytes);
    let payload = manifest
        .signature_payload()
        .expect("signing payload");
    let signature: Signature = signing_key.sign(&payload);
    manifest.signature = Some(signature.to_bytes());
    manifest
}
