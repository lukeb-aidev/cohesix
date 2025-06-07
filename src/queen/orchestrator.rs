// CLASSIFICATION: COMMUNITY
// Filename: orchestrator.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-07

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
    pub role: String,
    pub trust: String,
    pub capabilities: Vec<String>,
}

/// Queen orchestrator state.
pub struct QueenOrchestrator {
    workers: HashMap<String, WorkerRecord>,
    timeout: Duration,
    policy: SchedulePolicy,
    next_idx: usize,
}

#[derive(Clone, Copy)]
pub enum SchedulePolicy {
    RoundRobin,
    GpuPriority,
    LatencyAware,
}

impl QueenOrchestrator {
    /// Initialize the orchestrator with a heartbeat timeout.
    pub fn new(timeout_secs: u64, policy: SchedulePolicy) -> Self {
        Self {
            workers: HashMap::new(),
            timeout: Duration::from_secs(timeout_secs),
            policy,
            next_idx: 0,
        }
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
                            role: fs::read_to_string(ent.path().join("role")).unwrap_or_else(|_| "unknown".into()).trim().into(),
                            trust: fs::read_to_string(ent.path().join("trust")).unwrap_or_else(|_| "normal".into()).trim().into(),
                            capabilities: fs::read_to_string(ent.path().join("caps")).map(|c| c.lines().map(|s| s.to_string()).collect()).unwrap_or_else(|_| Vec::new()),
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

    /// Export orchestrator status to `/srv/orch/status`.
    pub fn export_status(&self) {
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open("/srv/orch/status")
        {
            for w in self.workers.values() {
                let _ = writeln!(f, "{} {} {} {}", w.id, w.role, w.status, w.ip);
            }
        }
    }

    /// Schedule a task based on the configured policy.
    pub fn schedule(&mut self, _agent_id: &str) -> Option<String> {
        let ids: Vec<_> = self.workers.keys().cloned().collect();
        if ids.is_empty() {
            return None;
        }
        let idx = match self.policy {
            SchedulePolicy::RoundRobin => {
                let i = self.next_idx % ids.len();
                self.next_idx += 1;
                i
            }
            _ => 0,
        };
        ids.get(idx).cloned()
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
