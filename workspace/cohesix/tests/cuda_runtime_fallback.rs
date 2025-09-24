// CLASSIFICATION: COMMUNITY
// Filename: cuda_runtime_fallback.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-21

use cohesix::cuda::runtime::CudaExecutor;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn launch_without_endpoint_reports_error() {
    let _guard = env_lock().lock().unwrap();
    std::env::remove_var("CUDA_SERVER");
    let mut exec = CudaExecutor::new();
    let err = exec.launch().expect_err("expected missing endpoint error");
    assert!(err.contains("no CUDA endpoint"));

    let telemetry = exec.telemetry().expect("telemetry should be available");
    assert_eq!(telemetry.fallback_reason, "missing_remote_endpoint");
    assert!(!telemetry.cuda_present);
}

#[test]
fn launch_with_unsupported_scheme_reports_error() {
    let _guard = env_lock().lock().unwrap();
    std::env::set_var("CUDA_SERVER", "unix:/tmp/fake.socket");
    let mut exec = CudaExecutor::new();
    let err = exec
        .launch()
        .expect_err("expected unsupported endpoint scheme error");
    assert!(err.contains("tcp:<host>:<port>"));

    let telemetry = exec.telemetry().expect("telemetry should be available");
    assert_eq!(telemetry.fallback_reason, "unsupported_endpoint_scheme");
    assert!(!telemetry.cuda_present);
    std::env::remove_var("CUDA_SERVER");
}
