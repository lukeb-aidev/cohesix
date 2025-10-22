// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::serial::pl011::{
    CR_OFFSET, DR_OFFSET, FBRD_OFFSET, FR_OFFSET, IBRD_OFFSET, ICR_OFFSET, IMSC_OFFSET, LCRH_OFFSET,
};

#[test]
fn pl011_offsets_match_qemu_layout() {
    assert_eq!(DR_OFFSET, 0x00);
    assert_eq!(FR_OFFSET, 0x18);
    assert_eq!(IBRD_OFFSET, 0x24);
    assert_eq!(FBRD_OFFSET, 0x28);
    assert_eq!(LCRH_OFFSET, 0x2C);
    assert_eq!(CR_OFFSET, 0x30);
    assert_eq!(IMSC_OFFSET, 0x38);
    assert_eq!(ICR_OFFSET, 0x44);
}
