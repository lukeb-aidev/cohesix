// CLASSIFICATION: COMMUNITY
// Filename: mmu.rs v0.1
// Author: Lukas Bower
// Date Modified: 2028-01-21

use core::ptr::write_volatile;

#[repr(align(4096))]
struct Table([u64; 512]);

static mut L1_TABLE: Table = Table([0; 512]);
static mut L2_TABLE: Table = Table([0; 512]);

const BLOCK_FLAGS: u64 = 0b11; // AF=1 | SH=0 | AP=00 | AttrIdx=0
const DEVICE_FLAGS: u64 = 0b11 | (1 << 2); // device memory attr index 1

pub unsafe fn init(_text_start: usize, _image_end: usize, _dtb: usize, _dtb_end: usize) {
    for entry in L1_TABLE.0.iter_mut() { *entry = 0; }
    for entry in L2_TABLE.0.iter_mut() { *entry = 0; }

    // Map lower 1GB via L1 -> L2 table
    L1_TABLE.0[0] = (&L2_TABLE as *const _ as u64) | 0b11;

    // Identity map first 64MB with normal memory
    for i in 0..16 {
        L2_TABLE.0[i] = ((i as u64) << 21) | BLOCK_FLAGS;
    }

    // Map DTB as device
    let dtb_idx = dtb >> 21;
    let dtb_end_idx = (dtb_end + 0x1FFFFF) >> 21;
    for i in dtb_idx..dtb_end_idx {
        L2_TABLE.0[i] = ((i as u64) << 21) | DEVICE_FLAGS;
    }

    core::arch::asm!(
        "msr TTBR0_EL1, {0}",
        "dsb ishst",
        "isb",
        in(reg) (&L1_TABLE as *const _ as u64),
        options(nostack, preserves_flags)
    );
}
