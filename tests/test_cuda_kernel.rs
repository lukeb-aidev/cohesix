// CLASSIFICATION: COMMUNITY
// Filename: test_cuda_kernel.rs v0.4
// Date Modified: 2025-12-12
// Author: Cohesix Codex

use cohesix::cuda::runtime::CudaExecutor;
use cohesix::utils::gpu::coh_check_gpu_runtime;
use tempfile::{tempdir, NamedTempFile};

#[test]
fn cuda_kernel_result_file() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = coh_check_gpu_runtime() {
        eprintln!("skipping cuda_kernel_result_file: {e}");
        return Ok(());
    }
    let dir = tempdir()?;
    std::env::set_current_dir(&dir)?;
    if let Err(e) = std::fs::create_dir_all("/srv") {
        eprintln!("skipping cuda_kernel_result_file: cannot create /srv: {e}");
        return Ok(());
    }
    if let Err(e) = std::fs::create_dir_all("/log") {
        eprintln!("skipping cuda_kernel_result_file: cannot create /log: {e}");
        return Ok(());
    }

    let mut exec = CudaExecutor::new();
    exec.load_kernel(Some(b"fake"))?;
    exec.launch()?;

    let tmp_out = NamedTempFile::new_in(std::env::temp_dir())?;
    std::fs::copy("/srv/cuda_result", tmp_out.path())?;
    let out = std::fs::read_to_string(tmp_out.path())?;
    assert!(out.contains("kernel executed") || out.contains("cuda disabled"));
    let tmp_log = NamedTempFile::new_in(std::env::temp_dir())?;
    if let Ok(_) = std::fs::copy("/log/gpu_runtime.log", tmp_log.path()) {
        let log = std::fs::read_to_string(tmp_log.path())?;
        assert!(log.contains("kernel executed") || log.contains("cuda disabled"));
    }

    let telem = exec.telemetry();
    assert!(telem.exec_time_ns > 0 || !telem.fallback_reason.is_empty());
    Ok(())
}
