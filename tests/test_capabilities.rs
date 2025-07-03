// CLASSIFICATION: COMMUNITY
// Filename: test_capabilities.rs v0.4
// Date Modified: 2026-12-31
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
    let prev = std::env::var("COHROLE").ok();
    std::fs::create_dir_all("/srv").ok();
    std::fs::write("/srv/cohrole", "QueenPrimary").ok();
    std::env::set_var("COHROLE", "QueenPrimary");

    let mount = Path::new("/srv/test_mount");
    std::fs::create_dir_all(mount)?;
    let file_path = mount.join("cap_test.txt");
    std::fs::write(&file_path, b"ok")?;
    let data = std::fs::read(&file_path)?;
    assert_eq!(data, b"ok");
    std::fs::remove_file(&file_path)?;

    match prev {
        Some(v) => {
            std::env::set_var("COHROLE", &v);
            std::fs::write("/srv/cohrole", v).ok();
        }
        None => {
            std::env::remove_var("COHROLE");
            let _ = std::fs::remove_file("/srv/cohrole");
        }
    }
    Ok(())
}
