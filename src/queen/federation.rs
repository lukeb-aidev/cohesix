// CLASSIFICATION: COMMUNITY
// Filename: federation.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-12

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use crate::CohError;
/// Queen-to-Queen federation manager.
//
/// Handles handshake, trust negotiation and heartbeat exchange between
/// peer QueenPrimary nodes. Uses the shared `federation` module for
/// cryptographic primitives and snapshot transfer.
use std::collections::HashMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::federation::{handshake, keyring::Keyring};

/// Representation of a remote Queen instance.
#[derive(Clone, Debug)]
pub struct PeerQueen {
    pub id: String,
    pub token: String,
    pub last_seen: u64,
    pub trust: String,
}

/// Registry of all known peer queens.
#[derive(Default)]
pub struct FederationRegistry {
    pub peers: HashMap<String, PeerQueen>,
}

/// Federation manager coordinating peer links.
pub struct FederationManager {
    pub id: String,
    kr: Keyring,
    pub registry: FederationRegistry,
}

impl FederationManager {
    /// Create a new federation manager for the given queen id.
    pub fn new(id: &str) -> Result<Self, CohError> {
        let kr = Keyring::load_or_generate(id)?;
        let dir = format!("/srv/{id}");
        fs::create_dir_all(&dir).ok();
        Ok(Self {
            id: id.into(),
            kr,
            registry: FederationRegistry::default(),
        })
    }

    /// Connect to a peer queen by initiating a signed handshake.
    pub fn connect(&mut self, peer_id: &str) -> Result<(), CohError> {
        handshake::initiate(&self.id, peer_id, &["orchestrator".into()], &self.kr)?;
        self.registry.peers.insert(
            peer_id.into(),
            PeerQueen {
                id: peer_id.into(),
                token: hex::encode(self.kr.sign(peer_id.as_bytes())),
                last_seen: timestamp(),
                trust: "normal".into(),
            },
        );
        Ok(())
    }

    /// Disconnect from a peer queen and remove related state.
    pub fn disconnect(&mut self, peer_id: &str) -> Result<(), CohError> {
        self.registry.peers.remove(peer_id);
        let dir = format!("/srv/federation/state/{peer_id}");
        if fs::metadata(&dir).is_ok() {
            fs::remove_dir_all(dir).ok();
        }
        Ok(())
    }

    /// Send a heartbeat file to all registered peers.
    pub fn heartbeat(&mut self) {
        for peer in self.registry.peers.values_mut() {
            let path = format!("/srv/federation/state/{}/heartbeat", peer.id);
            let _ = fs::write(&path, timestamp().to_string());
            peer.last_seen = timestamp();
        }
    }

    /// List all known peer ids.
    pub fn list_peers(&self) -> Vec<String> {
        self.registry.peers.keys().cloned().collect()
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
