// CLASSIFICATION: COMMUNITY
// Filename: gpuinfo.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-11-19

use super::Service;
/// GPU information service.
use crate::runtime::ServiceRegistry;
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use std::{env, fs};

#[derive(Default)]
pub struct GpuInfoService {
    initialized: bool,
}

impl Service for GpuInfoService {
    fn name(&self) -> &'static str {
        "GpuInfoService"
    }

    fn init(&mut self) {
        let info = env::var("CUDA_SERVER")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| fs::read_to_string("/srv/cuda").ok().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .map(|addr| format!("remote:{addr}"))
            .unwrap_or_else(|| "remote:unconfigured".into());
        fs::write("/srv/gpuinfo", info).ok();
        let _ = ServiceRegistry::register_service("gpuinfo", "/srv/gpuinfo");
        self.initialized = true;
        println!("[gpuinfo] initialized");
    }

    fn shutdown(&mut self) {
        if self.initialized {
            println!("[gpuinfo] shutting down");
            self.initialized = false;
        }
    }
}
