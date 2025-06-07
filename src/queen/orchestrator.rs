// CLASSIFICATION: COMMUNITY
// Filename: orchestrator.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-21

//! Queen orchestrator for managing worker nodes.
//!
//! Tracks workers via `/srv/netinit/<worker_id>` and issues spawn commands
//! through `/srv/agents/<worker_id>/spawn`. Stale workers are restarted if no
//! heartbeat is detected within a timeout.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ureq::Agent as HttpAgent;

/// Record of a worker node registered with the queen.
#[derive(Clone, Debug)]
pub struct WorkerRecord {
    pub id: String,
    pub ip: String,
    pub status: String,
    pub boot_ts: u64,
    pub last_seen: u64,
}

/// Queen orchestrator state.
pub struct QueenOrchestrator {
    workers: HashMap<String, WorkerRecord>,
    timeout: Duration,
}

impl QueenOrchestrator {
    /// Initialize the orchestrator with a heartbeat timeout.
    pub fn new(timeout_secs: u64) -> Self {
        Self { workers: HashMap::new(), timeout: Duration::from_secs(timeout_secs) }
    }

    /// Synchronize worker state from `/srv/netinit/` directories.
    pub fn sync_workers(&mut self) {
        if let Ok(entries) = fs::read_dir("/srv/netinit") {
            for ent in entries.flatten() {
                if let Ok(wid) = ent.file_name().into_string() {
                    let path = ent.path().join("ip");
                    if let Ok(ip) = fs::read_to_string(&path) {
                        let rec = self.workers.entry(wid.clone()).or_insert_with(|| WorkerRecord {
                            id: wid.clone(),
                            ip: ip.trim().to_string(),
                            status: "booting".into(),
                            boot_ts: timestamp(),
                            last_seen: timestamp(),
                        });
                        rec.last_seen = timestamp();
                        rec.ip = ip.trim().into();
                    }
                }
            }
        }
    }

    /// Send a spawn command to a worker.
    pub fn spawn_worker_agent(&self, worker_id: &str, agent: &str, args: &[&str]) {
        let path = format!("/srv/agents/{worker_id}/spawn");
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{} {:?}", agent, args);
        }
    }

    /// Restart workers that have not checked in recently.
    pub fn restart_stale(&mut self) {
        let now = timestamp();
        for rec in self.workers.values_mut() {
            if now.saturating_sub(rec.last_seen) > self.timeout.as_secs() {
                let url = format!("http://{}/reboot", rec.ip);
                let _ = HttpAgent::new().post(&url).call();
                rec.status = "restarting".into();
                rec.last_seen = now;
            }
        }
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
