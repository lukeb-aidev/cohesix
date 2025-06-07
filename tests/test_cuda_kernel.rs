// CLASSIFICATION: COMMUNITY
// Filename: test_cuda_kernel.rs v0.1
// Date Modified: 2025-06-25
// Author: Cohesix Codex

use cohesix::cuda::runtime::CudaExecutor;
use tempfile::tempdir;

#[test]
fn cuda_kernel_result_file() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::create_dir_all("srv").unwrap();

    let mut exec = CudaExecutor::new();
    exec.load_kernel(Some(b"fake")).unwrap();
    exec.launch().unwrap();

    let out = std::fs::read_to_string("/srv/cuda_result").unwrap();
    assert!(out.contains("ok") || out.contains("cuda disabled"));
}
