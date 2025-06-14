// CLASSIFICATION: COMMUNITY
// Filename: slm_validate_decryptor.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-22
// SPDX-License-Identifier: Apache-2.0
// SLM Action: validate
// Target: decryptor

use cohesix::slm::decryptor::{SLMDecryptor};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aead::Aead;
use std::fs;
use tempfile::TempDir;

#[test]
fn tampered_file_rejected() {
    let key = [0u8;32];
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.slmcoh");
    fs::write(&path, b"invalid").unwrap();
    let res = SLMDecryptor::decrypt_model(path.to_str().unwrap(), &key);
    assert!(res.is_err());
}

#[test]
fn decrypts_valid_container() {
    let key = [0u8;32];
    let aead = Aes256Gcm::new_from_slice(&key).unwrap();
    let nonce = Nonce::from_slice(b"unique_nonce");
    let ct = aead.encrypt(nonce, b"hello".as_ref()).unwrap();
    let mut data = Vec::from(b"unique_nonce" as &[u8]);
    data.extend_from_slice(&ct);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ok.slmcoh");
    fs::write(&path, &data).unwrap();
    let (plain, _tok) = SLMDecryptor::decrypt_model(path.to_str().unwrap(), &key).unwrap();
    assert_eq!(plain, b"hello");
}
