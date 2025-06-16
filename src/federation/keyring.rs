// CLASSIFICATION: COMMUNITY
// Filename: keyring.rs v1.2
// Author: Codex
// Date Modified: 2025-08-17
// This module uses ring::rand which relies on getrandom; it is disabled on UEFI.
#![cfg(not(target_os = "uefi"))]

//! Cryptographic keyring for trusted queen federation.
//!
//! Generates an Ed25519 keypair on first boot and stores the
//! private key under `/srv/federation` while exporting the
//! public key to `/srv/federation/known_hosts/` for peers.
//! Provides signing and verification helpers used during
//! handshake and agent migration.

use ring::rand::SystemRandom;
use ring::signature::{self, Ed25519KeyPair, KeyPair};
use std::fs;

/// Keyring holding the local queen's Ed25519 key pair.
pub struct Keyring {
    keypair: Ed25519KeyPair,
}

impl Keyring {
    /// Load an existing keypair or generate a new one.
    pub fn load_or_generate(queen_id: &str) -> anyhow::Result<Self> {
        fs::create_dir_all("/srv/federation/known_hosts")?;
        let priv_path = format!("/srv/federation/{}_key.pk8", queen_id);
        let pub_path = format!("/srv/federation/known_hosts/{}.pub", queen_id);
        if let Ok(buf) = fs::read(&priv_path) {
            let keypair = Ed25519KeyPair::from_pkcs8(&buf)
                .map_err(|_| anyhow::anyhow!("invalid key"))?;
            Ok(Self { keypair })
        } else {
            let rng = SystemRandom::new();
            let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng)
                .map_err(|_| anyhow::anyhow!("gen failed"))?;
            fs::write(&priv_path, pkcs8.as_ref())?;
            let keypair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
                .map_err(|_| anyhow::anyhow!("invalid key"))?;
            fs::write(&pub_path, keypair.public_key().as_ref())?;
            Ok(Self { keypair })
        }
    }

    /// Sign a message and return the raw signature bytes.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        self.keypair.sign(msg).as_ref().to_vec()
    }

    /// Verify a peer's signature using its published public key.
    pub fn verify_peer(peer_id: &str, msg: &[u8], sig: &[u8]) -> anyhow::Result<bool> {
        let path = format!("/srv/federation/known_hosts/{}.pub", peer_id);
        let pk = fs::read(path)?;
        let peer_key = signature::UnparsedPublicKey::new(&signature::ED25519, pk);
        Ok(peer_key.verify(msg, sig).is_ok())
    }
}
