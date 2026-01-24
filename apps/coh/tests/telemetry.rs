// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate coh telemetry pull behavior.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use coh::policy::{CohMountPolicy, CohPolicy, CohRetryPolicy, CohTelemetryPolicy};
use coh::telemetry::pull;
use coh::CohAudit;
use cohsh::client::{CohClient, InProcessTransport};
use cohesix_ticket::Role;
use nine_door::NineDoor;
use secure9p_codec::OpenMode;
use tempfile::TempDir;

fn base_policy() -> CohPolicy {
    CohPolicy {
        mount: CohMountPolicy {
            root: "/".to_owned(),
            allowlist: vec!["/queen".to_owned()],
        },
        telemetry: CohTelemetryPolicy {
            root: "/queen/telemetry".to_owned(),
            max_devices: 4,
            max_segments_per_device: 4,
            max_bytes_per_segment: 64 * 1024,
            max_total_bytes_per_device: 256 * 1024,
        },
        retry: CohRetryPolicy {
            max_attempts: 1,
            backoff_ms: 1,
            ceiling_ms: 1,
            timeout_ms: 1,
        },
    }
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

fn create_segment<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    device_id: &str,
    payload: &str,
) -> Result<String> {
    let ctl_path = format!("/queen/telemetry/{device_id}/ctl");
    let ctl_payload = "{\"new\":\"segment\",\"mime\":\"application/jsonl\"}\n";
    write_append(client, &ctl_path, ctl_payload.as_bytes())?;
    let latest_path = format!("/queen/telemetry/{device_id}/latest");
    let latest = read_all(client, &latest_path)?;
    let latest = String::from_utf8(latest).context("latest utf8")?;
    let seg_id = latest.lines().next().context("latest empty")?.to_owned();
    let seg_path = format!("/queen/telemetry/{device_id}/seg/{seg_id}");
    write_append(client, &seg_path, payload.as_bytes())?;
    Ok(seg_id)
}

#[test]
fn telemetry_pull_is_idempotent() -> Result<()> {
    let server = NineDoor::new();
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    let device_id = "device-1";
    let payload = "{\"seq\":1}\n{\"seq\":2}\n";
    let seg_id = create_segment(&mut client, device_id, payload)?;

    let temp = TempDir::new().expect("tempdir");
    let policy = base_policy();
    let mut audit = CohAudit::new();
    pull(&mut client, &policy, temp.path(), &mut audit)?;

    let output_path = temp
        .path()
        .join(device_id)
        .join("seg")
        .join(&seg_id);
    let stored = std::fs::read(&output_path).context("read output")?;
    assert_eq!(stored, payload.as_bytes());

    let mut second_audit = CohAudit::new();
    pull(&mut client, &policy, temp.path(), &mut second_audit)?;
    let stored_again = std::fs::read(&output_path).context("read output again")?;
    assert_eq!(stored_again, payload.as_bytes());
    Ok(())
}

#[test]
fn telemetry_pull_enforces_segment_bytes() -> Result<()> {
    let server = NineDoor::new();
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    let device_id = "device-2";
    let payload = "payload-too-large";
    create_segment(&mut client, device_id, payload)?;

    let temp = TempDir::new().expect("tempdir");
    let mut policy = base_policy();
    policy.telemetry.max_bytes_per_segment = 4;
    let mut audit = CohAudit::new();
    let err = pull(&mut client, &policy, temp.path(), &mut audit).unwrap_err();
    assert!(err.to_string().contains("exceeds max bytes"));
    Ok(())
}
