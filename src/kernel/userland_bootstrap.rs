// CLASSIFICATION: COMMUNITY
// Filename: userland_bootstrap.rs v0.7
// Author: Lukas Bower
// Date Modified: 2026-11-20

use crate::kernel::syscalls::syscall::init_syscall_trap;
/// Lightweight userland dispatcher.
use crate::kernel::{loader, proc_mgr};
use crate::prelude::*;
use crate::seL4::switch_to_user;

/// Load `/bin/init` via the ELF loader and prepare for user-mode transition.
pub fn dispatch_user(name: &str) {
    let path = format!("/bin/{}", name);
    match loader::load_user_elf(&path) {
        Ok(ctx) => {
            let name_static: &'static str = Box::leak(name.to_string().into_boxed_str());
            let pid = proc_mgr::spawn(name_static, ctx.entry_point);
            crate::debug!(
                "[userland_bootstrap] spawned {} as pid {}",
                name_static,
                pid
            );
            println!("Entry point: 0x{:x}", ctx.entry_point);
            println!("User stack allocated at 0x{:x}", ctx.stack_top);
            println!(
                "Switching to EL0 at entry 0x{:x}, stack 0x{:x}",
                ctx.entry_point, ctx.stack_top
            );
            unsafe {
                init_syscall_trap();
                switch_to_user(ctx.entry_point, ctx.stack_top);
            }
        }
        Err(e) => {
            eprintln!("[userland_bootstrap] failed to load {}: {}", path, e);
        }
    }
}
