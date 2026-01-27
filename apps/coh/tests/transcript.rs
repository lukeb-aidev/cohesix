// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compare coh transcript output against shared fixtures.
// Author: Lukas Bower
#![forbid(unsafe_code)]

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use anyhow::{Context, Result};
use coh::gpu;
use coh::peft;
use coh::policy::CohPolicy;
use coh::run::{self, RunSpec};
use coh::telemetry;
use coh::CohAudit;
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport};
use gpu_bridge_host::auto_bridge;
use gpu_bridge_host::auto_bridge_with_registry;
use nine_door::NineDoor;
use secure9p_codec::OpenMode;
use tempfile::TempDir;

const SCENARIO: &str = "converge_v0";
const RUN_SCENARIO: &str = "run_demo_v0";
const PEFT_SCENARIO: &str = "peft_roundtrip_v0";

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
    transcript_support::write_timing("coh", SCENARIO, "transcript", 0);
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
    audit.push_ack(
        cohsh_core::wire::AckStatus::Ok,
        "ECHO",
        Some(detail.as_str()),
    );
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
    audit.push_ack(
        cohsh_core::wire::AckStatus::Ok,
        "CAT",
        Some(detail.as_str()),
    );
    transcript.extend(audit.into_lines());

    let released_lease = lease_entry("RELEASED");
    let written = write_append_len(&mut client, lease_path, released_lease.as_bytes())?;
    let mut audit = CohAudit::new();
    let detail = format!("path={lease_path} bytes={written}");
    audit.push_ack(
        cohsh_core::wire::AckStatus::Ok,
        "ECHO",
        Some(detail.as_str()),
    );
    transcript.extend(audit.into_lines());

    transcript_support::compare_transcript("coh", RUN_SCENARIO, "cohsh.txt", &transcript);
    transcript_support::write_timing("coh", RUN_SCENARIO, "transcript", 0);
    Ok(())
}

#[test]
fn coh_peft_transcript_matches_cohsh_baseline() -> Result<()> {
    let server = NineDoor::new();
    seed_peft_export_job(&server)?;
    seed_gpu_nodes(&server)?;

    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    let policy = CohPolicy::from_generated();

    let export_out = TempDir::new().expect("tempdir");
    let adapter_dir = TempDir::new().expect("tempdir");
    let registry_root = TempDir::new().expect("tempdir");

    let registry_available = registry_root
        .path()
        .join("available")
        .join("vision-base-v1");
    write_file(
        registry_available.join("manifest.toml").as_path(),
        b"[model]\nid = \"vision-base-v1\"\n",
    )?;
    write_file(
        registry_root.path().join("active").as_path(),
        b"vision-base-v1\n",
    )?;

    write_file(
        adapter_dir.path().join("adapter.safetensors").as_path(),
        b"adapter-bytes",
    )?;
    write_file(
        adapter_dir.path().join("lora.json").as_path(),
        b"{\"rank\":8}",
    )?;

    let export_spec = peft::PeftExportSpec {
        job_id: "job_8932".to_owned(),
        out_dir: export_out.path().to_path_buf(),
    };
    let mut transcript = Vec::new();
    let mut audit = CohAudit::new();
    peft::export_job(&mut client, &policy, &export_spec, &mut audit)?;
    transcript.extend(audit.into_lines());

    let import_spec = peft::PeftImportSpec {
        model_id: "llama3-edge-v7".to_owned(),
        adapter_dir: adapter_dir.path().to_path_buf(),
        export_root: export_out.path().to_path_buf(),
        job_id: "job_8932".to_owned(),
        registry_root: registry_root.path().to_path_buf(),
    };
    let mut audit = CohAudit::new();
    peft::import_adapter(&policy, &import_spec, &mut audit)?;

    let bridge = auto_bridge_with_registry(true, Some(registry_root.path()))?;
    let snapshot = bridge.serialise_namespace()?;
    server.install_gpu_nodes(&snapshot)?;

    read_with_ack(
        &mut client,
        "/gpu/models/available/llama3-edge-v7/manifest.toml",
        &mut transcript,
    )?;

    let activate = peft::PeftActivateSpec {
        model_id: "llama3-edge-v7".to_owned(),
        registry_root: registry_root.path().to_path_buf(),
    };
    let mut audit = CohAudit::new();
    peft::activate_model(&mut client, &policy, &activate, &mut audit)?;
    transcript.extend(audit.into_lines());

    let rollback = peft::PeftRollbackSpec {
        registry_root: registry_root.path().to_path_buf(),
    };
    let mut audit = CohAudit::new();
    peft::rollback_model(&mut client, &policy, &rollback, &mut audit)?;
    transcript.extend(audit.into_lines());

    transcript_support::compare_transcript("coh", PEFT_SCENARIO, "cohsh.txt", &transcript);
    transcript_support::write_timing("coh", PEFT_SCENARIO, "transcript", 0);
    Ok(())
}

fn seed_telemetry<T: cohsh_core::Secure9pTransport>(client: &mut CohClient<T>) -> Result<()> {
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

fn read_with_ack<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
    transcript: &mut Vec<String>,
) -> Result<()> {
    let _payload = read_all(client, path)?;
    let mut audit = CohAudit::new();
    let detail = format!("path={path}");
    audit.push_ack(
        cohsh_core::wire::AckStatus::Ok,
        "CAT",
        Some(detail.as_str()),
    );
    transcript.extend(audit.into_lines());
    Ok(())
}

fn seed_gpu_nodes(server: &NineDoor) -> Result<()> {
    let bridge = auto_bridge(true)?;
    let snapshot = bridge.serialise_namespace()?;
    server.install_gpu_nodes(&snapshot)?;
    Ok(())
}

fn seed_peft_export_job(server: &NineDoor) -> Result<()> {
    let telemetry = b"telemetry-v1\n";
    let base_model = b"vision-base-v1\n";
    let policy = b"[policy]\nname = \"default\"\n";
    server.set_lora_export_job("job_8932", telemetry, base_model, policy)?;
    Ok(())
}

fn write_file(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;
    Ok(())
}
