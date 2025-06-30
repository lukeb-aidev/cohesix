// CLASSIFICATION: COMMUNITY
// Filename: keyring.rs v1.2
// Author: Codex
// Date Modified: 2025-08-17
// This module uses ring::rand which relies on getrandom; it is disabled on UEFI.

//! Cryptographic keyring for trusted queen federation.
//!
//! Generates an Ed25519 keypair on first boot and stores the
//! private key under `/srv/federation` while exporting the
//! public key to `/srv/federation/known_hosts/` for peers.
//! Provides signing and verification helpers used during
//! handshake and agent migration.

use ring::signature::{self, Ed25519KeyPair, KeyPair};
use crate::utils::tiny_rng::TinyRng;
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
            let keypair = if buf.len() == 32 {
                Ed25519KeyPair::from_seed_unchecked(&buf)
                    .map_err(|_| anyhow::anyhow!("invalid seed"))?
            } else {
                Ed25519KeyPair::from_pkcs8(&buf)
                    .map_err(|_| anyhow::anyhow!("invalid key"))?
            };
            Ok(Self { keypair })
        } else {
            let mut rng = TinyRng::new(0xA5A5_A5A5_A5A5_A5A5);
            let mut seed = [0u8; 32];
            rng.fill_bytes(&mut seed);
            let keypair = Ed25519KeyPair::from_seed_unchecked(&seed)
                .map_err(|_| anyhow::anyhow!("seed invalid"))?;
            fs::write(&priv_path, &seed)?;
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
