// CLASSIFICATION: COMMUNITY
// Filename: handshake.rs v1.0
// Author: Codex
// Date Modified: 2025-06-07

//! Queen-to-Queen handshake and capability negotiation.
//!
//! Each queen exposes its handshake files under
//! `/srv/federation/state/<peer>/`. The handshake payload is
//! JSON encoded and signed with the sender's keypair.

use crate::federation::keyring::Keyring;
use serde::{Deserialize, Serialize};
use std::fs;

/// Information exchanged during federation handshakes.
#[derive(Debug, Serialize, Deserialize)]
pub struct Handshake {
    pub queen_id: String,
    pub capabilities: Vec<String>,
}

/// Initiate a handshake with a peer queen.
pub fn initiate(
    me: &str,
    peer: &str,
    caps: &[String],
    kr: &Keyring,
) -> anyhow::Result<()> {
    fs::create_dir_all(format!("/srv/federation/state/{peer}"))?;
    let payload = Handshake {
        queen_id: me.into(),
        capabilities: caps.to_vec(),
    };
    let data = serde_json::to_vec(&payload)?;
    let sig = kr.sign(&data);
    fs::write(format!("/srv/federation/state/{peer}/handshake.bin"), &data)?;
    fs::write(format!("/srv/federation/state/{peer}/handshake.sig"), &sig)?;
    Ok(())
}

/// Verify an inbound handshake and return its contents.
pub fn verify(peer: &str, _kr: &Keyring) -> anyhow::Result<Handshake> {
    let data = fs::read(format!("/srv/federation/state/{peer}/handshake.bin"))?;
    let sig = fs::read(format!("/srv/federation/state/{peer}/handshake.sig"))?;
    if Keyring::verify_peer(peer, &data, &sig)? {
        let payload: Handshake = serde_json::from_slice(&data)?;
        Ok(payload)
    } else {
        Err(anyhow::anyhow!("signature mismatch"))
    }
}
