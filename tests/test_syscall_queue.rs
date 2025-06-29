// CLASSIFICATION: COMMUNITY
// Filename: test_syscall_queue.rs v1.3
// Author: Lukas Bower
// Date Modified: 2026-11-12

use cohesix::cohesix_types::Syscall;
use cohesix::sandbox::{dispatcher::SyscallDispatcher, queue::SyscallQueue};
use libc::geteuid;
use serial_test::serial;

#[test]
#[serial]
fn queue_dequeue_dispatch_order() {
    let calls = [
        Syscall::Spawn {
            program: "a".into(),
            args: vec!["1".into()],
        },
        Syscall::Mount {
            src: "/dev".into(),
            dest: "/mnt".into(),
        },
        Syscall::Exec {
            path: "/bin/run".into(),
        },
    ];

    for role in ["DroneWorker", "QueenPrimary"] {
        unsafe {
            std::env::set_var("COHROLE", role);
        }
        let _ = std::fs::remove_file("/srv/cohrole");
        let mut q = SyscallQueue::new();
        for sc in &calls {
            q.enqueue(sc.clone());
        }

        let mut results = Vec::new();
        while let Some(sc) = q.dequeue() {
            let role_cur = cohesix::cohesix_types::RoleManifest::current_role();
            let allowed =
                cohesix::sandbox::validator::validate("runtime", role_cur, &sc);
            SyscallDispatcher::dispatch(sc);
            results.push(allowed);
        }

        if role == "DroneWorker" {
            assert_eq!(results, vec![true, true, true]);
        } else {
            assert!(results.is_empty(), "{} should not dispatch", role);
        }

        assert!(q.dequeue().is_none(), "queue should be empty after dispatch");
    }
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
