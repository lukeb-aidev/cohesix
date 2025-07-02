// CLASSIFICATION: COMMUNITY
// Filename: decryptor.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-08-17
// Random token generation uses rand; this is skipped for UEFI builds.

use crate::prelude::*;
use crate::utils::tiny_ed25519::TinyEd25519;
use crate::utils::tiny_rng::TinyRng;
use crate::{coh_error, CohError};
use aead::Aead;
/// AES-GCM encrypted SLM container loader.
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use std::fs;

/// Memory token representing access to a decrypted SLM payload.
#[derive(Clone, Copy, Debug)]
pub struct MemoryToken(pub u64);

pub struct SLMDecryptor;

impl SLMDecryptor {
    /// Decrypt a `.slmcoh` container with the provided key.
    pub fn decrypt_model(path: &str, key: &[u8]) -> Result<(Vec<u8>, MemoryToken), CohError> {
        let data = fs::read(path)?;
        if data.len() < 28 {
            return Err(coh_error!("container too small"));
        }
        let nonce = Nonce::from_slice(&data[0..12]);
        let cipher = &data[12..];
        let aead = Aes256Gcm::new_from_slice(key)?;
        let plain = aead.decrypt(nonce, cipher).map_err(|e| coh_error!("{}", e))?;
        let mut rng = TinyRng::new(0xDEC0DE);
        Ok((plain, MemoryToken(rng.next_u64())))
    }

    /// Verify the container signature if present.
    pub fn verify_signature(path: &str) -> bool {
        let sig_path = format!("{path}.sig");
        if let (Ok(data), Ok(sig)) = (fs::read(path), fs::read(sig_path)) {
            if let Ok(pubkey) = fs::read("/keys/slm_signing.pub") {
                return TinyEd25519::verify(&pubkey, &data, &sig);
            }
        }
        false
    }

    /// Preload all models from the given directory.
    pub fn preload_from_dir(dir: &str, key: &[u8]) -> Result<(), CohError> {
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
