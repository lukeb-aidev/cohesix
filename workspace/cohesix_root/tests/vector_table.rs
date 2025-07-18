// CLASSIFICATION: COMMUNITY
// Filename: vector_table.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2028-01-21

use object::{Object, ObjectSection};
use std::fs;

#[test]
fn vector_table_pattern() {
    let data = fs::read("target/sel4-aarch64/release/cohesix_root").expect("read elf");
    let obj = object::File::parse(&*data).expect("parse elf");
    let section = obj.section_by_name(".vectors").expect("vectors section");
    let bytes = section.data().expect("section bytes");
    let word = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    assert_eq!(word & 0xFC00_0000, 0x1400_0000);
}
