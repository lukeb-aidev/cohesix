// CLASSIFICATION: COMMUNITY
// Filename: word_size_bits.rs v0.1
// Author: Lukas Bower
// Date Modified: 2028-11-08

#[test]
fn word_size_bits_present() {
    // Ensure the bindgen exposed seL4_WordSizeBits
    assert!(sel4_sys::seL4_WordSizeBits > 0);
}
