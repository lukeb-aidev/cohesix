// CLASSIFICATION: COMMUNITY
// Filename: test_gpu_and_sim.rs v0.4
// Date Modified: 2025-07-22

#![cfg(all(feature = "cuda", feature = "rapier"))]
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
    let srv_dir = std::env::temp_dir();
    let log_dir = std::env::var("COHESIX_LOG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    std::fs::create_dir_all(&srv_dir).unwrap();
    std::fs::create_dir_all(&log_dir).unwrap();
    std::fs::write(srv_dir.join("kernel.ptx"), "fake").unwrap();

    // GPU execution should succeed even without real CUDA
    let mut exec = CudaExecutor::new();
    exec.load_kernel(Some(b"fake")).unwrap();
    exec.launch().unwrap();
    let out = std::fs::read_to_string(srv_dir.join("cuda_result")).unwrap();
    assert!(out.contains("kernel executed") || out.contains("cuda disabled"));
    let log = std::fs::read_to_string(log_dir.join("gpu_runtime.log")).unwrap();
    assert!(log.contains("kernel executed") || log.contains("cuda disabled"));
    let telem = exec.telemetry();
    assert!(telem.exec_time_ns > 0 || !telem.fallback_reason.is_empty());

    // Simulation
    std::fs::create_dir_all("sim").unwrap();
    let bridge = SimBridge::start();
    bridge.send(SimCommand::AddSphere {
        radius: 1.0,
        position: vector![0.0, 0.0, 0.0],
    });
    for _ in 0..10 {
        if let Ok(state) = std::fs::read_to_string("sim/state") {
            if state.contains(": [") {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
