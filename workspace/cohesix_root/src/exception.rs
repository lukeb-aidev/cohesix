// CLASSIFICATION: COMMUNITY
// Filename: exception.rs v0.3
// Author: Lukas Bower
// Date Modified: 2029-10-09

use crate::{abort, coherr};
use core::arch::asm;

#[derive(Clone, Copy)]
struct ExceptionState {
    esr_el1: u64,
    elr_el1: u64,
    far_el1: u64,
    spsr_el1: u64,
    sp: u64,
    sp_el0: u64,
    sp_el1: u64,
    lr: u64,
}

#[inline(always)]
fn capture_state() -> ExceptionState {
    let esr_el1: u64;
    let elr_el1: u64;
    let far_el1: u64;
    let spsr_el1: u64;
    let sp: u64;
    let sp_el0: u64;
    let sp_el1: u64;
    let lr: u64;
    unsafe {
        asm!("mrs {0}, esr_el1", out(reg) esr_el1);
        asm!("mrs {0}, elr_el1", out(reg) elr_el1);
        asm!("mrs {0}, far_el1", out(reg) far_el1);
        asm!("mrs {0}, spsr_el1", out(reg) spsr_el1);
        asm!("mrs {0}, sp_el0", out(reg) sp_el0);
        asm!("mrs {0}, sp_el1", out(reg) sp_el1);
        asm!("mov {0}, sp", out(reg) sp);
        asm!("mov {0}, x30", out(reg) lr);
    }
    ExceptionState {
        esr_el1,
        elr_el1,
        far_el1,
        spsr_el1,
        sp,
        sp_el0,
        sp_el1,
        lr,
    }
}

fn exception_class_name(ec: u8) -> &'static str {
    match ec {
        0b000000 => "unknown",
        0b000001 => "wfi_wfe",
        0b000111 => "svc64",
        0b001100 => "hlt64",
        0b001101 => "sve_access",
        0b010000 => "iabt_el1",
        0b010001 => "iabt_el0",
        0b010010 => "pc_align",
        0b010011 => "sp_align",
        0b010100 => "dabt_el1",
        0b010101 => "dabt_el0",
        0b010110 => "sp_align",
        0b011000 => "serror",
        0b011010 => "brk64",
        _ => "reserved",
    }
}

fn data_fault_status(code: u8) -> &'static str {
    match code {
        0b000000 => "addr_size_l0",
        0b000001 => "addr_size_l1",
        0b000010 => "addr_size_l2",
        0b000011 => "addr_size_l3",
        0b000100 => "tran_fault_l0",
        0b000101 => "tran_fault_l1",
        0b000110 => "tran_fault_l2",
        0b000111 => "tran_fault_l3",
        0b001000 => "access_fault_l0",
        0b001001 => "access_fault_l1",
        0b001010 => "access_fault_l2",
        0b001011 => "access_fault_l3",
        0b001100 => "perm_fault_l0",
        0b001101 => "perm_fault_l1",
        0b001110 => "perm_fault_l2",
        0b001111 => "perm_fault_l3",
        0b010001 => "sync_ext",
        0b010011 => "sync_parity",
        0b011000 => "sync_ext_abort",
        0b011100 => "alignment",
        0b100001 => "async_sync",
        0b100011 => "async_parity",
        0b110000 => "tlb_conflict",
        0b110001 => "unsupported",
        0b111100 => "implementation",
        _ => "unknown",
    }
}

fn log_exception_state(vector: &str, state: &ExceptionState) {
    let ec = ((state.esr_el1 >> 26) & 0x3f) as u8;
    let iss = state.esr_el1 & 0x01ff_ffff;
    let class = exception_class_name(ec);
    coherr!(
        "exc_state {} esr={:#x} ec={:#x}({}) iss={:#x}",
        vector,
        state.esr_el1,
        ec,
        class,
        iss
    );
    coherr!(
        "exc_state {} elr={:#x} lr={:#x} far={:#x} spsr={:#x}",
        vector,
        state.elr_el1,
        state.lr,
        state.far_el1,
        state.spsr_el1
    );
    coherr!(
        "exc_state {} sp={:#x} sp_el0={:#x} sp_el1={:#x}",
        vector,
        state.sp,
        state.sp_el0,
        state.sp_el1
    );
    if matches!(ec, 0b010100 | 0b010101) {
        let dfsc = (iss & 0x3f) as u8;
        let wnr = ((iss >> 6) & 1) != 0;
        let s1ptw = ((iss >> 7) & 1) != 0;
        coherr!(
            "exc_data_abort {} wnr={} s1ptw={} dfsc={:#x}({})",
            vector,
            wnr as u8,
            s1ptw as u8,
            dfsc,
            data_fault_status(dfsc)
        );
    }
    if matches!(ec, 0b010000 | 0b010001) {
        let ifsc = (iss & 0x3f) as u8;
        coherr!("exc_inst_abort {} ifsc={:#x}", vector, ifsc);
    }
}

fn svc_dispatch(num: u16) {
    match num as i64 {
        -9 => coherr!("svc_debug_putchar"),
        -3 => coherr!("svc_send"),
        -5 => coherr!("svc_recv"),
        -7 => coherr!("svc_yield"),
        -11 => coherr!("svc_debug_halt"),
        _ => coherr!("unknown_svc {num}"),
    }
}

#[no_mangle]
pub extern "C" fn handle_el1_sync() -> ! {
    let state = capture_state();
    log_exception_state("el1_sync", &state);
    coherr!("exc_el1_sync");
    abort("exc el1 sync")
}

#[no_mangle]
pub extern "C" fn handle_el1_irq() -> ! {
    let state = capture_state();
    log_exception_state("el1_irq", &state);
    coherr!("exc_el1_irq");
    abort("exc el1 irq")
}

#[no_mangle]
pub extern "C" fn handle_el1_fiq() -> ! {
    let state = capture_state();
    log_exception_state("el1_fiq", &state);
    coherr!("exc_el1_fiq");
    abort("exc el1 fiq")
}

#[no_mangle]
pub extern "C" fn handle_el1_serror() -> ! {
    let state = capture_state();
    log_exception_state("el1_serror", &state);
    coherr!("exc_el1_serr");
    abort("exc el1 serr")
}

#[no_mangle]
pub extern "C" fn handle_el1_sync_sp0() -> ! {
    let state = capture_state();
    log_exception_state("el1_sync_sp0", &state);
    coherr!("exc_el1_sync_sp0");
    abort("exc el1 sync sp0")
}

#[no_mangle]
pub extern "C" fn handle_el1_irq_sp0() -> ! {
    let state = capture_state();
    log_exception_state("el1_irq_sp0", &state);
    coherr!("exc_el1_irq_sp0");
    abort("exc el1 irq sp0")
}

#[no_mangle]
pub extern "C" fn handle_el1_fiq_sp0() -> ! {
    let state = capture_state();
    log_exception_state("el1_fiq_sp0", &state);
    coherr!("exc_el1_fiq_sp0");
    abort("exc el1 fiq sp0")
}

#[no_mangle]
pub extern "C" fn handle_el1_serror_sp0() -> ! {
    let state = capture_state();
    log_exception_state("el1_serr_sp0", &state);
    coherr!("exc_el1_serr_sp0");
    abort("exc el1 serr sp0")
}

#[no_mangle]
pub extern "C" fn handle_el0_sync() -> ! {
    let state = capture_state();
    log_exception_state("el0_sync", &state);
    let svc_num = (state.esr_el1 & 0xffff) as u16;
    coherr!("exc_el0_sync svc={:#x}", svc_num);
    svc_dispatch(svc_num);
    abort("exc el0 sync")
}

#[no_mangle]
pub extern "C" fn handle_el0_irq() -> ! {
    let state = capture_state();
    log_exception_state("el0_irq", &state);
    coherr!("exc_el0_irq");
    abort("exc el0 irq")
}

#[no_mangle]
pub extern "C" fn handle_el0_fiq() -> ! {
    let state = capture_state();
    log_exception_state("el0_fiq", &state);
    coherr!("exc_el0_fiq");
    abort("exc el0 fiq")
}

#[no_mangle]
pub extern "C" fn handle_el0_serror() -> ! {
    let state = capture_state();
    log_exception_state("el0_serror", &state);
    coherr!("exc_el0_serr");
    abort("exc el0 serr")
}

#[no_mangle]
pub extern "C" fn handle_el0_32_sync() -> ! {
    let state = capture_state();
    log_exception_state("el0_32_sync", &state);
    coherr!("exc_el0_32_sync");
    abort("exc el0 32 sync")
}

#[no_mangle]
pub extern "C" fn handle_el0_32_irq() -> ! {
    let state = capture_state();
    log_exception_state("el0_32_irq", &state);
    coherr!("exc_el0_32_irq");
    abort("exc el0 32 irq")
}

#[no_mangle]
pub extern "C" fn handle_el0_32_fiq() -> ! {
    let state = capture_state();
    log_exception_state("el0_32_fiq", &state);
    coherr!("exc_el0_32_fiq");
    abort("exc el0 32 fiq")
}

#[no_mangle]
pub extern "C" fn handle_el0_32_serror() -> ! {
    let state = capture_state();
    log_exception_state("el0_32_serr", &state);
    coherr!("exc_el0_32_serr");
    abort("exc el0 32 serr")
}
