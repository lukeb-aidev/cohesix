// CLASSIFICATION: COMMUNITY
// Filename: userland_bootstrap.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-27

//! Lightweight userland dispatcher for in-kernel demo programs.

use crate::kernel::proc_mgr;

/// Signature for a user entry function.
type UserFn = fn();

struct Entry {
    name: &'static str,
    func: UserFn,
}

fn init_program() {
    println!("Hello from init");
}

fn panic_program() {
    panic!("Intentional panic from panic_test");
}

static ENTRIES: &[Entry] = &[
    Entry { name: "init", func: init_program },
    Entry { name: "panic_test", func: panic_program },
];

/// Spawn a user program by name.
pub fn dispatch_user(name: &str) {
    if let Some(entry) = ENTRIES.iter().find(|e| e.name == name) {
        let pid = proc_mgr::spawn(entry.name, entry.func as usize);
        println!("[userland_bootstrap] spawned {} as pid {}", entry.name, pid);
        (entry.func)();
    } else {
        eprintln!("[userland_bootstrap] unknown user entry: {}", name);
    }
}
