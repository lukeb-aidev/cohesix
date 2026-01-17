// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: SwarmUI Tauri entry point and command wiring.
// Author: Lukas Bower
//! SwarmUI desktop entry point and Tauri command wiring.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::env;
use std::sync::Mutex;
use std::time::Duration;

use tauri::State;

use swarmui::{
    parse_role_label, SwarmUiBackend, SwarmUiConfig, SwarmUiTranscript, TcpTransportFactory,
};

struct AppState {
    backend: Mutex<SwarmUiBackend<TcpTransportFactory>>,
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

fn main() {
    let data_dir = tauri::api::path::data_dir().unwrap_or_else(|| std::env::temp_dir());
    let config = SwarmUiConfig::from_generated(data_dir);
    let host = env::var("SWARMUI_9P_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = env::var("SWARMUI_9P_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(5640);
    let timeout = Duration::from_secs(2);
    let factory = TcpTransportFactory::new(
        host,
        port,
        timeout,
        swarmui::SECURE9P_MSIZE,
    );
    let state = AppState {
        backend: Mutex::new(SwarmUiBackend::new(config, factory)),
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            swarmui_connect,
            swarmui_offline,
            swarmui_tail_telemetry,
            swarmui_list_namespace,
            swarmui_fleet_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SwarmUI");
}
