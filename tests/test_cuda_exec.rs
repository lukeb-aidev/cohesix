// CLASSIFICATION: COMMUNITY
// Filename: test_cuda_exec.rs v0.2
// Date Modified: 2025-07-03
// Author: Cohesix Codex

use cohesix::cuda::runtime::CudaExecutor;
use tempfile::tempdir;

#[test]
fn cuda_executor_launches() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::create_dir_all("/srv").unwrap();

    let mut exec = CudaExecutor::new();
    exec.load_kernel(Some(b"fake")).unwrap();
    exec.launch().unwrap();

    let out = std::fs::read_to_string("/srv/cuda_result").unwrap();
    assert!(out.contains("kernel executed") || out.contains("cuda disabled"));
}
