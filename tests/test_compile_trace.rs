// CLASSIFICATION: COMMUNITY
// Filename: test_compile_trace.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-17

use cohesix::coh_cc::{backend::registry::get_backend, guard, parser::input_type::CohInput};
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn compile_reproducible() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    fs::create_dir_all("/log").unwrap();
    let src = dir.path().join("hello.c");
    File::create(&src)
        .unwrap()
        .write_all(b"int main(){return 0;}")
        .unwrap();
    let out = dir.path().join("hello");
    let backend = get_backend("tcc").unwrap();
    let input = CohInput::new(src.clone(), vec![]);
    backend.compile(&input, &out).unwrap();
    let h1 = guard::hash_output(&out).unwrap();
    backend.compile(&input, &out).unwrap();
    let h2 = guard::hash_output(&out).unwrap();
    assert_eq!(h1, h2);
    fs::write("/log/cohcc_ci_trace.log", b"ok").unwrap();
}

#[test]
#[should_panic]
fn reject_dynamic_flags() {
    let _ = guard::check_static_flags(&["-fPIC".to_string()]).unwrap();
}
