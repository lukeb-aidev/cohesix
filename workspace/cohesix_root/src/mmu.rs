// CLASSIFICATION: COMMUNITY
// Filename: mmu.rs v0.1
// Author: Lukas Bower
// Date Modified: 2028-01-21


#[repr(align(4096))]
struct Table([u64; 512]);

static mut L1_TABLE: Table = Table([0; 512]);
static mut L2_TABLE: Table = Table([0; 512]);

const BLOCK_FLAGS: u64 = 0b11; // AF=1 | SH=0 | AP=00 | AttrIdx=0
const DEVICE_FLAGS: u64 = 0b11 | (1 << 2); // device memory attr index 1

fn init_tables(l1: &mut [u64; 512], l2: &mut [u64; 512], dtb: usize, dtb_end: usize) {
    for entry in l1.iter_mut() { *entry = 0; }
    for entry in l2.iter_mut() { *entry = 0; }

    l1[0] = (l2.as_ptr() as u64) | 0b11;

    for i in 0..16 {
        l2[i] = ((i as u64) << 21) | BLOCK_FLAGS;
    }

    let dtb_idx = dtb >> 21;
    let dtb_end_idx = (dtb_end + 0x1FFFFF) >> 21;
    for i in dtb_idx..dtb_end_idx {
        if i < 512 {
            l2[i] = ((i as u64) << 21) | DEVICE_FLAGS;
        }
    }
}

pub unsafe fn init(_text_start: usize, _image_end: usize, dtb: usize, dtb_end: usize) {
    init_tables(&mut L1_TABLE.0, &mut L2_TABLE.0, dtb, dtb_end);
    core::arch::asm!(
        "msr TTBR0_EL1, {0}",
        "dsb ishst",
        "isb",
        in(reg) (&L1_TABLE as *const _ as u64),
        options(nostack, preserves_flags)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_init_match_snapshot() {
        let mut l1 = [0u64; 512];
        let mut l2 = [0u64; 512];
        init_tables(&mut l1, &mut l2, 0x300000, 0x350000);
        assert_eq!(l1[0], (l2.as_ptr() as u64) | 0b11);
        assert_eq!(l2[0], BLOCK_FLAGS);
    }
}
