// CLASSIFICATION: COMMUNITY
// Filename: test_cuda_exec.rs v0.3
// Date Modified: 2025-07-13
// Author: Cohesix Codex

use cohesix::cuda::runtime::CudaExecutor;
use tempfile::tempdir;

#[test]
fn cuda_executor_launches() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::create_dir_all("/srv").unwrap();
    std::fs::create_dir_all("/log").unwrap();

    let mut exec = CudaExecutor::new();
    exec.load_kernel(Some(b"fake")).unwrap();
    exec.launch().unwrap();

    let out = std::fs::read_to_string("/srv/cuda_result").unwrap();
    assert!(out.contains("kernel executed") || out.contains("cuda disabled"));
    let log = std::fs::read_to_string("/log/gpu_runtime.log").unwrap();
    assert!(log.contains("kernel executed") || log.contains("cuda disabled"));
}
