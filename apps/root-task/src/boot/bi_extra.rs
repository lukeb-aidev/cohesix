// Author: Lukas Bower
#![allow(dead_code)]

use core::fmt;
use core::mem::size_of;
use core::ops::Range;
use core::str;

use sel4_sys::{seL4_BootInfo, seL4_CPtr, seL4_UntypedDesc};

use crate::sel4::BootInfoExt;
use crate::trace;

const BOOTINFO_HEADER_DUMP_LIMIT: usize = 256;
const FDT_MAGIC: u32 = 0xD00D_FEEDu32;
const FDT_HEADER_LEN: usize = 10 * size_of::<u32>();
const FDT_PROP_MAX_LEN: usize = 4 << 20; // 4 MiB hard cap.

const FDT_BEGIN_NODE: u32 = 0x0000_0001;
const FDT_END_NODE: u32 = 0x0000_0002;
const FDT_PROP: u32 = 0x0000_0003;
const FDT_NOP: u32 = 0x0000_0004;
const FDT_END: u32 = 0x0000_0009;

/// Errors encountered when parsing the bootinfo-provided device tree blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// The blob was shorter than the minimum FDT header length.
    TooShort,
    /// The blob did not begin with the `0xd00dfeed` magic value.
    BadMagic,
    /// Reported offsets or lengths exceeded the declared FDT length.
    Bounds,
    /// Encountered truncated data while parsing the structure block.
    Truncated,
    /// A string field was missing its terminating null byte.
    UnterminatedString,
    /// A string field could not be converted from UTF-8.
    BadString,
    /// A property declared an excessively large payload.
    PropertyTooLarge,
    /// An unexpected FDT token was encountered.
    InvalidToken(u32),
    /// The structure block terminated while nodes were still open.
    UnexpectedEnd,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "DTB shorter than header"),
            Self::BadMagic => write!(f, "DTB magic mismatch"),
            Self::Bounds => write!(f, "DTB section exceeds bounds"),
            Self::Truncated => write!(f, "DTB structure truncated"),
            Self::UnterminatedString => write!(f, "DTB string missing terminator"),
            Self::BadString => write!(f, "DTB string invalid UTF-8"),
            Self::PropertyTooLarge => write!(f, "DTB property too large"),
            Self::InvalidToken(token) => write!(f, "DTB token 0x{token:08x} invalid"),
            Self::UnexpectedEnd => write!(f, "DTB structure ended prematurely"),
        }
    }
}

/// Parsed metadata describing the top-level device tree header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DtbHeader {
    totalsize: u32,
    off_dt_struct: u32,
    off_dt_strings: u32,
    size_dt_struct: u32,
    size_dt_strings: u32,
}

impl DtbHeader {
    /// Total length of the DTB in bytes.
    #[must_use]
    pub fn totalsize(&self) -> usize {
        self.totalsize as usize
    }

    /// Byte offset of the structure block.
    #[must_use]
    pub fn structure_offset(&self) -> usize {
        self.off_dt_struct as usize
    }

    /// Byte offset of the strings block.
    #[must_use]
    pub fn strings_offset(&self) -> usize {
        self.off_dt_strings as usize
    }
}

/// Thin wrapper over a validated DTB slice.
pub struct Dtb<'a> {
    header: DtbHeader,
    blob: &'a [u8],
    structure_range: Range<usize>,
    strings_range: Range<usize>,
}

impl<'a> Dtb<'a> {
    /// Returns the validated header metadata.
    #[must_use]
    pub fn header(&self) -> DtbHeader {
        self.header
    }

    /// Returns the structure block slice.
    #[must_use]
    pub fn structure_block(&self) -> &'a [u8] {
        &self.blob[self.structure_range.clone()]
    }

    /// Returns the strings block slice.
    #[must_use]
    pub fn strings_block(&self) -> &'a [u8] {
        &self.blob[self.strings_range.clone()]
    }

    /// Returns a cursor that iterates over the structure block tokens.
    #[must_use]
    pub fn structure_cursor(&self) -> StructureCursor<'a> {
        StructureCursor::new(self.structure_block(), self.strings_block())
    }
}

fn read_be_u32(blob: &[u8], offset: usize) -> Result<u32, ParseError> {
    let end = offset
        .checked_add(size_of::<u32>())
        .ok_or(ParseError::Bounds)?;
    if end > blob.len() {
        return Err(ParseError::TooShort);
    }

    let bytes: [u8; 4] = blob[offset..end]
        .try_into()
        .expect("slice length verified via bounds check");
    Ok(u32::from_be_bytes(bytes))
}

fn bounded_range(len: usize, offset: u32, size: u32) -> Result<Range<usize>, ParseError> {
    let start = usize::try_from(offset).map_err(|_| ParseError::Bounds)?;
    let span = usize::try_from(size).map_err(|_| ParseError::Bounds)?;
    let end = start.checked_add(span).ok_or(ParseError::Bounds)?;
    if end > len {
        return Err(ParseError::Bounds);
    }
    Ok(start..end)
}

/// Parses the DTB header found inside the bootinfo extra region.
pub fn parse_dtb(extra: &[u8]) -> Result<Dtb<'_>, ParseError> {
    if extra.len() < FDT_HEADER_LEN {
        return Err(ParseError::TooShort);
    }

    let magic = read_be_u32(extra, 0)?;
    if magic != FDT_MAGIC {
        return Err(ParseError::BadMagic);
    }

    let totalsize = read_be_u32(extra, 4)?;
    let off_dt_struct = read_be_u32(extra, 8)?;
    let off_dt_strings = read_be_u32(extra, 12)?;
    let size_dt_strings = read_be_u32(extra, 32)?;
    let size_dt_struct = read_be_u32(extra, 36)?;

    let header = DtbHeader {
        totalsize,
        off_dt_struct,
        off_dt_strings,
        size_dt_struct,
        size_dt_strings,
    };

    let blob_len = usize::try_from(totalsize).map_err(|_| ParseError::Bounds)?;
    if blob_len == 0 || blob_len > extra.len() {
        return Err(ParseError::Bounds);
    }

    let structure_range = bounded_range(blob_len, off_dt_struct, size_dt_struct)?;
    let strings_range = bounded_range(blob_len, off_dt_strings, size_dt_strings)?;

    let blob = &extra[..blob_len];
    Ok(Dtb {
        header,
        blob,
        structure_range,
        strings_range,
    })
}

fn align_up(value: usize, align: usize) -> Result<usize, ParseError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(ParseError::Bounds);
    }
    let mask = align - 1;
    value
        .checked_add(mask)
        .map(|aligned| aligned & !mask)
        .ok_or(ParseError::Bounds)
}

fn read_cstr<'a>(blob: &'a [u8], offset: usize) -> Result<&'a str, ParseError> {
    if offset >= blob.len() {
        return Err(ParseError::Bounds);
    }
    let tail = &blob[offset..];
    let len = tail
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(ParseError::UnterminatedString)?;
    let bytes = &tail[..len];
    str::from_utf8(bytes).map_err(|_| ParseError::BadString)
}

/// Iterator over the structure block tokens of a DTB.
pub struct StructureCursor<'a> {
    structure: &'a [u8],
    strings: &'a [u8],
    offset: usize,
    finished: bool,
    depth: usize,
}

/// Tokens yielded by [`StructureCursor`].
#[derive(Debug, PartialEq, Eq)]
pub enum StructureItem<'a> {
    /// A new node has begun with the provided name.
    BeginNode(&'a str),
    /// A node has ended.
    EndNode,
    /// A property with a resolved name and payload.
    Property { name: &'a str, value: &'a [u8] },
}

impl<'a> StructureCursor<'a> {
    const ALIGNMENT: usize = 4;

    fn new(structure: &'a [u8], strings: &'a [u8]) -> Self {
        Self {
            structure,
            strings,
            offset: 0,
            finished: false,
            depth: 0,
        }
    }

    fn read_u32(&self, offset: usize) -> Result<u32, ParseError> {
        match read_be_u32(self.structure, offset) {
            Ok(value) => Ok(value),
            Err(ParseError::TooShort) => Err(ParseError::Truncated),
            Err(err) => Err(err),
        }
    }

    fn align_offset(&mut self, value: usize) -> Result<(), ParseError> {
        self.offset = align_up(value, Self::ALIGNMENT)?;
        if self.offset > self.structure.len() {
            return Err(ParseError::Truncated);
        }
        Ok(())
    }

    /// Returns the next structure item or `None` if the stream has ended.
    pub fn next(&mut self) -> Result<Option<StructureItem<'a>>, ParseError> {
        if self.finished {
            return Ok(None);
        }
        if self.offset >= self.structure.len() {
            return Err(ParseError::Truncated);
        }

        let token = self.read_u32(self.offset)?;
        self.offset = self
            .offset
            .checked_add(size_of::<u32>())
            .ok_or(ParseError::Bounds)?;

        match token {
            FDT_BEGIN_NODE => self.handle_begin_node(),
            FDT_END_NODE => self.handle_end_node(),
            FDT_PROP => self.handle_property(),
            FDT_NOP => self.next(),
            FDT_END => self.handle_end(),
            other => Err(ParseError::InvalidToken(other)),
        }
    }

    fn handle_begin_node(&mut self) -> Result<Option<StructureItem<'a>>, ParseError> {
        let name_start = self.offset;
        if name_start >= self.structure.len() {
            return Err(ParseError::Truncated);
        }
        let name_len = self.structure[name_start..]
            .iter()
            .position(|&byte| byte == 0)
            .ok_or(ParseError::UnterminatedString)?;
        let name_end = name_start.checked_add(name_len).ok_or(ParseError::Bounds)?;
        let name_bytes = &self.structure[name_start..name_end];
        let name = str::from_utf8(name_bytes).map_err(|_| ParseError::BadString)?;
        let after_null = name_end.checked_add(1).ok_or(ParseError::Bounds)?;
        self.align_offset(after_null)?;
        self.depth = self.depth.checked_add(1).ok_or(ParseError::Bounds)?;
        Ok(Some(StructureItem::BeginNode(name)))
    }

    fn handle_end_node(&mut self) -> Result<Option<StructureItem<'a>>, ParseError> {
        if self.depth == 0 {
            return Err(ParseError::UnexpectedEnd);
        }
        self.depth -= 1;
        Ok(Some(StructureItem::EndNode))
    }

    fn handle_property(&mut self) -> Result<Option<StructureItem<'a>>, ParseError> {
        let base = self.offset;
        let len_u32 = self.read_u32(base)?;
        let nameoff_u32 = self.read_u32(base + size_of::<u32>())?;
        self.offset = base
            .checked_add(2 * size_of::<u32>())
            .ok_or(ParseError::Bounds)?;

        let len = usize::try_from(len_u32).map_err(|_| ParseError::Bounds)?;
        if len > FDT_PROP_MAX_LEN {
            return Err(ParseError::PropertyTooLarge);
        }
        let nameoff = usize::try_from(nameoff_u32).map_err(|_| ParseError::Bounds)?;

        let data_end = self.offset.checked_add(len).ok_or(ParseError::Bounds)?;
        if data_end > self.structure.len() {
            return Err(ParseError::Truncated);
        }
        let name = read_cstr(self.strings, nameoff)?;
        let value = &self.structure[self.offset..data_end];
        self.align_offset(data_end)?;
        Ok(Some(StructureItem::Property { name, value }))
    }

    fn handle_end(&mut self) -> Result<Option<StructureItem<'a>>, ParseError> {
        if self.depth != 0 {
            return Err(ParseError::UnexpectedEnd);
        }
        self.finished = true;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::convert::TryFrom;
    use std::vec::Vec;

    struct SampleDtb {
        blob: Vec<u8>,
        structure_offset: usize,
        kaslr_prop_base: usize,
    }

    fn push_be32(buf: &mut Vec<u8>, value: u32) {
        buf.extend_from_slice(&value.to_be_bytes());
    }

    fn push_string(strings: &mut Vec<u8>, value: &str) -> usize {
        let offset = strings.len();
        strings.extend_from_slice(value.as_bytes());
        strings.push(0);
        offset
    }

    fn build_sample_dtb() -> SampleDtb {
        const HEADER_LEN: usize = FDT_HEADER_LEN;
        const RSVMAP_LEN: usize = 16;

        let mut structure = Vec::new();
        let mut strings = Vec::new();

        let kaslr_off = push_string(&mut strings, "kaslr-seed");
        let rng_off = push_string(&mut strings, "rng-seed");

        push_be32(&mut structure, FDT_BEGIN_NODE);
        structure.push(0);
        while structure.len() % 4 != 0 {
            structure.push(0);
        }

        push_be32(&mut structure, FDT_PROP);
        let kaslr_prop_base = structure.len();
        push_be32(&mut structure, 8);
        push_be32(&mut structure, u32::try_from(kaslr_off).unwrap());
        push_be32(&mut structure, 0x643C_F141);
        push_be32(&mut structure, 0x52FB_34EB);

        push_be32(&mut structure, FDT_PROP);
        push_be32(&mut structure, 32);
        push_be32(&mut structure, u32::try_from(rng_off).unwrap());
        for word in [
            0x4D2D_8C3F,
            0xD1F1_6373,
            0x3AA5_A97F,
            0xD5F3_1ED1,
            0x2256_2F98,
            0x608C_1EAD,
            0x006B_D520,
            0xCDC2_7707,
        ] {
            push_be32(&mut structure, word);
        }

        push_be32(&mut structure, FDT_END_NODE);
        push_be32(&mut structure, FDT_END);

        let structure_len = structure.len();
        let strings_len = strings.len();
        let off_dt_struct = HEADER_LEN + RSVMAP_LEN;
        let off_dt_strings = off_dt_struct + structure_len;
        let totalsize = off_dt_strings + strings_len;

        let mut blob = Vec::with_capacity(totalsize);
        push_be32(&mut blob, FDT_MAGIC);
        push_be32(&mut blob, u32::try_from(totalsize).unwrap());
        push_be32(&mut blob, u32::try_from(off_dt_struct).unwrap());
        push_be32(&mut blob, u32::try_from(off_dt_strings).unwrap());
        push_be32(&mut blob, u32::try_from(HEADER_LEN).unwrap());
        push_be32(&mut blob, 17);
        push_be32(&mut blob, 16);
        push_be32(&mut blob, 0);
        push_be32(&mut blob, u32::try_from(strings_len).unwrap());
        push_be32(&mut blob, u32::try_from(structure_len).unwrap());

        blob.resize(blob.len() + RSVMAP_LEN, 0);
        blob.extend_from_slice(&structure);
        blob.extend_from_slice(&strings);

        SampleDtb {
            blob,
            structure_offset: off_dt_struct,
            kaslr_prop_base,
        }
    }

    #[test]
    fn structure_cursor_parses_properties() {
        let sample = build_sample_dtb();
        let dtb = parse_dtb(&sample.blob).expect("dtb should parse");
        let mut cursor = dtb.structure_cursor();

        match cursor.next().expect("root node event") {
            Some(StructureItem::BeginNode(name)) => assert!(name.is_empty()),
            other => panic!("unexpected first item: {other:?}"),
        }

        match cursor.next().expect("kaslr property") {
            Some(StructureItem::Property { name, value }) => {
                assert_eq!(name, "kaslr-seed");
                assert_eq!(value.len(), 8);
            }
            other => panic!("unexpected second item: {other:?}"),
        }

        match cursor.next().expect("rng property") {
            Some(StructureItem::Property { name, value }) => {
                assert_eq!(name, "rng-seed");
                assert_eq!(value.len(), 32);
            }
            other => panic!("unexpected third item: {other:?}"),
        }

        match cursor.next().expect("end node") {
            Some(StructureItem::EndNode) => {}
            other => panic!("unexpected fourth item: {other:?}"),
        }

        assert!(cursor.next().expect("stream end").is_none());
    }

    #[test]
    fn structure_cursor_rejects_bad_name_offset() {
        let mut sample = build_sample_dtb();
        let prop_nameoff_index =
            sample.structure_offset + sample.kaslr_prop_base + core::mem::size_of::<u32>();
        sample.blob[prop_nameoff_index..prop_nameoff_index + 4]
            .copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());

        let dtb = parse_dtb(&sample.blob).expect("header still parses");
        let mut cursor = dtb.structure_cursor();

        assert!(matches!(
            cursor.next(),
            Ok(Some(StructureItem::BeginNode(_)))
        ));
        let err = cursor.next().expect_err("invalid name offset should fail");
        assert_eq!(err, ParseError::Bounds);
    }

    #[test]
    fn structure_cursor_caps_property_length() {
        let mut sample = build_sample_dtb();
        let prop_len_index = sample.structure_offset + sample.kaslr_prop_base;
        let capped = u32::try_from(FDT_PROP_MAX_LEN).unwrap().saturating_add(1);
        sample.blob[prop_len_index..prop_len_index + 4].copy_from_slice(&capped.to_be_bytes());

        let dtb = parse_dtb(&sample.blob).expect("header still parses");
        let mut cursor = dtb.structure_cursor();

        assert!(matches!(
            cursor.next(),
            Ok(Some(StructureItem::BeginNode(_)))
        ));
        let err = cursor.next().expect_err("oversized property must error");
        assert_eq!(err, ParseError::PropertyTooLarge);
    }
}

/// Emits a diagnostic dump of the bootinfo header and extra region.
pub fn dump_bootinfo(
    bootinfo: &'static seL4_BootInfo,
    extra_dump_limit: usize,
) -> Option<(&'static [u8], usize)> {
    let header_bytes = bootinfo.header_bytes();
    trace::hex_dump_slice("bootinfo.header", header_bytes, BOOTINFO_HEADER_DUMP_LIMIT);
    let extra_slice = bootinfo.extra_bytes();
    if extra_slice.is_empty() {
        return None;
    }

    trace::hex_dump_slice("bootinfo.extra", extra_slice, extra_dump_limit);
    Some((extra_slice, extra_slice.len()))
}

/// Minimal mirror of [`seL4_UntypedDesc`] with idiomatic field names for the root task.
#[derive(Clone, Copy)]
pub struct UntypedDesc {
    pub paddr: u64,
    pub size_bits: u8,
    pub is_device: u8,
}

impl From<seL4_UntypedDesc> for UntypedDesc {
    fn from(value: seL4_UntypedDesc) -> Self {
        Self {
            paddr: value.paddr as u64,
            size_bits: value.sizeBits,
            is_device: value.isDevice,
        }
    }
}

/// Returns the first RAM-backed untyped descriptor advertised by the kernel.
pub fn first_regular_untyped_from_extra(bi: &seL4_BootInfo) -> Option<(seL4_CPtr, UntypedDesc)> {
    let count = (bi.untyped.end - bi.untyped.start) as usize;
    let descriptors = &bi.untypedList[..count];

    descriptors.iter().enumerate().find_map(|(index, desc)| {
        if desc.isDevice == 0 {
            let cap = bi.untyped.start + index as seL4_CPtr;
            Some((cap, (*desc).into()))
        } else {
            None
        }
    })
}
