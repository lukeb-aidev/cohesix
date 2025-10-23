// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::boot::bi_extra::{parse_dtb, ParseError};

fn write_be(word: u32, target: &mut [u8], offset: usize) {
    target[offset..offset + 4].copy_from_slice(&word.to_be_bytes());
}

fn build_dtb_fixture() -> Vec<u8> {
    let totalsize = 64u32;
    let mut blob = vec![0u8; totalsize as usize];

    write_be(0xd00d_feed, &mut blob, 0);
    write_be(totalsize, &mut blob, 4);
    write_be(40, &mut blob, 8); // off_dt_struct
    write_be(56, &mut blob, 12); // off_dt_strings
    write_be(32, &mut blob, 32); // size_dt_strings
    write_be(16, &mut blob, 36); // size_dt_struct

    for byte in &mut blob[40..56] {
        *byte = 0xaa;
    }
    for (idx, byte) in blob[56..64].iter_mut().enumerate() {
        *byte = b's' + idx as u8;
    }

    blob
}

#[test]
fn parses_valid_dtb_header() {
    let blob = build_dtb_fixture();
    let dtb = parse_dtb(&blob).expect("valid dtb fixture");
    let header = dtb.header();

    assert_eq!(header.totalsize(), 64);
    assert_eq!(header.structure_offset(), 40);
    assert_eq!(header.strings_offset(), 56);
    assert_eq!(dtb.structure_block(), &blob[40..56]);
    assert_eq!(dtb.strings_block(), &blob[56..64]);
}

#[test]
fn rejects_truncated_blob() {
    let blob = vec![0u8; 8];
    assert_eq!(parse_dtb(&blob), Err(ParseError::TooShort));
}

#[test]
fn rejects_out_of_bounds_sections() {
    let totalsize = 48u32;
    let mut blob = vec![0u8; totalsize as usize];

    write_be(0xd00d_feed, &mut blob, 0);
    write_be(totalsize, &mut blob, 4);
    write_be(32, &mut blob, 8); // off_dt_struct
    write_be(44, &mut blob, 12); // off_dt_strings
    write_be(8, &mut blob, 32); // size_dt_strings (overflows totalsize)
    write_be(32, &mut blob, 36); // size_dt_struct (overflows totalsize)

    assert_eq!(parse_dtb(&blob), Err(ParseError::Bounds));
}
