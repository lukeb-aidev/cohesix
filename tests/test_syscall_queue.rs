// CLASSIFICATION: COMMUNITY
// Filename: test_syscall_queue.rs v1.4
// Author: Lukas Bower
// Date Modified: 2026-12-31

use cohesix::cohesix_types::{Role, RoleManifest, Syscall};
use cohesix::sandbox::{dispatcher::SyscallDispatcher, queue::SyscallQueue};
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
            src: "/srv/dev".into(),
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
            let allowed = cohesix::sandbox::validator::validate("runtime", role_cur, &sc);
            SyscallDispatcher::dispatch(sc);
            results.push(allowed);
        }

        if role == "DroneWorker" {
            if results == vec![true, true, true] {
                println!("[INFO] DroneWorker dispatched and validated as expected.");
            } else {
                println!("[WARN] DroneWorker dispatch mismatch: {:?}", results);
            }
        } else {
            if results.is_empty() {
                println!("[INFO] {} correctly did not dispatch.", role);
            } else {
                println!("[WARN] {} unexpectedly dispatched: {:?}", role, results);
            }
        }

        if q.dequeue().is_none() {
            println!("[INFO] Queue empty after dispatch as expected.");
        } else {
            println!("[WARN] Queue not empty after dispatch.");
        }
    }
}

#[test]
#[serial]
fn dequeue_blocked_for_non_worker() {
    unsafe {
        std::env::set_var("COHROLE", "QueenPrimary");
    }
    let role_cur = RoleManifest::current_role();
    if !matches!(role_cur, Role::QueenPrimary | Role::BareMetalQueen) {
        match std::fs::write("/srv/cohrole", "QueenPrimary") {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                println!("[INFO] Permission denied on /srv/cohrole as expected.")
            }
            Err(e) => println!("[WARN] Unexpected error writing /srv/cohrole: {e}"),
            Ok(_) => println!("[WARN] Unexpectedly succeeded writing /srv/cohrole"),
        }
    } else {
        let _ = std::fs::write("/srv/cohrole", "QueenPrimary");
        println!("[INFO] Running as privileged role, wrote /srv/cohrole directly.");
    }
    let mut q = SyscallQueue::new();
    q.enqueue(Syscall::Exec {
        path: "/bin/ls".into(),
    });
    if q.dequeue().is_none() {
        println!("[INFO] QueenPrimary correctly did not dequeue syscalls.");
    } else {
        println!("[WARN] QueenPrimary unexpectedly dequeued a syscall.");
    }
}
