// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide deterministic Worker supervisor backend and ELF fixtures.
// Author: Lukas Bower

#![allow(dead_code)]

use root_task::generated::TemporalTaskConfig;
use root_task::hal::worker_image::{
    plan_canonical_worker_image, ExpectedWorkerImage, WorkerImagePlan,
};
use root_task::worker_supervisor::{
    WorkerChildContract, WorkerConstructionPhase, WorkerContainmentProof, WorkerKernelBackend,
    WorkerResumeDisposition, WorkerSupervisor, WorkerSupervisorError, WorkerTerminalReason,
};
use sha2::{Digest, Sha256};
use worker_task_abi::{
    Digest32, WorkerCompletionRecord, WorkerControlRecord, WorkerIdentity, WorkerReadyRecord,
    WorkerRole, WorkerRuntimeInit,
};

const METADATA_OFFSET: usize = 0x180;
const TEXT_OFFSET: usize = 0x1000;
const ENTRY: u64 = 0x20_1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    Allocate,
    Map,
    Configure,
    Admit,
    Resume,
    Control(u64),
    Signal(u64),
    Contain(WorkerTerminalReason),
}

#[derive(Default)]
pub struct FakeBackend {
    pub events: Vec<Event>,
    pub fail_phase: Option<WorkerConstructionPhase>,
    pub containment_complete: bool,
    pub defer_resume: bool,
    pub init: Option<WorkerRuntimeInit>,
}

impl FakeBackend {
    pub fn passing() -> Self {
        Self {
            containment_complete: true,
            ..Self::default()
        }
    }

    fn fail(&self, phase: WorkerConstructionPhase) -> bool {
        self.fail_phase == Some(phase)
    }
}

impl WorkerKernelBackend for FakeBackend {
    type Bundle = u64;

    fn allocate(
        &mut self,
        _identity: WorkerIdentity,
        _contract: WorkerChildContract,
    ) -> Result<Self::Bundle, WorkerSupervisorError> {
        self.events.push(Event::Allocate);
        if self.fail(WorkerConstructionPhase::Allocate) {
            return Err(WorkerSupervisorError::Backend);
        }
        Ok(7)
    }

    fn map_image(
        &mut self,
        _bundle: Self::Bundle,
        _plan: &WorkerImagePlan,
        _image: &[u8],
    ) -> Result<(), WorkerSupervisorError> {
        self.events.push(Event::Map);
        if self.fail(WorkerConstructionPhase::Map) {
            Err(WorkerSupervisorError::Backend)
        } else {
            Ok(())
        }
    }

    fn configure(
        &mut self,
        _bundle: Self::Bundle,
        init: WorkerRuntimeInit,
        _entry_vaddr: u64,
    ) -> Result<(), WorkerSupervisorError> {
        self.events.push(Event::Configure);
        if self.fail(WorkerConstructionPhase::Configure) {
            Err(WorkerSupervisorError::Backend)
        } else {
            self.init = Some(init);
            Ok(())
        }
    }

    fn admit(
        &mut self,
        _bundle: Self::Bundle,
        _temporal: TemporalTaskConfig,
    ) -> Result<(), WorkerSupervisorError> {
        self.events.push(Event::Admit);
        if self.fail(WorkerConstructionPhase::Admit) {
            Err(WorkerSupervisorError::Backend)
        } else {
            Ok(())
        }
    }

    fn resume(
        &mut self,
        _bundle: Self::Bundle,
    ) -> Result<WorkerResumeDisposition, WorkerSupervisorError> {
        self.events.push(Event::Resume);
        if self.fail(WorkerConstructionPhase::Resume) {
            Err(WorkerSupervisorError::Backend)
        } else if self.defer_resume {
            Ok(WorkerResumeDisposition::Deferred)
        } else {
            Ok(WorkerResumeDisposition::Running)
        }
    }

    fn publish_control(
        &mut self,
        _bundle: Self::Bundle,
        control: WorkerControlRecord,
        control_bit: u64,
    ) -> Result<(), WorkerSupervisorError> {
        self.events.push(Event::Control(control.sequence));
        self.events.push(Event::Signal(control_bit));
        Ok(())
    }

    fn signal_lifecycle(
        &mut self,
        _bundle: Self::Bundle,
        badge: u64,
    ) -> Result<(), WorkerSupervisorError> {
        self.events.push(Event::Signal(badge));
        Ok(())
    }

    fn contain(
        &mut self,
        _bundle: Self::Bundle,
        _identity: WorkerIdentity,
        reason: WorkerTerminalReason,
    ) -> Result<WorkerContainmentProof, WorkerSupervisorError> {
        self.events.push(Event::Contain(reason));
        Ok(WorkerContainmentProof {
            tcb_suspended: self.containment_complete,
            records_cleared: self.containment_complete,
            scheduling_context_unbound: self.containment_complete,
            mappings_scrubbed: self.containment_complete,
            descendants_revoked: self.containment_complete,
            objects_deleted: self.containment_complete,
            generation_fenced: self.containment_complete,
        })
    }
}

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

pub fn image_fixture(role: WorkerRole) -> (Vec<u8>, WorkerImagePlan) {
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
    put_u32(&mut image, METADATA_OFFSET, 0x574b_4d31);
    put_u16(&mut image, METADATA_OFFSET + 4, 1);
    put_u16(&mut image, METADATA_OFFSET + 6, 64);
    put_u16(&mut image, METADATA_OFFSET + 8, role as u16);
    put_u16(&mut image, METADATA_OFFSET + 10, 1);
    put_u32(&mut image, METADATA_OFFSET + 12, 3);
    image[METADATA_OFFSET + 16..METADATA_OFFSET + 22].copy_from_slice(b"_start");
    image[TEXT_OFFSET..].copy_from_slice(&[0xc0, 0x03, 0x5f, 0xd6]);
    let (name, label) = match role {
        WorkerRole::Heartbeat => ("worker-heart", "worker-heartbeat"),
        WorkerRole::Gpu => ("worker-gpu", "worker-gpu"),
        WorkerRole::Lora => ("worker-lora", "worker-lora"),
    };
    let expected = ExpectedWorkerImage {
        name,
        role: label,
        archive_path: name,
        image_sha256: digest(&image),
        image_bytes: image.len() as u64,
        entry_vaddr: ENTRY,
        load_base_vaddr: 0x20_0000,
        load_limit_vaddr: ENTRY + 4,
        metadata_vaddr: 0x20_0000 + METADATA_OFFSET as u64,
        metadata_sha256: digest(&image[METADATA_OFFSET..METADATA_OFFSET + 64]),
    };
    let plan = plan_canonical_worker_image(&image, expected).expect("fixture plan");
    (image, plan)
}

pub fn starting(role: WorkerRole, lease_epoch: u64) -> (WorkerSupervisor<FakeBackend>, Vec<u8>) {
    let (image, plan) = image_fixture(role);
    let mut supervisor =
        WorkerSupervisor::new(FakeBackend::passing()).expect("generated Worker pool");
    supervisor
        .spawn(role, 0, lease_epoch, &plan, &image, 100)
        .expect("spawn must construct");
    (supervisor, image)
}

pub fn ready(
    role: WorkerRole,
    lease_epoch: u64,
) -> (WorkerSupervisor<FakeBackend>, WorkerIdentity) {
    let (mut supervisor, _image) = starting(role, lease_epoch);
    let init = supervisor.backend().init.expect("configured init");
    let record = WorkerReadyRecord::staged(init, 1).committed();
    let snapshot = supervisor
        .accept_ready(record)
        .expect("READY must validate");
    (supervisor, snapshot.identity.expect("READY identity"))
}

pub fn nonzero_digest(byte: u8) -> Digest32 {
    Digest32::new([byte; 32])
}

pub fn ready_record(init: WorkerRuntimeInit, sequence: u64) -> WorkerReadyRecord {
    WorkerReadyRecord::staged(init, sequence).committed()
}

pub fn completion_for(control: WorkerControlRecord) -> WorkerCompletionRecord {
    WorkerCompletionRecord::staged_for_control(control).committed()
}
