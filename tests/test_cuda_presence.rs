// CLASSIFICATION: COMMUNITY
// Filename: test_cuda_presence.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-07-23
#![cfg(feature = "cuda")]

#[cfg(feature = "cuda")]
use cohesix::cuda::runtime::{CudaExecutor, CudaRuntime};
use cohesix::validator::{self, RuleViolation};
use tempfile::tempdir;

#[test]
fn cuda_presence_and_telemetry() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("COH_GPU").unwrap_or_default() == "0" {
        eprintln!("skipping cuda_presence_and_telemetry: COH_GPU=0");
        return Ok(());
    }
    let dir = tempdir()?;
    std::env::set_current_dir(&dir)?;
    if let Err(e) = std::fs::create_dir_all("/log") {
        eprintln!("skipping cuda_presence_and_telemetry: cannot create /log: {e}");
        return Ok(());
    }
    unsafe {
        std::env::set_var("COHESIX_LOG_DIR", "/log");
    }

    let rt = match CudaRuntime::try_new() {
        Ok(rt) => rt,
        Err(_) => {
            eprintln!("skipping cuda_presence_and_telemetry: CUDA unavailable");
            return Ok(());
        }
    };
    validator::log_violation(RuleViolation {
        type_: if rt.is_present() {
            "cuda_available"
        } else {
            "cuda_unavailable"
        },
        file: "test_cuda_presence.rs".into(),
        agent: "cuda_presence".into(),
        time: validator::timestamp(),
    });

    let log = std::fs::read_to_string("/log/validator_runtime.log")?;
    assert!(log.contains("cuda_available") || log.contains("cuda_unavailable"));

    let exec = CudaExecutor::new();
    let telem = exec.telemetry();
    assert_eq!(telem.cuda_present, rt.is_present());
    Ok(())
}
