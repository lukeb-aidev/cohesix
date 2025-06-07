// CLASSIFICATION: COMMUNITY
// Filename: queen.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-04

//! Queen-side orchestration for Workers.
//!
//! Handles worker join requests, monitors health, and
//! exposes the registry under `/srv/registry`.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::orchestrator::protocol::{HealthPing, JoinRequest};
use rmp_serde::decode::from_read;

/// State for each registered worker.
#[derive(Debug, Clone)]
pub struct WorkerInfo {
    last_seen: u64,
    quarantined: bool,
}

/// Queen orchestrator.
pub struct Queen {
    workers: HashMap<String, WorkerInfo>,
    timeout: Duration,
}

impl Queen {
    /// Initialize the registry directory and orchestrator state.
    pub fn new(timeout_secs: u64) -> anyhow::Result<Self> {
        fs::create_dir_all("/srv/registry")?;
        fs::create_dir_all("/srv/registry/join")?;
        fs::create_dir_all("/srv/registry/ping")?;
        Ok(Self { workers: HashMap::new(), timeout: Duration::from_secs(timeout_secs) })
    }

    /// Process join requests queued in `/srv/registry/join`.
    pub fn process_joins(&mut self) {
        if let Ok(entries) = fs::read_dir("/srv/registry/join") {
            for e in entries.flatten() {
                if let Ok(mut f) = fs::File::open(e.path()) {
                    if let Ok(req) = from_read::<_, JoinRequest>(&mut f) {
                        self.workers.insert(
                            req.worker_id.clone(),
                            WorkerInfo { last_seen: timestamp(), quarantined: false },
                        );
                    }
                }
                let _ = fs::remove_file(e.path());
            }
        }
    }

    /// Record health pings from workers.
    pub fn ingest_pings(&mut self) {
        if let Ok(entries) = fs::read_dir("/srv/registry/ping") {
            for e in entries.flatten() {
                if let Ok(mut f) = fs::File::open(e.path()) {
                    if let Ok(ping) = from_read::<_, HealthPing>(&mut f) {
                        if let Some(w) = self.workers.get_mut(&ping.worker_id) {
                            w.last_seen = ping.ts;
                            w.quarantined = false;
                        }
                    }
                }
                let _ = fs::remove_file(e.path());
            }
        }
    }

    /// Periodically check workers and mark stale ones as quarantined.
    pub fn check_timeouts(&mut self) {
        let now = timestamp();
        for (id, info) in self.workers.iter_mut() {
            if now.saturating_sub(info.last_seen) > self.timeout.as_secs() {
                if !info.quarantined {
                    log_fault(id);
                    info.quarantined = true;
                }
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

fn log_fault(id: &str) {
    let path = "/srv/registry/faults";
    let _ = fs::create_dir_all("/srv/registry");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} quarantined", id);
    }
}
