// CLASSIFICATION: COMMUNITY
// Filename: kernel_proc_mgr.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-26

use cohesix::kernel::proc_mgr;

#[test]
fn spawn_and_list_processes() {
    let pid1 = proc_mgr::spawn("init", 0x1000);
    let pid2 = proc_mgr::spawn("shell", 0x2000);
    let list = proc_mgr::list();
    assert!(list.contains(&format!("{}:init", pid1)));
    assert!(list.contains(&format!("{}:shell", pid2)));
    proc_mgr::kill(pid1);
    proc_mgr::kill(pid2);
}
