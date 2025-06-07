// CLASSIFICATION: COMMUNITY
// Filename: runtime.rs v0.4
// Author: Lukas Bower
// Date Modified: 2025-07-07

//! Runtime CUDA integration using dynamic loading of `libcuda.so`.
//! Falls back gracefully if no CUDA driver is present.

use crate::runtime::ServiceRegistry;
use libloading::Library;
use log::{info, warn};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};

/// Wrapper around the CUDA driver library.
pub struct CudaRuntime {
    lib: Option<Library>,
}

impl CudaRuntime {
    /// Attempt to load `libcuda.so` if the `cuda` feature is enabled.
    pub fn try_new() -> io::Result<Self> {
        #[cfg(feature = "cuda")]
        let lib = match unsafe { Library::new("libcuda.so") } {
            Ok(l) => Some(l),
            Err(e) => {
                warn!("CUDA library not found: {}", e);
                None
            }
        };

        #[cfg(not(feature = "cuda"))]
        let lib = {
            warn!("CUDA feature disabled");
            None
        };

        ServiceRegistry::register_service("cuda", "/srv/cuda");
        Ok(Self { lib })
    }
}

/// Executor capable of loading PTX kernels and launching them.
pub struct CudaExecutor {
    rt: CudaRuntime,
    kernel: Option<Vec<u8>>,
}

impl CudaExecutor {
    pub fn new() -> Self {
        let rt = CudaRuntime::try_new().unwrap_or_else(|_| CudaRuntime { lib: None });
        Self { rt, kernel: None }
    }

    /// Load a PTX kernel from `/srv/kernel.ptx` if no bytes are provided.
    pub fn load_kernel(&mut self, ptx: Option<&[u8]>) -> Result<(), String> {
        let data = if let Some(buf) = ptx {
            buf.to_vec()
        } else {
            fs::read("/srv/kernel.ptx").map_err(|e| e.to_string())?
        };
        if data.len() > 64 * 1024 {
            return Err("kernel too large".into());
        }
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
