// CLASSIFICATION: COMMUNITY
// Filename: test_syscall_queue.rs v1.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

use cohesix::cohesix_types::Syscall;
use cohesix::sandbox::{dispatcher::SyscallDispatcher, queue::SyscallQueue};
use libc::geteuid;
use serial_test::serial;

#[test]
#[serial]
fn queue_dequeue_dispatch_order() {
    unsafe {
        std::env::set_var("COHROLE", "DroneWorker");
    }
    let mut q = SyscallQueue::new();
    q.enqueue(Syscall::Spawn {
        program: "a".into(),
        args: vec!["1".into()],
    });
    q.enqueue(Syscall::Mount {
        src: "/dev".into(),
        dest: "/mnt".into(),
    });
    q.enqueue(Syscall::Exec {
        path: "/bin/run".into(),
    });

    SyscallDispatcher::dispatch_queue(&mut q);
    assert!(
        q.dequeue().is_none(),
        "queue should be empty after dispatch"
    );
}

#[test]
#[serial]
fn dequeue_blocked_for_non_worker() {
    unsafe {
        std::env::set_var("COHROLE", "QueenPrimary");
    }
    // Attempting to write directly to `/srv/cohrole` should fail for
    // non-Worker roles when not running as root.
    if unsafe { geteuid() } != 0 {
        match std::fs::write("/srv/cohrole", "QueenPrimary") {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => (),
            Err(e) => panic!("Unexpected error: {e}"),
            Ok(_) => panic!("expected permission denial when writing /srv/cohrole"),
        }
    } else {
        let _ = std::fs::write("/srv/cohrole", "QueenPrimary");
    }
    let mut q = SyscallQueue::new();
    q.enqueue(Syscall::Exec {
        path: "/bin/ls".into(),
    });
    assert!(
        q.dequeue().is_none(),
        "queen should not dequeue syscalls"
    );
    // This is the expected outcome for non-Worker roles
}
