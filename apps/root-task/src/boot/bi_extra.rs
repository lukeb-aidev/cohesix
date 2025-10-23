// Author: Lukas Bower
#![allow(dead_code)]

use core::fmt;
use core::mem::size_of;
use core::ops::Range;

use sel4_sys::{seL4_BootInfo, seL4_CPtr, seL4_UntypedDesc};

use crate::sel4::BootInfoExt;
use crate::trace;

const BOOTINFO_HEADER_DUMP_LIMIT: usize = 256;
const FDT_MAGIC: u32 = 0xD00D_FEEDu32;
const FDT_HEADER_LEN: usize = 10 * size_of::<u32>();

/// Errors encountered when parsing the bootinfo-provided device tree blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// The blob was shorter than the minimum FDT header length.
    TooShort,
    /// The blob did not begin with the `0xd00dfeed` magic value.
    BadMagic,
    /// Reported offsets or lengths exceeded the declared FDT length.
    Bounds,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "DTB shorter than header"),
            Self::BadMagic => write!(f, "DTB magic mismatch"),
            Self::Bounds => write!(f, "DTB section exceeds bounds"),
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
