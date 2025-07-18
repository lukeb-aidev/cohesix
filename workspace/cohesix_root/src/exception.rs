// CLASSIFICATION: COMMUNITY
// Filename: exception.rs v0.2
// Author: Lukas Bower
// Date Modified: 2028-01-25

use crate::{coherr, abort};

fn svc_dispatch(num: u16) {
    match num as i64 {
        9 => coherr!("svc_debug_putchar"),
        3 => coherr!("svc_send"),
        5 => coherr!("svc_recv"),
        7 => coherr!("svc_yield"),
        11 => coherr!("svc_debug_halt"),
        _ => coherr!("unknown_svc {num}"),
    }
}

#[no_mangle]
pub extern "C" fn handle_el1_sync() -> ! {
    coherr!("exc_el1_sync");
    abort("exc el1 sync")
}

#[no_mangle]
pub extern "C" fn handle_el1_irq() -> ! {
    coherr!("exc_el1_irq");
    abort("exc el1 irq")
}

#[no_mangle]
pub extern "C" fn handle_el1_fiq() -> ! {
    coherr!("exc_el1_fiq");
    abort("exc el1 fiq")
}

#[no_mangle]
pub extern "C" fn handle_el1_serror() -> ! {
    coherr!("exc_el1_serr");
    abort("exc el1 serr")
}

#[no_mangle]
pub extern "C" fn handle_el1_sync_sp0() -> ! {
    coherr!("exc_el1_sync_sp0");
    abort("exc el1 sync sp0")
}

#[no_mangle]
pub extern "C" fn handle_el1_irq_sp0() -> ! {
    coherr!("exc_el1_irq_sp0");
    abort("exc el1 irq sp0")
}

#[no_mangle]
pub extern "C" fn handle_el1_fiq_sp0() -> ! {
    coherr!("exc_el1_fiq_sp0");
    abort("exc el1 fiq sp0")
}

#[no_mangle]
pub extern "C" fn handle_el1_serror_sp0() -> ! {
    coherr!("exc_el1_serr_sp0");
    abort("exc el1 serr sp0")
}

#[no_mangle]
pub extern "C" fn handle_el0_sync() -> ! {
    let esr: u64;
    unsafe { core::arch::asm!("mrs {0}, esr_el1", out(reg) esr); }
    let svc_num = (esr & 0xffff) as u16;
    coherr!("exc_el0_sync svc={:#x}", svc_num);
    svc_dispatch(svc_num);
    abort("exc el0 sync")
}

#[no_mangle]
pub extern "C" fn handle_el0_irq() -> ! {
    coherr!("exc_el0_irq");
    abort("exc el0 irq")
}

#[no_mangle]
pub extern "C" fn handle_el0_fiq() -> ! {
    coherr!("exc_el0_fiq");
    abort("exc el0 fiq")
}

#[no_mangle]
pub extern "C" fn handle_el0_serror() -> ! {
    coherr!("exc_el0_serr");
    abort("exc el0 serr")
}

#[no_mangle]
pub extern "C" fn handle_el0_32_sync() -> ! {
    coherr!("exc_el0_32_sync");
    abort("exc el0 32 sync")
}

#[no_mangle]
pub extern "C" fn handle_el0_32_irq() -> ! {
    coherr!("exc_el0_32_irq");
    abort("exc el0 32 irq")
}

#[no_mangle]
pub extern "C" fn handle_el0_32_fiq() -> ! {
    coherr!("exc_el0_32_fiq");
    abort("exc el0 32 fiq")
}

#[no_mangle]
pub extern "C" fn handle_el0_32_serror() -> ! {
    coherr!("exc_el0_32_serr");
    abort("exc el0 32 serr")
}

