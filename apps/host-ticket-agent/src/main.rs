// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for processing host control tickets from /host/tickets/*.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host ticket agent binary.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use coh::policy::{default_policy_path, load_policy, CohPolicy};
use cohesix_ticket::Role;
use cohsh::{NineDoorTransport, RestTransport, RoleArg, Session, TcpTransport, Transport};
use host_ticket_agent::executors::ExecutorConfig;
use host_ticket_agent::{
    process_ticket_lane_once_with_journal, process_ticket_lane_snapshot_once_with_journal_state,
    relay, unix_time_ms_now,
    wal::{bind_execution_lane_topology, AgentFence},
    HostTicketManifest, ProcessSummary, TicketLaneState, DEFAULT_CURSOR_STATE_PATH,
    DEFAULT_EXECUTION_JOURNAL_PATH, DEFAULT_EXECUTION_LOCK_PATH, DEFAULT_RELAY_WAL_PATH,
    DEFAULT_RESOLVED_MANIFEST_PATH,
};
use nine_door::{HostNamespaceConfig, HostProvider, HostTicketPolicy, NineDoor};

/// CLI arguments for `host-ticket-agent`.
#[derive(Debug, Clone, Parser)]
#[command(author, version, about = "Cohesix host control ticket agent")]
struct Args {
    /// Role used to attach to the namespace.
    #[arg(long, default_value_t = RoleArg::Queen)]
    role: RoleArg,
    /// Optional capability ticket payload.
    #[arg(long)]
    ticket: Option<String>,
    /// Path to `configs/generated/root_task_resolved.json`.
    #[arg(long, value_name = "FILE", default_value = DEFAULT_RESOLVED_MANIFEST_PATH)]
    manifest: PathBuf,
    /// Cursor state file for deterministic resume.
    #[arg(long, value_name = "FILE", default_value = DEFAULT_CURSOR_STATE_PATH)]
    cursor: PathBuf,
    /// Crash-safe version-2 provider execution journal.
    #[arg(long, value_name = "FILE", default_value = DEFAULT_EXECUTION_JOURNAL_PATH)]
    execution_journal: PathBuf,
    /// Process-lifetime single-agent execution fence.
    #[arg(long, value_name = "FILE", default_value = DEFAULT_EXECUTION_LOCK_PATH)]
    agent_lock: PathBuf,
    /// Optional mount override (defaults to manifest value).
    #[arg(long, value_name = "PATH")]
    mount: Option<String>,
    /// Poll interval in milliseconds when running continuously.
    #[arg(long, default_value_t = 1000)]
    poll_ms: u64,
    /// Fixed number of durable version-2 execution lanes (1..=64).
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=64))]
    execution_lanes: u8,
    /// Enable federated relay forwarding (`ecosystem.host.federation`).
    #[arg(long, action = ArgAction::SetTrue)]
    relay: bool,
    /// Relay WAL state file for deterministic resume.
    #[arg(long, value_name = "FILE", default_value = DEFAULT_RELAY_WAL_PATH)]
    relay_wal: PathBuf,
    /// Process one pass and exit.
    #[arg(long, action = ArgAction::SetTrue)]
    run_once: bool,
    /// Use mock in-process NineDoor.
    #[arg(long, action = ArgAction::SetTrue)]
    mock: bool,
    /// REST gateway base URL.
    #[arg(long, value_name = "URL")]
    rest_url: Option<String>,
    /// REST request auth token.
    #[arg(long, value_name = "TOKEN")]
    rest_auth_token: Option<String>,
    /// TCP host for direct console mode.
    #[arg(long, default_value = "127.0.0.1")]
    tcp_host: String,
    /// TCP port for direct console mode.
    #[arg(long, default_value_t = cohsh::COHSH_TCP_PORT)]
    tcp_port: u16,
    /// TCP auth token for direct console mode.
    #[arg(long, default_value = "changeme")]
    auth_token: String,
    /// Optional host policy path for PEFT defaults.
    #[arg(long, value_name = "FILE")]
    policy: Option<PathBuf>,
    /// Optional explicit PEFT registry root override.
    #[arg(long, value_name = "DIR")]
    registry_root: Option<PathBuf>,
    /// Confined destination root for PEFT exports.
    #[arg(long, value_name = "DIR", default_value = "out/peft_exports")]
    export_root: PathBuf,
    /// Confined source root for PEFT adapter bundles.
    #[arg(long, value_name = "DIR", default_value = "out/peft_adapters")]
    adapter_root: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut manifest = HostTicketManifest::from_resolved_manifest(&args.manifest)?;
    if let Some(mount) = args.mount.as_deref() {
        manifest = manifest.with_mount_path(mount)?;
    }
    if !manifest.enabled {
        println!(
            "host-ticket-agent: tickets disabled (manifest={}, mount={})",
            args.manifest.display(),
            manifest.mount_path
        );
        return Ok(());
    }
    let _agent_fence = AgentFence::acquire(&args.agent_lock)?;
    let _lane_topology =
        bind_execution_lane_topology(&args.execution_journal, args.execution_lanes)?;

    let registry_root = args
        .registry_root
        .clone()
        .unwrap_or_else(|| resolve_registry_root(args.policy.as_deref()));
    let executor_config = ExecutorConfig {
        mount: manifest.mount_path.clone(),
        registry_root,
        export_root: args.export_root.clone(),
        adapter_root: args.adapter_root.clone(),
    };
    if args.mock && args.execution_lanes != 1 {
        return Err(anyhow::anyhow!(
            "mock NineDoor supports exactly one in-process execution lane"
        ));
    }

    let running = Arc::new(AtomicBool::new(true));
    let lane_count = usize::from(args.execution_lanes);
    let shared_snapshot = (lane_count > 1).then(|| Arc::new(SnapshotFeed::default()));
    let mut handles = Vec::with_capacity(
        lane_count + usize::from(args.relay) + usize::from(shared_snapshot.is_some()),
    );
    if let Some(feed) = shared_snapshot.as_ref() {
        let ingress_args = args.clone();
        let ingress_manifest = manifest.clone();
        let ingress_running = Arc::clone(&running);
        let ingress_feed = Arc::clone(feed);
        handles.push(thread::spawn(move || {
            let result = run_snapshot_ingress(
                &ingress_args,
                &ingress_manifest,
                &ingress_feed,
                &ingress_running,
            );
            if result.is_err() {
                ingress_running.store(false, Ordering::Release);
            }
            result
        }));
    }
    for lane_index in 0..lane_count {
        let lane_args = args.clone();
        let lane_manifest = manifest.clone();
        let lane_config = executor_config.clone();
        let lane_running = Arc::clone(&running);
        let lane_feed = shared_snapshot.as_ref().map(Arc::clone);
        handles.push(thread::spawn(move || {
            let result = if let Some(feed) = lane_feed {
                run_ticket_lane_from_snapshot(
                    &lane_args,
                    &lane_manifest,
                    &lane_config,
                    lane_index,
                    usize::from(lane_args.execution_lanes),
                    &feed,
                    &lane_running,
                )
            } else {
                run_ticket_lane(
                    &lane_args,
                    &lane_manifest,
                    &lane_config,
                    lane_index,
                    usize::from(lane_args.execution_lanes),
                    &lane_running,
                )
            };
            if result.is_err() {
                lane_running.store(false, Ordering::Release);
            }
            result
        }));
    }
    if args.relay {
        let relay_args = args.clone();
        let relay_manifest = manifest.clone();
        let relay_running = Arc::clone(&running);
        handles.push(thread::spawn(move || {
            let result = run_relay_lane(&relay_args, &relay_manifest, &relay_running);
            if result.is_err() {
                relay_running.store(false, Ordering::Release);
            }
            result
        }));
    }

    let mut first_error = None;
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) if first_error.is_none() => first_error = Some(error),
            Ok(Err(_)) => {}
            Err(_) if first_error.is_none() => {
                first_error = Some(anyhow::anyhow!("host-ticket-agent lane panicked"));
                running.store(false, Ordering::Release);
            }
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[derive(Debug, Default)]
struct SnapshotFeed {
    state: Mutex<SnapshotState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct SnapshotState {
    generation: u64,
    dropped_through_generation: u64,
    admission_watermark: Option<u64>,
    snapshots: VecDeque<(u64, Arc<Vec<String>>)>,
    closed: bool,
}

const SNAPSHOT_FEED_CAPACITY: usize = 128;

fn snapshot_admission_watermark(lines: &[String]) -> Option<u64> {
    lines
        .iter()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| value.get("admission_sequence")?.as_u64())
        .max()
}

impl SnapshotFeed {
    fn publish(&self, lines: Vec<String>) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("host-ticket snapshot feed lock poisoned"))?;
        let admission_watermark = snapshot_admission_watermark(lines.as_slice());
        let unchanged = if admission_watermark.is_some() {
            state.generation != 0 && state.admission_watermark == admission_watermark
        } else {
            state
                .snapshots
                .back()
                .is_some_and(|(_, current)| current.as_slice() == lines.as_slice())
        };
        if unchanged {
            return Ok(());
        }
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("host-ticket snapshot generation overflow"))?;
        let generation = state.generation;
        if state.snapshots.len() == SNAPSHOT_FEED_CAPACITY {
            let (dropped_generation, _) = state
                .snapshots
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("host-ticket snapshot feed underflow"))?;
            state.dropped_through_generation = dropped_generation;
        }
        state.admission_watermark = admission_watermark;
        state.snapshots.push_back((generation, Arc::new(lines)));
        self.changed.notify_all();
        Ok(())
    }

    fn close(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("host-ticket snapshot feed lock poisoned"))?;
        state.closed = true;
        self.changed.notify_all();
        Ok(())
    }

    fn wait_after(
        &self,
        generation: u64,
        running: &AtomicBool,
    ) -> Result<Option<(u64, Arc<Vec<String>>)>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("host-ticket snapshot feed lock poisoned"))?;
        while state.generation <= generation && !state.closed && running.load(Ordering::Acquire) {
            let waited = self
                .changed
                .wait_timeout(state, Duration::from_millis(100))
                .map_err(|_| anyhow::anyhow!("host-ticket snapshot feed wait poisoned"))?;
            state = waited.0;
        }
        if generation < state.dropped_through_generation {
            return Err(anyhow::anyhow!(
                "host-ticket snapshot feed overrun: requested after generation {} but dropped through {}",
                generation,
                state.dropped_through_generation,
            ));
        }
        Ok(state
            .snapshots
            .iter()
            .find(|(candidate, _)| *candidate > generation)
            .map(|(candidate, lines)| (*candidate, Arc::clone(lines))))
    }
}

fn run_snapshot_ingress(
    args: &Args,
    manifest: &HostTicketManifest,
    feed: &SnapshotFeed,
    running: &AtomicBool,
) -> Result<()> {
    let role = Role::from(args.role);
    let mut transport = build_transport(args, manifest)?;
    let session = transport
        .attach(role, args.ticket.as_deref())
        .context("attach host-ticket-agent snapshot ingress")?;
    let snapshot_path = manifest.spec_snapshot_path();
    let run_result = (|| -> Result<()> {
        while running.load(Ordering::Acquire) {
            match transport.read(&session, snapshot_path.as_str()) {
                Ok(lines) => feed.publish(lines)?,
                Err(error) if args.run_once => {
                    return Err(error).with_context(|| format!("read {snapshot_path}"));
                }
                Err(error) => {
                    eprintln!("host-ticket-agent: snapshot ingress failed: {error}");
                }
            }
            if args.run_once {
                break;
            }
            thread::sleep(Duration::from_millis(args.poll_ms.max(10)));
        }
        Ok(())
    })();
    let close_result = feed.close();
    let quit_result = transport.quit(&session);
    run_result?;
    close_result?;
    quit_result
}

#[allow(clippy::too_many_arguments)]
fn run_ticket_lane_from_snapshot(
    args: &Args,
    manifest: &HostTicketManifest,
    executor_config: &ExecutorConfig,
    lane_index: usize,
    lane_count: usize,
    feed: &SnapshotFeed,
    running: &AtomicBool,
) -> Result<()> {
    let cursor = lane_state_path(&args.cursor, lane_index, lane_count)?;
    let journal = lane_state_path(&args.execution_journal, lane_index, lane_count)?;
    let role = Role::from(args.role);
    let mut transport = build_transport(args, manifest)?;
    let session = transport
        .attach(role, args.ticket.as_deref())
        .with_context(|| format!("attach host-ticket-agent lane {lane_index}/{lane_count}"))?;
    let mut snapshot_generation = 0u64;
    let mut lane_state = TicketLaneState::load(&cursor)?;

    while running.load(Ordering::Acquire) {
        let Some((generation, snapshot_lines)) = feed.wait_after(snapshot_generation, running)?
        else {
            break;
        };
        snapshot_generation = generation;
        let pass = process_ticket_lane_snapshot_once_with_journal_state(
            transport.as_mut(),
            &session,
            manifest,
            &cursor,
            &journal,
            executor_config,
            unix_time_ms_now(),
            lane_index,
            lane_count,
            snapshot_lines.as_slice(),
            &mut lane_state,
        );
        report_lane_pass(args, lane_index, lane_count, &cursor, pass)?;
        if args.run_once {
            break;
        }
    }

    let _ = transport.quit(&session);
    Ok(())
}

fn run_ticket_lane(
    args: &Args,
    manifest: &HostTicketManifest,
    executor_config: &ExecutorConfig,
    lane_index: usize,
    lane_count: usize,
    running: &AtomicBool,
) -> Result<()> {
    let cursor = lane_state_path(&args.cursor, lane_index, lane_count)?;
    let journal = lane_state_path(&args.execution_journal, lane_index, lane_count)?;
    let role = Role::from(args.role);
    let mut transport = build_transport(args, manifest)?;
    let session = transport
        .attach(role, args.ticket.as_deref())
        .with_context(|| format!("attach host-ticket-agent lane {lane_index}/{lane_count}"))?;

    while running.load(Ordering::Acquire) {
        let pass = process_ticket_lane_once_with_journal(
            transport.as_mut(),
            &session,
            manifest,
            &cursor,
            &journal,
            executor_config,
            unix_time_ms_now(),
            lane_index,
            lane_count,
        );
        report_lane_pass(args, lane_index, lane_count, &cursor, pass)?;
        if args.run_once {
            break;
        }
        thread::sleep(Duration::from_millis(args.poll_ms.max(10)));
    }

    let _ = transport.quit(&session);
    Ok(())
}

fn report_lane_pass(
    args: &Args,
    lane_index: usize,
    lane_count: usize,
    cursor: &Path,
    pass: Result<ProcessSummary>,
) -> Result<()> {
    match pass {
        Ok(summary) => {
            if summary.seen > 0 || summary.terminal_updates() > 0 {
                println!(
                    "host-ticket-agent: lane={}/{} seen={} succeeded={} failed={} expired={} skipped_terminal={} skipped_remote_target={} cursor={}",
                    lane_index.saturating_add(1),
                    lane_count,
                    summary.seen,
                    summary.succeeded,
                    summary.failed,
                    summary.expired,
                    summary.skipped_terminal,
                    summary.skipped_remote_target,
                    cursor.display()
                );
            }
            Ok(())
        }
        Err(error) if args.run_once => Err(error),
        Err(error) => {
            eprintln!(
                "host-ticket-agent: lane={}/{} pass failed: {error}",
                lane_index.saturating_add(1),
                lane_count
            );
            Ok(())
        }
    }
}

fn run_relay_lane(args: &Args, manifest: &HostTicketManifest, running: &AtomicBool) -> Result<()> {
    let role = Role::from(args.role);
    let mut transport = build_transport(args, manifest)?;
    let session = transport
        .attach(role, args.ticket.as_deref())
        .context("attach host-ticket-agent relay session")?;
    while running.load(Ordering::Acquire) {
        let pass = relay::relay_once(transport.as_mut(), &session, manifest, &args.relay_wal);
        match pass {
            Ok(summary) => {
                if summary.seen > 0
                    || summary.forwarded > 0
                    || summary.remote_write_failures > 0
                    || summary.queue_depth > 0
                {
                    println!(
                        "host-ticket-agent relay: seen={} candidates={} deduped={} forwarded={} remote_failures={} backpressure={} queue_depth={} wal={}",
                        summary.seen,
                        summary.candidates,
                        summary.deduped,
                        summary.forwarded,
                        summary.remote_write_failures,
                        summary.backpressure_drops,
                        summary.queue_depth,
                        args.relay_wal.display()
                    );
                }
            }
            Err(error) if args.run_once => return Err(error),
            Err(error) => eprintln!("host-ticket-agent relay: pass failed: {error}"),
        }
        if args.run_once {
            break;
        }
        thread::sleep(Duration::from_millis(args.poll_ms.max(10)));
    }
    let _ = transport.quit(&session);
    Ok(())
}

fn lane_state_path(base: &Path, lane_index: usize, lane_count: usize) -> Result<PathBuf> {
    if lane_count == 1 {
        return Ok(base.to_path_buf());
    }
    let stem = base
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("lane state path requires a UTF-8 file name"))?;
    let extension = base.extension().and_then(|value| value.to_str());
    let suffix = format!("lane-{lane_index:02}-of-{lane_count:02}");
    let file_name = extension.map_or_else(
        || format!("{stem}.{suffix}"),
        |extension| format!("{stem}.{suffix}.{extension}"),
    );
    Ok(base.with_file_name(file_name))
}

fn build_transport(args: &Args, manifest: &HostTicketManifest) -> Result<Box<dyn Transport>> {
    if args.mock {
        return build_mock_transport(manifest);
    }
    if let Some(rest_url) = args.rest_url.as_deref() {
        let auth = args
            .rest_auth_token
            .clone()
            .or_else(resolve_rest_auth_token_from_env);
        return Ok(Box::new(RestTransport::new(rest_url, auth)));
    }
    Ok(Box::new(
        TcpTransport::new(args.tcp_host.clone(), args.tcp_port)
            .with_auth_token(args.auth_token.clone()),
    ))
}

fn build_mock_transport(manifest: &HostTicketManifest) -> Result<Box<dyn Transport>> {
    let providers = [
        HostProvider::Systemd,
        HostProvider::K8s,
        HostProvider::Docker,
        HostProvider::Nvidia,
    ];
    let base = HostNamespaceConfig::enabled(manifest.mount_path.as_str(), &providers)
        .context("configure mock host namespace")?;
    let ticket_policy = manifest_ticket_policy(manifest)?;
    let host = base.with_ticket_policy(ticket_policy);
    let server = NineDoor::new_with_host_config(host);
    Ok(Box::new(NineDoorTransport::new(server)))
}

fn manifest_ticket_policy(manifest: &HostTicketManifest) -> Result<HostTicketPolicy> {
    if !manifest.enabled {
        return Ok(HostTicketPolicy::disabled());
    }
    let actions = manifest
        .action_allowlist
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let lifecycle = manifest
        .lifecycle
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    HostTicketPolicy::enabled(
        manifest.request_schema.as_str(),
        manifest.result_schema.as_str(),
        manifest.max_line_bytes,
        actions.as_slice(),
        lifecycle.as_slice(),
    )
    .map_err(|err| anyhow::anyhow!("mock host ticket policy: {err}"))
}

fn resolve_registry_root(policy_path: Option<&Path>) -> PathBuf {
    let path = policy_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_policy_path);
    match load_policy(path.as_path()) {
        Ok(policy) => PathBuf::from(policy.peft.import.registry_root),
        Err(err) => {
            eprintln!("host-ticket-agent: {} (using generated coh defaults)", err);
            PathBuf::from(CohPolicy::from_generated().peft.import.registry_root)
        }
    }
}

fn resolve_rest_auth_token_from_env() -> Option<String> {
    for key in [
        "HIVE_GATEWAY_REQUEST_AUTH_TOKEN",
        "COHSH_REST_AUTH_TOKEN",
        "COH_REST_AUTH_TOKEN",
    ] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

#[allow(dead_code)]
fn _ping_transport(transport: &mut dyn Transport, session: &Session) -> Result<String> {
    transport.ping(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_feed_deduplicates_and_delivers_changed_generations_in_order() {
        let feed = SnapshotFeed::default();
        let running = AtomicBool::new(true);
        feed.publish(vec!["first".to_owned()]).expect("first");
        feed.publish(vec!["first".to_owned()])
            .expect("duplicate first");
        feed.publish(vec!["latest".to_owned()]).expect("latest");

        let (first_generation, first_lines) = feed
            .wait_after(0, &running)
            .expect("wait")
            .expect("published snapshot");
        assert_eq!(first_generation, 1);
        assert_eq!(first_lines.as_slice(), ["first"]);

        let (latest_generation, latest_lines) = feed
            .wait_after(first_generation, &running)
            .expect("wait latest")
            .expect("latest snapshot");
        assert_eq!(latest_generation, 2);
        assert_eq!(latest_lines.as_slice(), ["latest"]);

        feed.close().expect("close");
        assert!(feed
            .wait_after(latest_generation, &running)
            .expect("closed wait")
            .is_none());
    }

    #[test]
    fn snapshot_feed_ignores_volatile_updates_without_new_admission() {
        let feed = SnapshotFeed::default();
        let running = AtomicBool::new(true);
        feed.publish(vec![
            r#"{"admission_sequence":7,"resolved_worker_slot":1}"#.to_owned()
        ])
        .expect("first admission");
        feed.publish(vec![
            r#"{"admission_sequence":7,"resolved_worker_slot":2}"#.to_owned()
        ])
        .expect("volatile update");
        feed.publish(vec![
            r#"{"admission_sequence":8,"resolved_worker_slot":2}"#.to_owned()
        ])
        .expect("next admission");

        let (first_generation, first_lines) = feed
            .wait_after(0, &running)
            .expect("wait first")
            .expect("first snapshot");
        assert_eq!(first_generation, 1);
        assert!(first_lines[0].contains(":7"));

        let (second_generation, second_lines) = feed
            .wait_after(first_generation, &running)
            .expect("wait second")
            .expect("second snapshot");
        assert_eq!(second_generation, 2);
        assert!(second_lines[0].contains(":8"));
        assert!(feed
            .wait_after(second_generation, &AtomicBool::new(false))
            .expect("no third generation")
            .is_none());
    }

    #[test]
    fn snapshot_feed_fails_closed_when_a_consumer_exceeds_the_bound() {
        let feed = SnapshotFeed::default();
        let running = AtomicBool::new(true);
        for generation in 0..=SNAPSHOT_FEED_CAPACITY {
            feed.publish(vec![generation.to_string()])
                .expect("bounded publish");
        }

        let error = feed
            .wait_after(0, &running)
            .expect_err("evicted generation must fail closed");
        assert!(error.to_string().contains("snapshot feed overrun"));
    }
}
