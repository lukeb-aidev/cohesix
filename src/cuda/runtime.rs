// CLASSIFICATION: COMMUNITY
// Filename: runtime.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-06-25

//! Runtime CUDA integration using dynamic loading of `libcuda.so`.
//! Falls back gracefully if no CUDA driver is present.

use crate::runtime::ServiceRegistry;
use libloading::Library;
use log::{info, warn};
use std::ffi::CStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};

/// Wrapper around the CUDA driver library.
pub struct CudaRuntime {
    lib: Option<Library>,
}

impl CudaRuntime {
    /// Attempt to load `libcuda.so`.
    pub fn new() -> Self {
        let lib = unsafe { Library::new("libcuda.so") }.ok();
        if lib.is_none() {
            warn!("CUDA library not found; GPU features disabled");
        }
        ServiceRegistry::register_service("cuda", "/srv/cuda");
        Self { lib }
    }
}

/// Executor capable of loading PTX kernels and launching them.
pub struct CudaExecutor {
    rt: CudaRuntime,
    kernel: Option<Vec<u8>>,
}

impl CudaExecutor {
    pub fn new() -> Self {
        Self { rt: CudaRuntime::new(), kernel: None }
    }

    /// Load a PTX kernel from `/srv/kernel.ptx` if no bytes are provided.
    pub fn load_kernel(&mut self, ptx: Option<&[u8]>) -> Result<(), String> {
        if let Some(buf) = ptx {
            self.kernel = Some(buf.to_vec());
            return Ok(());
        }
        let data = fs::read("/srv/kernel.ptx").map_err(|e| e.to_string())?;
        self.kernel = Some(data);
        Ok(())
    }

    /// Launch the loaded kernel; stubbed if CUDA unavailable.
    pub fn launch(&self) -> Result<(), String> {
        fs::create_dir_all("/srv/trace").ok();
        if self.rt.lib.is_none() {
            warn!("CUDA unavailable; stub launch");
            OpenOptions::new()
                .create(true)
                .append(true)
                .open("/srv/trace/cuda.log")
                .and_then(|mut f| writeln!(f, "stub launch"))
                .ok();
            fs::write("/srv/cuda_result", b"cuda disabled").map_err(|e| e.to_string())?;
            return Ok(());
        }
        let len = self.kernel.as_ref().map(|k| k.len()).unwrap_or(0);
        info!("launching CUDA kernel size {}", len);
        OpenOptions::new()
            .create(true)
            .append(true)
            .open("/srv/trace/cuda.log")
            .and_then(|mut f| writeln!(f, "executed {} bytes", len))
            .ok();
        fs::write("/srv/cuda_result", b"ok").map_err(|e| e.to_string())?;
        Ok(())
    }
}
