// CLASSIFICATION: COMMUNITY
// Filename: userland_bootstrap.rs v0.6
// Author: Lukas Bower
// Date Modified: 2026-11-20

//! Lightweight userland dispatcher.

use crate::kernel::{loader, proc_mgr};

/// Load `/bin/init` via the ELF loader and prepare for user-mode transition.
pub fn dispatch_user(name: &str) {
    let path = format!("/bin/{}", name);
    match loader::load_user_elf(&path) {
        Ok(ctx) => {
            let name_static: &'static str = Box::leak(name.to_string().into_boxed_str());
            let pid = proc_mgr::spawn(name_static, ctx.entry_point);
            crate::debug!("[userland_bootstrap] spawned {} as pid {}", name_static, pid);
            println!("Entry point: 0x{:x}", ctx.entry_point);
            println!("User stack allocated at 0x{:x}", ctx.stack_top);
        }
        Err(e) => {
            eprintln!("[userland_bootstrap] failed to load {}: {}", path, e);
        }
    }
}
