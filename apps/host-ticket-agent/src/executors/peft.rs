// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Execute PEFT import/activate/rollback ticket actions via existing coh PEFT helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use coh::peft::{
    activate_model, import_adapter, rollback_model, PeftActivateSpec, PeftImportSpec,
    PeftRollbackSpec,
};
use coh::policy::CohPolicy;
use coh::CohAudit;
use cohsh::{Session, Transport};

use super::{arg_bool, arg_str, ExecutorConfig, TransportAccess};
use crate::HostTicketSpec;

/// Execute PEFT ticket actions.
pub fn execute(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    match spec.action.as_str() {
        "peft.import" => execute_import(spec, config),
        "peft.activate" => execute_activate(transport, session, spec, config),
        "peft.rollback" => execute_rollback(transport, session, spec, config),
        other => Err(anyhow!("unsupported peft action {other}")),
    }
}

fn execute_import(spec: &HostTicketSpec, config: &ExecutorConfig) -> Result<String> {
    let model_id = arg_str(spec, "model_id")
        .or_else(|| arg_str(spec, "model"))
        .ok_or_else(|| anyhow!("peft.import requires args.model_id"))?
        .to_owned();
    let job_id = arg_str(spec, "job_id")
        .or_else(|| arg_str(spec, "job"))
        .ok_or_else(|| anyhow!("peft.import requires args.job_id"))?
        .to_owned();
    let adapter_dir = path_arg(spec, "adapter_dir")
        .or_else(|| path_arg(spec, "from"))
        .ok_or_else(|| anyhow!("peft.import requires args.adapter_dir"))?;
    let export_root = path_arg(spec, "export_root")
        .or_else(|| path_arg(spec, "export"))
        .ok_or_else(|| anyhow!("peft.import requires args.export_root"))?;
    let registry_root = path_arg(spec, "registry_root")
        .or_else(|| path_arg(spec, "registry"))
        .unwrap_or_else(|| config.registry_root.clone());

    let policy = CohPolicy::from_generated();
    let mut audit = CohAudit::new();
    let summary = import_adapter(
        &policy,
        &PeftImportSpec {
            model_id: model_id.clone(),
            adapter_dir,
            export_root,
            job_id: job_id.clone(),
            registry_root,
        },
        &mut audit,
    )?;
    let publish = arg_bool(spec, "publish").unwrap_or(false);
    Ok(format!(
        "peft import model={model_id} job={job_id} manifest={} adapter_bytes={} publish_requested={publish}",
        summary.manifest_path.display(),
        summary.adapter_bytes
    ))
}

fn execute_activate(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    let model_id = arg_str(spec, "model_id")
        .or_else(|| arg_str(spec, "model"))
        .ok_or_else(|| anyhow!("peft.activate requires args.model_id"))?
        .to_owned();
    let registry_root = path_arg(spec, "registry_root")
        .or_else(|| path_arg(spec, "registry"))
        .unwrap_or_else(|| config.registry_root.clone());

    let policy = CohPolicy::from_generated();
    let mut access = TransportAccess::new(transport, session);
    let mut audit = CohAudit::new();
    activate_model(
        &mut access,
        &policy,
        &PeftActivateSpec {
            model_id: model_id.clone(),
            registry_root,
        },
        &mut audit,
    )?;
    Ok(format!(
        "peft activate model={model_id} ack_lines={}",
        audit.lines().len()
    ))
}

fn execute_rollback(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    let registry_root = path_arg(spec, "registry_root")
        .or_else(|| path_arg(spec, "registry"))
        .unwrap_or_else(|| config.registry_root.clone());
    let policy = CohPolicy::from_generated();
    let mut access = TransportAccess::new(transport, session);
    let mut audit = CohAudit::new();
    rollback_model(
        &mut access,
        &policy,
        &PeftRollbackSpec { registry_root },
        &mut audit,
    )?;
    Ok(format!("peft rollback ack_lines={}", audit.lines().len()))
}

fn path_arg(spec: &HostTicketSpec, key: &str) -> Option<PathBuf> {
    arg_str(spec, key).map(PathBuf::from)
}
