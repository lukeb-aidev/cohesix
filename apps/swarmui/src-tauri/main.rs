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

use cohsh::ticket_mint::{mint_ticket_from_config, mint_ticket_from_secret, TicketMintRequest};
use cohsh::COHSH_TCP_PORT;
use cohsh_core::command::MAX_LINE_LEN;
use cohsh_core::trace::{TraceLog, TracePolicy};
use swarmui::{
    parse_role_label, SwarmUiBackend, SwarmUiConfig, SwarmUiConsoleBackend, SwarmUiTranscript,
    TcpTransportFactory, TraceTransportFactory,
};

enum SwarmUiService {
    Secure9p(SwarmUiBackend<TcpTransportFactory>),
    Trace(SwarmUiBackend<TraceTransportFactory>),
    Console(SwarmUiConsoleBackend),
}

impl SwarmUiService {
    fn attach(&mut self, role: cohesix_ticket::Role, ticket: Option<&str>) -> SwarmUiTranscript {
        match self {
            SwarmUiService::Secure9p(backend) => backend.attach(role, ticket),
            SwarmUiService::Trace(backend) => backend.attach(role, ticket),
            SwarmUiService::Console(backend) => backend.attach(role, ticket),
        }
    }

    fn set_offline(&mut self, offline: bool) {
        match self {
            SwarmUiService::Secure9p(backend) => backend.set_offline(offline),
            SwarmUiService::Trace(backend) => backend.set_offline(offline),
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
            SwarmUiService::Trace(backend) => backend.tail_telemetry(role, ticket, worker_id),
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
            SwarmUiService::Trace(backend) => backend.list_namespace(role, ticket, path),
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
            SwarmUiService::Trace(backend) => backend.fleet_snapshot(role, ticket),
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
            SwarmUiService::Trace(backend) => backend
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
            SwarmUiService::Trace(backend) => backend
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
            SwarmUiService::Trace(backend) => backend
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
            SwarmUiService::Trace(backend) => backend
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

struct MintArgs {
    role: Option<String>,
    subject: Option<String>,
    config: Option<PathBuf>,
    secret: Option<String>,
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

#[tauri::command]
fn swarmui_mint_ticket(role: String, subject: Option<String>) -> Result<String, String> {
    mint_ticket_for_role(&role, subject.as_deref(), None, None)
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

fn parse_trace_replay_path(args: &[String]) -> Option<PathBuf> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--replay-trace" {
            return iter.next().map(PathBuf::from);
        }
        if let Some(value) = arg.strip_prefix("--replay-trace=") {
            return Some(PathBuf::from(value));
        }
    }
    None
}

fn parse_mint_args(args: &[String]) -> Option<MintArgs> {
    let mut mint = false;
    let mut role = None;
    let mut subject = None;
    let mut config = None;
    let mut secret = None;
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--mint-ticket" {
            mint = true;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--role=") {
            role = Some(value.to_owned());
            continue;
        }
        if arg == "--role" {
            if let Some(value) = iter.next() {
                role = Some(value.to_owned());
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--ticket-subject=") {
            subject = Some(value.to_owned());
            continue;
        }
        if arg == "--ticket-subject" {
            if let Some(value) = iter.next() {
                subject = Some(value.to_owned());
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--ticket-config=") {
            config = Some(PathBuf::from(value));
            continue;
        }
        if arg == "--ticket-config" {
            if let Some(value) = iter.next() {
                config = Some(PathBuf::from(value));
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--ticket-secret=") {
            secret = Some(value.to_owned());
            continue;
        }
        if arg == "--ticket-secret" {
            if let Some(value) = iter.next() {
                secret = Some(value.to_owned());
            }
        }
    }
    if mint {
        Some(MintArgs {
            role,
            subject,
            config,
            secret,
        })
    } else {
        None
    }
}

fn resolve_ticket_config(cli_path: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = cli_path {
        return Ok(path);
    }
    if let Ok(value) = env::var("SWARMUI_TICKET_CONFIG")
        .or_else(|_| env::var("COHSH_TICKET_CONFIG"))
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    Ok(PathBuf::from("configs/root_task.toml"))
}

fn resolve_ticket_secret(cli_secret: Option<String>) -> Result<Option<String>, String> {
    if cli_secret.is_some() {
        return Ok(cli_secret);
    }
    match env::var("SWARMUI_TICKET_SECRET")
        .or_else(|_| env::var("COHSH_TICKET_SECRET"))
    {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_owned()))
            }
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(format!("failed to read SWARMUI_TICKET_SECRET: {err}")),
    }
}

fn mint_ticket_for_role(
    role_label: &str,
    subject: Option<&str>,
    config: Option<PathBuf>,
    secret: Option<String>,
) -> Result<String, String> {
    let role = parse_role_label(role_label).map_err(|err| err.to_string())?;
    let request =
        TicketMintRequest::new(role, subject, None).map_err(|err| err.to_string())?;
    if let Some(secret) = resolve_ticket_secret(secret)? {
        return mint_ticket_from_secret(&request, secret.as_str())
            .map_err(|err| err.to_string());
    }
    let config_path = resolve_ticket_config(config)?;
    mint_ticket_from_config(&request, config_path.as_path()).map_err(|err| err.to_string())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mint_args = parse_mint_args(&args);
    let replay_path = parse_replay_path(&args);
    let trace_replay_path = parse_trace_replay_path(&args);
    if let Some(mint_args) = mint_args {
        if replay_path.is_some() || trace_replay_path.is_some() {
            eprintln!("cannot use --mint-ticket with --replay or --replay-trace");
            std::process::exit(2);
        }
        let role = mint_args
            .role
            .ok_or_else(|| "missing --role for --mint-ticket")
            .unwrap_or_else(|err| {
                eprintln!("{err}");
                std::process::exit(2);
            });
        let token = mint_ticket_for_role(
            &role,
            mint_args.subject.as_deref(),
            mint_args.config,
            mint_args.secret,
        )
        .unwrap_or_else(|err| {
            eprintln!("{err}");
            std::process::exit(2);
        });
        println!("{token}");
        return;
    }
    if replay_path.is_some() && trace_replay_path.is_some() {
        panic!("cannot use --replay and --replay-trace together");
    }
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
    let mut backend = if let Some(path) = trace_replay_path.clone() {
        let resolved = if path.is_relative() {
            data_dir.join("traces").join(path)
        } else {
            path
        };
        let payload = fs::read(&resolved)
            .unwrap_or_else(|err| panic!("failed to read trace {}: {err}", resolved.display()));
        let policy = TracePolicy::new(
            config.trace_max_bytes as u32,
            swarmui::SECURE9P_MSIZE,
            MAX_LINE_LEN as u32,
        );
        let trace = TraceLog::decode(&payload, policy)
            .unwrap_or_else(|err| panic!("failed to decode trace: {err}"));
        let factory = TraceTransportFactory::new(trace.frames);
        SwarmUiService::Trace(SwarmUiBackend::new(config, factory))
    } else {
        match transport.as_str() {
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
        }
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
            swarmui_mint_ticket,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SwarmUI");
}
