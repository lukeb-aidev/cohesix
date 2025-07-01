// CLASSIFICATION: COMMUNITY
// Filename: worker.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-04

use crate::prelude::*;
//! Worker-side orchestration logic.
//!
//! Sends join requests to the Queen and responds to ping
//! files for health monitoring.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::orchestrator::protocol::{HealthPing, JoinAck, JoinRequest};
use rmp_serde::encode::to_vec;
use rmp_serde::decode::from_read;

/// Basic worker orchestrator helper.
pub struct Worker {
    pub id: String,
    queen_path: String,
}

impl Worker {
    /// Create a new worker handle.
    pub fn new(id: &str, queen_path: &str) -> Self {
        Self { id: id.into(), queen_path: queen_path.into() }
    }

    /// Send a join request to the Queen.
    pub fn join(&self, ip: &str) -> anyhow::Result<()> {
        fs::create_dir_all(format!("{}/join", self.queen_path))?;
        let req = JoinRequest { worker_id: self.id.clone(), ip: ip.into() };
        let data = to_vec(&req)?;
        fs::write(format!("{}/join/{}.msg", self.queen_path, self.id), data)?;
        Ok(())
    }

    /// Respond to a ping file if present.
    pub fn respond_ping(&self) {
        let ping_path = format!("{}/ping/{}.req", self.queen_path, self.id);
        if fs::metadata(&ping_path).is_ok() {
            let _ = fs::remove_file(&ping_path);
            let hp = HealthPing { worker_id: self.id.clone(), ts: timestamp() };
            if let Ok(data) = to_vec(&hp) {
                let _ = fs::create_dir_all(format!("{}/ping", self.queen_path));
                let _ = fs::write(format!("{}/ping/{}.res", self.queen_path, self.id), data);
            }
        }
    }

    /// Check for a join acknowledgement from the Queen.
    pub fn check_ack(&self) -> Option<JoinAck> {
        let path = format!("{}/ack/{}.msg", self.queen_path, self.id);
        if let Ok(mut f) = fs::File::open(&path) {
            if let Ok(ack) = from_read::<_, JoinAck>(&mut f) {
                let _ = fs::remove_file(path);
                return Some(ack);
            }
        }
        None
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
