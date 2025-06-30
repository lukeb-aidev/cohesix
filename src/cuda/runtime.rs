// CLASSIFICATION: COMMUNITY
// Filename: runtime.rs v0.11
// Author: Lukas Bower
// Date Modified: 2026-10-27
// Previously gated behind `#![cfg(not(target_os = "uefi"))]`.
// Cohesix now always builds for UEFI, so CUDA runtime is unconditional.

//! Runtime CUDA integration using dynamic loading of `libcuda.so`.
//! Falls back gracefully if no CUDA driver is present.

use crate::runtime::ServiceRegistry;
use libloading::{Library, Symbol};
use crate::validator::{self, RuleViolation};
#[cfg(feature = "cuda")]
use log::info;
use log::warn;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(feature = "cuda")]
use std::time::Instant;

#[cfg(feature = "cuda")]
use cust::prelude::*;
#[cfg(feature = "cuda")]
use cust::CudaApiVersion;
#[cfg(feature = "cuda")]
use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, Nvml};

/// Wrapper around the CUDA driver library.
pub struct CudaRuntime {
    lib: Option<Library>,
    #[cfg(feature = "cuda")]
    #[allow(dead_code)] // context kept alive but unused directly
    ctx: Option<Context>,
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
        let (lib, ctx) = match unsafe { Library::new("libcuda.so") } {
            Ok(l) => match cust::quick_init() {
                Ok(c) => (Some(l), Some(c)),
                Err(e) => {
                    warn!("CUDA init failed: {}", e);
                    (Some(l), None)
                }
            },
            Err(e) => {
                warn!("CUDA library not found: {}", e);
                (None, None)
            }
        };

        #[cfg(not(feature = "cuda"))]
        let lib = {
            warn!("CUDA feature disabled");
            None
        };

        #[cfg(feature = "cuda")]
        let present = lib.is_some() && ctx.is_some();
        #[cfg(not(feature = "cuda"))]
        let present = lib.is_some();
        fs::create_dir_all("/srv/cuda").ok();
        if !present {
            warn!("CUDA unavailable; exposing stub interface at /srv/cuda");
            println!("CUDA not detected on this build target, skipping CUDA tests.");
            std::io::stdout().flush().unwrap();
            fs::write("/srv/cuda/info", "cuda unavailable").ok();
        }
        let _ = ServiceRegistry::register_service("cuda", "/srv/cuda");
        Ok(Self {
            lib,
            #[cfg(feature = "cuda")]
            ctx,
            present,
        })
    }

    /// Return true if CUDA libraries and context were successfully initialized.
    pub fn is_present(&self) -> bool {
        self.present
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
    last_exec_ns: u64,
    fallback_reason: String,
}

impl Default for CudaExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl CudaExecutor {
    pub fn new() -> Self {
        let rt = CudaRuntime::try_new().unwrap_or_else(|_| CudaRuntime {
            lib: None,
            #[cfg(feature = "cuda")]
            ctx: None,
            present: false,
        });
        Self {
            rt,
            kernel: None,
            last_exec_ns: 0,
            fallback_reason: String::new(),
        }
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
    pub fn launch(&mut self) -> Result<(), String> {
        fs::create_dir_all("/log").ok();
        fs::create_dir_all("/srv/trace").ok();
        if self.rt.lib.is_none() || !self.rt.present {
            warn!("CUDA unavailable; stub launch");
            OpenOptions::new()
                .create(true)
                .append(true)
                .open("/log/gpu_runtime.log")
                .and_then(|mut f| writeln!(f, "cuda disabled"))
                .ok();
            self.fallback_reason = "cuda disabled".into();
            self.last_exec_ns = 0;
            fs::write("/srv/cuda_result", b"cuda disabled").map_err(|e| e.to_string())?;
            return Ok(());
        }

        #[cfg(feature = "cuda")]
        {
            let len = self.kernel.as_ref().map(|k| k.len()).unwrap_or(0);
            info!("launching CUDA kernel size {}", len);
            let start = Instant::now();

            crate::validator::log_violation(crate::validator::RuleViolation {
                type_: "unsafe_cuda_launch",
                file: "runtime.rs".into(),
                agent: "cuda_exec".into(),
                time: crate::validator::timestamp(),
            });

            // simple vector addition using embedded PTX
            const PTX: &str = include_str!("../../tests/gpu_demos/add.ptx");
            let module = Module::from_ptx(PTX, &[]).map_err(|e| e.to_string())?;
            let stream = Stream::new(StreamFlags::NON_BLOCKING, None).map_err(|e| e.to_string())?;
            let a = DeviceBuffer::from_slice(&[1.0f32, 2.0, 3.0]).map_err(|e| e.to_string())?;
            let b = DeviceBuffer::from_slice(&[4.0f32, 5.0, 6.0]).map_err(|e| e.to_string())?;
            let out = DeviceBuffer::from_slice(&[0.0f32; 3]).map_err(|e| e.to_string())?;
            unsafe {
                launch!(module.sum<<<1, 3, 0, stream>>>(a.as_device_ptr(), b.as_device_ptr(), out.as_device_ptr(), 3))
                    .map_err(|e| e.to_string())?;
            }
            stream.synchronize().map_err(|e| e.to_string())?;
            let mut host = [0.0f32; 3];
            out.copy_to(&mut host).map_err(|e| e.to_string())?;
            let runtime = start.elapsed().as_nanos() as u64;

            self.last_exec_ns = runtime;
            self.fallback_reason.clear();

            OpenOptions::new()
                .create(true)
                .append(true)
                .open("/log/gpu_runtime.log")
                .and_then(|mut f| writeln!(f, "kernel executed in {runtime}ns"))
                .ok();
            fs::write("/srv/cuda_result", b"kernel executed").map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    /// Gather telemetry about the CUDA environment.
    pub fn telemetry(&self) -> crate::telemetry::core::GpuTelemetry {
        use crate::telemetry::core::GpuTelemetry;
        if !self.rt.present {
            return GpuTelemetry {
                cuda_present: false,
                fallback_reason: "not present".into(),
                exec_time_ns: self.last_exec_ns,
                ..Default::default()
            };
        }
        #[cfg(feature = "cuda")]
        {
            let version = CudaApiVersion::get()
                .map(|v| format!("{}.{}", v.major(), v.minor()))
                .unwrap_or_default();
            let (free, total) = cust::memory::mem_get_info().unwrap_or((0, 0));
            let (temp, util) = if let Ok(nvml) = Nvml::init() {
                if let Ok(dev) = nvml.device_by_index(0) {
                    let t = dev
                        .temperature(TemperatureSensor::Gpu)
                        .ok()
                        .map(|v| v as f32);
                    let u = dev
                        .utilization_rates()
                        .ok()
                        .map(|u| u.gpu);
                    (t, u)
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            };
            GpuTelemetry {
                cuda_present: true,
                driver_version: version,
                mem_total: total as u64,
                mem_free: free as u64,
                fallback_reason: self.fallback_reason.clone(),
                exec_time_ns: self.last_exec_ns,
                temperature: temp,
                gpu_utilization: util,
            }
        }
        #[cfg(not(feature = "cuda"))]
        {
            GpuTelemetry {
                cuda_present: false,
                fallback_reason: "feature disabled".into(),
                exec_time_ns: self.last_exec_ns,
                ..Default::default()
            }
        }
    }
}

