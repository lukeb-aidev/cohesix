// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Coh peft helpers for export/import/activation flows.
// Author: Lukas Bower
#![forbid(unsafe_code)]

//! Helpers for PEFT/LoRA lifecycle flows.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

pub mod activate;
pub mod export;
pub mod import;

pub use activate::{activate_model, rollback_model, PeftActivateSpec, PeftRollbackSpec};
pub use export::{export_job, PeftExportSpec, PeftExportSummary};
pub use import::{import_adapter, PeftImportSpec, PeftImportSummary};

pub(crate) const EXPORT_TELEMETRY_FILE: &str = "telemetry.cbor";
pub(crate) const EXPORT_BASE_MODEL_FILE: &str = "base_model.ref";
pub(crate) const EXPORT_POLICY_FILE: &str = "policy.toml";

pub(crate) const IMPORT_ADAPTER_FILE: &str = "adapter.safetensors";
pub(crate) const IMPORT_LORA_FILE: &str = "lora.json";
pub(crate) const IMPORT_METRICS_FILE: &str = "metrics.json";

pub(crate) const REGISTRY_ACTIVE_FILE: &str = "active";
pub(crate) const REGISTRY_STATE_FILE: &str = "active_state.toml";
pub(crate) const REGISTRY_AVAILABLE_DIR: &str = "available";

pub(crate) fn write_atomic(path: &Path, payload: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?;
    }
    let tmp_path = path.with_extension("partial");
    fs::write(&tmp_path, payload)
        .with_context(|| format!("write temp file {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).with_context(|| format!("commit file {}", path.display()))?;
    Ok(())
}
