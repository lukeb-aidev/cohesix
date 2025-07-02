// CLASSIFICATION: COMMUNITY
// Filename: test_compile_trace.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-12-09

use cohesix::coh_cc::{
    backend::registry::get_backend, guard, parser::input_type::CohInput, toolchain::Toolchain,
};
use cohesix::CohError;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tempfile::Builder;

#[test]
fn compile_reproducible() -> Result<(), CohError> {
    let work = tempfile::tempdir()?;
    let dir = Builder::new().prefix("cohcc").tempdir_in(work.path())?;
    std::env::set_current_dir(&dir)?;
    let log_dir = std::env::var("COHESIX_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    fs::create_dir_all(&log_dir)?;
    let src = dir.path().join("hello.c");
    File::create(&src)?.write_all(b"int main(){return 0;}")?;
    let out = dir.path().join("hello");
    let backend = get_backend("tcc")?;
    let input = CohInput::new(src.clone(), vec![]);
    std::env::set_var("COHESIX_TOOLCHAIN_ROOT", dir.path());
    let tc_dir = dir.path().join("toolchain");
    fs::create_dir_all(&tc_dir)?;
    fs::set_permissions(&tc_dir, fs::Permissions::from_mode(0o755))?;
    let sysroot = dir.path().join("sysroot");
    fs::create_dir_all(&sysroot)?;
    fs::set_permissions(&sysroot, fs::Permissions::from_mode(0o755))?;
    let tc = Toolchain::new(&tc_dir)?;
    backend.compile(&input, &out, "x86_64-linux-musl", &sysroot, &tc)?;
    let h1 = guard::hash_output(&out)?;
    backend.compile(&input, &out, "x86_64-linux-musl", &sysroot, &tc)?;
    let h2 = guard::hash_output(&out)?;
    assert_eq!(h1, h2);
    fs::write(log_dir.join("cohcc_ci_trace.log"), h2.as_bytes())?;
    std::env::remove_var("COHESIX_TOOLCHAIN_ROOT");
    Ok(())
}

#[test]
#[should_panic]
fn reject_dynamic_flags() {
    guard::check_static_flags(&["-fPIC".to_string()]).unwrap();
}
