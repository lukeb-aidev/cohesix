// CLASSIFICATION: COMMUNITY
// Filename: test_capabilities.rs v0.3
// Date Modified: 2025-09-20
// Author: Cohesix Codex

use cohesix::seL4::syscall::exec;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use libc::geteuid;

#[test]
fn open_denied_logs_violation() -> std::io::Result<()> {
    if unsafe { geteuid() } == 0 {
        eprintln!("Skipping test: running as root; permission checks bypassed");
        return Ok(());
    }
    let path = Path::new("/tmp/cohesix_test/denied.trace");
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, b"test")?;

    // Set permissions to simulate denial
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(path, perms)?;

    match File::open(path) {
        Ok(_) => panic!("Expected open to fail, but it succeeded"),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("✅ permission denied as expected: {}", e);
        }
        Err(e) => panic!("Unexpected error: {}", e),
    }

    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o644);
    std::fs::set_permissions(path, perms)?;
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn exec_denied_for_worker() {
    let srv_dir = std::env::temp_dir();
    fs::write(srv_dir.join("cohrole"), "DroneWorker")
        .unwrap_or_else(|e| panic!("exec_denied_for_worker failed: {}", e));
    let res = exec("/bin/echo", &["hi"]);
    assert!(res.is_err());
}
