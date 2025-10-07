// CLASSIFICATION: COMMUNITY
// Filename: cuda.rs v1.1
// Author: Lukas Bower
// Date Modified: 2029-11-19

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use std::{env, fs};
/// CUDA physics integration module for Cohesix kernel-space.
/// Provides CUDA dispatch scaffolding and GPU acceleration hooks for physics kernels.
/// Describes the status of CUDA support at runtime.
#[derive(Debug)]
pub enum CudaStatus {
    Available,
    NotDetected,
    UnsupportedDriver,
    Error(String),
}

/// Entry point for probing CUDA capability on the system.
pub fn check_cuda_status() -> CudaStatus {
    println!("[CUDA] Probing remote CUDA orchestration...");
    if env::var("CUDA_SERVER")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return CudaStatus::Available;
    }

    if let Ok(contents) = fs::read_to_string("/srv/cuda") {
        if !contents.trim().is_empty() {
            return CudaStatus::Available;
        }
    }

    CudaStatus::NotDetected
}

/// Launches a physics compute kernel on the GPU.
pub fn launch_physics_kernel() -> Result<(), String> {
    match check_cuda_status() {
        CudaStatus::Available => {
            let mut exec = crate::cuda::runtime::CudaExecutor::new();
            exec.load_kernel(None)?;
            exec.launch()
        }
        CudaStatus::NotDetected => Err("remote cuda endpoint not configured".into()),
        CudaStatus::UnsupportedDriver => Err("cuda driver unsupported".into()),
        CudaStatus::Error(e) => Err(e),
    }
}
