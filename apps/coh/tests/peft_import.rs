// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate coh peft import and activation behavior.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use coh::peft::{
    activate_model, import_adapter, rollback_model, PeftActivateSpec, PeftImportSpec,
    PeftRollbackSpec,
};
use coh::policy::CohPolicy;
use coh::CohAudit;
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport};
use gpu_bridge_host::auto_bridge;
use nine_door::NineDoor;
use tempfile::TempDir;

const JOB_ID: &str = "job_8932";
const MODEL_ID: &str = "llama3-edge-v7";
const BASE_MODEL: &str = "vision-base-v1";

fn write_file(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create dir {}", parent.display()))?;
    }
    std::fs::write(path, contents).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[test]
fn peft_import_activate_rollback_roundtrip() -> Result<()> {
    let export_root = TempDir::new().expect("export tempdir");
    let adapter_root = TempDir::new().expect("adapter tempdir");
    let registry_root = TempDir::new().expect("registry tempdir");

    let export_job = export_root.path().join(JOB_ID);
    write_file(
        export_job.join("telemetry.cbor").as_path(),
        b"telemetry-v1\n",
    )?;
    write_file(
        export_job.join("base_model.ref").as_path(),
        format!("{}\n", BASE_MODEL).as_bytes(),
    )?;
    write_file(
        export_job.join("policy.toml").as_path(),
        b"[policy]\nname = \"default\"\n",
    )?;

    write_file(
        adapter_root.path().join("adapter.safetensors").as_path(),
        b"adapter-bytes",
    )?;
    write_file(
        adapter_root.path().join("lora.json").as_path(),
        b"{\"rank\":8}",
    )?;

    let registry_available = registry_root.path().join("available").join(BASE_MODEL);
    write_file(
        registry_available.join("manifest.toml").as_path(),
        b"[model]\nid = \"vision-base-v1\"\n",
    )?;
    write_file(
        registry_root.path().join("active").as_path(),
        format!("{}\n", BASE_MODEL).as_bytes(),
    )?;

    let policy = CohPolicy::from_generated();
    let spec = PeftImportSpec {
        model_id: MODEL_ID.to_owned(),
        adapter_dir: adapter_root.path().to_path_buf(),
        export_root: export_root.path().to_path_buf(),
        job_id: JOB_ID.to_owned(),
        registry_root: registry_root.path().to_path_buf(),
    };
    let mut audit = CohAudit::new();
    let summary = import_adapter(&policy, &spec, &mut audit)?;
    assert!(summary.manifest_path.is_file());

    let server = NineDoor::new();
    let bridge = auto_bridge(true)?;
    let snapshot = bridge.serialise_namespace()?;
    server.install_gpu_nodes(&snapshot)?;

    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    let mut audit = CohAudit::new();
    let activate = PeftActivateSpec {
        model_id: MODEL_ID.to_owned(),
        registry_root: registry_root.path().to_path_buf(),
    };
    activate_model(&mut client, &policy, &activate, &mut audit)?;

    let active = std::fs::read_to_string(registry_root.path().join("active"))?;
    assert!(active.trim() == MODEL_ID);

    let mut audit = CohAudit::new();
    let rollback = PeftRollbackSpec {
        registry_root: registry_root.path().to_path_buf(),
    };
    rollback_model(&mut client, &policy, &rollback, &mut audit)?;

    let active_after = std::fs::read_to_string(registry_root.path().join("active"))?;
    assert!(active_after.trim() == BASE_MODEL);
    Ok(())
}

#[test]
fn peft_import_rejects_large_adapter() -> Result<()> {
    let export_root = TempDir::new().expect("export tempdir");
    let adapter_root = TempDir::new().expect("adapter tempdir");
    let registry_root = TempDir::new().expect("registry tempdir");

    let export_job = export_root.path().join(JOB_ID);
    write_file(
        export_job.join("telemetry.cbor").as_path(),
        b"telemetry-v1\n",
    )?;
    write_file(
        export_job.join("base_model.ref").as_path(),
        format!("{}\n", BASE_MODEL).as_bytes(),
    )?;
    write_file(
        export_job.join("policy.toml").as_path(),
        b"[policy]\nname = \"default\"\n",
    )?;

    write_file(
        adapter_root.path().join("adapter.safetensors").as_path(),
        b"adapter-bytes",
    )?;
    write_file(
        adapter_root.path().join("lora.json").as_path(),
        b"{\"rank\":8}",
    )?;

    let mut policy = CohPolicy::from_generated();
    policy.peft.import.max_adapter_bytes = 4;

    let spec = PeftImportSpec {
        model_id: MODEL_ID.to_owned(),
        adapter_dir: adapter_root.path().to_path_buf(),
        export_root: export_root.path().to_path_buf(),
        job_id: JOB_ID.to_owned(),
        registry_root: registry_root.path().to_path_buf(),
    };
    let mut audit = CohAudit::new();
    let err = import_adapter(&policy, &spec, &mut audit).unwrap_err();
    assert!(err.to_string().contains("exceeds max bytes"));
    Ok(())
}
