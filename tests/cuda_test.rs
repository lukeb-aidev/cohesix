// CLASSIFICATION: COMMUNITY
// Filename: cuda_test.rs v0.1
// Date Modified: 2025-07-22
// Author: Cohesix Codex

use cohesix::cuda::runtime::CudaRuntime;
use cohesix::validator::{self, RuleViolation};
use tempfile::tempdir;

#[test]
fn cuda_runtime_reports_availability() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::create_dir_all("/log").unwrap();

    let rt = CudaRuntime::try_new().unwrap();
    validator::log_violation(RuleViolation {
        type_: if rt.is_present() { "cuda_available" } else { "cuda_unavailable" },
        file: "cuda_test.rs".into(),
        agent: "cuda_test".into(),
        time: validator::timestamp(),
    });

    let log = std::fs::read_to_string("/log/validator_runtime.log").unwrap();
    assert!(log.contains("cuda_available") || log.contains("cuda_unavailable"));
}
