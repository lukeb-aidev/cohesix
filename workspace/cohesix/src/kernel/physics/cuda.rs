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
        .filter(|v| is_valid_endpoint(v))
        .is_some()
    {
        return CudaStatus::Available;
    }

    if let Ok(contents) = fs::read_to_string("/srv/cuda") {
        if contents
            .lines()
            .map(str::trim)
            .any(|line| !line.is_empty() && is_valid_endpoint(line))
        {
            return CudaStatus::Available;
        }
    }

    CudaStatus::NotDetected
}

fn is_valid_endpoint(candidate: &str) -> bool {
    let trimmed = candidate.trim();
    trimmed
        .strip_prefix("tcp:")
        .map(|rest| !rest.trim().is_empty())
        .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::is_valid_endpoint;

    #[test]
    fn accepts_valid_tcp_endpoints() {
        assert!(is_valid_endpoint("tcp:gpu.example:9000"));
        assert!(is_valid_endpoint("  tcp:gpu.internal:3000  "));
    }

    #[test]
    fn rejects_invalid_or_placeholder_endpoints() {
        assert!(!is_valid_endpoint(""));
        assert!(!is_valid_endpoint("ready"));
        assert!(!is_valid_endpoint("tcp:"));
        assert!(!is_valid_endpoint("tcp:   "));
        assert!(!is_valid_endpoint("udp:gpu.internal:3000"));
    }
}
