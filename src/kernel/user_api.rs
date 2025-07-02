// CLASSIFICATION: COMMUNITY
// Filename: user_api.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-27
// SPDX-License-Identifier: MIT

/// Kernel-exported userland API shims.
/// Provides simple logging and exit hooks for minimal user programs.
use crate::kernel::proc_mgr;

pub type SysLogFn = extern "C" fn(*const u8, usize);
pub type SysExitFn = extern "C" fn(u32);

#[no_mangle]
pub static mut SYS_LOG: Option<SysLogFn> = None;
#[no_mangle]
pub static mut SYS_EXIT: Option<SysExitFn> = None;

pub fn init_user_api() {
    unsafe {
        SYS_LOG = Some(sys_log_impl);
        SYS_EXIT = Some(sys_exit_impl);
    }
}

extern "C" fn sys_log_impl(ptr: *const u8, len: usize) {
    if let Ok(msg) = unsafe { std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) } {
        println!("{msg}");
    }
}

extern "C" fn sys_exit_impl(code: u32) {
    let pid = proc_mgr::current_pid();
    proc_mgr::mark_exited(pid, code);
    println!("EXIT pid={} code={}", pid, code);
}
