// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate coh peft export behavior.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use coh::peft::{export_job, PeftExportSpec};
use coh::policy::CohPolicy;
use coh::CohAudit;
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport};
use nine_door::NineDoor;
use tempfile::TempDir;

const JOB_ID: &str = "job_8932";

fn seed_job(server: &NineDoor) -> Result<()> {
    let telemetry = b"telemetry-v1\n";
    let base_model = b"vision-base-v1\n";
    let policy = b"[policy]\nname = \"default\"\n";
    server
        .set_lora_export_job(JOB_ID, telemetry, base_model, policy)
        .context("seed export job")?;
    Ok(())
}

#[test]
fn peft_export_is_idempotent() -> Result<()> {
    let server = NineDoor::new();
    seed_job(&server)?;

    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    let policy = CohPolicy::from_generated();
    let temp = TempDir::new().expect("tempdir");
    let spec = PeftExportSpec {
        job_id: JOB_ID.to_owned(),
        out_dir: temp.path().to_path_buf(),
    };

    let mut audit = CohAudit::new();
    export_job(&mut client, &policy, &spec, &mut audit)?;

    let job_dir = temp.path().join(JOB_ID);
    let telemetry = std::fs::read(job_dir.join("telemetry.cbor"))?;
    let base_model = std::fs::read(job_dir.join("base_model.ref"))?;
    let policy_text = std::fs::read(job_dir.join("policy.toml"))?;
    assert_eq!(telemetry, b"telemetry-v1\n");
    assert_eq!(base_model, b"vision-base-v1\n");
    assert_eq!(policy_text, b"[policy]\nname = \"default\"\n");

    let mut second_audit = CohAudit::new();
    export_job(&mut client, &policy, &spec, &mut second_audit)?;
    let telemetry_again = std::fs::read(job_dir.join("telemetry.cbor"))?;
    assert_eq!(telemetry_again, telemetry);
    Ok(())
}

#[test]
fn peft_export_missing_job_is_deterministic() -> Result<()> {
    let server = NineDoor::new();
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    let policy = CohPolicy::from_generated();
    let temp = TempDir::new().expect("tempdir");
    let spec = PeftExportSpec {
        job_id: "missing-job".to_owned(),
        out_dir: temp.path().to_path_buf(),
    };

    let mut audit = CohAudit::new();
    let err = export_job(&mut client, &policy, &spec, &mut audit).unwrap_err();
    assert!(err.to_string().contains("export job missing-job not found"));
    Ok(())
}
