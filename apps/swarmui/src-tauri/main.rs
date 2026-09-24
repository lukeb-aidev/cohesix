// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: SwarmUI Tauri entry point, governed command wiring and Apple Keychain enrollment.
// Author: Lukas Bower
//! SwarmUI desktop entry point and Tauri command wiring.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::Duration;
use swarmui::workbench::{self, Connection, ConnectionRequest, HostRequest, HostResult};

use serde::Serialize;
use tauri::{Manager, State};

#[cfg(feature = "rest")]
use cohsh::RestTransport as CohshRestTransport;
use cohsh::TcpTransport as CohshTcpTransport;
use cohsh::COHSH_TCP_PORT;
use cohsh_core::command::MAX_LINE_LEN;
use cohsh_core::trace::TracePolicy;
#[cfg(feature = "rest")]
use swarmui::resolve_rest_auth_token;
use swarmui::{
    mint_ticket_for_role, parse_mint_args, parse_replay_path, parse_role_label,
    parse_trace_replay_path, resolve_console_auth_token, resolve_replay_path, SwarmUiBackend,
    SwarmUiConfig, SwarmUiConsoleBackend, SwarmUiLogDump, SwarmUiTranscript, TcpTransportFactory,
    TraceTransportFactory,
};

const SWARMUI_HELP: &str = "\
SwarmUI desktop client for Cohesix

Usage: swarmui [OPTIONS]

Options:
      --replay <FILE>             Load a Hive CBOR snapshot for offline replay
      --replay-trace <FILE>       Load a Secure9P trace for offline replay
      --mint-ticket               Mint a capability ticket and exit
      --role <ROLE>               Role for --mint-ticket
      --ticket-subject <SUBJECT>  Subject identity for --mint-ticket
      --ticket-config <FILE>      Ticket configuration for --mint-ticket
      --ticket-secret <SECRET>    Ticket signing secret for --mint-ticket
  -h, --help                      Print help

Open Connect a hive to configure Queen TCP or Hive Gateway access in the app.
Operations provides reviewed forms for installed coh commands. Settings selects
matching installed tools. Existing SWARMUI_* environment inputs remain supported.
";

fn help_requested(args: &[String]) -> bool {
    args.iter()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
}

enum SwarmUiService {
    Secure9p(SwarmUiBackend<TcpTransportFactory>),
    Trace(SwarmUiBackend<TraceTransportFactory>),
    Console(SwarmUiConsoleBackend<CohshTcpTransport>),
    Captured(SwarmUiConsoleBackend<cohsh::trace_capture::CapturedNamespace>),
    #[cfg(feature = "rest")]
    Rest(SwarmUiConsoleBackend<CohshRestTransport>),
}

impl SwarmUiService {
    fn attach(&mut self, role: cohesix_ticket::Role, ticket: Option<&str>) -> SwarmUiTranscript {
        match self {
            SwarmUiService::Secure9p(backend) => backend.attach(role, ticket),
            SwarmUiService::Trace(backend) => backend.attach(role, ticket),
            SwarmUiService::Console(backend) => backend.attach(role, ticket),
            SwarmUiService::Captured(backend) => backend.attach(role, ticket),
            #[cfg(feature = "rest")]
            SwarmUiService::Rest(backend) => backend.attach(role, ticket),
        }
    }

    fn set_offline(&mut self, offline: bool) {
        match self {
            SwarmUiService::Secure9p(backend) => backend.set_offline(offline),
            SwarmUiService::Trace(backend) => backend.set_offline(offline),
            SwarmUiService::Console(backend) => backend.set_offline(offline),
            SwarmUiService::Captured(backend) => backend.set_offline(offline),
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
            SwarmUiService::Captured(backend) => backend.tail_telemetry(role, ticket, worker_id),
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
            SwarmUiService::Captured(backend) => backend.list_namespace(role, ticket, path),
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
            SwarmUiService::Captured(backend) => backend.fleet_snapshot(role, ticket),
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
            SwarmUiService::Captured(backend) => backend
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
            SwarmUiService::Captured(backend) => backend
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
            SwarmUiService::Captured(backend) => backend
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
            SwarmUiService::Captured(backend) => backend
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
            SwarmUiService::Captured(backend) => Ok(backend.console_command(line)),
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
            SwarmUiService::Captured(backend) => {
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
    mode: Mutex<SwarmUiMode>,
    data_dir: PathBuf,
    connection: Mutex<Option<Connection>>,
    configured: Mutex<Connection>,
    host_busy: AtomicBool,
    tool_dir: Mutex<PathBuf>,
    review: Mutex<workbench::ReviewGate>,
    acceptance: Option<Mutex<swarmui::native_evidence::Recorder>>,
}

impl AppState {
    fn record(&self, action: &str, observation: Value) -> Result<(), String> {
        if let Some(recorder) = &self.acceptance {
            recorder
                .lock()
                .map_err(|_| "state locked")?
                .record(action, observation)?;
        }
        Ok(())
    }
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
    let parsed_role = parse_role_label(&role).map_err(|err| err.to_string())?;
    let mut review = state.review.lock().map_err(|_| "state locked")?;
    if state.host_busy.load(Ordering::Acquire) {
        return Err("busy: wait for the host operation".into());
    }
    review.invalidate();
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    let transcript = backend.attach(parsed_role, ticket.as_deref());
    if transcript.ok {
        let mut connection = state.configured.lock().map_err(|_| "state locked")?.clone();
        connection.role = role;
        connection.ticket = ticket;
        *state.connection.lock().map_err(|_| "state locked")? = Some(connection);
    }
    Ok(transcript)
}

#[tauri::command]
fn swarmui_offline(state: State<'_, AppState>, offline: bool) -> Result<(), String> {
    let mut review = state.review.lock().map_err(|_| "state locked")?;
    if state.host_busy.load(Ordering::Acquire) {
        return Err("busy: wait for the host operation".into());
    }
    if !offline
        && state
            .connection
            .lock()
            .map_err(|_| "state locked")?
            .is_none()
    {
        return Err("disconnected: connect explicitly to return live".into());
    }
    review.invalidate();
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    let mut mode = state.mode.lock().map_err(|_| "state locked")?;
    if !offline && (mode.trace_replay || mode.hive_replay) {
        return Err("replay_isolated: reconnect explicitly to return live".into());
    }
    backend.set_offline(offline);
    mode.offline = offline;
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
    let active = state.connection.lock().map_err(|_| "state locked")?;
    let ticket = active.as_ref().and_then(|c| c.ticket.clone()).or(ticket);
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
    let active = state.connection.lock().map_err(|_| "state locked")?;
    let ticket = active.as_ref().and_then(|c| c.ticket.clone()).or(ticket);
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
    let active = state.connection.lock().map_err(|_| "state locked")?;
    let ticket = active.as_ref().and_then(|c| c.ticket.clone()).or(ticket);
    Ok(backend.fleet_snapshot(role, ticket.as_deref()))
}

#[tauri::command]
fn swarmui_console_command(
    state: State<'_, AppState>,
    line: String,
) -> Result<SwarmUiTranscript, String> {
    let mut review = state.review.lock().map_err(|_| "state locked")?;
    if state.host_busy.load(Ordering::Acquire) {
        return Err("busy: wait for the host operation".into());
    }
    review.invalidate();
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    let result = backend.console_command(&line)?;
    let verb = line.split_whitespace().next().unwrap_or_default();
    if !result.ok && matches!(verb, "attach" | "login") {
        *state.connection.lock().map_err(|_| "state locked")? = None;
    }
    if result.ok {
        let trimmed = line.trim();
        let canonical = if let Some(rest) = trimmed.strip_prefix("login ") {
            format!("attach {rest}")
        } else {
            trimmed.to_owned()
        };
        if matches!(trimmed, "quit" | "detach") {
            *state.connection.lock().map_err(|_| "state locked")? = None;
            // The expert console may explicitly attach again using the configured transport.
        } else if let Ok(cohsh_core::Command::Attach { role, ticket }) =
            cohsh_core::CommandParser::parse_line_str(&canonical)
        {
            let mut connection = state.configured.lock().map_err(|_| "state locked")?.clone();
            connection.role = role.to_string();
            connection.ticket = ticket.map(|value| value.to_string());
            *state.connection.lock().map_err(|_| "state locked")? = Some(connection);
        }
    }
    Ok(result)
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
    let active = state.connection.lock().map_err(|_| "state locked")?;
    let ticket = active.as_ref().and_then(|c| c.ticket.clone()).or(ticket);
    let result = backend.hive_bootstrap(role, ticket.as_deref(), snapshot_key.as_deref())?;
    state.record(
        "hive_bootstrap",
        json!({"agents":result.agents.len(),"replay":result.replay}),
    )?;
    Ok(result)
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
    let active = state.connection.lock().map_err(|_| "state locked")?;
    let ticket = active.as_ref().and_then(|c| c.ticket.clone()).or(ticket);
    let result = backend.hive_poll(role, ticket.as_deref(), detail_agent.as_deref())?;
    state.record("hive_poll",json!({"response_sha256":workbench::digest(serde_json::to_string(&result).map_err(|_|"hive serialization")?.as_bytes())}))?;
    Ok(result)
}

#[tauri::command]
fn swarmui_hive_reset(
    state: State<'_, AppState>,
    role: String,
    ticket: Option<String>,
) -> Result<(), String> {
    let role = parse_role_label(&role).map_err(|err| err.to_string())?;
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    let active = state.connection.lock().map_err(|_| "state locked")?;
    let ticket = active.as_ref().and_then(|c| c.ticket.clone()).or(ticket);
    backend.hive_reset(role, ticket.as_deref())
}

#[tauri::command]
fn swarmui_mint_ticket(role: String, subject: Option<String>) -> Result<String, String> {
    mint_ticket_for_role(&role, subject.as_deref(), None, None)
}

#[tauri::command]
fn swarmui_mode(state: State<'_, AppState>) -> SwarmUiMode {
    state.mode.lock().map(|m| m.clone()).unwrap_or(SwarmUiMode {
        trace_replay: false,
        hive_replay: false,
        offline: true,
    })
}

#[tauri::command]
fn swarmui_apple_actions_enrol(state: State<'_, AppState>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mode = state.mode.lock().map_err(|_| "state locked")?.clone();
        if mode.offline || mode.trace_replay || mode.hive_replay {
            return Err(
                "offline: connect a delegated Hive Gateway before enabling Apple actions".into(),
            );
        }
        let connection = state
            .connection
            .lock()
            .map_err(|_| "state locked")?
            .clone()
            .ok_or("disconnected: connect before enabling Apple actions")?;
        workbench::store_apple_delegation(&connection)?;
        state
            .record(
                "apple_actions_enrol",
                json!({"stored":true,"credential":false}),
            )
            .map_err(|_| {
                "Keychain saved, but the local action record failed; remove access before retrying"
            })?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
        Err("unsupported: Apple actions require macOS".into())
    }
}

#[tauri::command]
fn swarmui_apple_actions_remove(state: State<'_, AppState>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        workbench::remove_apple_delegation()?;
        state
            .record("apple_actions_remove", json!({"removed":true}))
            .map_err(|_| "Keychain access removed, but the local action record failed")?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
        Err("unsupported: Apple actions require macOS".into())
    }
}

#[tauri::command]
async fn swarmui_session_open(
    app: tauri::AppHandle,
    request: ConnectionRequest,
) -> Result<SwarmUiTranscript, String> {
    let connection = request.resolve().inspect_err(|error| {
        let _ = app.state::<AppState>().record(
            "connection_input_refused",
            json!({"reason":error.split(':').next()}),
        );
    })?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut review = state.review.lock().map_err(|_| "state locked")?;
        if state.host_busy.load(Ordering::Acquire) {
            return Err("busy: wait for the current host operation".into());
        }
        review.invalidate();
        let mut backend = state.backend.lock().map_err(|_| "state locked")?;
        let mut active = state.connection.lock().map_err(|_| "state locked")?;
        let _ = backend.console_command("quit");
        *active = None;
        let config = SwarmUiConfig::from_generated(state.data_dir.clone());
        let mut candidate = if connection.transport == "console" {
            SwarmUiService::Console(SwarmUiConsoleBackend::new(
                config,
                connection.host.clone(),
                connection.port,
                connection.credential.clone(),
            ))
        } else {
            #[cfg(feature = "rest")]
            {
                SwarmUiService::Rest(SwarmUiConsoleBackend::with_rest_transport(
                    config,
                    CohshRestTransport::new(
                        connection.endpoint.clone(),
                        Some(connection.credential.clone()),
                    ),
                    connection.endpoint.clone(),
                    Some(connection.credential.clone()),
                ))
            }
            #[cfg(not(feature = "rest"))]
            {
                return Err("unsupported: this build has no gateway transport".into());
            }
        };
        let role = parse_role_label(&connection.role).map_err(|e| e.to_string())?;
        let transcript = candidate.attach(role, connection.ticket.as_deref());
        if transcript.ok {
            *state.configured.lock().map_err(|_| "state locked")? = connection.clone();
            *active = Some(connection);
            *state.mode.lock().map_err(|_| "state locked")? = SwarmUiMode {
                trace_replay: false,
                hive_replay: false,
                offline: false,
            };
        } else {
            candidate.set_offline(true);
            *state.mode.lock().map_err(|_| "state locked")? = SwarmUiMode {
                trace_replay: false,
                hive_replay: false,
                offline: true,
            };
        }
        *backend = candidate;
        state.record("connect",json!({"ok":transcript.ok,"endpoint":request.endpoint,"transport":request.transport,"role":request.role}))?;
        Ok(transcript)
    })
    .await
    .map_err(|_| "connection: session task failed".to_owned())?
}

#[tauri::command]
fn swarmui_session_close(state: State<'_, AppState>) -> Result<(), String> {
    let mut review = state.review.lock().map_err(|_| "state locked")?;
    if state.host_busy.load(Ordering::Acquire) {
        return Err("busy: wait for the host operation before disconnecting".into());
    }
    review.invalidate();
    let mut backend = state.backend.lock().map_err(|_| "state locked")?;
    let _ = backend.console_command("quit");
    backend.set_offline(true);
    *state.connection.lock().map_err(|_| "state locked")? = None;
    state.mode.lock().map_err(|_| "state locked")?.offline = true;
    state.record("disconnect", json!({"offline":true}))?;
    Ok(())
}

#[tauri::command]
fn swarmui_workbench_info(state: State<'_, AppState>) -> Result<Value, String> {
    let config = SwarmUiConfig::from_generated(state.data_dir.clone());
    let active = state.connection.lock().map_err(|_| "state locked")?;
    Ok(
        json!({"catalog":workbench::host_catalog(), "controls":workbench::structured_controls(), "roots":config.paths.namespace_roots,
        "connection":active.as_ref().map(|c| json!({"endpoint":c.endpoint,"transport":c.transport,"role":c.role,"delegated":c.ticket.is_some()})),
        "tool_directory":state.tool_dir.lock().map_err(|_| "state locked")?.to_string_lossy(),
        "mode":swarmui_mode(state.clone())}),
    )
}

#[tauri::command]
fn swarmui_control(
    state: State<'_, AppState>,
    request: workbench::ControlRequest,
    submit: bool,
    review_id: Option<String>,
) -> Result<Value, String> {
    let mut review = state.review.lock().map_err(|_| "state locked")?;
    if state.host_busy.load(Ordering::Acquire) {
        return Err("busy: wait for the host operation".into());
    }
    if state.mode.lock().map_err(|_| "state locked")?.offline {
        state.record("control_refused", json!({"reason":"offline"}))?;
        return Err("offline: reconnect before submitting control actions".into());
    }
    if state
        .connection
        .lock()
        .map_err(|_| "state locked")?
        .is_none()
    {
        return Err("disconnected: connect first".into());
    }
    if matches!(
        request.action.as_str(),
        "bootinfo"
            | "caps"
            | "smp"
            | "mem"
            | "cachelog"
            | "test"
            | "nettest"
            | "netstats"
            | "reboot"
    ) && state
        .connection
        .lock()
        .map_err(|_| "state locked")?
        .as_ref()
        .is_some_and(|c| c.transport == "rest")
    {
        return Err("unsupported_transport: this root diagnostic requires direct Queen TCP; use Namespace for published state through a gateway".into());
    }
    let roots = SwarmUiConfig::from_generated(state.data_dir.clone())
        .paths
        .namespace_roots;
    let line = workbench::control_line(&request, &roots)?;
    let action = json!({"control":request});
    if !submit {
        let id = review.preview(action);
        return Ok(json!({"preview":line,"review_id":id}));
    }
    review.consume(review_id.as_deref().unwrap_or(""), &action)?;
    let result = state
        .backend
        .lock()
        .map_err(|_| "state locked")?
        .console_command(&line)?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

#[tauri::command]
fn swarmui_namespace(
    state: State<'_, AppState>,
    verb: String,
    path: String,
) -> Result<SwarmUiTranscript, String> {
    let roots = SwarmUiConfig::from_generated(state.data_dir.clone())
        .paths
        .namespace_roots;
    let line = workbench::namespace_command(&verb, &path, &roots)?;
    let result = state
        .backend
        .lock()
        .map_err(|_| "state locked")?
        .console_command(&line)?;
    state.record("namespace",json!({"verb":verb,"path":path,"ok":result.ok,"lines":result.lines.len(),"transcript_sha256":workbench::digest(result.lines.join("\n").as_bytes())}))?;
    Ok(result)
}

#[tauri::command]
fn swarmui_host_preview(state: State<'_, AppState>, request: HostRequest) -> Result<Value, String> {
    let mut review = state.review.lock().map_err(|_| "state locked")?;
    let active = state.connection.lock().map_err(|_| "state locked")?;
    let offline = state.mode.lock().map_err(|_| "state locked")?.offline;
    let argv = workbench::host_arguments(&request, active.as_ref(), offline)?;
    let id = review.preview(json!({"host":request}));
    Ok(
        json!({"argv":argv,"review_id":id,"authority":"The installed tool validates deployment policy and delegated authority. Command completion does not establish a verified outcome."}),
    )
}

#[tauri::command]
async fn swarmui_host_run(
    app: tauri::AppHandle,
    request: HostRequest,
    review_id: String,
) -> Result<HostResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut review = state.review.lock().map_err(|_| "state locked")?;
        if state.host_busy.load(Ordering::Acquire) {
            return Err("busy: another host operation is running".into());
        }
        review.consume(&review_id, &json!({"host":request}))?;
        state.host_busy.store(true, Ordering::Release);
        drop(review);
        let result = (|| {
            let active = state.connection.lock().map_err(|_| "state locked")?.clone();
            let offline = state.mode.lock().map_err(|_| "state locked")?.offline;
            let directory = state.tool_dir.lock().map_err(|_| "state locked")?.clone();
            let result=workbench::run_host(&directory, &request, active.as_ref(), offline)?;
            state.record("host",json!({"operation":request.operation,"success":result.success,"outcome":result.outcome,"stdout_sha256":workbench::digest(result.stdout.as_bytes())}))?;
            Ok(result)
        })();
        state.host_busy.store(false, Ordering::Release);
        result
    })
    .await
    .map_err(|_| "host_operation: task failed".to_owned())?
}

#[tauri::command]
fn swarmui_tool_directory(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let mut review = state.review.lock().map_err(|_| "state locked")?;
    if state.host_busy.load(Ordering::Acquire) {
        return Err("busy: wait for the host operation".into());
    }
    let directory = PathBuf::from(path);
    if !directory.is_absolute() || !directory.join("coh").is_file() {
        return Err("invalid_tool_directory: choose an installed bundle containing coh".into());
    }
    review.invalidate();
    *state.tool_dir.lock().map_err(|_| "state locked")? = directory;
    Ok(())
}

#[tauri::command]
async fn swarmui_choose_path(kind: String) -> Result<Option<String>, String> {
    if !matches!(kind.as_str(), "file" | "folder" | "save") {
        return Err("invalid_picker: choose a file or folder".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = rfd::FileDialog::new().set_title("Choose a Cohesix artifact or installation");
        let path = match kind.as_str() {
            "folder" => dialog.pick_folder(),
            "save" => dialog.save_file(),
            _ => dialog.pick_file(),
        };
        path.map(|p| {
            p.into_os_string()
                .into_string()
                .map_err(|_| "invalid_path: file path must be UTF-8".to_owned())
        })
        .transpose()
    })
    .await
    .map_err(|_| "picker: native dialog failed".to_owned())?
}

#[tauri::command]
async fn swarmui_reference(app: tauri::AppHandle, name: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut review = state.review.lock().map_err(|_| "state locked")?;
        if state.host_busy.load(Ordering::Acquire) {
            return Err("busy: wait for the host operation".into());
        }
        review.invalidate();
        let mut backend = state.backend.lock().map_err(|_| "state locked")?;
        let _ = backend.console_command("quit");
        backend.set_offline(true);
        *state.connection.lock().map_err(|_| "state locked")? = None;
        *state.mode.lock().map_err(|_| "state locked")? = SwarmUiMode {
            trace_replay: true,
            hive_replay: false,
            offline: true,
        };
        let directory = state.tool_dir.lock().map_err(|_| "state locked")?.clone();
        let story=swarmui::reference::open(&name, &state.data_dir, &directory)?;
        state.record("reference",json!({"name":name,"graph_sha256":story["graph"]["graph_sha256"],"outcome":story["graph"]["outcome"],"mode":"REPLAY","offline":true}))?;
        Ok(story)
    })
    .await
    .map_err(|_| "reference_operation: task failed".to_owned())?
}

#[tauri::command]
async fn swarmui_open_artifact(
    app: tauri::AppHandle,
    kind: String,
    path: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let state = app.state::<AppState>();
    let mut review = state.review.lock().map_err(|_| "state locked")?;
    if state.host_busy.load(Ordering::Acquire) { return Err("busy: wait for the host operation".into()); }
    let path = PathBuf::from(path);
    if !path.is_absolute() { return Err("invalid_artifact: choose an absolute artifact path".into()); }
    let mut config = SwarmUiConfig::from_generated(state.data_dir.clone());
    let result = match kind.as_str() {
        "pack" | "report" => workbench::inspect_artifact(&state.tool_dir.lock().map_err(|_| "state locked")?, &kind, &path)?,
        "trace" | "hive" => {
            review.invalidate();
            let bytes = workbench::read_artifact(&path, config.trace_max_bytes).map_err(|e|e.to_string())?;
            let policy = TracePolicy::new(config.trace_max_bytes as u32,swarmui::SECURE9P_MSIZE,MAX_LINE_LEN as u32);
            let mut counts=json!({"trace_frames":null,"ack_lines":null});
            let mut replay = if kind == "trace" {
                let trace = cohsh::trace_capture::read_trace(&path,policy).map_err(|e|e.to_string())?;
                counts=json!({"trace_frames":trace.frames.len(),"ack_lines":trace.ack_lines.len()});
                if trace.capture.is_some() {
                    let expected = cohsh::trace_capture::policy_digest(policy,cohsh::CohshPolicy::from_generated().trace.max_duration_ms);
                    let captured = cohsh::trace_capture::CapturedNamespace::new(&trace,&expected).map_err(|e|e.to_string())?;
                    config.offline=false; config.cache.enabled=false;
                    SwarmUiService::Captured(SwarmUiConsoleBackend::with_transport(config,captured))
                } else { SwarmUiService::Trace(SwarmUiBackend::new(config,TraceTransportFactory::new(trace.frames))) }
            } else {
                config.offline=true;
                SwarmUiService::Console(SwarmUiConsoleBackend::new(config,String::new(),1,String::new()))
            };
            if kind == "hive" { replay.load_hive_replay(&bytes)?; }
            let mut backend = state.backend.lock().map_err(|_| "state locked")?;
            let _ = backend.console_command("quit");
            *backend = replay;
            *state.connection.lock().map_err(|_| "state locked")? = None;
            *state.mode.lock().map_err(|_| "state locked")? = SwarmUiMode { trace_replay:kind=="trace",hive_replay:kind=="hive",offline:true };
            json!({"source":path,"sha256":workbench::digest(&bytes),"bytes":bytes.len(),"counts":counts,"limits":{"max_bytes":policy.max_bytes,"max_frame_bytes":policy.max_frame_bytes,"max_ack_bytes":policy.max_ack_bytes},"mode":"REPLAY","proof":"retained artifact; not current readiness"})
        },
        _ => return Err("invalid_artifact: choose an evidence pack, trace, hive snapshot or canonical report".into()),
    };
    state.record("artifact",json!({"kind":kind,"mode":swarmui_mode(state.clone()),"result_sha256":workbench::digest(result.to_string().as_bytes())}))?;
    Ok(result)
    }).await.map_err(|_| "artifact_operation: task failed".to_owned())?
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if help_requested(&args) {
        print!("{SWARMUI_HELP}");
        return;
    }
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
            let offline = config.offline || trace_replay;
            // Retain legacy launch configuration; authentication still happens only at attach.
            let configured = Connection {
                transport: if matches!(transport.as_str(), "gateway" | "rest") { "rest" } else { transport.as_str() }.to_owned(),
                endpoint: if matches!(transport.as_str(), "gateway" | "rest") {
                    env::var("SWARMUI_REST_URL").or_else(|_| env::var("COH_REST_URL")).unwrap_or_else(|_| "http://127.0.0.1:8080".into())
                } else { format!("tcp://{host}:{port}") },
                host: host.clone(), port,
                credential: if matches!(transport.as_str(), "gateway" | "rest") {
                    #[cfg(feature="rest")] { resolve_rest_auth_token().unwrap_or_default() }
                    #[cfg(not(feature="rest"))] { String::new() }
                } else { resolve_console_auth_token().unwrap_or_default() },
                role: "queen".into(), ticket: None,
            };
            let mut trace_replay_resolved = None;
            let mut backend = if let Some(path) = trace_replay_path.clone() {
                let resolved = resolve_replay_path(&path, &data_dir, "traces");
                let policy = TracePolicy::new(
                    config.trace_max_bytes as u32,
                    swarmui::SECURE9P_MSIZE,
                    MAX_LINE_LEN as u32,
                );
                let trace = cohsh::trace_capture::read_trace(&resolved, policy)?;
                if trace.capture.is_some() {
                    let expected = cohsh::trace_capture::policy_digest(
                        policy,
                        cohsh::CohshPolicy::from_generated().trace.max_duration_ms,
                    );
                    let captured = cohsh::trace_capture::CapturedNamespace::new(&trace, &expected)?;
                    // The supplied transport enforces offline, read-only access.
                    // Disable the separate cache-only path so retained observations
                    // reach the normal parsers; the app mode remains offline.
                    config.offline = false;
                    config.cache.enabled = false;
                    SwarmUiService::Captured(SwarmUiConsoleBackend::with_transport(
                        config, captured,
                    ))
                } else {
                    // The legacy paired hive snapshot has no binding to a live
                    // capture header. Only version-1 fixture replay may load it.
                    trace_replay_resolved = Some(resolved.clone());
                    let factory = TraceTransportFactory::new(trace.frames);
                    SwarmUiService::Trace(SwarmUiBackend::new(config, factory))
                }
            } else {
                match transport.as_str() {
                    "9p" | "secure9p" => {
                        let factory =
                            TcpTransportFactory::new(host, port, timeout, swarmui::SECURE9P_MSIZE);
                        SwarmUiService::Secure9p(SwarmUiBackend::new(config, factory))
                    }
                    "console" | "tcp" => {
                        // No network opens before attach; first launch can configure credentials in the UI.
                        let auth_token = resolve_console_auth_token().unwrap_or_default();
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
                mode: Mutex::new(SwarmUiMode {
                    trace_replay,
                    hive_replay: hive_replay_loaded,
                    offline,
                }),
                data_dir,
                connection: Mutex::new(None),
                configured: Mutex::new(configured),
                host_busy: AtomicBool::new(false),
                review: Mutex::new(workbench::ReviewGate::default()),
                acceptance: env::var_os("SWARMUI_ACCEPTANCE_LOG").map(|path|swarmui::native_evidence::Recorder::create(&PathBuf::from(path)).map(Mutex::new)).transpose()?,
                tool_dir: Mutex::new(workbench::tool_directory()?),
            });
            app.state::<AppState>().record("startup",json!({"pid":std::process::id(),"bridge":"tauri","fixture":false,"desktop_source_sha256":env!("SWARMUI_SOURCE_SHA256")}))?;
            Ok(())
        })
        .on_window_event(|window,event| {
            if let tauri::WindowEvent::CloseRequested {api,..}=event {
                if let Some(state)=window.try_state::<AppState>() {
                    if state.host_busy.load(Ordering::Acquire) {api.prevent_close();}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            swarmui_session_open,
            swarmui_session_close,
            swarmui_workbench_info,
            swarmui_namespace,
            swarmui_control,
            swarmui_host_preview,
            swarmui_host_run,
            swarmui_tool_directory,
            swarmui_open_artifact,
            swarmui_reference,
            swarmui_choose_path,
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
            swarmui_apple_actions_enrol,
            swarmui_apple_actions_remove,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build SwarmUI native application")
        .run(|app, event| {
            if let Some(state) = app.try_state::<AppState>() {
                match event {
                    tauri::RunEvent::ExitRequested { api, .. } => {
                        // Application-menu quit must preserve the same single-owner
                        // controller lifetime as the window close button.
                        if state.host_busy.load(Ordering::Acquire) {
                            api.prevent_exit();
                        }
                    }
                    tauri::RunEvent::Exit => {
                        let _ = state.record("shutdown", json!({"host_busy": false}));
                    }
                    _ => {}
                }
            }
        });
}
