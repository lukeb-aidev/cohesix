// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide CUDA driver/runtime probes for host GPU inventory.
// Author: Lukas Bower
#![warn(missing_docs)]

//! Host-side CUDA probe utilities for GPU inventory.

use anyhow::Result;

/// CUDA device descriptor discovered via driver/runtime APIs.
#[derive(Debug, Clone)]
pub struct CudaDeviceInfo {
    /// Human-friendly GPU name.
    pub name: String,
    /// Total memory available to the device (bytes).
    pub total_memory_bytes: u64,
    /// Streaming multiprocessor count or equivalent.
    pub sm_count: u32,
    /// CUDA driver version (major.minor).
    pub driver_version: String,
    /// CUDA runtime version (major.minor).
    pub runtime_version: String,
}

/// Enumerate CUDA devices on the host using the driver/runtime APIs.
pub fn enumerate_devices() -> Result<Vec<CudaDeviceInfo>> {
    #[cfg(target_os = "linux")]
    {
        linux::enumerate_devices()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(anyhow::anyhow!(
            "cuda backend unsupported on {}",
            std::env::consts::OS
        ))
    }
}

#[cfg(any(test, target_os = "linux"))]
fn format_cuda_version(raw: i32) -> String {
    if raw <= 0 {
        return "unknown".to_owned();
    }
    let major = raw / 1000;
    let minor = (raw % 1000) / 10;
    format!("{major}.{minor}")
}

#[cfg(target_os = "linux")]
mod linux {
    // Safety: this module performs CUDA FFI calls via libcuda/libcudart; unsafe is isolated here.
    use super::{format_cuda_version, CudaDeviceInfo};
    use anyhow::{anyhow, Context, Result};
    use libloading::Library;
    use std::ffi::{c_char, c_void, CStr};

    const CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT: i32 = 16;

    type CUdevice = i32;
    type CUresult = i32;

    type CuInit = unsafe extern "C" fn(u32) -> CUresult;
    type CuDeviceGetCount = unsafe extern "C" fn(*mut i32) -> CUresult;
    type CuDeviceGetName = unsafe extern "C" fn(*mut c_char, i32, CUdevice) -> CUresult;
    type CuDeviceTotalMem = unsafe extern "C" fn(*mut usize, CUdevice) -> CUresult;
    type CuDeviceGetAttribute = unsafe extern "C" fn(*mut i32, i32, CUdevice) -> CUresult;
    type CuDriverGetVersion = unsafe extern "C" fn(*mut i32) -> CUresult;

    type CudaRuntimeGetVersion = unsafe extern "C" fn(*mut i32) -> i32;
    type CudaMemGetInfo = unsafe extern "C" fn(*mut usize, *mut usize) -> i32;
    type CudaSetDevice = unsafe extern "C" fn(i32) -> i32;
    type CudaFree = unsafe extern "C" fn(*mut c_void) -> i32;

    pub(super) fn enumerate_devices() -> Result<Vec<CudaDeviceInfo>> {
        let driver = CudaDriver::load()?;
        driver.init()?;

        let device_count = driver.device_count()?;
        if device_count <= 0 {
            return Err(anyhow!("cuda reported no devices"));
        }

        let driver_version = driver.driver_version()?;
        let runtime = CudaRuntime::load().ok();
        let runtime_version = runtime
            .as_ref()
            .and_then(|rt| rt.runtime_version().ok())
            .unwrap_or_else(|| driver_version.clone());

        let mut devices = Vec::new();
        for index in 0..device_count {
            let device = CUdevice::from(index);
            let name = driver.device_name(device)?;
            let sm_count =
                driver.device_attribute(device, CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT)?;
            let total_memory_bytes = driver.device_total_mem(device).or_else(|err| {
                runtime
                    .as_ref()
                    .ok_or(err)
                    .and_then(|rt| rt.device_total_mem(device))
            })?;
            devices.push(CudaDeviceInfo {
                name,
                total_memory_bytes,
                sm_count: sm_count.max(0) as u32,
                driver_version: driver_version.clone(),
                runtime_version: runtime_version.clone(),
            });
        }
        Ok(devices)
    }

    struct CudaDriver {
        _lib: Library,
        cu_init: CuInit,
        cu_device_get_count: CuDeviceGetCount,
        cu_device_get_name: CuDeviceGetName,
        cu_device_total_mem_v2: Option<CuDeviceTotalMem>,
        cu_device_get_attribute: CuDeviceGetAttribute,
        cu_driver_get_version: CuDriverGetVersion,
    }

    impl CudaDriver {
        fn load() -> Result<Self> {
            let lib = load_library(&["libcuda.so.1", "libcuda.so"])?;
            unsafe {
                let cu_init = load_symbol::<CuInit>(&lib, b"cuInit\0")?;
                let cu_device_get_count =
                    load_symbol::<CuDeviceGetCount>(&lib, b"cuDeviceGetCount\0")?;
                let cu_device_get_name =
                    load_symbol::<CuDeviceGetName>(&lib, b"cuDeviceGetName\0")?;
                let cu_device_get_attribute =
                    load_symbol::<CuDeviceGetAttribute>(&lib, b"cuDeviceGetAttribute\0")?;
                let cu_driver_get_version =
                    load_symbol::<CuDriverGetVersion>(&lib, b"cuDriverGetVersion\0")?;
                let cu_device_total_mem_v2 =
                    load_symbol::<CuDeviceTotalMem>(&lib, b"cuDeviceTotalMem_v2\0").ok();
                Ok(Self {
                    _lib: lib,
                    cu_init,
                    cu_device_get_count,
                    cu_device_get_name,
                    cu_device_total_mem_v2,
                    cu_device_get_attribute,
                    cu_driver_get_version,
                })
            }
        }

        fn init(&self) -> Result<()> {
            cuda_ok(unsafe { (self.cu_init)(0) }, "cuInit")
        }

        fn device_count(&self) -> Result<i32> {
            let mut count = 0i32;
            cuda_ok(
                unsafe { (self.cu_device_get_count)(&mut count as *mut i32) },
                "cuDeviceGetCount",
            )?;
            Ok(count)
        }

        fn device_name(&self, device: CUdevice) -> Result<String> {
            let mut buffer = vec![0 as c_char; 256];
            cuda_ok(
                unsafe {
                    (self.cu_device_get_name)(buffer.as_mut_ptr(), buffer.len() as i32, device)
                },
                "cuDeviceGetName",
            )?;
            // Safety: CUDA returns a NUL-terminated device name within the provided buffer.
            let name = unsafe { CStr::from_ptr(buffer.as_ptr()) }
                .to_string_lossy()
                .trim()
                .to_owned();
            if name.is_empty() {
                return Err(anyhow!("cuda device name unavailable"));
            }
            Ok(name)
        }

        fn device_total_mem(&self, device: CUdevice) -> Result<u64> {
            let Some(func) = self.cu_device_total_mem_v2 else {
                return Err(anyhow!("cuDeviceTotalMem_v2 unavailable"));
            };
            let mut bytes = 0usize;
            cuda_ok(
                unsafe { func(&mut bytes as *mut usize, device) },
                "cuDeviceTotalMem_v2",
            )?;
            Ok(bytes as u64)
        }

        fn device_attribute(&self, device: CUdevice, attr: i32) -> Result<i32> {
            let mut value = 0i32;
            cuda_ok(
                unsafe { (self.cu_device_get_attribute)(&mut value as *mut i32, attr, device) },
                "cuDeviceGetAttribute",
            )?;
            Ok(value)
        }

        fn driver_version(&self) -> Result<String> {
            let mut version = 0i32;
            cuda_ok(
                unsafe { (self.cu_driver_get_version)(&mut version as *mut i32) },
                "cuDriverGetVersion",
            )?;
            Ok(format_cuda_version(version))
        }
    }

    struct CudaRuntime {
        _lib: Library,
        cuda_runtime_get_version: CudaRuntimeGetVersion,
        cuda_mem_get_info: CudaMemGetInfo,
        cuda_set_device: CudaSetDevice,
        cuda_free: CudaFree,
    }

    impl CudaRuntime {
        fn load() -> Result<Self> {
            let lib = load_library(&["libcudart.so.12", "libcudart.so"])?;
            unsafe {
                let cuda_runtime_get_version =
                    load_symbol::<CudaRuntimeGetVersion>(&lib, b"cudaRuntimeGetVersion\0")?;
                let cuda_mem_get_info = load_symbol::<CudaMemGetInfo>(&lib, b"cudaMemGetInfo\0")?;
                let cuda_set_device = load_symbol::<CudaSetDevice>(&lib, b"cudaSetDevice\0")?;
                let cuda_free = load_symbol::<CudaFree>(&lib, b"cudaFree\0")?;
                Ok(Self {
                    _lib: lib,
                    cuda_runtime_get_version,
                    cuda_mem_get_info,
                    cuda_set_device,
                    cuda_free,
                })
            }
        }

        fn runtime_version(&self) -> Result<String> {
            let mut version = 0i32;
            runtime_ok(
                unsafe { (self.cuda_runtime_get_version)(&mut version as *mut i32) },
                "cudaRuntimeGetVersion",
            )?;
            Ok(format_cuda_version(version))
        }

        fn device_total_mem(&self, device: CUdevice) -> Result<u64> {
            runtime_ok(unsafe { (self.cuda_set_device)(device) }, "cudaSetDevice")?;
            let _ = unsafe { (self.cuda_free)(std::ptr::null_mut()) };
            let mut free = 0usize;
            let mut total = 0usize;
            runtime_ok(
                unsafe {
                    (self.cuda_mem_get_info)(&mut free as *mut usize, &mut total as *mut usize)
                },
                "cudaMemGetInfo",
            )?;
            Ok(total as u64)
        }
    }

    fn load_library(names: &[&str]) -> Result<Library> {
        let mut last_err = None;
        for name in names {
            // Safety: loading a dynamic library by name is required for CUDA probing.
            match unsafe { Library::new(name) } {
                Ok(lib) => return Ok(lib),
                Err(err) => last_err = Some(err),
            }
        }
        Err(anyhow!(
            "failed to load CUDA library {:?}: {}",
            names,
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        ))
    }

    unsafe fn load_symbol<T>(lib: &Library, symbol: &[u8]) -> Result<T>
    where
        T: Copy,
    {
        // Safety: symbol signatures match the CUDA driver/runtime ABI.
        let sym = lib
            .get::<T>(symbol)
            .with_context(|| format!("missing CUDA symbol {}", String::from_utf8_lossy(symbol)))?;
        Ok(*sym)
    }

    fn cuda_ok(result: CUresult, context: &str) -> Result<()> {
        if result == 0 {
            return Ok(());
        }
        Err(anyhow!("{context} failed with code {result}"))
    }

    fn runtime_ok(result: i32, context: &str) -> Result<()> {
        if result == 0 {
            return Ok(());
        }
        Err(anyhow!("{context} failed with code {result}"))
    }
}

#[cfg(test)]
mod tests {
    use super::format_cuda_version;

    #[test]
    fn cuda_version_formatting() {
        assert_eq!(format_cuda_version(12060), "12.6");
        assert_eq!(format_cuda_version(11040), "11.4");
        assert_eq!(format_cuda_version(0), "unknown");
        assert_eq!(format_cuda_version(-1), "unknown");
    }
}
