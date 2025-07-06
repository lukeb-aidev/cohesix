// CLASSIFICATION: COMMUNITY
// Filename: gpu.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-12-31

use crate::cuda::runtime::CudaExecutor;
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};

/// Validate CUDA runtime availability by opening `/srv/nvidia0` and
/// launching a minimal kernel. Returns `Ok(())` on success, or an
/// error explaining why GPU execution is not possible.
pub fn coh_check_gpu_runtime() -> Result<(), String> {
    let mut exec = CudaExecutor::new();
    exec.load_kernel(None)?;
    exec.launch()?;
    Ok(())
}
