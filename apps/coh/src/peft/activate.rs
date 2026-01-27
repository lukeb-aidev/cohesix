// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Coh peft activation and rollback helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use cohsh_core::wire::AckStatus;
use serde::{Deserialize, Serialize};

use crate::peft::{
    write_atomic, REGISTRY_ACTIVE_FILE, REGISTRY_AVAILABLE_DIR, REGISTRY_STATE_FILE,
};
use crate::policy::{CohPeftActivatePolicy, CohPolicy};
use crate::{validate_component, CohAccess, CohAudit};

const GPU_ACTIVE_PATH: &str = "/gpu/models/active";

/// Activation request parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeftActivateSpec {
    /// Model identifier to activate.
    pub model_id: String,
    /// Registry root containing the model manifests.
    pub registry_root: PathBuf,
}

/// Rollback request parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeftRollbackSpec {
    /// Registry root containing activation state.
    pub registry_root: PathBuf,
}

/// Activate a model by swapping the active pointer.
pub fn activate_model<C: CohAccess>(
    client: &mut C,
    policy: &CohPolicy,
    spec: &PeftActivateSpec,
    audit: &mut CohAudit,
) -> Result<()> {
    validate_component(&spec.model_id)?;
    enforce_id_bytes(&spec.model_id, policy.peft.activate.max_model_id_bytes)?;

    let registry_root = &spec.registry_root;
    ensure_registry_model(registry_root, &spec.model_id)?;

    let mut state = load_state(registry_root, &policy.peft.activate)?;
    let previous = if state.current.trim().is_empty() {
        None
    } else {
        Some(state.current.clone())
    };
    state.previous = previous;
    state.current = spec.model_id.clone();

    write_active_pointer(registry_root, &state.current)?;
    write_state(registry_root, &policy.peft.activate, &state)?;

    write_active_9p(client, &spec.model_id, audit)?;
    audit.push_line(format!("peft activated model={}", spec.model_id));
    Ok(())
}

/// Roll back to the previous active model pointer.
pub fn rollback_model<C: CohAccess>(
    client: &mut C,
    policy: &CohPolicy,
    spec: &PeftRollbackSpec,
    audit: &mut CohAudit,
) -> Result<()> {
    let registry_root = &spec.registry_root;
    let mut state = load_state(registry_root, &policy.peft.activate)?;
    let previous = state
        .previous
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("no previous model available for rollback"))?;

    ensure_registry_model(registry_root, &previous)?;

    let current = state.current.clone();
    state.current = previous.clone();
    state.previous = Some(current.clone());

    write_active_pointer(registry_root, &state.current)?;
    write_state(registry_root, &policy.peft.activate, &state)?;

    write_active_9p(client, &state.current, audit)?;
    audit.push_line(format!(
        "peft rollback from={} to={}",
        current, state.current
    ));
    Ok(())
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PeftState {
    current: String,
    previous: Option<String>,
}

fn load_state(root: &Path, policy: &CohPeftActivatePolicy) -> Result<PeftState> {
    let state_path = root.join(REGISTRY_STATE_FILE);
    if !state_path.is_file() {
        let current = read_active_pointer(root, policy).unwrap_or_default();
        return Ok(PeftState {
            current,
            previous: None,
        });
    }
    let payload = read_bounded(&state_path, policy.max_state_bytes as usize)?;
    let text = String::from_utf8(payload)
        .map_err(|_| anyhow!("state file {} is not UTF-8", state_path.display()))?;
    let state: PeftState = toml::from_str(&text)
        .with_context(|| format!("invalid state TOML in {}", state_path.display()))?;
    Ok(state)
}

fn write_state(root: &Path, policy: &CohPeftActivatePolicy, state: &PeftState) -> Result<()> {
    let payload = toml::to_string(state).context("render state TOML")?;
    if payload.len() > policy.max_state_bytes as usize {
        return Err(anyhow!(
            "state bytes {} exceeds max_state_bytes {}",
            payload.len(),
            policy.max_state_bytes
        ));
    }
    let state_path = root.join(REGISTRY_STATE_FILE);
    write_atomic(&state_path, payload.as_bytes())
}

fn write_active_pointer(root: &Path, model_id: &str) -> Result<()> {
    let path = root.join(REGISTRY_ACTIVE_FILE);
    let payload = format!("{}\n", model_id);
    write_atomic(&path, payload.as_bytes())
}

fn read_active_pointer(root: &Path, policy: &CohPeftActivatePolicy) -> Result<String> {
    let path = root.join(REGISTRY_ACTIVE_FILE);
    let payload = read_bounded(&path, policy.max_model_id_bytes as usize + 1)?;
    let text = String::from_utf8(payload)
        .map_err(|_| anyhow!("active pointer {} is not UTF-8", path.display()))?;
    let line = text
        .lines()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("active pointer {} is empty", path.display()))?;
    Ok(line.to_owned())
}

fn write_active_9p<C: CohAccess>(
    client: &mut C,
    model_id: &str,
    audit: &mut CohAudit,
) -> Result<()> {
    let payload = format!("{}\n", model_id);
    let written = client.write_append(GPU_ACTIVE_PATH, payload.as_bytes())?;
    let detail = format!("path={GPU_ACTIVE_PATH} bytes={written}");
    audit.push_ack(AckStatus::Ok, "ECHO", Some(detail.as_str()));
    Ok(())
}

fn ensure_registry_model(root: &Path, model_id: &str) -> Result<()> {
    let manifest = root
        .join(REGISTRY_AVAILABLE_DIR)
        .join(model_id)
        .join("manifest.toml");
    if !manifest.is_file() {
        return Err(anyhow!("model {} is not available", model_id));
    }
    Ok(())
}

fn read_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let read = file.read(&mut tmp)?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > max_bytes {
            return Err(anyhow!(
                "{} exceeds max bytes {}",
                path.display(),
                max_bytes
            ));
        }
        buf.extend_from_slice(&tmp[..read]);
    }
    Ok(buf)
}

fn enforce_id_bytes(value: &str, max_bytes: u32) -> Result<()> {
    let len = value.as_bytes().len() as u32;
    if len > max_bytes {
        return Err(anyhow!("id length {len} exceeds max {max_bytes}"));
    }
    Ok(())
}
