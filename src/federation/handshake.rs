// CLASSIFICATION: COMMUNITY
// Filename: handshake.rs v1.1
// Author: Codex
// Date Modified: 2025-07-12

/// Queen-to-Queen handshake and capability negotiation.
//
/// Each queen exposes its handshake files under
/// `/srv/federation/state/<peer>/`. The handshake payload is
/// JSON encoded and signed with the sender's keypair.
use crate::federation::keyring::Keyring;
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use crate::queen::trust;
use crate::{coh_error, CohError};
use serde::{Deserialize, Serialize};
use std::fs;

/// Information exchanged during federation handshakes.
#[derive(Debug, Serialize, Deserialize)]
pub struct Handshake {
    pub queen_id: String,
    pub capabilities: Vec<String>,
    pub parent_role: Option<String>,
    pub trust_zones: Vec<String>,
    pub timestamp: u64,
}

/// Initiate a handshake with a peer queen.
pub fn initiate(me: &str, peer: &str, caps: &[String], kr: &Keyring) -> Result<(), CohError> {
    fs::create_dir_all(format!("/srv/federation/state/{peer}"))?;
    let payload = Handshake {
        queen_id: me.into(),
        capabilities: caps.to_vec(),
        parent_role: std::env::var("COH_ROLE").ok(),
        trust_zones: trust::list_trust()
            .into_iter()
            .map(|(w, l)| format!("{w}:{l}"))
            .collect(),
        timestamp: current_time(),
    };
    let data = serde_json::to_vec(&payload)?;
    let sig = kr.sign(&data);
    fs::write(format!("/srv/federation/state/{peer}/handshake.bin"), &data)?;
    fs::write(format!("/srv/federation/state/{peer}/handshake.sig"), &sig)?;
    Ok(())
}

/// Verify an inbound handshake and return its contents.
pub fn verify(peer: &str, _kr: &Keyring) -> Result<Handshake, CohError> {
    let data = fs::read(format!("/srv/federation/state/{peer}/handshake.bin"))?;
    let sig = fs::read(format!("/srv/federation/state/{peer}/handshake.sig"))?;
    if Keyring::verify_peer(peer, &data, &sig)? {
        let payload: Handshake = serde_json::from_slice(&data)?;
        Ok(payload)
    } else {
        Err(coh_error!("signature mismatch"))
    }
}

fn current_time() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
