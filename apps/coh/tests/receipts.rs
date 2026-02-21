// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate receipt artifacts emitted by coh gpu lease and coh run.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use coh::evidence::build_local_bounds;
use coh::gpu;
use coh::policy::CohPolicy;
use coh::run::{self, RunSpec};
use coh::CohAudit;
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport};
use gpu_bridge_host::auto_bridge;
use nine_door::NineDoor;
use serde_json::Value;
use tempfile::TempDir;

#[test]
fn gpu_lease_receipt_includes_proc_lease_snapshot() -> Result<()> {
    let server = NineDoor::new();
    let bridge = auto_bridge(true)?;
    let snapshot = bridge.serialise_namespace()?;
    server.install_gpu_nodes(&snapshot)?;
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    // Seed /proc/lease with a known entry.
    let grant = "{\"op\":\"grant\",\"id\":\"lease-1\",\"subject\":\"queen\",\"resource\":\"gpu0\",\"ttl_s\":60,\"priority\":1}\n";
    coh::CohAccess::write_append(&mut client, "/queen/lease/ctl", grant.as_bytes())?;

    let temp = TempDir::new().expect("tempdir");
    let receipt_path = temp.path().join("lease_receipt.json");
    let args = gpu::GpuLeaseArgs {
        gpu_id: "GPU-0".to_owned(),
        mem_mb: 1024,
        streams: 1,
        ttl_s: 60,
        priority: Some(1),
        budget_ttl_s: None,
        budget_ops: None,
    };
    let bounds = build_local_bounds();
    let mut audit = CohAudit::new();
    gpu::lease_with_receipt(
        &mut client,
        &mut audit,
        &args,
        Some(receipt_path.as_path()),
        &bounds,
    )?;

    let receipt_text = std::fs::read_to_string(&receipt_path)
        .with_context(|| format!("read {}", receipt_path.display()))?;
    let receipt: Value = serde_json::from_str(&receipt_text).context("parse receipt json")?;
    assert_eq!(
        receipt.get("kind").and_then(Value::as_str),
        Some("gpu-lease")
    );

    let entries = receipt
        .pointer("/proc_lease/active_entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        entries
            .iter()
            .any(|entry| entry.get("id").and_then(Value::as_str) == Some("lease-1")),
        "expected proc lease active entry to be present"
    );

    Ok(())
}

#[test]
fn run_receipt_is_written_without_secrets() -> Result<()> {
    let server = NineDoor::new();
    let bridge = auto_bridge(true)?;
    let snapshot = bridge.serialise_namespace()?;
    server.install_gpu_nodes(&snapshot)?;

    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    // Seed a valid GPU lease entry so `coh run` validation passes.
    let lease = "{\"schema\":\"gpu-lease/v1\",\"state\":\"ACTIVE\",\"gpu_id\":\"GPU-0\",\"worker_id\":\"worker-1\",\"mem_mb\":1,\"streams\":1,\"ttl_s\":60,\"priority\":1}\n";
    coh::CohAccess::write_append(&mut client, "/gpu/GPU-0/lease", lease.as_bytes())?;

    // Seed /proc/lease for snapshot.
    let grant = "{\"op\":\"grant\",\"id\":\"lease-2\",\"subject\":\"queen\",\"resource\":\"gpu0\",\"ttl_s\":60,\"priority\":1}\n";
    coh::CohAccess::write_append(&mut client, "/queen/lease/ctl", grant.as_bytes())?;

    let temp = TempDir::new().expect("tempdir");
    let receipt_path = temp.path().join("run_receipt.json");
    let policy = CohPolicy::from_generated();
    let spec = RunSpec {
        gpu_id: "GPU-0".to_owned(),
        command: vec!["echo".to_owned(), "ok".to_owned()],
    };
    let bounds = build_local_bounds();
    let mut audit = CohAudit::new();
    run::execute_with_receipt(
        &mut client,
        &policy,
        &mut audit,
        &spec,
        Some(receipt_path.as_path()),
        &bounds,
    )?;

    let receipt_text = std::fs::read_to_string(&receipt_path)
        .with_context(|| format!("read {}", receipt_path.display()))?;
    let receipt: Value = serde_json::from_str(&receipt_text).context("parse receipt json")?;
    assert_eq!(receipt.get("kind").and_then(Value::as_str), Some("run"));
    assert_eq!(receipt.get("status").and_then(Value::as_str), Some("ok"));

    assert!(
        !receipt_text.contains("changeme"),
        "receipt should not include auth tokens"
    );
    assert!(
        !receipt_text.contains("cohesix-ticket-"),
        "receipt should not include ticket tokens"
    );

    Ok(())
}
