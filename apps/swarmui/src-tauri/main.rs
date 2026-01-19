// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: SwarmUI Tauri entry point and command wiring.
// Author: Lukas Bower
//! SwarmUI desktop entry point and Tauri command wiring.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use tauri::State;

use cohsh::COHSH_TCP_PORT;
use swarmui::{
    parse_role_label, SwarmUiBackend, SwarmUiConfig, SwarmUiConsoleBackend, SwarmUiTranscript,
    TcpTransportFactory,
};

enum SwarmUiService {
    Secure9p(SwarmUiBackend<TcpTransportFactory>),
    Console(SwarmUiConsoleBackend),
}

impl SwarmUiService {
    fn attach(&mut self, role: cohesix_ticket::Role, ticket: Option<&str>) -> SwarmUiTranscript {
        match self {
            SwarmUiService::Secure9p(backend) => backend.attach(role, ticket),
            SwarmUiService::Console(backend) => backend.attach(role, ticket),
        }
    }

    fn set_offline(&mut self, offline: bool) {
        match self {
            SwarmUiService::Secure9p(backend) => backend.set_offline(offline),
            SwarmUiService::Console(backend) => backend.set_offline(offline),
        }
    }

    fn tail_telemetry(
        &mut self,
        role: cohesix_ticket::Role,
        ticket: Option<&str>,
        worker_id: &str,
    ) -> SwarmUiTranscript {
        match self {
            SwarmUiService::Secure9p(backend) => backend.tail_telemetry(role, ticket, worker_id),
            SwarmUiService::Console(backend) => backend.tail_telemetry(role, ticket, worker_id),
        }
    }

    fn list_namespace(
        &mut self,
        role: cohesix_ticket::Role,
        ticket: Option<&str>,
        path: &str,
    ) -> SwarmUiTranscript {
        match self {
            SwarmUiService::Secure9p(backend) => backend.list_namespace(role, ticket, path),
            SwarmUiService::Console(backend) => backend.list_namespace(role, ticket, path),
        }
    }

    fn fleet_snapshot(
        &mut self,
        role: cohesix_ticket::Role,
        ticket: Option<&str>,
    ) -> SwarmUiTranscript {
        match self {
            SwarmUiService::Secure9p(backend) => backend.fleet_snapshot(role, ticket),
            SwarmUiService::Console(backend) => backend.fleet_snapshot(role, ticket),
        }
    }

    fn hive_bootstrap(
        &mut self,
        role: cohesix_ticket::Role,
        ticket: Option<&str>,
        snapshot_key: Option<&str>,
    ) -> Result<swarmui::SwarmUiHiveBootstrap, String> {
        match self {
            SwarmUiService::Secure9p(backend) => backend
                .hive_bootstrap(role, ticket, snapshot_key)
                .map_err(|err| err.to_string()),
            SwarmUiService::Console(backend) => backend
                .hive_bootstrap(role, ticket, snapshot_key)
                .map_err(|err| err.to_string()),
        }
    }

    fn hive_poll(
        &mut self,
        role: cohesix_ticket::Role,
        ticket: Option<&str>,
    ) -> Result<swarmui::SwarmUiHiveBatch, String> {
        match self {
            SwarmUiService::Secure9p(backend) => backend
                .hive_poll(role, ticket)
                .map_err(|err| err.to_string()),
            SwarmUiService::Console(backend) => backend
                .hive_poll(role, ticket)
                .map_err(|err| err.to_string()),
        }
    }

    fn hive_reset(
        &mut self,
        role: cohesix_ticket::Role,
        ticket: Option<&str>,
    ) -> Result<(), String> {
        match self {
            SwarmUiService::Secure9p(backend) => backend
                .hive_reset(role, ticket)
                .map_err(|err| err.to_string()),
            SwarmUiService::Console(backend) => backend
                .hive_reset(role, ticket)
                .map_err(|err| err.to_string()),
        }
    }

    fn load_hive_replay(&mut self, payload: &[u8]) -> Result<(), String> {
        match self {
            SwarmUiService::Secure9p(backend) => backend
                .load_hive_replay(payload)
                .map_err(|err| err.to_string()),
            SwarmUiService::Console(backend) => backend
                .load_hive_replay(payload)
                .map_err(|err| err.to_string()),
        }
    }
}

struct AppState {
    backend: Mutex<SwarmUiService>,
}

#[tauri::command]
fn swarmui_connect(
    state: State<'_, AppState>,
    role: Option<String>,
    ticket: Option<String>,
) -> Result<SwarmUiTranscript, String> {
    let role = role.unwrap_or_else(|| "queen".to_owned());
    let role = parse_role_label(&role).map_err(|err| err.to_string())?;
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    Ok(backend.attach(role, ticket.as_deref()))
}

#[tauri::command]
fn swarmui_offline(state: State<'_, AppState>, offline: bool) -> Result<(), String> {
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    backend.set_offline(offline);
    Ok(())
}

#[tauri::command]
fn swarmui_tail_telemetry(
    state: State<'_, AppState>,
    role: String,
    ticket: Option<String>,
    worker_id: String,
) -> Result<SwarmUiTranscript, String> {
    let role = parse_role_label(&role).map_err(|err| err.to_string())?;
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    Ok(backend.tail_telemetry(role, ticket.as_deref(), &worker_id))
}

#[tauri::command]
fn swarmui_list_namespace(
    state: State<'_, AppState>,
    role: String,
    ticket: Option<String>,
    path: String,
) -> Result<SwarmUiTranscript, String> {
    let role = parse_role_label(&role).map_err(|err| err.to_string())?;
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    Ok(backend.list_namespace(role, ticket.as_deref(), &path))
}

#[tauri::command]
fn swarmui_fleet_snapshot(
    state: State<'_, AppState>,
    role: String,
    ticket: Option<String>,
) -> Result<SwarmUiTranscript, String> {
    let role = parse_role_label(&role).map_err(|err| err.to_string())?;
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    Ok(backend.fleet_snapshot(role, ticket.as_deref()))
}

#[tauri::command]
fn swarmui_hive_bootstrap(
    state: State<'_, AppState>,
    role: Option<String>,
    ticket: Option<String>,
    snapshot_key: Option<String>,
) -> Result<swarmui::SwarmUiHiveBootstrap, String> {
    let role = role.unwrap_or_else(|| "queen".to_owned());
    let role = parse_role_label(&role).map_err(|err| err.to_string())?;
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    backend.hive_bootstrap(role, ticket.as_deref(), snapshot_key.as_deref())
}

#[tauri::command]
fn swarmui_hive_poll(
    state: State<'_, AppState>,
    role: String,
    ticket: Option<String>,
) -> Result<swarmui::SwarmUiHiveBatch, String> {
    let role = parse_role_label(&role).map_err(|err| err.to_string())?;
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    backend.hive_poll(role, ticket.as_deref())
}

#[tauri::command]
fn swarmui_hive_reset(
    state: State<'_, AppState>,
    role: String,
    ticket: Option<String>,
) -> Result<(), String> {
    let role = parse_role_label(&role).map_err(|err| err.to_string())?;
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    backend.hive_reset(role, ticket.as_deref())
}

fn parse_replay_path(args: &[String]) -> Option<PathBuf> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--replay" {
            return iter.next().map(PathBuf::from);
        }
        if let Some(value) = arg.strip_prefix("--replay=") {
            return Some(PathBuf::from(value));
        }
    }
    None
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let replay_path = parse_replay_path(&args);
    let data_dir = tauri::api::path::data_dir().unwrap_or_else(|| std::env::temp_dir());
    let mut config = SwarmUiConfig::from_generated(data_dir.clone());
    if replay_path.is_some() {
        config.offline = true;
    }
    let host = env::var("SWARMUI_9P_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = env::var("SWARMUI_9P_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(COHSH_TCP_PORT);
    let transport = env::var("SWARMUI_TRANSPORT")
        .unwrap_or_else(|_| "console".to_owned())
        .trim()
        .to_ascii_lowercase();
    let timeout = Duration::from_secs(2);
    let mut backend = match transport.as_str() {
        "9p" | "secure9p" => {
            let factory = TcpTransportFactory::new(
                host,
                port,
                timeout,
                swarmui::SECURE9P_MSIZE,
            );
            SwarmUiService::Secure9p(SwarmUiBackend::new(config, factory))
        }
        "console" | "tcp" => {
            let auth_token = env::var("SWARMUI_AUTH_TOKEN")
                .or_else(|_| env::var("COHSH_AUTH_TOKEN"))
                .unwrap_or_else(|_| "changeme".to_owned());
            SwarmUiService::Console(SwarmUiConsoleBackend::new(
                config,
                host,
                port,
                auth_token,
            ))
        }
        other => panic!("unsupported SWARMUI_TRANSPORT '{other}' (use console or 9p)"),
    };
    if let Some(path) = replay_path {
        let resolved = if path.is_relative() {
            data_dir.join("snapshots").join(path)
        } else {
            path
        };
        let payload = fs::read(&resolved)
            .unwrap_or_else(|err| panic!("failed to read replay {}: {err}", resolved.display()));
        backend
            .load_hive_replay(&payload)
            .unwrap_or_else(|err| panic!("failed to load replay: {err}"));
    }
    let state = AppState {
        backend: Mutex::new(backend),
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            swarmui_connect,
            swarmui_offline,
            swarmui_tail_telemetry,
            swarmui_list_namespace,
            swarmui_fleet_snapshot,
            swarmui_hive_bootstrap,
            swarmui_hive_poll,
            swarmui_hive_reset,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SwarmUI");
}
