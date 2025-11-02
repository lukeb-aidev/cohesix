// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::bootstrap::cspace_sys::super_bits_as_u8_for_test;

#[test]
fn bits_fit_common_values() {
    for v in [8usize, 12, 13, 14, 16, 21] {
        let bits = super_bits_as_u8_for_test(v);
        assert_eq!(bits as usize, v);
    }
}

#[test]
fn bits_out_of_range_falls_back() {
    let bits = super_bits_as_u8_for_test(1_000usize);
    assert_eq!(bits, 13);
}
