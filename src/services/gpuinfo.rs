// CLASSIFICATION: COMMUNITY
// Filename: gpuinfo.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

use crate::prelude::*;
/// GPU information service.


use crate::runtime::ServiceRegistry;
use super::Service;
use std::process::Command;

#[derive(Default)]
pub struct GpuInfoService {
    initialized: bool,
}

impl Service for GpuInfoService {
    fn name(&self) -> &'static str { "GpuInfoService" }

    fn init(&mut self) {
        let info = Command::new("nvidia-smi")
            .arg("--query-gpu=name")
            .arg("--format=csv,noheader")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_else(|| "None".into());
        std::fs::write("/srv/gpuinfo", info.trim()).ok();
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
