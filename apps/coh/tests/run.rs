// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate coh run wrapper behavior.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use coh::gpu;
use coh::policy::CohPolicy;
use coh::run::{self, RunSpec};
use coh::CohAudit;
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport};
use gpu_bridge_host::auto_bridge;
use nine_door::NineDoor;
use secure9p_codec::OpenMode;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct BreadcrumbLine {
    schema: String,
    event: String,
    command: String,
    status: String,
    exit_code: Option<i32>,
}

fn setup_gpu() -> Result<(NineDoor, CohClient<InProcessTransport>)> {
    let server = NineDoor::new();
    let bridge = auto_bridge(true)?;
    let snapshot = bridge.serialise_namespace()?;
    server.install_gpu_nodes(&snapshot)?;
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let client = CohClient::connect(transport, Role::Queen, None)?;
    Ok((server, client))
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

#[test]
fn run_denies_without_lease() -> Result<()> {
    let (_server, mut client) = setup_gpu()?;
    let policy = CohPolicy::from_generated();
    let spec = RunSpec {
        gpu_id: "GPU-0".to_owned(),
        command: vec!["echo".to_owned(), "ok".to_owned()],
    };
    let mut audit = CohAudit::new();
    let err = run::execute(&mut client, &policy, &mut audit, &spec).unwrap_err();
    assert!(err.to_string().contains("no active lease"));

    let status_path = "/gpu/GPU-0/status";
    let status_bytes = read_all(&mut client, status_path)?;
    assert!(status_bytes.is_empty());
    Ok(())
}

#[test]
fn run_appends_ordered_breadcrumbs() -> Result<()> {
    let (_server, mut client) = setup_gpu()?;
    let mut audit = CohAudit::new();
    let lease_args = gpu::GpuLeaseArgs {
        gpu_id: "GPU-0".to_owned(),
        mem_mb: 1024,
        streams: 1,
        ttl_s: 60,
        priority: Some(1),
        budget_ttl_s: None,
        budget_ops: None,
    };
    gpu::lease(&mut client, &mut audit, &lease_args)?;

    let policy = CohPolicy::from_generated();
    let spec = RunSpec {
        gpu_id: "GPU-0".to_owned(),
        command: vec!["echo".to_owned(), "ok".to_owned()],
    };
    let mut audit = CohAudit::new();
    run::execute(&mut client, &policy, &mut audit, &spec)?;

    let status_path = "/gpu/GPU-0/status";
    let status_bytes = read_all(&mut client, status_path)?;
    let status_text = String::from_utf8(status_bytes).context("status utf8")?;
    let lines: Vec<BreadcrumbLine> = status_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("breadcrumb json"))
        .collect::<Result<Vec<_>>>()?;
    assert!(lines.len() >= 2, "expected at least 2 breadcrumbs");
    let first = &lines[0];
    let last = &lines[lines.len() - 1];
    assert_eq!(first.schema, policy.run.breadcrumb.schema);
    assert_eq!(first.event, "START");
    assert_eq!(first.status, "RUNNING");
    assert!(first.command.contains("echo"));
    assert_eq!(last.schema, policy.run.breadcrumb.schema);
    assert_eq!(last.event, "EXIT");
    assert_eq!(last.status, "OK");
    assert_eq!(last.exit_code, Some(0));

    for line in status_text.lines().filter(|line| !line.trim().is_empty()) {
        assert!(
            line.len() <= policy.run.breadcrumb.max_line_bytes as usize,
            "breadcrumb line exceeded max_line_bytes"
        );
    }

    Ok(())
}

#[test]
fn run_rejects_invalid_lease_schema() -> Result<()> {
    let (_server, mut client) = setup_gpu()?;
    let bad_lease =
        "{\"schema\":\"wrong\",\"state\":\"ACTIVE\",\"gpu_id\":\"GPU-0\",\"worker_id\":\"worker-1\",\"mem_mb\":1,\"streams\":1,\"ttl_s\":60,\"priority\":1}\n";
    write_append(&mut client, "/gpu/GPU-0/lease", bad_lease.as_bytes())?;

    let policy = CohPolicy::from_generated();
    let spec = RunSpec {
        gpu_id: "GPU-0".to_owned(),
        command: vec!["echo".to_owned(), "ok".to_owned()],
    };
    let mut audit = CohAudit::new();
    let err = run::execute(&mut client, &policy, &mut audit, &spec).unwrap_err();
    assert!(err.to_string().contains("lease schema mismatch"));

    let status_bytes = read_all(&mut client, "/gpu/GPU-0/status")?;
    assert!(status_bytes.is_empty());
    Ok(())
}
