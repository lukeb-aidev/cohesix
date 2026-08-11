// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate and plan least-authority W^X mappings for Worker images.
// Author: Lukas Bower

//! The Worker image loader accepts only the canonical, sectionless ELF subset
//! emitted by `scripts/worker_image_manifest.py`. It binds the rootserver-
//! embedded archive and manifest to digests compiled into root after Worker
//! packaging, then exposes each load segment to a HAL mapper with exact W^X
//! rights. The outer system CPIO remains a release/host-tool projection; seL4
//! BootInfo extra bytes contain FDT records and are not a Worker image source.

use core::cmp::Ordering;

use sha2::{Digest, Sha256};

const ELF_HEADER_BYTES: usize = 64;
const ELF_PROGRAM_HEADER_BYTES: usize = 56;
const ELF_MACHINE_AARCH64: u16 = 183;
const ELF_TYPE_EXEC: u16 = 2;
const ELF_PROGRAM_LOAD: u32 = 1;
const ELF_FLAG_EXECUTE: u32 = 1;
const ELF_FLAG_WRITE: u32 = 2;
const ELF_FLAG_READ: u32 = 4;
const ELF_FLAGS_READ_EXECUTE: u32 = ELF_FLAG_READ | ELF_FLAG_EXECUTE;
const ELF_FLAGS_READ_WRITE: u32 = ELF_FLAG_READ | ELF_FLAG_WRITE;
const WORKER_METADATA_MAGIC: u32 = 0x574b_4d31;
const WORKER_METADATA_BYTES: usize = 64;
const WORKER_ABI_VERSION: u16 = 1;
const WORKER_ENTRY_VERSION: u16 = 1;
const WORKER_METADATA_FLAGS: u32 = 3;
const WORKER_ARCHIVE_PATH: &str = "cohesix/artifacts/cohesix-worker-images.cpio";
const WORKER_MANIFEST_PATH: &str = "cohesix/artifacts/cohesix-worker-image-manifest.json";
const MAX_SEGMENTS: usize = 8;
const MAX_LOAD_SPAN: u64 = 2 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 512 * 1024;
const PAGE_BYTES: u64 = 4096;

/// One image identity generated from `cohesix-worker-image-manifest/v1`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpectedWorkerImage {
    /// Exact target binary name.
    pub name: &'static str,
    /// Exact canonical Worker role.
    pub role: &'static str,
    /// Path inside the separately packaged Worker archive.
    pub archive_path: &'static str,
    /// SHA-256 of the canonical sectionless ELF bytes.
    pub image_sha256: [u8; 32],
    /// Exact canonical byte length.
    pub image_bytes: u64,
    /// Exact `_start` address.
    pub entry_vaddr: u64,
    /// Lowest load-segment virtual address.
    pub load_base_vaddr: u64,
    /// Exclusive highest load-segment memory address.
    pub load_limit_vaddr: u64,
    /// Virtual address of the retained 64-byte metadata record.
    pub metadata_vaddr: u64,
    /// SHA-256 of the retained metadata record.
    pub metadata_sha256: [u8; 32],
}

mod generated_identity {
    ::core::include!(::core::concat!(
        ::core::env!("OUT_DIR"),
        "/worker_image_identity.rs"
    ));
}

pub use generated_identity::{
    WORKER_ARCHIVE_SHA256, WORKER_IMAGE_IDENTITIES, WORKER_IMAGE_IDENTITY_BOUND,
    WORKER_MANIFEST_SHA256,
};

/// Exact rights assigned to one planned Worker ELF segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerSegmentRights {
    /// Readable, immutable data.
    ReadOnly,
    /// Readable executable code.
    ReadExecute,
    /// Readable writable data; never executable.
    ReadWrite,
}

/// One validated mapping operation for a Worker child VSpace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerLoadSegment {
    /// Offset of initialized bytes in the canonical ELF.
    pub file_offset: usize,
    /// Number of initialized bytes to copy.
    pub file_bytes: usize,
    /// Total memory extent; the tail is zero-filled.
    pub memory_bytes: usize,
    /// Exact child virtual address.
    pub vaddr: u64,
    /// Least mapping rights derived from the ELF program header.
    pub rights: WorkerSegmentRights,
}

const EMPTY_SEGMENT: WorkerLoadSegment = WorkerLoadSegment {
    file_offset: 0,
    file_bytes: 0,
    memory_bytes: 0,
    vaddr: 0,
    rights: WorkerSegmentRights::ReadOnly,
};

/// Complete bounded mapping plan for one admitted Worker image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerImagePlan {
    /// Exact role-bound image identity.
    pub expected: ExpectedWorkerImage,
    /// Entrypoint installed into the child TCB.
    pub entry_vaddr: u64,
    /// Valid prefix of [`Self::segments`].
    pub segment_count: usize,
    /// Bounded W^X segment mappings.
    pub segments: [WorkerLoadSegment; MAX_SEGMENTS],
    /// SHA-256 rechecked by root over the canonical image bytes.
    pub image_sha256: [u8; 32],
}

impl WorkerImagePlan {
    /// Return only initialized mapping records.
    #[must_use]
    pub fn mappings(&self) -> &[WorkerLoadSegment] {
        &self.segments[..self.segment_count]
    }
}

/// Worker image admission or mapping failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerImageError {
    /// Root was compiled without a target-qualified Worker archive identity.
    IdentityNotBound,
    /// The system payload omits the separate archive or its manifest.
    PackageMissing,
    /// Archive, manifest, image, or metadata content differs from its digest.
    DigestMismatch,
    /// The requested binary or role is not in the mandatory role matrix.
    UnknownImage,
    /// A newc archive is malformed, ambiguous, or truncated.
    InvalidArchive,
    /// ELF class, data encoding, type, architecture, or header shape is wrong.
    InvalidElf,
    /// The ELF entrypoint is not the exact executable `_start` address.
    InvalidEntry,
    /// A load segment is invalid, overlapping, out of range, or W+X.
    InvalidSegment,
    /// The retained role/ABI metadata is missing or wrong.
    InvalidMetadata,
    /// A HAL mapper refused a validated mapping.
    MappingFailed,
}

/// HAL boundary used by the generic loader after all bytes have been admitted.
pub trait WorkerImageMapper {
    /// Map/copy one segment with the exact validated rights and zero its tail.
    fn map_segment(
        &mut self,
        segment: WorkerLoadSegment,
        initialized: &[u8],
    ) -> Result<(), WorkerImageError>;
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?,
    ))
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn read_hex(bytes: &[u8]) -> Option<usize> {
    let mut value = 0usize;
    for byte in bytes {
        let digit = match byte {
            b'0'..=b'9' => usize::from(byte - b'0'),
            b'a'..=b'f' => usize::from(byte - b'a' + 10),
            _ => return None,
        };
        value = value.checked_mul(16)?.checked_add(digit)?;
    }
    Some(value)
}

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|aligned| aligned & !3)
}

fn cpio_entry<'a>(archive: &'a [u8], expected: &str) -> Result<&'a [u8], WorkerImageError> {
    const HEADER_BYTES: usize = 110;
    let mut cursor = 0usize;
    let mut found = None;
    while cursor
        .checked_add(HEADER_BYTES)
        .is_some_and(|end| end <= archive.len())
    {
        let header = &archive[cursor..cursor + HEADER_BYTES];
        if &header[..6] != b"070701" {
            return Err(WorkerImageError::InvalidArchive);
        }
        let file_bytes = read_hex(&header[54..62]).ok_or(WorkerImageError::InvalidArchive)?;
        let name_bytes = read_hex(&header[94..102]).ok_or(WorkerImageError::InvalidArchive)?;
        if name_bytes < 2 {
            return Err(WorkerImageError::InvalidArchive);
        }
        let name_start = cursor + HEADER_BYTES;
        let name_end = name_start
            .checked_add(name_bytes)
            .filter(|end| *end <= archive.len())
            .ok_or(WorkerImageError::InvalidArchive)?;
        if archive[name_end - 1] != 0 || archive[name_start..name_end - 1].contains(&0) {
            return Err(WorkerImageError::InvalidArchive);
        }
        let name = core::str::from_utf8(&archive[name_start..name_end - 1])
            .map_err(|_| WorkerImageError::InvalidArchive)?;
        let data_start = align4(name_end).ok_or(WorkerImageError::InvalidArchive)?;
        let data_end = data_start
            .checked_add(file_bytes)
            .filter(|end| *end <= archive.len())
            .ok_or(WorkerImageError::InvalidArchive)?;
        if name == "TRAILER!!!" {
            return found.ok_or(WorkerImageError::PackageMissing);
        }
        let normalized = name.strip_prefix("./").unwrap_or(name);
        if normalized == expected {
            if found.is_some() {
                return Err(WorkerImageError::InvalidArchive);
            }
            found = Some(&archive[data_start..data_end]);
        }
        cursor = align4(data_end).ok_or(WorkerImageError::InvalidArchive)?;
    }
    Err(WorkerImageError::InvalidArchive)
}

fn role_number(role: &str) -> Option<u16> {
    match role {
        "worker-heartbeat" => Some(1),
        "worker-gpu" => Some(2),
        "worker-lora" => Some(3),
        _ => None,
    }
}

fn metadata_file_offset(image: &[u8], plan: &WorkerImagePlan) -> Result<usize, WorkerImageError> {
    for segment in plan.mappings() {
        let file_limit = segment
            .vaddr
            .checked_add(segment.file_bytes as u64)
            .ok_or(WorkerImageError::InvalidMetadata)?;
        if segment.vaddr <= plan.expected.metadata_vaddr
            && plan
                .expected
                .metadata_vaddr
                .checked_add(WORKER_METADATA_BYTES as u64)
                .is_some_and(|end| end <= file_limit)
        {
            if segment.rights != WorkerSegmentRights::ReadOnly {
                return Err(WorkerImageError::InvalidMetadata);
            }
            let relative = usize::try_from(plan.expected.metadata_vaddr - segment.vaddr)
                .map_err(|_| WorkerImageError::InvalidMetadata)?;
            let offset = segment
                .file_offset
                .checked_add(relative)
                .ok_or(WorkerImageError::InvalidMetadata)?;
            if offset
                .checked_add(WORKER_METADATA_BYTES)
                .is_none_or(|end| end > image.len())
            {
                return Err(WorkerImageError::InvalidMetadata);
            }
            return Ok(offset);
        }
    }
    Err(WorkerImageError::InvalidMetadata)
}

fn validate_metadata(image: &[u8], plan: &WorkerImagePlan) -> Result<(), WorkerImageError> {
    let offset = metadata_file_offset(image, plan)?;
    let metadata = &image[offset..offset + WORKER_METADATA_BYTES];
    if hash(metadata) != plan.expected.metadata_sha256
        || read_u32(metadata, 0) != Some(WORKER_METADATA_MAGIC)
        || read_u16(metadata, 4) != Some(WORKER_ABI_VERSION)
        || read_u16(metadata, 6) != Some(WORKER_METADATA_BYTES as u16)
        || read_u16(metadata, 8) != role_number(plan.expected.role)
        || read_u16(metadata, 10) != Some(WORKER_ENTRY_VERSION)
        || read_u32(metadata, 12) != Some(WORKER_METADATA_FLAGS)
        || &metadata[16..22] != b"_start"
        || metadata[22..].iter().any(|byte| *byte != 0)
    {
        return Err(WorkerImageError::InvalidMetadata);
    }
    Ok(())
}

/// Validate canonical ELF bytes against one compiler-bound expected identity.
pub fn plan_canonical_worker_image(
    image: &[u8],
    expected: ExpectedWorkerImage,
) -> Result<WorkerImagePlan, WorkerImageError> {
    if image.len() > MAX_IMAGE_BYTES
        || image.len() as u64 != expected.image_bytes
        || hash(image) != expected.image_sha256
    {
        return Err(WorkerImageError::DigestMismatch);
    }
    if image.len() < ELF_HEADER_BYTES
        || &image[..4] != b"\x7fELF"
        || image[4] != 2
        || image[5] != 1
        || image[6] != 1
        || !matches!(image[7], 0 | 3)
        || read_u16(image, 16) != Some(ELF_TYPE_EXEC)
        || read_u16(image, 18) != Some(ELF_MACHINE_AARCH64)
        || read_u32(image, 20) != Some(1)
        || read_u16(image, 52) != Some(ELF_HEADER_BYTES as u16)
        || read_u64(image, 40) != Some(0)
        || read_u16(image, 58) != Some(0)
        || read_u16(image, 60) != Some(0)
        || read_u16(image, 62) != Some(0)
    {
        return Err(WorkerImageError::InvalidElf);
    }
    let entry = read_u64(image, 24).ok_or(WorkerImageError::InvalidElf)?;
    let program_offset = usize::try_from(read_u64(image, 32).ok_or(WorkerImageError::InvalidElf)?)
        .map_err(|_| WorkerImageError::InvalidElf)?;
    let program_entry_bytes = read_u16(image, 54).ok_or(WorkerImageError::InvalidElf)? as usize;
    let program_count = read_u16(image, 56).ok_or(WorkerImageError::InvalidElf)? as usize;
    if entry != expected.entry_vaddr
        || program_entry_bytes != ELF_PROGRAM_HEADER_BYTES
        || program_count == 0
        || program_count > MAX_SEGMENTS + 8
    {
        return Err(WorkerImageError::InvalidEntry);
    }
    let table_end = program_offset
        .checked_add(
            program_entry_bytes
                .checked_mul(program_count)
                .ok_or(WorkerImageError::InvalidElf)?,
        )
        .filter(|end| *end <= image.len())
        .ok_or(WorkerImageError::InvalidElf)?;
    let _ = table_end;
    let mut segments = [EMPTY_SEGMENT; MAX_SEGMENTS];
    let mut segment_count = 0usize;
    let mut entry_executable = false;
    for index in 0..program_count {
        let offset = program_offset + index * program_entry_bytes;
        if read_u32(image, offset) != Some(ELF_PROGRAM_LOAD) {
            continue;
        }
        if segment_count == MAX_SEGMENTS {
            return Err(WorkerImageError::InvalidSegment);
        }
        let flags = read_u32(image, offset + 4).ok_or(WorkerImageError::InvalidElf)?;
        let file_offset =
            usize::try_from(read_u64(image, offset + 8).ok_or(WorkerImageError::InvalidElf)?)
                .map_err(|_| WorkerImageError::InvalidSegment)?;
        let vaddr = read_u64(image, offset + 16).ok_or(WorkerImageError::InvalidElf)?;
        let file_bytes =
            usize::try_from(read_u64(image, offset + 32).ok_or(WorkerImageError::InvalidElf)?)
                .map_err(|_| WorkerImageError::InvalidSegment)?;
        let memory_bytes =
            usize::try_from(read_u64(image, offset + 40).ok_or(WorkerImageError::InvalidElf)?)
                .map_err(|_| WorkerImageError::InvalidSegment)?;
        let alignment = read_u64(image, offset + 48).ok_or(WorkerImageError::InvalidElf)?;
        if flags & ELF_FLAG_READ == 0
            || flags & !(ELF_FLAG_READ | ELF_FLAG_WRITE | ELF_FLAG_EXECUTE) != 0
            || flags & ELF_FLAG_WRITE != 0 && flags & ELF_FLAG_EXECUTE != 0
            || file_bytes > memory_bytes
            || file_offset
                .checked_add(file_bytes)
                .is_none_or(|end| end > image.len())
            || alignment < PAGE_BYTES
            || !alignment.is_power_of_two()
            || vaddr.checked_add(memory_bytes as u64).is_none()
        {
            return Err(WorkerImageError::InvalidSegment);
        }
        let rights = match flags {
            ELF_FLAG_READ => WorkerSegmentRights::ReadOnly,
            ELF_FLAGS_READ_EXECUTE => WorkerSegmentRights::ReadExecute,
            ELF_FLAGS_READ_WRITE => WorkerSegmentRights::ReadWrite,
            _ => return Err(WorkerImageError::InvalidSegment),
        };
        if rights == WorkerSegmentRights::ReadExecute
            && vaddr <= entry
            && entry < vaddr + memory_bytes as u64
        {
            entry_executable = true;
        }
        segments[segment_count] = WorkerLoadSegment {
            file_offset,
            file_bytes,
            memory_bytes,
            vaddr,
            rights,
        };
        segment_count += 1;
    }
    if segment_count == 0 || !entry_executable {
        return Err(WorkerImageError::InvalidEntry);
    }
    segments[..segment_count].sort_unstable_by(|left, right| {
        left.vaddr.cmp(&right.vaddr).then_with(|| {
            if left.file_offset < right.file_offset {
                Ordering::Less
            } else if left.file_offset > right.file_offset {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
    });
    for pair in segments[..segment_count].windows(2) {
        let left_end = pair[0]
            .vaddr
            .checked_add(pair[0].memory_bytes as u64)
            .ok_or(WorkerImageError::InvalidSegment)?;
        if left_end > pair[1].vaddr {
            return Err(WorkerImageError::InvalidSegment);
        }
    }
    let load_base = segments[0].vaddr;
    let load_limit = segments[..segment_count]
        .iter()
        .map(|segment| segment.vaddr + segment.memory_bytes as u64)
        .max()
        .ok_or(WorkerImageError::InvalidSegment)?;
    if load_base != expected.load_base_vaddr
        || load_limit != expected.load_limit_vaddr
        || load_limit
            .checked_sub(load_base)
            .is_none_or(|span| span > MAX_LOAD_SPAN)
    {
        return Err(WorkerImageError::InvalidSegment);
    }
    let plan = WorkerImagePlan {
        expected,
        entry_vaddr: entry,
        segment_count,
        segments,
        image_sha256: hash(image),
    };
    validate_metadata(image, &plan)?;
    Ok(plan)
}

fn plan_bound_worker_image<'a>(
    archive: &'a [u8],
    manifest: &[u8],
    name: &str,
) -> Result<(WorkerImagePlan, &'a [u8]), WorkerImageError> {
    if !WORKER_IMAGE_IDENTITY_BOUND {
        return Err(WorkerImageError::IdentityNotBound);
    }
    let expected = WORKER_IMAGE_IDENTITIES
        .iter()
        .find(|identity| identity.name == name)
        .copied()
        .ok_or(WorkerImageError::UnknownImage)?;
    if hash(archive) != WORKER_ARCHIVE_SHA256 || hash(manifest) != WORKER_MANIFEST_SHA256 {
        return Err(WorkerImageError::DigestMismatch);
    }
    let image = cpio_entry(archive, expected.archive_path)?;
    let plan = plan_canonical_worker_image(image, expected)?;
    Ok((plan, image))
}

/// Resolve and admit one Worker image from root's retained target payload.
pub fn plan_embedded_worker_image(
    name: &str,
) -> Result<(WorkerImagePlan, &'static [u8]), WorkerImageError> {
    let archive: &'static [u8] = &generated_identity::EMBEDDED_WORKER_ARCHIVE;
    let manifest: &'static [u8] = &generated_identity::EMBEDDED_WORKER_MANIFEST;
    plan_bound_worker_image(archive, manifest, name)
}

/// Resolve and admit one Worker image from a packaged outer system CPIO.
///
/// This is retained for host/package validation. Target bootstrap uses
/// [`plan_embedded_worker_image`] because BootInfo extra bytes are typed FDT
/// records rather than the QEMU `-initrd` archive.
pub fn plan_packaged_worker_image<'a>(
    system_payload: &'a [u8],
    name: &str,
) -> Result<(WorkerImagePlan, &'a [u8]), WorkerImageError> {
    let archive = cpio_entry(system_payload, WORKER_ARCHIVE_PATH)?;
    let manifest = cpio_entry(system_payload, WORKER_MANIFEST_PATH)?;
    plan_bound_worker_image(archive, manifest, name)
}

/// Execute a validated mapping plan through the least-authority HAL boundary.
pub fn load_worker_image(
    mapper: &mut impl WorkerImageMapper,
    plan: &WorkerImagePlan,
    image: &[u8],
) -> Result<(), WorkerImageError> {
    if hash(image) != plan.image_sha256 {
        return Err(WorkerImageError::DigestMismatch);
    }
    for segment in plan.mappings() {
        let end = segment
            .file_offset
            .checked_add(segment.file_bytes)
            .filter(|end| *end <= image.len())
            .ok_or(WorkerImageError::InvalidSegment)?;
        mapper
            .map_segment(*segment, &image[segment.file_offset..end])
            .map_err(|_| WorkerImageError::MappingFailed)?;
    }
    Ok(())
}
