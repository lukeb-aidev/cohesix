// Copyright 2026 Lukas Bower
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
use gpu_bridge_host::auto_bridge_with_registry;
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
        b"[model]\nid = \"vision-base-v1\"\ncas_sha256 = \"c8a5d3a4b77a641011355372491893a492d81e570ddd2d1dbca05e573d3052bd\"\nformat = \"gguf\"\n",
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

    let manifest = std::fs::read_to_string(&summary.manifest_path)?;
    let manifest: toml::Value = toml::from_str(&manifest)?;
    let model = manifest
        .get("model")
        .and_then(toml::Value::as_table)
        .expect("model table");
    let hashes = manifest
        .get("hashes")
        .and_then(toml::Value::as_table)
        .expect("hashes table");
    let adapter_sha256 = "cd06a2d3968bd0a5ed8d1a66b3bb8f27a0b58d2f99d9b3921a2f9ed778d489a3";
    assert_eq!(
        model.get("cas_sha256").and_then(toml::Value::as_str),
        Some(adapter_sha256)
    );
    assert_eq!(
        model.get("adapter_sha256").and_then(toml::Value::as_str),
        Some(adapter_sha256)
    );
    assert_eq!(
        hashes.get("adapter_sha256").and_then(toml::Value::as_str),
        Some(adapter_sha256)
    );
    assert_eq!(
        model.get("format").and_then(toml::Value::as_str),
        Some("safetensors+lora")
    );

    let server = NineDoor::new();
    let bridge = auto_bridge_with_registry(true, Some(registry_root.path()))?;
    let snapshot = bridge.serialise_namespace()?;
    let imported = snapshot
        .models
        .available
        .iter()
        .find(|model| model.model_id == MODEL_ID)
        .cloned()
        .expect("imported model in bridge catalog");
    assert_eq!(imported.cas_sha256, adapter_sha256);
    assert_eq!(imported.adapter_sha256.as_deref(), Some(adapter_sha256));
    assert_eq!(imported.base_model_id.as_deref(), Some(BASE_MODEL));
    server.install_gpu_nodes(&snapshot)?;

    let mut audit = CohAudit::new();
    let activate = PeftActivateSpec {
        model_id: MODEL_ID.to_owned(),
        registry_root: registry_root.path().to_path_buf(),
    };
    activate_model(&policy, &activate, &mut audit)?;

    let active = std::fs::read_to_string(registry_root.path().join("active"))?;
    assert_eq!(active.trim(), MODEL_ID);
    assert_eq!(
        audit.lines(),
        &[format!(
            "peft activated model={MODEL_ID} execution_location=host projection=pending"
        )]
    );

    let mut audit = CohAudit::new();
    let rollback = PeftRollbackSpec {
        registry_root: registry_root.path().to_path_buf(),
    };
    rollback_model(&policy, &rollback, &mut audit)?;

    let active_after = std::fs::read_to_string(registry_root.path().join("active"))?;
    assert_eq!(active_after.trim(), BASE_MODEL);
    Ok(())
}

#[test]
fn activation_preparation_failure_preserves_active_pointer() -> Result<()> {
    let registry = TempDir::new()?;
    write_file(
        &registry.path().join("available/new/manifest.toml"),
        b"[model]\nid=\"new\"\n",
    )?;
    write_file(&registry.path().join("active"), b"old\n")?;
    let mut policy = CohPolicy::from_generated();
    policy.peft.activate.max_state_bytes = 1;
    let error = activate_model(
        &policy,
        &PeftActivateSpec {
            model_id: "new".to_owned(),
            registry_root: registry.path().to_owned(),
        },
        &mut CohAudit::new(),
    )
    .expect_err("oversized prepared state must fail");
    assert!(error.to_string().contains("max_state_bytes"));
    assert_eq!(std::fs::read(registry.path().join("active"))?, b"old\n");
    assert!(!registry.path().join("active_state.toml").exists());
    Ok(())
}

#[test]
fn interrupted_pointer_commit_requires_reconciliation() -> Result<()> {
    let registry = TempDir::new()?;
    write_file(&registry.path().join("active"), b"old\n")?;
    write_file(
        &registry.path().join("active_state.toml"),
        b"current=\"new\"\nprevious=\"old\"\n",
    )?;
    let error = rollback_model(
        &CohPolicy::from_generated(),
        &PeftRollbackSpec {
            registry_root: registry.path().to_owned(),
        },
        &mut CohAudit::new(),
    )
    .expect_err("partial state must not be treated as a committed activation");
    assert!(error
        .to_string()
        .contains("reconcile the interrupted commit"));
    assert_eq!(std::fs::read(registry.path().join("active"))?, b"old\n");
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

#[test]
fn peft_import_rejects_invalid_base_identity_before_registry_write() -> Result<()> {
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
        b"../vision-base-v1\n",
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

    let spec = PeftImportSpec {
        model_id: MODEL_ID.to_owned(),
        adapter_dir: adapter_root.path().to_path_buf(),
        export_root: export_root.path().to_path_buf(),
        job_id: JOB_ID.to_owned(),
        registry_root: registry_root.path().to_path_buf(),
    };
    let mut audit = CohAudit::new();
    let error = import_adapter(&CohPolicy::from_generated(), &spec, &mut audit)
        .expect_err("invalid base identity must fail");
    assert!(error.to_string().contains("contains '/'"));
    assert!(!registry_root.path().join("available").exists());
    Ok(())
}
