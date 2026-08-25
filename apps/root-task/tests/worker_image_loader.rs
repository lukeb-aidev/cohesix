// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify fail-closed W^X planning and mapping for Worker child images.
// Author: Lukas Bower

use root_task::hal::worker_image::{
    load_worker_image, plan_canonical_worker_image, ExpectedWorkerImage, WorkerImageError,
    WorkerImageMapper, WorkerLoadSegment, WorkerSegmentRights,
};
use sha2::{Digest, Sha256};

const METADATA_OFFSET: usize = 0x180;
const TEXT_OFFSET: usize = 0x1000;
const ENTRY: u64 = 0x20_1000;

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn fixture() -> (Vec<u8>, ExpectedWorkerImage) {
    let mut image = vec![0u8; TEXT_OFFSET + 4];
    image[..8].copy_from_slice(b"\x7fELF\x02\x01\x01\x00");
    put_u16(&mut image, 16, 2);
    put_u16(&mut image, 18, 183);
    put_u32(&mut image, 20, 1);
    put_u64(&mut image, 24, ENTRY);
    put_u64(&mut image, 32, 64);
    put_u16(&mut image, 52, 64);
    put_u16(&mut image, 54, 56);
    put_u16(&mut image, 56, 2);

    put_u32(&mut image, 64, 1);
    put_u32(&mut image, 68, 4);
    put_u64(&mut image, 72, 0);
    put_u64(&mut image, 80, 0x20_0000);
    put_u64(&mut image, 88, 0x20_0000);
    put_u64(&mut image, 96, 0x200);
    put_u64(&mut image, 104, 0x200);
    put_u64(&mut image, 112, 0x1000);

    let second = 64 + 56;
    put_u32(&mut image, second, 1);
    put_u32(&mut image, second + 4, 5);
    put_u64(&mut image, second + 8, TEXT_OFFSET as u64);
    put_u64(&mut image, second + 16, ENTRY);
    put_u64(&mut image, second + 24, ENTRY);
    put_u64(&mut image, second + 32, 4);
    put_u64(&mut image, second + 40, 4);
    put_u64(&mut image, second + 48, 0x1000);

    put_u32(&mut image, METADATA_OFFSET, 0x574b_4d32);
    put_u16(&mut image, METADATA_OFFSET + 4, 2);
    put_u16(&mut image, METADATA_OFFSET + 6, 64);
    put_u16(&mut image, METADATA_OFFSET + 8, 1);
    put_u16(&mut image, METADATA_OFFSET + 10, 2);
    put_u32(&mut image, METADATA_OFFSET + 12, 3);
    image[METADATA_OFFSET + 16..METADATA_OFFSET + 22].copy_from_slice(b"_start");
    image[TEXT_OFFSET..].copy_from_slice(&[0xc0, 0x03, 0x5f, 0xd6]);
    let expected = ExpectedWorkerImage {
        name: "worker-heart",
        role: "worker-heartbeat",
        archive_path: "cohesix/worker/worker-heart",
        image_sha256: digest(&image),
        image_bytes: image.len() as u64,
        entry_vaddr: ENTRY,
        load_base_vaddr: 0x20_0000,
        load_limit_vaddr: ENTRY + 4,
        metadata_vaddr: 0x20_0000 + METADATA_OFFSET as u64,
        metadata_sha256: digest(&image[METADATA_OFFSET..METADATA_OFFSET + 64]),
    };
    (image, expected)
}

#[derive(Default)]
struct RecordingMapper {
    mappings: Vec<(WorkerLoadSegment, Vec<u8>)>,
    fail: bool,
}

impl WorkerImageMapper for RecordingMapper {
    fn map_segment(
        &mut self,
        segment: WorkerLoadSegment,
        initialized: &[u8],
    ) -> Result<(), WorkerImageError> {
        if self.fail {
            return Err(WorkerImageError::MappingFailed);
        }
        self.mappings.push((segment, initialized.to_vec()));
        Ok(())
    }
}

#[test]
fn canonical_image_plans_exact_wx_segments_and_maps_once() {
    let (image, expected) = fixture();
    let plan = plan_canonical_worker_image(&image, expected).expect("valid fixture must plan");
    assert_eq!(plan.entry_vaddr, ENTRY);
    assert_eq!(plan.mappings().len(), 2);
    assert_eq!(plan.mappings()[0].rights, WorkerSegmentRights::ReadOnly);
    assert_eq!(plan.mappings()[1].rights, WorkerSegmentRights::ReadExecute);
    let mut mapper = RecordingMapper::default();
    load_worker_image(&mut mapper, &plan, &image).expect("valid plan must map");
    assert_eq!(mapper.mappings.len(), 2);
    assert_eq!(mapper.mappings[1].1, [0xc0, 0x03, 0x5f, 0xd6]);
}

#[test]
fn writable_executable_and_role_forgery_fail_closed() {
    let (mut image, mut expected) = fixture();
    put_u32(&mut image, 64 + 56 + 4, 7);
    expected.image_sha256 = digest(&image);
    assert_eq!(
        plan_canonical_worker_image(&image, expected),
        Err(WorkerImageError::InvalidSegment)
    );

    let (mut image, mut expected) = fixture();
    put_u16(&mut image, METADATA_OFFSET + 8, 2);
    expected.image_sha256 = digest(&image);
    expected.metadata_sha256 = digest(&image[METADATA_OFFSET..METADATA_OFFSET + 64]);
    assert_eq!(
        plan_canonical_worker_image(&image, expected),
        Err(WorkerImageError::InvalidMetadata)
    );
}

#[test]
fn tampering_and_mapper_failure_do_not_downgrade() {
    let (mut image, expected) = fixture();
    image[TEXT_OFFSET] ^= 1;
    assert_eq!(
        plan_canonical_worker_image(&image, expected),
        Err(WorkerImageError::DigestMismatch)
    );

    let (image, expected) = fixture();
    let plan = plan_canonical_worker_image(&image, expected).expect("fixture must plan");
    let mut mapper = RecordingMapper {
        fail: true,
        ..RecordingMapper::default()
    };
    assert_eq!(
        load_worker_image(&mut mapper, &plan, &image),
        Err(WorkerImageError::MappingFailed)
    );
    assert!(mapper.mappings.is_empty());
}
