// CLASSIFICATION: COMMUNITY
// Filename: userland_bootstrap.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-27

//! Lightweight userland dispatcher.

use crate::kernel::proc_mgr;

#[cfg(not(feature = "minimal_uefi"))]
mod static_table {
    use super::proc_mgr;

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

    pub fn dispatch(name: &str) {
        if let Some(entry) = ENTRIES.iter().find(|e| e.name == name) {
            let pid = proc_mgr::spawn(entry.name, entry.func as usize);
            println!("[userland_bootstrap] spawned {} as pid {}", entry.name, pid);
            (entry.func)();
        } else {
            eprintln!("[userland_bootstrap] unknown user entry: {}", name);
        }
    }
}

#[cfg(feature = "minimal_uefi")]
use crate::fs;

/// Spawn a user program by name, loading the binary from FAT when `minimal_uefi` is enabled.
pub fn dispatch_user(name: &str) {
    #[cfg(feature = "minimal_uefi")]
    {
        let path = format!("/bin/{}.efi", name);
        match fs::open_bin(&path) {
            Some(bin) => {
                let entry = bin.as_ptr() as usize;
                let pid = proc_mgr::spawn(name, entry);
                println!("[userland_bootstrap] spawned {} as pid {}", name, pid);
            }
            None => {
                eprintln!("[userland_bootstrap] missing or invalid {}", path);
            }
        }
    }
    #[cfg(not(feature = "minimal_uefi"))]
    {
        static_table::dispatch(name);
    }
}
