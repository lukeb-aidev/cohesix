// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::mem::size_of;
use core::ops::Range;

use root_task::boot::bi_extra::{locate_dtb, parse_dtb, ExtraError, ParseError};
use sel4_sys::seL4_Word;

const FDT_ID: seL4_Word = 6;

fn extra_range(extra: &[u8]) -> Range<usize> {
    let start = extra.as_ptr() as usize;
    let end = start + extra.len();
    start..end
}

fn write_be(word: u32, target: &mut [u8], offset: usize) {
    target[offset..offset + 4].copy_from_slice(&word.to_be_bytes());
}

fn write_word(word: seL4_Word, target: &mut [u8], offset: usize) {
    match size_of::<seL4_Word>() {
        4 => {
            let bytes = (word as u32).to_le_bytes();
            target[offset..offset + 4].copy_from_slice(&bytes);
        }
        8 => {
            let bytes = (word as u64).to_le_bytes();
            target[offset..offset + 8].copy_from_slice(&bytes);
        }
        _ => panic!("unsupported seL4_Word width"),
    }
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

fn build_extra_fixture(dtb: &[u8], id: seL4_Word) -> Vec<u8> {
    let header_size = size_of::<sel4_sys::seL4_BootInfoHeader>();
    let total_len = header_size + dtb.len();
    let mut blob = vec![0u8; total_len];

    write_word(id, &mut blob, 0);
    write_word(total_len as seL4_Word, &mut blob, size_of::<seL4_Word>());
    blob[header_size..].copy_from_slice(dtb);

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

#[test]
fn locate_dtb_finds_payload() {
    let dtb = build_dtb_fixture();
    let extra = build_extra_fixture(&dtb, FDT_ID);

    let located = locate_dtb(&extra, extra_range(&extra)).expect("dtb header present");
    assert_eq!(located, &dtb);
}

#[test]
fn locate_dtb_rejects_truncated_header() {
    let dtb = build_dtb_fixture();
    let mut extra = build_extra_fixture(&dtb, FDT_ID);
    extra.truncate(size_of::<seL4_Word>());

    assert_eq!(locate_dtb(&extra, extra_range(&extra)), Err(ExtraError::Truncated));
}

#[test]
fn locate_dtb_rejects_invalid_length() {
    let dtb = build_dtb_fixture();
    let mut extra = build_extra_fixture(&dtb, FDT_ID);
    let invalid = (size_of::<sel4_sys::seL4_BootInfoHeader>() - 4) as seL4_Word;
    write_word(invalid, &mut extra, size_of::<seL4_Word>());

    assert_eq!(locate_dtb(&extra, extra_range(&extra)), Err(ExtraError::InvalidLength));
}

#[test]
fn locate_dtb_reports_missing_record() {
    let dtb = build_dtb_fixture();
    let extra = build_extra_fixture(&dtb, 0);

    assert_eq!(locate_dtb(&extra, extra_range(&extra)), Err(ExtraError::MissingDtb));
}

#[test]
fn locate_dtb_rejects_range_mismatch() {
    let dtb = build_dtb_fixture();
    let extra = build_extra_fixture(&dtb, FDT_ID);
    let mut range = extra_range(&extra);
    range = (range.start + 1)..range.end;

    assert_eq!(locate_dtb(&extra, range), Err(ExtraError::Bounds));
}
