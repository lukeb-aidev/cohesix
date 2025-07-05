// CLASSIFICATION: COMMUNITY
// Filename: physics_cuda.rs v1.0
// Author: Codex
// Date Modified: 2025-07-22

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
#[cfg(feature = "rapier")]

use crate::cuda::runtime::CudaExecutor;
use crate::sim::rapier_bridge::{SimBridge, SimCommand};
use rapier3d::prelude::*;
use tempfile::tempdir;

#[test]
fn physics_and_cuda_harness() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::create_dir_all("/srv/test/physics").unwrap();

    let mut exec = CudaExecutor::new();
    exec.load_kernel(Some(b"fake")).unwrap();
    exec.launch().unwrap();

    let bridge = SimBridge::start();
    bridge.send(SimCommand::AddSphere { radius: 1.0, position: vector![0.0, 0.0, 0.0] });
    std::thread::sleep(std::time::Duration::from_millis(50));

    std::fs::write("/srv/test/physics/result.log", "ok").unwrap();
}
