// CLASSIFICATION: COMMUNITY
// Filename: keyring.rs v1.2
// Author: Codex
// Date Modified: 2025-08-17
// Uses TinyEd25519 with deterministic seeding for UEFI builds.

use crate::prelude::*;
/// Cryptographic keyring for trusted queen federation.
//
/// Generates an Ed25519 keypair on first boot and stores the
/// private key under `/srv/federation` while exporting the
/// public key to `/srv/federation/known_hosts/` for peers.
/// Provides signing and verification helpers used during
/// handshake and agent migration.

use crate::utils::tiny_ed25519::TinyEd25519;
use crate::utils::tiny_rng::TinyRng;
use std::fs;

/// Keyring holding the local queen's Ed25519 key pair.
pub struct Keyring {
    keypair: TinyEd25519,
}

impl Keyring {
    /// Load an existing keypair or generate a new one.
    pub fn load_or_generate(queen_id: &str) -> anyhow::Result<Self> {
        fs::create_dir_all("/srv/federation/known_hosts")?;
        let priv_path = format!("/srv/federation/{}_key.pk8", queen_id);
        let pub_path = format!("/srv/federation/known_hosts/{}.pub", queen_id);
        if let Ok(buf) = fs::read(&priv_path) {
            if buf.len() != 32 {
                return Err(anyhow::anyhow!("invalid seed length"));
            }
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&buf);
            let keypair = TinyEd25519::from_seed(&seed);
            Ok(Self { keypair })
        } else {
            let mut rng = TinyRng::new(0xA5A5_A5A5_A5A5_A5A5);
            let mut seed = [0u8; 32];
            rng.fill_bytes(&mut seed);
            let keypair = TinyEd25519::from_seed(&seed);
            fs::write(&priv_path, &seed)?;
            fs::write(&pub_path, &keypair.public_key_bytes())?;
            Ok(Self { keypair })
        }
    }

    /// Sign a message and return the raw signature bytes.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        self.keypair.sign(msg).to_vec()
    }

    /// Verify a peer's signature using its published public key.
    pub fn verify_peer(peer_id: &str, msg: &[u8], sig: &[u8]) -> anyhow::Result<bool> {
        let path = format!("/srv/federation/known_hosts/{}.pub", peer_id);
        let pk = fs::read(path)?;
        Ok(TinyEd25519::verify(&pk, msg, sig))
    }
}
