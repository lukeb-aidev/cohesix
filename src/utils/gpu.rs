// CLASSIFICATION: COMMUNITY
// Filename: gpu.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-07-23

#[cfg(feature = "cuda")]
use crate::cuda::runtime::{CudaExecutor, CudaRuntime};
use crate::prelude::*;
use std::fs::OpenOptions;

/// Validate CUDA runtime availability by opening `/srv/nvidia0` and
/// launching a minimal kernel. Returns `Ok(())` on success, or an
/// error explaining why GPU execution is not possible.
pub fn coh_check_gpu_runtime() -> Result<(), String> {
    if std::env::var("COH_GPU").unwrap_or_default() == "0" {
        return Err("COH_GPU=0".into());
    }
    let rt = CudaRuntime::try_new().map_err(|e| e.to_string())?;
    if !rt.is_present() {
        return Err("CUDA runtime unavailable".into());
    }
    OpenOptions::new()
        .read(true)
        .open("/srv/nvidia0")
        .map_err(|e| format!("cannot open /srv/nvidia0: {e}"))?;
    let mut exec = CudaExecutor::new();
    exec.load_kernel(Some(b"fake"))?;
    exec.launch()?;
    Ok(())
}
