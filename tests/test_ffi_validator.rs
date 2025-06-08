// CLASSIFICATION: COMMUNITY
// Filename: test_ffi_validator.rs v0.1
// Date Modified: 2025-07-13
// Author: Cohesix Codex

use cohesix::cuda::runtime::CudaRuntime;

#[test]
fn unknown_symbol_rejected() {
    let rt = CudaRuntime::try_new().unwrap();
    let res = rt.get_symbol::<unsafe extern "C" fn()>(b"cuUnknown");
    assert!(res.is_err());
}
