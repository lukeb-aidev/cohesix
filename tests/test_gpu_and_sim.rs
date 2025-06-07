// CLASSIFICATION: COMMUNITY
// Filename: test_gpu_and_sim.rs v0.1
// Date Modified: 2025-06-18
// Author: Cohesix Codex

use cohesix::cuda::runtime::CudaExecutor;
use cohesix::sim::rapier_bridge::{SimBridge, SimCommand};
use rapier3d::prelude::*;
use tempfile::tempdir;

#[test]
fn gpu_and_sim_integration() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    // Prepare fake kernel file
    std::fs::create_dir_all("srv").unwrap();
    std::fs::write("srv/kernel.ptx", "fake").unwrap();

    // GPU execution should succeed even without real CUDA
    let mut exec = CudaExecutor::new();
    exec.load_kernel(b"fake").unwrap();
    exec.launch().unwrap();
    let out = std::fs::read_to_string("srv/cuda_output").unwrap();
    assert!(out.contains("kernel executed") || out.contains("cuda disabled"));

    // Simulation
    std::fs::create_dir_all("sim").unwrap();
    let bridge = SimBridge::start();
    bridge.send(SimCommand::AddSphere {
        radius: 1.0,
        position: vector![0.0, 0.0, 0.0],
    });
    for _ in 0..10 {
        if let Ok(state) = std::fs::read_to_string("sim/state") {
            assert!(state.contains(": ["));
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("state not generated");
}
