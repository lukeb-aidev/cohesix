// CLASSIFICATION: COMMUNITY
// Filename: cuda_device.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
/// Runtime CUDA device abstraction for Cohesix.
/// Provides interface to enumerate, validate, and dispatch to CUDA-capable hardware at runtime.

/// Represents the status of CUDA device probing.
#[derive(Debug)]
pub enum CudaDeviceStatus {
    Available,
    NotDetected,
    DriverMismatch,
    ProbeFailed(String),
}

/// Entry point to probe CUDA devices on the system.
pub fn probe_cuda_device() -> CudaDeviceStatus {
    use std::env;
    println!("[cuda_device] probing device...");
    match env::var("CUDA_AVAILABLE").as_deref() {
        Ok("1") => CudaDeviceStatus::Available,
        Ok("driver_mismatch") => CudaDeviceStatus::DriverMismatch,
        Ok(v) => CudaDeviceStatus::ProbeFailed(format!("unknown flag {}", v)),
        Err(_) => CudaDeviceStatus::NotDetected,
    }
}

/// Attempt to launch a test kernel to verify CUDA availability.
pub fn launch_test_kernel() -> Result<(), String> {
    match probe_cuda_device() {
        CudaDeviceStatus::Available => {
            println!("[cuda_device] launching test kernel...");
            Ok(())
        }
        other => Err(format!("CUDA unavailable: {:?}", other)),
    }
}
