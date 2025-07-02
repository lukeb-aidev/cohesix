// CLASSIFICATION: COMMUNITY
// Filename: syscall.rs v1.5
// Author: Lukas Bower
// Date Modified: 2026-11-23

/// Kernel syscall interface layer for Cohesix.
/// Provides syscall entry point, argument validation, and dispatch wiring.
use super::syscall_table::dispatch;
use crate::kernel::security::l4_verified::{enforce_capability, CapabilityResult};
use std::fs::OpenOptions;
use std::io::Write;

#[cfg(all(target_os = "none", target_arch = "aarch64"))]
core::arch::global_asm!(
    r#"
    .global syscall_vector
syscall_vector:
    stp x0, x1, [sp, #-16]!
    stp x2, x3, [sp, #-16]!
    mov x8, x0
    bl syscall_trap
    ldp x2, x3, [sp], #16
    ldp x0, x1, [sp], #16
    eret
"#
);

#[cfg(all(target_os = "none", target_arch = "x86_64"))]
core::arch::global_asm!(
    r#"
    .global syscall_vector
syscall_vector:
    push %r11
    push %rcx
    push %rdi
    push %rsi
    push %rdx
    push %r10
    push %r8
    mov %rax, %rdi
    mov 32(%rsp), %rsi
    mov 24(%rsp), %rdx
    mov 16(%rsp), %rcx
    mov 8(%rsp), %r8
    call syscall_trap
    pop %r8
    pop %r10
    pop %rdx
    pop %rsi
    pop %rdi
    pop %rcx
    pop %r11
    sysretq
"#
);

/// Entry point invoked by the trap handler or syscall instruction.
pub fn handle_syscall(syscall_id: u32, args: &[u64]) -> i64 {
    println!(
        "[syscall] Handling syscall_id={} with args={:?}",
        syscall_id, args
    );
    crate::kernel::kernel_trace::log_syscall(&format!("{}", syscall_id));
    std::fs::create_dir_all("/log").ok();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/log/syscall.log")
    {
        let _ = writeln!(f, "id={syscall_id} args={:?}", args);
    }
    if enforce_capability(syscall_id, "syscall") != CapabilityResult::Allowed {
        return -1;
    }
    if args.len() > 8 {
        return -1;
    }
    dispatch(syscall_id, args)
}

/// Hardware trap entry invoked via `svc` or `syscall` from user mode.
#[no_mangle]
pub extern "C" fn syscall_trap(
    syscall_id: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
) -> i64 {
    println!("Trap: syscall num={} returned to kernel", syscall_id);
    let args = [a0 as u64, a1 as u64, a2 as u64, a3 as u64];
    handle_syscall(syscall_id as u32, &args)
}

/// Configure MSR/VBAR so user-mode traps vector to `syscall_trap`.
#[cfg(target_os = "none")]
pub unsafe fn init_syscall_trap() {
    #[cfg(target_arch = "aarch64")]
    {
        extern "C" {
            static syscall_vector: u8;
        }
        let addr = &syscall_vector as *const u8 as usize;
        core::arch::asm!("msr VBAR_EL1, {0}", in(reg) addr, options(nostack));
    }
    #[cfg(target_arch = "x86_64")]
    {
        extern "C" {
            static syscall_vector: u8;
        }
        let addr = &syscall_vector as *const u8 as u64;
        core::arch::asm!(
            "wrmsr",
            in("ecx") 0xC0000082u32,
            in("edx") (addr >> 32) as u32,
            in("eax") addr as u32,
            options(nostack)
        );
        let star: u64 = (0x08u64 << 32) | (0x1bu64 << 48);
        core::arch::asm!(
            "wrmsr",
            in("ecx") 0xC0000081u32,
            in("edx") (star >> 32) as u32,
            in("eax") star as u32,
            options(nostack)
        );
        core::arch::asm!(
            "mov $0xC0000080, %ecx; rdmsr; or $1, %eax; wrmsr",
            out("eax") _,
            out("edx") _,
            options(nostack)
        );
    }
}

/// Compiles only on bare-metal (target_os = "none"), safe stub otherwise.
#[cfg(not(target_os = "none"))]
pub unsafe fn init_syscall_trap() {
    panic!("init_syscall_trap attempted on non-bare-metal target");
}
