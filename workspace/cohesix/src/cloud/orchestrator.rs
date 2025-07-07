// CLASSIFICATION: COMMUNITY
// Filename: orchestrator.rs v0.9
// Author: Lukas Bower
// Date Modified: 2027-08-17

/// Cloud orchestration hooks for the Queen role.
/// Provides registration and heartbeat routines for
/// interacting with a remote orchestrator service.
use crate::{coh_error, CohError};
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use serde::Serialize;
use std::fs;
use std::io::{self, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tiny_http::{Method, Response, Server};
use ureq::Agent;

pub type QueenId = String;

/// Cloud orchestrator runtime handle.
pub struct CloudOrchestrator {
    _queen_id: QueenId,
}

impl CloudOrchestrator {
    /// Register the Queen, spawn heartbeat and command listener threads.
    pub fn start(cloud_url: &str) -> Result<Self, CohError> {
        let id = register_queen(cloud_url)?;
        let hb_id = id.clone();
        std::thread::spawn(move || loop {
            if let Err(e) = send_heartbeat(hb_id.clone()) {
                let _ = writeln!(std::io::stderr(), "heartbeat error: {e}");
            }
            std::thread::sleep(Duration::from_secs(10));
        });
        spawn_command_listener();
        Ok(Self { _queen_id: id })
    }
}

/// Register this Queen with the cloud orchestrator.
/// On success the returned ID is written to `/srv/cloud/queen_id`.
pub fn register_queen(cloud_url: &str) -> Result<QueenId, CohError> {
    let hostname = "cohesix-uefi";
    let host = hostname.to_string();
    let url = format!("{}/register", cloud_url.trim_end_matches('/'));
    let body = serde_json::json!({ "hostname": host });
    let resp = Agent::new_with_defaults()
        .post(&url)
        .send(body.to_string())?;
    if !(200..300).contains(&resp.status().as_u16()) {
        return Err(coh_error!("registration failed: {}", resp.status()));
    }
    let id = resp
        .into_body()
        .read_to_string()
        .unwrap_or_else(|_| "queen".into());
    println!("Opening file: {:?}", crate::with_srv_root!("cloud"));
    fs::create_dir_all(crate::with_srv_root!("cloud")).ok();
    println!(
        "Opening file: {:?}",
        crate::with_srv_root!("cloud/queen_id")
    );
    fs::write(crate::with_srv_root!("cloud/queen_id"), &id).ok();
    println!("Opening file: {:?}", crate::with_srv_root!("cloud/url"));
    fs::write(crate::with_srv_root!("cloud/url"), cloud_url).ok();
    println!("POST /register sent to {}", url);
    let _ = send_heartbeat(id.clone());
    std::io::stdout().flush().unwrap();
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
pub fn send_heartbeat(id: QueenId) -> Result<(), CohError> {
    println!("Opening file: {:?}", crate::with_srv_root!("cloud/url"));
    let url = fs::read_to_string(crate::with_srv_root!("cloud/url")).unwrap_or_default();
    if url.is_empty() {
        return Ok(());
    }
    let validator = crate::validator::validator_running();
    let role = crate::cohesix_types::RoleManifest::current_role();
    let role_name = match &role {
        crate::cohesix_types::Role::QueenPrimary => "QueenPrimary",
        crate::cohesix_types::Role::RegionalQueen => "RegionalQueen",
        crate::cohesix_types::Role::BareMetalQueen => "BareMetalQueen",
        crate::cohesix_types::Role::DroneWorker => "DroneWorker",
        crate::cohesix_types::Role::InteractiveAiBooth => "InteractiveAiBooth",
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
    let resp = Agent::new_with_defaults()
        .post(&format!("{}/heartbeat", url.trim_end_matches('/')))
        .send(&data)?;
    if !(200..300).contains(&resp.status().as_u16()) {
        return Err(coh_error!("heartbeat failed: {}", resp.status()));
    }
    println!("Opening file: {:?}", crate::with_srv_root!("cloud"));
    fs::create_dir_all(crate::with_srv_root!("cloud")).ok();
    println!(
        "Opening file: {:?}",
        crate::with_srv_root!("cloud/state.json")
    );
    fs::write(crate::with_srv_root!("cloud/state.json"), &data).ok();
    println!(
        "Opening file: {:?}",
        crate::with_srv_root!("cloud/last_heartbeat")
    );
    fs::write(
        crate::with_srv_root!("cloud/last_heartbeat"),
        ts.to_string(),
    )
    .ok();
    let url = url.trim_end_matches('/');
    println!("POST /heartbeat sent to {}", url);
    std::io::stdout().flush().unwrap();
    Ok(())
}

fn count_workers() -> usize {
    println!(
        "Opening file: {:?}",
        crate::with_srv_root!("agents/active.json")
    );
    if let Ok(data) = fs::read_to_string(crate::with_srv_root!("agents/active.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
            return v.as_array().map(|a| a.len()).unwrap_or(0);
        }
    }
    0
}

fn spawn_command_listener() {
    std::thread::spawn(|| {
        if let Ok(server) = Server::http("0.0.0.0:4070") {
            println!(
                "Opening file: {:?}",
                crate::with_srv_root!("cloud/commands")
            );
            fs::create_dir_all(crate::with_srv_root!("cloud/commands")).ok();
            for mut req in server.incoming_requests() {
                if req.method() == &Method::Post && req.url() == "/command" {
                    let mut body = String::new();
                    if io::Read::read_to_string(&mut req.as_reader(), &mut body).is_ok() {
                        let ts = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let path = crate::with_srv_root!(&format!("cloud/commands/{ts}"));
                        println!("Opening file: {:?}", path);
                        let _ = fs::write(&path, body);
                    }
                    let _ = req.respond(Response::empty(200));
                } else {
                    let _ = req.respond(Response::empty(404));
                }
            }
        }
    });
}

/// Placeholder for receiving orchestration commands from the cloud.
pub fn receive_commands() -> Vec<String> {
    println!(
        "Opening file: {:?}",
        crate::with_srv_root!("cloud/commands")
    );
    if let Ok(entries) = fs::read_dir(crate::with_srv_root!("cloud/commands")) {
        let mut cmds = Vec::new();
        for e in entries.flatten() {
            println!("Opening file: {:?}", e.path());
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
