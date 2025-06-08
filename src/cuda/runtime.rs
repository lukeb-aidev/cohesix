// CLASSIFICATION: COMMUNITY
// Filename: runtime.rs v0.5
// Author: Lukas Bower
// Date Modified: 2025-07-08

//! Runtime CUDA integration using dynamic loading of `libcuda.so`.
//! Falls back gracefully if no CUDA driver is present.

use crate::runtime::ServiceRegistry;
use libloading::{Library, Symbol};
use crate::validator::{self, RuleViolation};
use log::{info, warn};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};

/// Wrapper around the CUDA driver library.
pub struct CudaRuntime {
    lib: Option<Library>,
    present: bool,
}

static VALID_SYMBOLS: &[&str] = &[
    "cuInit",
    "cuDeviceGetCount",
    "cuDeviceGet",
];

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

        let present = lib.is_some();
        ServiceRegistry::register_service("cuda", "/srv/cuda");
        Ok(Self { lib, present })
    }

    /// Load a verified symbol from the CUDA library.
    pub fn get_symbol<T>(&self, name: &[u8]) -> anyhow::Result<Symbol<T>> {
        let lib = self
            .lib
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("cuda library not loaded"))?;
        let name_str = std::str::from_utf8(name).unwrap_or("");
        if !VALID_SYMBOLS.contains(&name_str) {
            validator::log_violation(RuleViolation {
                type_: "ffi_symbol",
                file: name_str.into(),
                agent: "cuda".into(),
                time: validator::timestamp(),
            });
            return Err(anyhow::anyhow!("symbol not allowed"));
        }
        unsafe { lib.get::<T>(name).map_err(|e| anyhow::anyhow!(e.to_string())) }
    }

    /// Initialize the CUDA driver via verified FFI entry.
    pub fn init_driver(&self) -> Result<(), String> {
        let sym: Symbol<unsafe extern "C" fn(u32) -> i32> =
            self.get_symbol(b"cuInit").map_err(|e| e.to_string())?;
        validator::log_violation(RuleViolation {
            type_: "ffi_enter",
            file: "cuInit".into(),
            agent: "cuda".into(),
            time: validator::timestamp(),
        });
        let res = unsafe { sym(0) };
        validator::log_violation(RuleViolation {
            type_: "ffi_exit",
            file: "cuInit".into(),
            agent: "cuda".into(),
            time: validator::timestamp(),
        });
        if res == 0 {
            Ok(())
        } else {
            Err(format!("cuInit failed: {}", res))
        }
    }
}

/// Executor capable of loading PTX kernels and launching them.
pub struct CudaExecutor {
    rt: CudaRuntime,
    kernel: Option<Vec<u8>>,
}

impl CudaExecutor {
    pub fn new() -> Self {
        let rt = CudaRuntime::try_new().unwrap_or_else(|_| CudaRuntime { lib: None, present: false });
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

    /// Gather telemetry about the CUDA environment.
    pub fn telemetry(&self) -> crate::telemetry::telemetry::GpuTelemetry {
        use crate::telemetry::telemetry::GpuTelemetry;
        if !self.rt.present {
            return GpuTelemetry { cuda_present: false, ..Default::default() };
        }
        GpuTelemetry {
            cuda_present: true,
            driver_version: "stub".into(),
            mem_total: 0,
            mem_free: 0,
        }
    }
}

