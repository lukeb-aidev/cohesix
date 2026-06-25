// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: SwarmUI Tauri entry point and command wiring.
// Author: Lukas Bower
//! SwarmUI desktop entry point and Tauri command wiring.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::env;
use std::fs;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{Manager, State};

#[cfg(feature = "rest")]
use cohsh::RestTransport as CohshRestTransport;
use cohsh::TcpTransport as CohshTcpTransport;
use cohsh::COHSH_TCP_PORT;
use cohsh_core::command::MAX_LINE_LEN;
use cohsh_core::trace::{TraceLog, TracePolicy};
#[cfg(feature = "rest")]
use swarmui::resolve_rest_auth_token;
use swarmui::{
    mint_ticket_for_role, parse_mint_args, parse_replay_path, parse_role_label,
    parse_trace_replay_path, resolve_console_auth_token, resolve_replay_path, SwarmUiBackend,
    SwarmUiConfig, SwarmUiConsoleBackend, SwarmUiLogDump, SwarmUiTranscript, TcpTransportFactory,
    TraceTransportFactory,
};

enum SwarmUiService {
    Secure9p(SwarmUiBackend<TcpTransportFactory>),
    Trace(SwarmUiBackend<TraceTransportFactory>),
    Console(SwarmUiConsoleBackend<CohshTcpTransport>),
    #[cfg(feature = "rest")]
    Rest(SwarmUiConsoleBackend<CohshRestTransport>),
}

impl SwarmUiService {
    fn attach(&mut self, role: cohesix_ticket::Role, ticket: Option<&str>) -> SwarmUiTranscript {
        match self {
            SwarmUiService::Secure9p(backend) => backend.attach(role, ticket),
            SwarmUiService::Trace(backend) => backend.attach(role, ticket),
            SwarmUiService::Console(backend) => backend.attach(role, ticket),
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => backend.attach(role, ticket),
        }
    }

    fn set_offline(&mut self, offline: bool) {
        match self {
            SwarmUiService::Secure9p(backend) => backend.set_offline(offline),
            SwarmUiService::Trace(backend) => backend.set_offline(offline),
            SwarmUiService::Console(backend) => backend.set_offline(offline),
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => backend.set_offline(offline),
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
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => backend.tail_telemetry(role, ticket, worker_id),
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
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => backend.list_namespace(role, ticket, path),
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
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => backend.fleet_snapshot(role, ticket),
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
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => backend
                .hive_bootstrap(role, ticket, snapshot_key)
                .map_err(|err| err.to_string()),
        }
    }

    fn hive_poll(
        &mut self,
        role: cohesix_ticket::Role,
        ticket: Option<&str>,
        detail_agent: Option<&str>,
    ) -> Result<swarmui::SwarmUiHiveBatch, String> {
        match self {
            SwarmUiService::Secure9p(backend) => backend
                .hive_poll(role, ticket, detail_agent)
                .map_err(|err| err.to_string()),
            SwarmUiService::Trace(backend) => backend
                .hive_poll(role, ticket, detail_agent)
                .map_err(|err| err.to_string()),
            SwarmUiService::Console(backend) => backend
                .hive_poll(role, ticket, detail_agent)
                .map_err(|err| err.to_string()),
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => backend
                .hive_poll(role, ticket, detail_agent)
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
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => backend
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
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => backend
                .load_hive_replay(payload)
                .map_err(|err| err.to_string()),
        }
    }

    fn console_command(&mut self, line: &str) -> Result<SwarmUiTranscript, String> {
        match self {
            SwarmUiService::Secure9p(backend) => Ok(backend.console_command(line)),
            SwarmUiService::Trace(backend) => Ok(backend.console_command(line)),
            SwarmUiService::Console(backend) => Ok(backend.console_command(line)),
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => Ok(backend.console_command(line)),
        }
    }

    fn dump_queen_log(&mut self) -> Result<SwarmUiLogDump, String> {
        match self {
            SwarmUiService::Secure9p(backend) => {
                backend.dump_queen_log().map_err(|err| err.to_string())
            }
            SwarmUiService::Trace(backend) => {
                backend.dump_queen_log().map_err(|err| err.to_string())
            }
            SwarmUiService::Console(backend) => {
                backend.dump_queen_log().map_err(|err| err.to_string())
            }
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => {
                backend.dump_queen_log().map_err(|err| err.to_string())
            }
        }
    }
}

struct AppState {
    backend: Mutex<SwarmUiService>,
    mode: SwarmUiMode,
}

#[derive(Clone, Serialize)]
struct SwarmUiMode {
    trace_replay: bool,
    hive_replay: bool,
    offline: bool,
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
fn swarmui_console_command(
    state: State<'_, AppState>,
    line: String,
) -> Result<SwarmUiTranscript, String> {
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    backend.console_command(&line)
}

#[tauri::command]
fn swarmui_dump_queen_log(state: State<'_, AppState>) -> Result<SwarmUiLogDump, String> {
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    backend.dump_queen_log()
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
    detail_agent: Option<String>,
) -> Result<swarmui::SwarmUiHiveBatch, String> {
    let role = parse_role_label(&role).map_err(|err| err.to_string())?;
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    backend.hive_poll(role, ticket.as_deref(), detail_agent.as_deref())
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

#[tauri::command]
fn swarmui_mode(state: State<'_, AppState>) -> SwarmUiMode {
    state.mode.clone()
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
            .ok_or("missing --role for --mint-ticket")
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
    let trace_replay = trace_replay_path.is_some();
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
    tauri::Builder::default()
        .setup(move |app| {
            let data_dir = app
                .path()
                .data_dir()
                .unwrap_or_else(|_| std::env::temp_dir());
            let mut config = SwarmUiConfig::from_generated(data_dir.clone());
            if replay_path.is_some() {
                config.offline = true;
            }
            let offline = config.offline;
            let mut trace_replay_resolved = None;
            let mut backend = if let Some(path) = trace_replay_path.clone() {
                let resolved = resolve_replay_path(&path, &data_dir, "traces");
                trace_replay_resolved = Some(resolved.clone());
                let payload = fs::read(&resolved).unwrap_or_else(|err| {
                    panic!("failed to read trace {}: {err}", resolved.display())
                });
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
                        let factory =
                            TcpTransportFactory::new(host, port, timeout, swarmui::SECURE9P_MSIZE);
                        SwarmUiService::Secure9p(SwarmUiBackend::new(config, factory))
                    }
                    "console" | "tcp" => {
                        let auth_token = resolve_console_auth_token().unwrap_or_else(|err| {
                            panic!("failed to resolve SwarmUI console auth token: {err}")
                        });
                        SwarmUiService::Console(SwarmUiConsoleBackend::new(
                            config, host, port, auth_token,
                        ))
                    }
                    "rest" | "gateway" => {
                        #[cfg(feature = "rest")]
                        {
                            let rest_url = env::var("SWARMUI_REST_URL")
                                .or_else(|_| env::var("COH_REST_URL"))
                                .unwrap_or_else(|_| "http://127.0.0.1:8080".to_owned());
                            let rest_auth_token = resolve_rest_auth_token();
                            let transport =
                                CohshRestTransport::new(rest_url.clone(), rest_auth_token.clone());
                            SwarmUiService::Rest(SwarmUiConsoleBackend::with_rest_transport(
                                config,
                                transport,
                                rest_url,
                                rest_auth_token,
                            ))
                        }
                        #[cfg(not(feature = "rest"))]
                        {
                            panic!(
                                "SWARMUI_TRANSPORT=rest requires swarmui built with --features rest"
                            );
                        }
                    }
                    other => {
                        panic!("unsupported SWARMUI_TRANSPORT '{other}' (use console, 9p, or rest)")
                    }
                }
            };
            let mut hive_replay_loaded = false;
            if let Some(resolved) = trace_replay_resolved.as_ref() {
                let hive_path = resolved.with_extension("hive.cbor");
                if hive_path.is_file() {
                    let payload = fs::read(&hive_path).unwrap_or_else(|err| {
                        panic!("failed to read hive replay {}: {err}", hive_path.display())
                    });
                    backend
                        .load_hive_replay(&payload)
                        .unwrap_or_else(|err| panic!("failed to load hive replay: {err}"));
                    hive_replay_loaded = true;
                }
            }
            if let Some(path) = replay_path.clone() {
                let resolved = resolve_replay_path(&path, &data_dir, "snapshots");
                let payload = fs::read(&resolved).unwrap_or_else(|err| {
                    panic!("failed to read replay {}: {err}", resolved.display())
                });
                backend
                    .load_hive_replay(&payload)
                    .unwrap_or_else(|err| panic!("failed to load replay: {err}"));
                hive_replay_loaded = true;
            }
            app.manage(AppState {
                backend: Mutex::new(backend),
                mode: SwarmUiMode {
                    trace_replay,
                    hive_replay: hive_replay_loaded,
                    offline,
                },
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            swarmui_connect,
            swarmui_offline,
            swarmui_tail_telemetry,
            swarmui_list_namespace,
            swarmui_fleet_snapshot,
            swarmui_console_command,
            swarmui_dump_queen_log,
            swarmui_hive_bootstrap,
            swarmui_hive_poll,
            swarmui_hive_reset,
            swarmui_mint_ticket,
            swarmui_mode,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SwarmUI");
}
