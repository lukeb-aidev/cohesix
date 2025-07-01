// CLASSIFICATION: COMMUNITY
// Filename: tiny_ed25519.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-12-31

//! Minimal Ed25519 wrapper using `ed25519-dalek`.
//! This is `no_std` compatible and relies on `TinyRng` for
//! deterministic key seeding on UEFI builds.

use ed25519_dalek::{Signer, Verifier, SigningKey, VerifyingKey, Signature};

/// Lightweight Ed25519 keypair.
#[derive(Clone)]
pub struct TinyEd25519 {
    secret: SigningKey,
    public: VerifyingKey,
}

impl TinyEd25519 {
    /// Create a keypair from a 32 byte seed.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let secret = SigningKey::from_bytes(seed);
        let public = VerifyingKey::from(&secret);
        Self { secret, public }
    }

    /// Return the 32 byte public key.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.public.to_bytes()
    }

    /// Sign the provided message.
    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.secret.sign(msg).to_bytes()
    }

    /// Verify a signature with the given public key bytes.
    pub fn verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
        if pk.len() != 32 || sig.len() != 64 {
            return false;
        }
        let public = match VerifyingKey::from_bytes(pk.try_into().unwrap()) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let sig = match Signature::from_bytes(sig.try_into().unwrap()) {
            Ok(s) => s,
            Err(_) => return false,
        };
        public.verify(msg, &sig).is_ok()
    }
}

