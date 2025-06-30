// CLASSIFICATION: COMMUNITY
// Filename: decryptor.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-08-17
// Random token generation uses rand; this is skipped for UEFI builds.

//! AES-GCM encrypted SLM container loader.

use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aead::Aead;
use ring::signature::{UnparsedPublicKey, ED25519};
use std::fs;

/// Memory token representing access to a decrypted SLM payload.
#[derive(Clone, Copy, Debug)]
pub struct MemoryToken(pub u64);

pub struct SLMDecryptor;

impl SLMDecryptor {
    /// Decrypt a `.slmcoh` container with the provided key.
    pub fn decrypt_model(path: &str, key: &[u8]) -> anyhow::Result<(Vec<u8>, MemoryToken)> {
        let data = fs::read(path)?;
        if data.len() < 28 { return Err(anyhow::anyhow!("container too small")); }
        let nonce = Nonce::from_slice(&data[0..12]);
        let cipher = &data[12..];
        let aead = Aes256Gcm::new_from_slice(key)?;
        let plain = aead.decrypt(nonce, cipher).map_err(|e| anyhow::anyhow!(e))?;
        Ok((plain, MemoryToken(rand::random())))
    }

    /// Verify the container signature if present.
    pub fn verify_signature(path: &str) -> bool {
        let sig_path = format!("{path}.sig");
        if let (Ok(data), Ok(sig)) = (fs::read(path), fs::read(sig_path)) {
            if let Ok(pubkey) = fs::read("/keys/slm_signing.pub") {
                let key = UnparsedPublicKey::new(&ED25519, pubkey);
                return key.verify(&data, &sig).is_ok();
            }
        }
        false
    }

    /// Preload all models from the given directory.
    pub fn preload_from_dir(dir: &str, key: &[u8]) -> anyhow::Result<()> {
        if let Ok(entries) = fs::read_dir(dir) {
            for e in entries.flatten() {
                if e.path().extension().map(|s| s == "slmcoh").unwrap_or(false) {
                    let _ = Self::decrypt_model(e.path().to_str().unwrap(), key);
                }
            }
        }
        Ok(())
    }
}
