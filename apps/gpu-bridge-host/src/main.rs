// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the host-side GPU bridge; prints mirrored namespace metadata.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! CLI entry point for the host-side GPU bridge. The binary prints discovered
//! GPU information as JSON, enabling integration tests to synchronise the
//! NineDoor namespace with host state.

use anyhow::{anyhow, Context, Result};
use clap::{ArgAction, Parser};
#[cfg(feature = "rest")]
use cohesix_rest::GatewayClient;
use cohsh_core::wire::{parse_ack, AckStatus};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use gpu_bridge_host::{
    auto_bridge_with_registry, build_publish_lines, namespace_to_json_pretty, GpuNamespaceSnapshot,
};

const DEFAULT_TCP_PORT: u16 = 31337;
const ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// CLI arguments for the GPU bridge host tool.
#[derive(Debug, Parser)]
#[command(author, version, about = "Cohesix GPU bridge host utilities")]
#[command(group(clap::ArgGroup::new("reference_mode").args(["reference_inventory", "reference_request", "mig_inventory"])))]
struct Args {
    /// Publish exact native device identity from the same pinned executor configuration.
    #[arg(long, conflicts_with_all = ["reference_mode", "mock", "workload_config"])]
    native_inventory_config: Option<PathBuf>,
    /// Serve admitted GPU workloads on the configured private authenticated Unix socket.
    #[arg(long, conflicts_with_all = ["reference_mode", "mock", "list", "publish"])]
    workload_config: Option<PathBuf>,
    /// Observe exact CUDA reference device identity without target publication.
    #[arg(long, requires_all = ["reference_helper", "reference_helper_sha256", "reference_state"], conflicts_with_all = ["mock", "publish", "list"])]
    reference_inventory: bool,
    /// Observe native NVML MIG instances without changing MIG mode or creating instances.
    #[arg(long, requires_all = ["reference_helper", "reference_helper_sha256", "reference_state"], conflicts_with_all = ["mock", "publish", "list"])]
    mig_inventory: bool,
    /// Physical parent index for explicit NVML MIG discovery.
    #[arg(long, requires = "mig_inventory", default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..32))]
    mig_parent_ordinal: u32,
    /// Exact independently enrolled MIG parent/CI identity for the generated MIG executor profile.
    #[arg(long, requires = "reference_mode", conflicts_with = "mig_inventory")]
    reference_mig_selection: Option<PathBuf>,
    /// Execute a bounded diagnostic CUDA request; never an admission or Worker receipt.
    #[arg(long, requires_all = ["reference_helper", "reference_helper_sha256", "reference_state"], conflicts_with_all = ["mock", "publish", "list"])]
    reference_request: Option<PathBuf>,
    /// Native child produced by scripts/build-gpu-reference.sh.
    #[arg(long, requires = "reference_mode")]
    reference_helper: Option<PathBuf>,
    /// Exact trusted build manifest digest of the native child.
    #[arg(long, requires = "reference_mode")]
    reference_helper_sha256: Option<String>,
    /// Fresh private invocation directory under the caller's evidence/CAS root.
    #[arg(long, requires = "reference_mode")]
    reference_state: Option<PathBuf>,
    /// Cancel a reference request after a bounded diagnostic delay.
    #[arg(long, requires = "reference_request", value_parser = clap::value_parser!(u32).range(1..=30_000))]
    reference_cancel_after_ms: Option<u32>,

    /// Use the deterministic mock backend instead of NVML.
    #[arg(long, action = ArgAction::SetTrue)]
    mock: bool,
    /// Host registry root containing available model manifests.
    #[arg(long, value_name = "DIR")]
    registry: Option<PathBuf>,
    /// Print GPU namespace JSON to stdout.
    #[arg(long, action = ArgAction::SetTrue)]
    list: bool,
    /// Publish the GPU namespace into /gpu/bridge/ctl on a live Queen.
    #[arg(long, action = ArgAction::SetTrue)]
    publish: bool,
    /// Interval in milliseconds between publish snapshots (requires --publish).
    #[arg(long, value_name = "MS")]
    interval_ms: Option<u64>,
    /// TCP host for the live console publish mode.
    #[arg(long, default_value = "127.0.0.1")]
    tcp_host: String,
    /// TCP port for the live console publish mode.
    #[arg(long, default_value_t = DEFAULT_TCP_PORT)]
    tcp_port: u16,
    /// Authentication token for the live console publish mode.
    #[arg(long)]
    auth_token: Option<String>,
    /// Optional ticket payload when attaching to the console.
    #[arg(long)]
    ticket: Option<String>,
    /// REST gateway base URL for hive-gateway publish mode.
    #[arg(long, value_name = "URL")]
    rest_url: Option<String>,
    /// Request auth token for REST mutating routes.
    #[arg(long, value_name = "TOKEN")]
    rest_auth_token: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(path) = &args.workload_config {
        return gpu_bridge_host::workload::serve(gpu_bridge_host::workload::Config::load(path)?);
    }
    if args.reference_inventory || args.reference_request.is_some() || args.mig_inventory {
        return run_reference(&args);
    }
    let bridge = if let Some(path) = &args.native_inventory_config {
        let bridge = gpu_bridge_host::GpuBridge::new_reference(
            gpu_bridge_host::workload::Config::load(path)?,
        )?;
        match &args.registry {
            Some(root) => bridge.with_registry_root(root),
            None => bridge,
        }
    } else {
        auto_bridge_with_registry(args.mock, args.registry.as_deref())?
    };
    let namespace: GpuNamespaceSnapshot = bridge.serialise_namespace()?;
    if args.list {
        println!("{}", namespace_to_json_pretty(&namespace));
    }
    if args.publish {
        let interval = args.interval_ms.map(Duration::from_millis);
        if let Some(rest_url) = args.rest_url.as_deref() {
            #[cfg(feature = "rest")]
            {
                let mut client = GatewayClient::new(rest_url);
                if let Some(ticket) = &args.ticket {
                    client = client.with_delegated_ticket(ticket);
                }
                if let Some(token) = resolve_rest_auth_token(args.rest_auth_token.as_deref())
                    .context("resolve REST authentication token")?
                {
                    client = client.with_request_auth_token(token);
                }
                loop {
                    let snapshot = bridge.serialise_namespace()?;
                    let publish = build_publish_lines(&snapshot)?;
                    for line in &publish.lines {
                        client
                            .echo("/gpu/bridge/ctl", line.as_str())
                            .context("publish gpu bridge snapshot via rest")?;
                    }
                    if let Some(delay) = interval {
                        thread::sleep(delay);
                    } else {
                        break;
                    }
                }
            }
            #[cfg(not(feature = "rest"))]
            {
                let _ = rest_url;
                anyhow::bail!("rest publish disabled; rebuild with --features rest");
            }
        } else {
            let auth_token = resolve_auth_token(args.auth_token.as_deref())
                .context("resolve live console authentication token")?;
            let mut client = ConsoleClient::connect(
                &args.tcp_host,
                args.tcp_port,
                auth_token.as_str(),
                args.ticket.as_deref(),
            )
            .context("connect to live console")?;
            loop {
                let snapshot = bridge.serialise_namespace()?;
                let publish = build_publish_lines(&snapshot)?;
                client
                    .publish_lines(&publish.lines)
                    .context("publish gpu bridge snapshot")?;
                if let Some(delay) = interval {
                    thread::sleep(delay);
                } else {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn run_reference(args: &Args) -> Result<()> {
    use gpu_bridge_host::reference::{self, ReferenceRequest};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let helper = args
        .reference_helper
        .as_deref()
        .ok_or_else(|| anyhow!("reference helper required"))?;
    let helper_hash = args
        .reference_helper_sha256
        .as_deref()
        .ok_or_else(|| anyhow!("reference helper digest required"))?;
    let state = args
        .reference_state
        .as_deref()
        .ok_or_else(|| anyhow!("reference state required"))?;
    if args.mig_inventory {
        println!(
            "{}",
            serde_json::to_string(&gpu_bridge_host::mig::discover(
                helper,
                helper_hash,
                state,
                args.mig_parent_ordinal
            )?)?
        );
        return Ok(());
    }
    let selection: Option<gpu_bridge_host::mig::Selection> = args
        .reference_mig_selection
        .as_ref()
        .map(|path| {
            let mut raw = Vec::new();
            std::fs::File::open(path)?
                .take(8193)
                .read_to_end(&mut raw)?;
            anyhow::ensure!(raw.len() <= 8192, "request_limit MIG_selection");
            let selection: gpu_bridge_host::mig::Selection = serde_json::from_slice(&raw)?;
            selection.validate()?;
            Ok::<_, anyhow::Error>(selection)
        })
        .transpose()?;
    if args.reference_inventory {
        println!(
            "{}",
            serde_json::to_string(&reference::inventory_selected(
                helper,
                helper_hash,
                state,
                0,
                selection.as_ref()
            )?)?
        );
        return Ok(());
    }
    let request_path = args
        .reference_request
        .as_deref()
        .ok_or_else(|| anyhow!("reference request required"))?;
    let mut raw = Vec::new();
    std::fs::File::open(request_path)?
        .take(8193)
        .read_to_end(&mut raw)?;
    if raw.len() > 8192 {
        anyhow::bail!("request_limit CUDA reference");
    }
    let request: ReferenceRequest = serde_json::from_slice(&raw)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let (done, completion) = std::sync::mpsc::channel();
    let timer = args.reference_cancel_after_ms.map(|delay| {
        let cancel = cancel.clone();
        thread::spawn(move || {
            if completion
                .recv_timeout(Duration::from_millis(u64::from(delay)))
                .is_err()
            {
                cancel.store(true, Ordering::Release);
            }
        })
    });
    let result = reference::execute_selected(
        helper,
        helper_hash,
        state,
        &request,
        &cancel,
        selection.as_ref(),
    );
    let _ = done.send(());
    if let Some(timer) = timer {
        timer
            .join()
            .map_err(|_| anyhow!("reference cancellation owner failed"))?;
    }
    println!("{}", serde_json::to_string(&result?)?);
    Ok(())
}

fn resolve_auth_token(cli_token: Option<&str>) -> Result<String> {
    if let Some(token) = cli_token {
        return validated_auth_token(token);
    }
    for name in ["COH_AUTH_TOKEN_REF", "COH_AUTH_TOKEN", "COHSH_AUTH_TOKEN"] {
        match std::env::var(name) {
            Ok(value) => return validated_auth_token(&value),
            Err(std::env::VarError::NotPresent) => continue,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(anyhow!("selected live authentication source is not UTF-8"));
            }
        }
    }
    Err(anyhow!(
        "live publish requires --auth-token, COH_AUTH_TOKEN_REF, COH_AUTH_TOKEN, or COHSH_AUTH_TOKEN"
    ))
}

fn validated_auth_token(value: &str) -> Result<String> {
    if value.starts_with("env:") || value.starts_with("file:") {
        return Ok(cohesix_authority::secret::resolve_reference(value)?);
    }
    Ok(cohesix_authority::secret::validate_value(value)?)
}

#[cfg(feature = "rest")]
fn resolve_rest_auth_token(cli_value: Option<&str>) -> Result<Option<String>> {
    if let Some(value) = cli_value {
        return validated_auth_token(value).map(Some);
    }
    for key in [
        "HIVE_GATEWAY_REQUEST_AUTH_TOKEN",
        "COHSH_REST_AUTH_TOKEN",
        "COH_REST_AUTH_TOKEN",
    ] {
        match std::env::var(key) {
            Ok(value) => return validated_auth_token(&value).map(Some),
            Err(std::env::VarError::NotPresent) => continue,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(anyhow!("selected REST authentication source is not UTF-8"));
            }
        }
    }
    Ok(None)
}

struct ConsoleClient {
    stream: TcpStream,
    frame_max_bytes: usize,
}

impl ConsoleClient {
    fn connect(host: &str, port: u16, auth_token: &str, ticket: Option<&str>) -> Result<Self> {
        let auth_token = validated_auth_token(auth_token)?;
        #[derive(serde::Deserialize)]
        struct Generated {
            authority: cohesix_authority::policy::AuthorityPolicy,
        }
        let generated: Generated = serde_json::from_str(include_str!(
            "../../../configs/generated/root_task_resolved.json"
        ))?;
        let frame_max_bytes = generated.authority.gpu_frame_max_bytes as usize;
        let stream = TcpStream::connect((host, port)).with_context(|| format!("{host}:{port}"))?;
        stream.set_read_timeout(Some(Duration::from_millis(200)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let mut client = Self {
            stream,
            frame_max_bytes,
        };
        client.send_line(&format!("AUTH {auth_token}"))?;
        client.wait_ack("AUTH")?;
        let ticket = ticket.unwrap_or("");
        client.send_line(&format!("ATTACH queen {ticket}"))?;
        client.wait_ack("ATTACH")?;
        Ok(client)
    }

    fn publish_lines(&mut self, lines: &[String]) -> Result<()> {
        for line in lines {
            self.send_line(&format!("ECHO /gpu/bridge/ctl {line}"))?;
            self.wait_ack("ECHO")?;
        }
        Ok(())
    }

    fn send_line(&mut self, line: &str) -> Result<()> {
        let total_len = line
            .len()
            .checked_add(4)
            .ok_or_else(|| anyhow!("console frame length overflow"))?;
        validate_frame_length(total_len, self.frame_max_bytes)?;
        let len_bytes = (total_len as u32).to_le_bytes();
        self.stream.write_all(&len_bytes)?;
        self.stream.write_all(line.as_bytes())?;
        Ok(())
    }

    fn read_frame(&mut self) -> Result<Option<String>> {
        let mut len_buf = [0u8; 4];
        match self.stream.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => return Ok(None),
            Err(err) => return Err(err.into()),
        }
        let total_len = u32::from_le_bytes(len_buf) as usize;
        validate_frame_length(total_len, self.frame_max_bytes)?;
        let payload_len = total_len.saturating_sub(4);
        let mut payload = vec![0u8; payload_len];
        self.stream.read_exact(&mut payload)?;
        let line = String::from_utf8(payload).map_err(|_| anyhow!("console frame is not UTF-8"))?;
        Ok(Some(line))
    }

    fn wait_ack(&mut self, verb: &str) -> Result<()> {
        let start = Instant::now();
        loop {
            if start.elapsed() > ACK_TIMEOUT {
                return Err(anyhow!("timeout waiting for {verb} ack"));
            }
            let Some(line) = self.read_frame()? else {
                continue;
            };
            let Some(ack) = parse_ack(line.trim()) else {
                continue;
            };
            if !ack.verb.eq_ignore_ascii_case(verb) {
                continue;
            }
            if matches!(ack.status, AckStatus::Ok) {
                return Ok(());
            }
            let detail = ack
                .detail
                .map(|value| value.to_owned())
                .unwrap_or_else(|| "unknown".to_owned());
            return Err(anyhow!("{verb} failed: {detail}"));
        }
    }
}

fn validate_frame_length(total_len: usize, maximum: usize) -> Result<()> {
    if !(256..=8192).contains(&maximum) || !(4..=maximum).contains(&total_len) {
        return Err(anyhow!(
            "invalid console frame length {total_len}; permitted 4..={maximum}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn peer_length_is_bounded_before_payload_allocation() {
        for length in [0, 3, 8193, u32::MAX as usize] {
            assert!(super::validate_frame_length(length, 8192).is_err());
        }
        assert!(super::validate_frame_length(4, 8192).is_ok());
        assert!(super::validate_frame_length(8192, 8192).is_ok());
    }
    use super::*;

    #[test]
    fn explicit_live_token_is_accepted() {
        assert_eq!(
            resolve_auth_token(Some("real-secret")).expect("valid token"),
            "real-secret"
        );
    }

    #[test]
    fn invalid_selected_source_never_falls_back() {
        for value in ["", " ", "env:", "file:relative"] {
            assert!(resolve_auth_token(Some(value)).is_err());
            #[cfg(feature = "rest")]
            assert!(resolve_rest_auth_token(Some(value)).is_err());
        }
    }

    #[test]
    fn placeholder_live_token_is_rejected_before_connect() {
        let placeholder = ["change", "me"].concat();
        let err = resolve_auth_token(Some(&placeholder)).expect_err("placeholder must fail");
        assert!(err.to_string().contains("placeholder"));
    }

    #[cfg(feature = "rest")]
    #[test]
    fn placeholder_rest_token_is_rejected_before_request() {
        let placeholder = ["change", "me"].concat();
        let err = resolve_rest_auth_token(Some(&placeholder))
            .expect_err("placeholder REST token must fail");
        assert!(err.to_string().contains("placeholder"));
    }
}
