// CLASSIFICATION: COMMUNITY
// Filename: gpu.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! GPU driver interface for Cohesix kernel-space runtime.
//! This module provides initialization hooks and runtime checks for GPU availability and basic interaction.

/// Enumeration of supported GPU backends.
#[derive(Debug, Clone, Copy)]
pub enum GpuBackend {
    NvidiaCuda,
    SoftwareFallback,
    None,
}

/// Represents the state of the GPU driver at runtime.
pub struct GpuDriver {
    pub backend: GpuBackend,
    pub initialized: bool,
}

impl GpuDriver {
    /// Attempt to initialize the GPU driver.
    pub fn initialize() -> Self {
        use std::env;
        println!("[GPU] Initializing GPU driver...");
        let backend = match env::var("COHESIX_GPU").as_deref() {
            Ok("cuda") => GpuBackend::NvidiaCuda,
            Ok("none") => GpuBackend::None,
            _ => GpuBackend::SoftwareFallback,
        };
        GpuDriver {
            backend,
            initialized: true,
        }
    }

    /// Query GPU availability.
    pub fn is_available(&self) -> bool {
        matches!(self.backend, GpuBackend::NvidiaCuda)
    }

    /// TODO: Launch a GPU task (stub)
    pub fn launch_task(&self) {
        match self.backend {
            GpuBackend::NvidiaCuda => println!("[GPU] Dispatching CUDA kernel..."),
            GpuBackend::SoftwareFallback => println!("[GPU] Running software fallback"),
            GpuBackend::None => println!("[GPU] No GPU backend available"),
        }
    }
}

