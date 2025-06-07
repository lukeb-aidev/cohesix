// CLASSIFICATION: COMMUNITY
// Filename: queen.rs v0.2
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

use crate::orchestrator::protocol::{HealthPing, JoinAck, JoinRequest};
use rmp_serde::decode::from_read;
use rmp_serde::encode::to_vec;

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
        fs::create_dir_all("/srv/registry/ack")?;
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
                        // create worker dir and ack
                        fs::create_dir_all(format!("/srv/worker/{}", req.worker_id)).ok();
                        let ack = JoinAck { worker_id: req.worker_id.clone(), queen_id: hostname() };
                        if let Ok(data) = to_vec(&ack) {
                            fs::create_dir_all("/srv/registry/ack").ok();
                            let path = format!("/srv/registry/ack/{}.msg", req.worker_id);
                            let _ = fs::write(path, data);
                        }
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

fn hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "queen".into())
}

fn log_fault(id: &str) {
    let path = "/srv/registry/faults";
    let _ = fs::create_dir_all("/srv/registry");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} quarantined", id);
    }
}
