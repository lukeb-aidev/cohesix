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
use coh::telemetry;
use coh::CohAudit;
use cohsh::client::{CohClient, InProcessTransport};
use cohesix_ticket::Role;
use gpu_bridge_host::auto_bridge;
use nine_door::NineDoor;
use secure9p_codec::OpenMode;
use tempfile::TempDir;

const SCENARIO: &str = "converge_v0";

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

