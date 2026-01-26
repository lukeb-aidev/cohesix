// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compare coh transcript output against shared fixtures.
// Author: Lukas Bower
#![forbid(unsafe_code)]

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use anyhow::{Context, Result};
use coh::gpu;
use coh::policy::CohPolicy;
use coh::run::{self, RunSpec};
use coh::telemetry;
use coh::CohAudit;
use cohsh::client::{CohClient, InProcessTransport};
use cohesix_ticket::Role;
use gpu_bridge_host::auto_bridge;
use nine_door::NineDoor;
use secure9p_codec::OpenMode;
use tempfile::TempDir;

const SCENARIO: &str = "converge_v0";
const RUN_SCENARIO: &str = "run_demo_v0";

#[test]
fn coh_transcript_matches_fixture() -> Result<()> {
    let server = NineDoor::new();
    let bridge = auto_bridge(true)?;
    let snapshot = bridge.serialise_namespace()?;
    server.install_gpu_nodes(&snapshot)?;

    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    seed_telemetry(&mut client)?;

    let mut transcript = Vec::new();

    let mut audit = CohAudit::new();
    gpu::list(&mut client, &mut audit)?;
    transcript.extend(audit.into_lines());

    let mut audit = CohAudit::new();
    let lease_args = gpu::GpuLeaseArgs {
        gpu_id: "GPU-0".to_owned(),
        mem_mb: 4096,
        streams: 2,
        ttl_s: 120,
        priority: Some(1),
        budget_ttl_s: None,
        budget_ops: None,
    };
    gpu::lease(&mut client, &mut audit, &lease_args)?;
    transcript.extend(audit.into_lines());

    let policy = CohPolicy::from_generated();
    let temp = TempDir::new().expect("tempdir");
    let mut audit = CohAudit::new();
    telemetry::pull(&mut client, &policy, temp.path(), &mut audit)?;
    transcript.extend(audit.into_lines());

    transcript_support::compare_transcript("coh", SCENARIO, "coh.txt", &transcript);
    transcript_support::write_timing(
        "coh",
        SCENARIO,
        "transcript",
        0,
    );
    Ok(())
}

#[test]
fn coh_run_transcript_matches_cohsh_baseline() -> Result<()> {
    let server = NineDoor::new();
    let bridge = auto_bridge(true)?;
    let snapshot = bridge.serialise_namespace()?;
    server.install_gpu_nodes(&snapshot)?;

    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    let mut transcript = Vec::new();
    let policy = CohPolicy::from_generated();

    let lease_path = "/gpu/GPU-0/lease";
    let active_lease = lease_entry("ACTIVE");
    let written = write_append_len(&mut client, lease_path, active_lease.as_bytes())?;
    let mut audit = CohAudit::new();
    let detail = format!("path={lease_path} bytes={written}");
    audit.push_ack(cohsh_core::wire::AckStatus::Ok, "ECHO", Some(detail.as_str()));
    transcript.extend(audit.into_lines());

    let spec = RunSpec {
        gpu_id: "GPU-0".to_owned(),
        command: vec!["echo".to_owned(), "ok".to_owned()],
    };
    let mut audit = CohAudit::new();
    run::execute(&mut client, &policy, &mut audit, &spec)?;
    transcript.extend(audit.into_lines());

    let status_path = "/gpu/GPU-0/status";
    let _ = read_all(&mut client, status_path)?;
    let mut audit = CohAudit::new();
    let detail = format!("path={status_path}");
    audit.push_ack(cohsh_core::wire::AckStatus::Ok, "CAT", Some(detail.as_str()));
    transcript.extend(audit.into_lines());

    let released_lease = lease_entry("RELEASED");
    let written = write_append_len(&mut client, lease_path, released_lease.as_bytes())?;
    let mut audit = CohAudit::new();
    let detail = format!("path={lease_path} bytes={written}");
    audit.push_ack(cohsh_core::wire::AckStatus::Ok, "ECHO", Some(detail.as_str()));
    transcript.extend(audit.into_lines());

    transcript_support::compare_transcript("coh", RUN_SCENARIO, "cohsh.txt", &transcript);
    transcript_support::write_timing("coh", RUN_SCENARIO, "transcript", 0);
    Ok(())
}

fn seed_telemetry<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
) -> Result<()> {
    let device_id = "device-1";
    let ctl_path = format!("/queen/telemetry/{device_id}/ctl");
    let ctl_payload = "{\"new\":\"segment\",\"mime\":\"application/jsonl\"}\n";
    write_append(client, &ctl_path, ctl_payload.as_bytes())?;

    let latest_path = format!("/queen/telemetry/{device_id}/latest");
    let latest = read_all(client, &latest_path)?;
    let latest = String::from_utf8(latest).context("latest utf8")?;
    let seg_id = latest.lines().next().context("latest empty")?;

    let seg_path = format!("/queen/telemetry/{device_id}/seg/{seg_id}");
    let payload = "{\"seq\":1}\n{\"seq\":2}\n";
    write_append(client, &seg_path, payload.as_bytes())?;
    Ok(())
}

fn write_append<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
    payload: &[u8],
) -> Result<()> {
    let fid = client.open(path, OpenMode::write_append())?;
    let written = client.write(fid, u64::MAX, payload)?;
    client.clunk(fid)?;
    if written as usize != payload.len() {
        anyhow::bail!("short write to {path}");
    }
    Ok(())
}

fn write_append_len<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
    payload: &[u8],
) -> Result<usize> {
    let fid = client.open(path, OpenMode::write_append())?;
    let written = client.write(fid, u64::MAX, payload)?;
    client.clunk(fid)?;
    if written as usize != payload.len() {
        anyhow::bail!("short write to {path}");
    }
    Ok(written as usize)
}

fn lease_entry(state: &str) -> String {
    format!(
        "{{\"schema\":\"gpu-lease/v1\",\"state\":\"{state}\",\"gpu_id\":\"GPU-0\",\"worker_id\":\"worker-1\",\"mem_mb\":1024,\"streams\":1,\"ttl_s\":60,\"priority\":1}}\n"
    )
}

fn read_all<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
) -> Result<Vec<u8>> {
    let fid = client.open(path, OpenMode::read_only())?;
    let mut offset = 0u64;
    let mut out = Vec::new();
    loop {
        let chunk = client.read(fid, offset, client.negotiated_msize())?;
        if chunk.is_empty() {
            break;
        }
        offset += chunk.len() as u64;
        out.extend_from_slice(&chunk);
        if chunk.len() < client.negotiated_msize() as usize {
            break;
        }
    }
    client.clunk(fid)?;
    Ok(out)
}
