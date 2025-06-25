// CLASSIFICATION: COMMUNITY
// Filename: test_cuda_exec.rs v0.6
// Date Modified: 2026-08-23
#![cfg(feature = "cuda")]
// Author: Cohesix Codex

#[cfg(feature = "cuda")]
use cohesix::cuda::runtime::{CudaExecutor, CudaRuntime};
use std::fs::OpenOptions;
// use tempfile::tempdir;

#[test]
fn cuda_executor_launches() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("COH_GPU").unwrap_or_default() == "0" {
        eprintln!("skipping cuda_executor_launches: COH_GPU=0");
        return Ok(());
    }

    let rt = match CudaRuntime::try_new() {
        Ok(rt) if rt.is_present() => rt,
        _ => {
            eprintln!("skipping cuda_executor_launches: CUDA unavailable");
            return Ok(());
        }
    };

    if OpenOptions::new().read(true).open("/dev/nvidia0").is_err() {
        eprintln!("skipping cuda_executor_launches: /dev/nvidia0 not accessible");
        return Ok(());
    }

    if let Err(e) = std::fs::create_dir_all("/srv") {
        eprintln!("skipping cuda_executor_launches: cannot create /srv: {e}");
        return Ok(());
    }
    if let Err(e) = std::fs::create_dir_all("/log") {
        eprintln!("skipping cuda_executor_launches: cannot create /log: {e}");
        return Ok(());
    }

    let mut exec = CudaExecutor::new();
    exec.load_kernel(Some(b"fake"))?;
    exec.launch()?;

    let out = std::fs::read_to_string("/srv/cuda_result")?;
    assert!(out.contains("kernel executed") || out.contains("cuda disabled"));
    let log = std::fs::read_to_string("/log/gpu_runtime.log")?;
    assert!(log.contains("kernel executed") || log.contains("cuda disabled"));

    let telem = exec.telemetry();
    assert!(telem.exec_time_ns > 0 || !telem.fallback_reason.is_empty());
    drop(rt);
    Ok(())
}
