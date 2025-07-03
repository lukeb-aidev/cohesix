// CLASSIFICATION: COMMUNITY
// Filename: test_capabilities.rs v0.3
// Date Modified: 2026-09-30
// Author: Cohesix Codex

#[allow(unused_imports)]
use cohesix::seL4::syscall::exec;
#[allow(unused_imports)]
use env_logger;
#[allow(unused_imports)]
use std::fs::File;
#[allow(unused_imports)]
use std::path::Path;

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
