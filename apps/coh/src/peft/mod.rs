// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Coh peft helpers for export/import/activation flows.
// Author: Lukas Bower
#![forbid(unsafe_code)]

//! Helpers for PEFT/LoRA lifecycle flows.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{anyhow, Context, Result};

/// Activate and rollback helpers for PEFT models.
pub mod activate;
/// Export helpers for PEFT jobs.
pub mod export;
/// Import helpers for PEFT adapters.
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

static ATOMIC_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_atomic(path: &Path, payload: &[u8]) -> Result<()> {
    let (temp, mut file) = open_unique_atomic_temp(path)?;
    let result = (|| -> Result<()> {
        file.write_all(payload)
            .with_context(|| format!("write temp file {}", temp.display()))?;
        commit_atomic_temp(&temp, path, file)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub(crate) fn open_unique_atomic_temp(path: &Path) -> Result<(PathBuf, File)> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("atomic target has no UTF-8 file name"))?;
    for _ in 0..32 {
        let sequence = ATOMIC_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(
            ".{name}.{}.{}.partial",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("create temp file {}", temp.display()));
            }
        }
    }
    Err(anyhow!(
        "could not allocate unique atomic temp file under {}",
        parent.display()
    ))
}

pub(crate) fn commit_atomic_temp(temp: &Path, path: &Path, file: File) -> Result<()> {
    file.sync_all()
        .with_context(|| format!("sync temp file {}", temp.display()))?;
    drop(file);
    fs::rename(temp, path).with_context(|| format!("commit file {}", path.display()))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(parent)
        .with_context(|| format!("open parent dir {}", parent.display()))?
        .sync_all()
        .with_context(|| format!("sync parent dir {}", parent.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_target_and_leaves_no_partial_file() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("active");
        write_atomic(&path, b"old\n").expect("initial write");
        write_atomic(&path, b"new\n").expect("replacement write");
        assert_eq!(fs::read(&path).expect("read target"), b"new\n");
        assert!(temp.path().read_dir().expect("read dir").all(|entry| {
            !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .contains("partial")
        }));
    }
}
