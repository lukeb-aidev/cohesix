// CLASSIFICATION: COMMUNITY
// Filename: test_capabilities.rs v0.6
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
    std::fs::create_dir_all(cohesix::with_srv_root!("")).ok();
    std::fs::write(cohesix::with_srv_root!("cohrole"), "QueenPrimary").ok();
    std::env::set_var("COHROLE", "QueenPrimary");

    std::fs::create_dir_all(cohesix::with_srv_root!("queen")).ok();
    println!("[TEST] Secure9P capability granted for plan9_mount_read_write");
    let mount_path = cohesix::with_srv_root!("queen/test_mount");
    let mount = Path::new(&mount_path);
    std::fs::create_dir_all(mount)?;
    let file_path = mount.join("cap_test.txt");
    assert!(cohesix::security::capabilities::role_allows(
        "QueenPrimary",
        "open",
        mount.to_str().unwrap()
    ));
    std::fs::write(&file_path, b"ok")?;
    let data = std::fs::read(&file_path)?;
    assert_eq!(data, b"ok");
    println!("[TEST] Secure9P write permission confirmed for {}", mount.display());
    std::fs::remove_file(&file_path)?;

    match prev {
        Some(v) => {
            std::env::set_var("COHROLE", &v);
            std::fs::write(cohesix::with_srv_root!("cohrole"), v).ok();
        }
        None => {
            std::env::remove_var("COHROLE");
            let _ = std::fs::remove_file(cohesix::with_srv_root!("cohrole"));
        }
    }
    Ok(())
}
