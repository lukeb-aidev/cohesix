// CLASSIFICATION: COMMUNITY
// Filename: cuda.rs v1.1
// Author: Lukas Bower
// Date Modified: 2026-07-23

//! CUDA physics integration module for Cohesix kernel-space.
//! Provides CUDA dispatch scaffolding and GPU acceleration hooks for physics kernels.

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
    println!("[CUDA] Probing CUDA support...");
    if std::path::Path::new("/srv/nvhost").exists() {
        CudaStatus::Available
    } else {
        CudaStatus::NotDetected
    }
}

/// Launches a physics compute kernel on the GPU.
pub fn launch_physics_kernel() -> Result<(), String> {
    match check_cuda_status() {
        CudaStatus::Available => {
            #[cfg(feature = "cuda")]
            {
                let mut exec = crate::cuda::runtime::CudaExecutor::new();
                exec.load_kernel(None)?;
                return exec.launch();
            }
            #[cfg(not(feature = "cuda"))]
            {
                Err("cuda feature disabled".into())
            }
        }
        CudaStatus::NotDetected => Err("cuda not detected".into()),
        CudaStatus::UnsupportedDriver => Err("cuda driver unsupported".into()),
        CudaStatus::Error(e) => Err(e),
    }
}

