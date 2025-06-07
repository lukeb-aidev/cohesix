// CLASSIFICATION: COMMUNITY
// Filename: test_syscall_queue.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-06-17

use cohesix::cohesix_types::Syscall;
use cohesix::sandbox::{dispatcher::SyscallDispatcher, queue::SyscallQueue};
use serial_test::serial;

#[test]
#[serial]
fn queue_dequeue_dispatch_order() {
    std::env::set_var("COHROLE", "DroneWorker");
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
    std::env::set_var("COHROLE", "QueenPrimary");
    std::fs::write("/srv/cohrole", "QueenPrimary").unwrap();
    let mut q = SyscallQueue::new();
    q.enqueue(Syscall::Exec {
        path: "/bin/ls".into(),
    });
    assert!(q.dequeue().is_none(), "queen should not dequeue syscalls");
}
