// CLASSIFICATION: COMMUNITY
// Filename: test_capabilities.rs v0.3
// Date Modified: 2026-09-30
// Author: Cohesix Codex

use cohesix::seL4::syscall::exec;
use std::fs::File;
use std::path::Path;
use env_logger;

#[test]
fn plan9_mount_read_write() -> std::io::Result<()> {
    std::fs::create_dir_all("/srv").ok();
    let path = Path::new("/srv/cap_test.txt");
    std::fs::write(path, b"ok")?;
    let data = std::fs::read(path)?;
    assert_eq!(data, b"ok");
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn exec_denied_for_worker() {
    let _ = env_logger::builder().is_test(true).try_init();
    std::env::set_var("COHROLE", "DroneWorker");
    let res = exec("/bin/echo", &["hi"]);
    assert!(res.is_err(), "worker exec unexpectedly succeeded");
}
