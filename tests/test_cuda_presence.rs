// CLASSIFICATION: COMMUNITY
// Filename: test_cuda_presence.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-23

use cohesix::cuda::runtime::{CudaRuntime, CudaExecutor};
use cohesix::validator::{self, RuleViolation};
use tempfile::tempdir;

#[test]
fn cuda_presence_and_telemetry() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::create_dir_all("/log").unwrap();
    std::env::set_var("COHESIX_LOG_DIR", "/log");

    let rt = match CudaRuntime::try_new() {
        Ok(rt) => rt,
        Err(_) => {
            eprintln!("skipping cuda_presence_and_telemetry: CUDA unavailable");
            return;
        }
    };
    validator::log_violation(RuleViolation {
        type_: if rt.is_present() { "cuda_available" } else { "cuda_unavailable" },
        file: "test_cuda_presence.rs".into(),
        agent: "cuda_presence".into(),
        time: validator::timestamp(),
    });

    let log = std::fs::read_to_string("/log/validator_runtime.log").unwrap();
    assert!(log.contains("cuda_available") || log.contains("cuda_unavailable"));

    let exec = CudaExecutor::new();
    let telem = exec.telemetry();
    assert_eq!(telem.cuda_present, rt.is_present());
}
