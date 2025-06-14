// CLASSIFICATION: COMMUNITY
// Filename: test_compile_trace.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-18

use cohesix::coh_cc::{backend::registry::get_backend, guard, parser::input_type::CohInput, toolchain::Toolchain};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tempfile::Builder;

#[test]
fn compile_reproducible() {
    fs::create_dir_all("/mnt/data").unwrap();
    let dir = Builder::new().prefix("cohcc").tempdir_in("/mnt/data").unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let log_dir = std::env::var("COHESIX_LOG_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    fs::create_dir_all(&log_dir).unwrap();
    let src = dir.path().join("hello.c");
    File::create(&src)
        .unwrap()
        .write_all(b"int main(){return 0;}")
        .unwrap();
    let out = dir.path().join("hello");
    let backend = get_backend("tcc").unwrap();
    let input = CohInput::new(src.clone(), vec![]);
    let tc = Toolchain::new("/mnt/data/toolchain").unwrap();
    fs::create_dir_all("/mnt/data/toolchain").unwrap();
    fs::create_dir_all("/mnt/data/sysroot").unwrap();
    backend
        .compile(&input, &out, "x86_64-linux-musl", Path::new("/mnt/data/sysroot"), &tc)
        .unwrap();
    let h1 = guard::hash_output(&out).unwrap();
    backend
        .compile(&input, &out, "x86_64-linux-musl", Path::new("/mnt/data/sysroot"), &tc)
        .unwrap();
    let h2 = guard::hash_output(&out).unwrap();
    assert_eq!(h1, h2);
    fs::write(log_dir.join("cohcc_ci_trace.log"), h2.as_bytes()).unwrap();
}

#[test]
#[should_panic]
fn reject_dynamic_flags() {
    let _ = guard::check_static_flags(&["-fPIC".to_string()]).unwrap();
}
