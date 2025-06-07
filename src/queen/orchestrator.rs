// CLASSIFICATION: COMMUNITY
// Filename: orchestrator.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-08

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
use serde_json;

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

/// GPU-aware metadata for scheduling decisions.
#[derive(Clone, Debug)]
pub struct GpuNode {
    pub worker_id: String,
    pub status: String,
    pub perf_watt: f32,
    pub mem_total: u64,
    pub mem_free: u64,
    pub last_temp: u32,
    pub jobs: Vec<String>,
}

/// Queen orchestrator state.
pub struct QueenOrchestrator {
    workers: HashMap<String, WorkerRecord>,
    gpu_nodes: HashMap<String, GpuNode>,
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
            gpu_nodes: HashMap::new(),
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

    /// Collect GPU telemetry for workers exposing CUDA.
    pub fn sync_gpu_telemetry(&mut self) {
        for rec in self.workers.values() {
            if !rec.capabilities.iter().any(|c| c == "cuda") {
                continue;
            }
            let tpath = format!("/srv/telemetry/{}/gpu.json", rec.id);
            if let Ok(data) = fs::read_to_string(&tpath) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
                    let perf_watt = val["perf_watt"].as_f64().unwrap_or(0.0) as f32;
                    let mem_total = val["mem_total"].as_u64().unwrap_or(0);
                    let mem_free = val["mem_free"].as_u64().unwrap_or(0);
                    let last_temp = val["temp"].as_u64().unwrap_or(0) as u32;
                    let node = self.gpu_nodes.entry(rec.id.clone()).or_insert(GpuNode {
                        worker_id: rec.id.clone(),
                        status: "online".into(),
                        perf_watt,
                        mem_total,
                        mem_free,
                        last_temp,
                        jobs: Vec::new(),
                    });
                    node.perf_watt = perf_watt;
                    node.mem_total = mem_total;
                    node.mem_free = mem_free;
                    node.last_temp = last_temp;
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
    pub fn schedule(&mut self, agent_id: &str) -> Option<String> {
        let ids: Vec<_> = self.workers.keys().cloned().collect();
        if ids.is_empty() {
            return None;
        }
        match self.policy {
            SchedulePolicy::RoundRobin => {
                let i = self.next_idx % ids.len();
                self.next_idx += 1;
                ids.get(i).cloned()
            }
            SchedulePolicy::GpuPriority => self.schedule_gpu(agent_id),
            _ => ids.get(0).cloned(),
        }
    }

    fn schedule_gpu(&mut self, job: &str) -> Option<String> {
        self.sync_gpu_telemetry();
        let mut best_id: Option<String> = None;
        let mut best_weight = 0f32;
        for (id, node) in self.gpu_nodes.iter() {
            let weight = node.perf_watt;
            if best_id.is_none() || weight > best_weight {
                best_id = Some(id.clone());
                best_weight = weight;
            }
        }
        if let Some(id) = best_id {
            if let Some(gn) = self.gpu_nodes.get_mut(&id) {
                gn.jobs.push(job.into());
            }
            Some(id)
        } else {
            None
        }
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
