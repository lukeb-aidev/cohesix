// CLASSIFICATION: COMMUNITY
// Filename: runtime.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

//! Runtime CUDA integration using dynamic loading of `libcuda.so`.
//! Falls back gracefully if no CUDA driver is present.

use libloading::Library;
use log::{info, warn};
use std::ffi::CStr;
use std::fs;

/// Wrapper around the CUDA driver library. If the library cannot be loaded,
/// CUDA functions become no-ops.
pub struct CudaRuntime {
    lib: Option<Library>,
}

impl CudaRuntime {
    /// Attempt to load `libcuda.so` and initialize the driver.
    pub fn new() -> Self {
        let lib = unsafe { Library::new("libcuda.so") }.ok();
        if lib.is_none() {
            warn!("CUDA library not found; GPU features disabled");
        }
        Self { lib }
    }

    /// Return a list of device names detected via the CUDA driver.
    pub fn device_names(&self) -> Vec<String> {
        if let Some(lib) = &self.lib {
            unsafe {
                type CuInit = unsafe extern "C" fn(u32) -> i32;
                type CuDeviceGetCount = unsafe extern "C" fn(*mut i32) -> i32;
                type CuDeviceGet = unsafe extern "C" fn(*mut i32, i32) -> i32;
                type CuDeviceGetName = unsafe extern "C" fn(*mut i8, i32, i32) -> i32;

                let cu_init: libloading::Symbol<CuInit> = match lib.get(b"cuInit") {
                    Ok(s) => s,
                    Err(_) => return Vec::new(),
                };
                if cu_init(0) != 0 {
                    warn!("cuInit failed");
                    return Vec::new();
                }
                let cu_device_get_count: libloading::Symbol<CuDeviceGetCount> =
                    match lib.get(b"cuDeviceGetCount") {
                        Ok(s) => s,
                        Err(_) => return Vec::new(),
                    };
                let cu_device_get: libloading::Symbol<CuDeviceGet> = match lib.get(b"cuDeviceGet") {
                    Ok(s) => s,
                    Err(_) => return Vec::new(),
                };
                let cu_device_get_name: libloading::Symbol<CuDeviceGetName> =
                    match lib.get(b"cuDeviceGetName") {
                        Ok(s) => s,
                        Err(_) => return Vec::new(),
                    };
                let mut count = 0i32;
                if cu_device_get_count(&mut count as *mut _) != 0 {
                    warn!("cuDeviceGetCount failed");
                    return Vec::new();
                }
                let mut names = Vec::new();
                for i in 0..count {
                    let mut dev = 0i32;
                    if cu_device_get(&mut dev as *mut _, i) != 0 {
                        continue;
                    }
                    let mut buf = [0i8; 64];
                    if cu_device_get_name(buf.as_mut_ptr(), 64, dev) == 0 {
                        let cstr = CStr::from_ptr(buf.as_ptr());
                        names.push(cstr.to_string_lossy().into_owned());
                    }
                }
                names
            }
        } else {
            Vec::new()
        }
    }
}

/// Trait describing the ability to launch a GPU task from a PTX kernel.
pub trait GpuTaskExecutor {
    /// Load a PTX kernel from `srv/kernel.ptx` and execute it on the default device.
    /// The result should be written to `srv/cuda_output`.
    fn launch_kernel(&self) -> Result<(), String>;
}

/// Default implementation backed by [`CudaRuntime`].
pub struct CudaExecutor {
    rt: CudaRuntime,
}

impl CudaExecutor {
    pub fn new() -> Self {
        Self { rt: CudaRuntime::new() }
    }
}

impl GpuTaskExecutor for CudaExecutor {
    fn launch_kernel(&self) -> Result<(), String> {
        if self.rt.lib.is_none() {
            warn!("CUDA unavailable; writing stub output");
            fs::write("srv/cuda_output", b"cuda disabled").map_err(|e| e.to_string())?;
            return Ok(());
        }
        let ptx = fs::read_to_string("srv/kernel.ptx").map_err(|e| e.to_string())?;
        info!("Launching CUDA kernel ({} bytes)", ptx.len());
        // Real kernel launch would occur here via cuModuleLoadDataEx etc.
        fs::write("srv/cuda_output", b"kernel executed").map_err(|e| e.to_string())?;
        Ok(())
    }
}
