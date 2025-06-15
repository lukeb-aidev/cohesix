// CLASSIFICATION: COMMUNITY
// Filename: kernel_drivers_test.rs v0.1
// Date Modified: 2025-06-05
// Author: Cohesix Codex

use cohesix::kernel::{drivers::net::NetDriver, fs::initfs, fs::busybox, drivers::gpu::GpuDriver};

#[test]
fn initfs_lists_files() {
    let files: Vec<_> = initfs::list_files().collect();
    assert!(files.contains(&"init.rc"));
}

#[test]
fn net_driver_loopback_roundtrip() {
    let mut driver = NetDriver::initialize();
    let packet = b"hello";
    driver.transmit(packet);
    let recv = driver.receive().expect("packet");
    assert_eq!(recv, packet);
}

#[test]
fn gpu_driver_env_selection() {
    unsafe {
        std::env::set_var("COHESIX_GPU", "cuda");
    }
    let driver = GpuDriver::initialize();
    assert!(driver.is_available());
}
