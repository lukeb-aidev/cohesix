// CLASSIFICATION: COMMUNITY
// Filename: lang_items.rs v0.2
// Author: Lukas Bower
// Date Modified: 2029-10-09

use core::panic::PanicInfo;
use core::{alloc::Layout, arch::asm};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    crate::putstr("[root] panic");
    if let Some(message) = info.message() {
        crate::coherr!("[panic] message: {}", message);
    } else {
        crate::coherr!("[panic] message: <none>");
    }
    if let Some(location) = info.location() {
        crate::coherr!(
            "[panic] location: {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    } else {
        crate::coherr!("[panic] location: <unknown>");
    }
    unsafe {
        let mut sp: usize;
        let mut lr: usize;
        let mut elr: u64;
        let mut spsr: u64;
        let mut far: u64;
        let mut sp_el0: u64;
        let mut sp_el1: u64;
        asm!("mov {0}, sp", out(reg) sp);
        asm!("mov {0}, x30", out(reg) lr);
        asm!("mrs {0}, elr_el1", out(reg) elr);
        asm!("mrs {0}, spsr_el1", out(reg) spsr);
        asm!("mrs {0}, far_el1", out(reg) far);
        asm!("mrs {0}, sp_el0", out(reg) sp_el0);
        asm!("mrs {0}, sp_el1", out(reg) sp_el1);
        crate::coherr!(
            "[panic] regs sp={:#x} lr={:#x} elr_el1={:#x} spsr_el1={:#x} far_el1={:#x}",
            sp,
            lr,
            elr,
            spsr,
            far
        );
        crate::coherr!("[panic] sp_el0={:#x} sp_el1={:#x}", sp_el0, sp_el1);
    }
    crate::bootlog::flush_to_uart_if_ready();
    loop {
        core::hint::spin_loop();
    }
}

#[alloc_error_handler]
fn alloc_error(layout: Layout) -> ! {
    crate::putstr("[root] alloc_error");
    crate::coherr!(
        "[alloc_error] size={} align={}",
        layout.size(),
        layout.align()
    );
    crate::bootlog::flush_to_uart_if_ready();
    loop {
        core::hint::spin_loop();
    }
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}
