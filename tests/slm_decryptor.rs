// CLASSIFICATION: COMMUNITY
// Filename: slm_decryptor.rs v0.1
// Date Modified: 2025-07-10
// Author: Cohesix Codex

use cohesix::slm::decryptor::{SLMDecryptor};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aead::Aead;
use std::fs;

#[test]
fn tampered_file_rejected() {
    let key = [0u8;32];
    fs::write("/tmp/test.slmcoh", b"invalid").unwrap();
    let res = SLMDecryptor::decrypt_model("/tmp/test.slmcoh", &key);
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
    fs::write("/tmp/ok.slmcoh", &data).unwrap();
    let (plain, _tok) = SLMDecryptor::decrypt_model("/tmp/ok.slmcoh", &key).unwrap();
    assert_eq!(plain, b"hello");
}
