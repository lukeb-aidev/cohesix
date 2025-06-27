// CLASSIFICATION: COMMUNITY
// Filename: orchestrator.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-10-10
#![cfg(not(target_os = "uefi"))]

//! Cloud orchestration hooks for the Queen role.
//! Provides registration and heartbeat routines for
//! interacting with a remote orchestrator service.

use anyhow::Error;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use ureq::Agent;

pub type QueenId = String;

/// Register this Queen with the cloud orchestrator.
/// On success the returned ID is written to `/srv/cloud/queen_id`.
pub fn register_queen(cloud_url: &str) -> Result<QueenId, Error> {
    let host = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "queen".into());
    let url = format!("{}/register", cloud_url.trim_end_matches('/'));
    let body = serde_json::json!({ "hostname": host });
    let resp = Agent::new().post(&url).send_string(&body.to_string())?;
    if !(200..300).contains(&resp.status()) {
        return Err(anyhow::anyhow!("registration failed: {}", resp.status()));
    }
    let id = resp.into_string().unwrap_or_else(|_| "queen".into());
    fs::create_dir_all("/srv/cloud").ok();
    fs::write("/srv/cloud/queen_id", &id).ok();
    fs::write("/srv/cloud/url", cloud_url).ok();
    Ok(id)
}

#[derive(Serialize)]
struct Heartbeat<'a> {
    queen_id: &'a str,
    validator: bool,
    role: &'a str,
    ts: u64,
    worker_count: usize,
}

/// Send a status heartbeat to the cloud orchestrator.
/// Updates `/srv/cloud/state.json` with the latest info.
pub fn send_heartbeat(id: QueenId) -> Result<(), Error> {
    let url = fs::read_to_string("/srv/cloud/url").unwrap_or_default();
    if url.is_empty() {
        return Ok(());
    }
    let validator = crate::validator::validator_running();
    let role = crate::cohesix_types::RoleManifest::current_role();
    let role_name = match &role {
        crate::cohesix_types::Role::QueenPrimary => "QueenPrimary",
        crate::cohesix_types::Role::DroneWorker => "DroneWorker",
        crate::cohesix_types::Role::InteractiveAIBooth => "InteractiveAIBooth",
        crate::cohesix_types::Role::KioskInteractive => "KioskInteractive",
        crate::cohesix_types::Role::GlassesAgent => "GlassesAgent",
        crate::cohesix_types::Role::SensorRelay => "SensorRelay",
        crate::cohesix_types::Role::SimulatorTest => "SimulatorTest",
        crate::cohesix_types::Role::Other(n) => n,
    };
    let worker_count = count_workers();
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let hb = Heartbeat {
        queen_id: &id,
        validator,
        role: role_name,
        ts,
        worker_count,
    };
    let data = serde_json::to_string(&hb)?;
    let resp = Agent::new()
        .post(&format!("{}/heartbeat", url.trim_end_matches('/')))
        .send_string(&data)?;
    if !(200..300).contains(&resp.status()) {
        return Err(anyhow::anyhow!("heartbeat failed: {}", resp.status()));
    }
    fs::create_dir_all("/srv/cloud").ok();
    fs::write("/srv/cloud/state.json", &data).ok();
    fs::write("/srv/cloud/last_heartbeat", ts.to_string()).ok();
    Ok(())
}

fn count_workers() -> usize {
    if let Ok(data) = fs::read_to_string("/srv/agents/active.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
            return v.as_array().map(|a| a.len()).unwrap_or(0);
        }
    }
    0
}

/// Placeholder for receiving orchestration commands from the cloud.
pub fn receive_commands() -> Vec<String> {
    if let Ok(entries) = fs::read_dir("/srv/cloud/commands") {
        let mut cmds = Vec::new();
        for e in entries.flatten() {
            if let Ok(c) = fs::read_to_string(e.path()) {
                cmds.push(c);
            }
            let _ = fs::remove_file(e.path());
        }
        cmds
    } else {
        Vec::new()
    }
}
