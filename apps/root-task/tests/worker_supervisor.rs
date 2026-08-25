// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify transactional Worker construction and generation reuse.
// Author: Lukas Bower

mod support {
    pub mod worker_supervisor_fixture;
}

use root_task::worker_supervisor::{
    WorkerConstructionPhase, WorkerLifecycleState, WorkerSupervisor, WorkerSupervisorError,
    WorkerTerminalReason,
};
use support::worker_supervisor_fixture::{
    completion_for, image_fixture, nonzero_digest, ready, ready_record, Event, FakeBackend,
};
use worker_task_abi::{
    GpuLeaseReceiptRecord, PeftReceiptRecord, ReceiptDigests, WorkerAction, WorkerCompletionStatus,
    WorkerControlRecord, WorkerOutcome, WorkerRole,
};

#[test]
fn all_construction_failures_are_terminal_and_contained() {
    for phase in [
        WorkerConstructionPhase::Allocate,
        WorkerConstructionPhase::Map,
        WorkerConstructionPhase::Configure,
        WorkerConstructionPhase::Admit,
        WorkerConstructionPhase::Resume,
    ] {
        let (image, plan) = image_fixture(WorkerRole::Heartbeat);
        let mut supervisor = WorkerSupervisor::new(FakeBackend {
            fail_phase: Some(phase),
            containment_complete: true,
            ..FakeBackend::default()
        })
        .expect("generated Worker pool");
        assert_eq!(
            supervisor.spawn(WorkerRole::Heartbeat, 0, 1, &plan, &image, 0),
            Err(WorkerSupervisorError::Backend)
        );
        let snapshot = supervisor
            .snapshot(WorkerRole::Heartbeat, 0)
            .expect("snapshot");
        assert_eq!(snapshot.lifecycle, WorkerLifecycleState::Terminal);
        assert_eq!(
            snapshot.terminal_reason,
            Some(WorkerTerminalReason::ConstructionFailure)
        );
        if phase != WorkerConstructionPhase::Allocate {
            assert!(supervisor
                .backend()
                .events
                .contains(&Event::Contain(WorkerTerminalReason::ConstructionFailure)));
        }
    }
}

#[test]
fn ready_requires_exact_identity_and_slot_reuse_advances_generations() {
    let (image, plan) = image_fixture(WorkerRole::Heartbeat);
    let mut supervisor =
        WorkerSupervisor::new(FakeBackend::passing()).expect("generated Worker pool");
    let first = supervisor
        .spawn(WorkerRole::Heartbeat, 0, 1, &plan, &image, 10)
        .expect("first spawn");
    assert_eq!(
        supervisor.spawn(WorkerRole::Heartbeat, 0, 2, &plan, &image, 10),
        Err(WorkerSupervisorError::SlotBusy)
    );
    let init = supervisor.backend().init.expect("init");
    supervisor
        .accept_ready(ready_record(init, 1))
        .expect("exact READY");
    supervisor
        .revoke(WorkerRole::Heartbeat, 0)
        .expect("complete revoke");
    let second = supervisor
        .spawn(WorkerRole::Heartbeat, 0, 2, &plan, &image, 20)
        .expect("fresh generation");
    assert!(second.identity.supervisor_generation > first.identity.supervisor_generation);
    assert!(second.identity.cap_generation > first.identity.cap_generation);
    assert_eq!(
        supervisor.accept_ready(ready_record(init, 2)),
        Err(WorkerSupervisorError::InvalidRecord)
    );
}

#[test]
fn gpu_completion_requires_the_exact_durable_receipt_first() {
    let (mut supervisor, identity) = ready(WorkerRole::Gpu, 1);
    let digests = ReceiptDigests {
        ticket: nonzero_digest(1),
        idempotency: nonzero_digest(2),
        operation: nonzero_digest(3),
        subject: nonzero_digest(4),
        result: nonzero_digest(5),
    };
    let control = WorkerControlRecord::staged(
        1,
        identity,
        WorkerAction::GpuLeaseGrant,
        WorkerOutcome::Confirmed,
        10,
        20,
        digests,
    )
    .committed();
    supervisor
        .submit_control(control)
        .expect("control must be admitted");
    let completion = completion_for(control);
    assert_eq!(
        supervisor.accept_completion(completion),
        Err(WorkerSupervisorError::NoControlPending)
    );

    let mut wrong = GpuLeaseReceiptRecord::staged(control)
        .expect("valid receipt body")
        .committed();
    wrong.digests.result = nonzero_digest(9);
    assert_eq!(
        supervisor.accept_gpu_receipt(wrong),
        Err(WorkerSupervisorError::InvalidRecord)
    );

    let receipt = GpuLeaseReceiptRecord::staged(control)
        .expect("valid receipt body")
        .committed();
    let received = supervisor
        .accept_gpu_receipt(receipt)
        .expect("exact receipt must correlate");
    assert_eq!(received.receipt_sequence, control.sequence);
    assert_eq!(
        supervisor.accept_gpu_receipt(receipt),
        Err(WorkerSupervisorError::InvalidState)
    );
    let completed = supervisor
        .accept_completion(completion)
        .expect("completion follows exact receipt");
    assert_eq!(completed.control_sequence, 0);
    assert_eq!(completed.receipt_sequence, 0);
}

#[test]
fn stale_gpu_and_peft_receipts_complete_without_becoming_rejected() {
    let digests = ReceiptDigests {
        ticket: nonzero_digest(1),
        idempotency: nonzero_digest(2),
        operation: nonzero_digest(3),
        subject: nonzero_digest(4),
        result: nonzero_digest(5),
    };

    let (mut gpu, gpu_identity) = ready(WorkerRole::Gpu, 1);
    let gpu_control = WorkerControlRecord::staged(
        1,
        gpu_identity,
        WorkerAction::GpuLeaseRelease,
        WorkerOutcome::Stale,
        10,
        20,
        digests,
    )
    .committed();
    gpu.submit_control(gpu_control).expect("GPU stale control");
    gpu.accept_gpu_receipt(
        GpuLeaseReceiptRecord::staged(gpu_control)
            .expect("GPU stale receipt")
            .committed(),
    )
    .expect("accept GPU stale receipt");
    let gpu_completion = completion_for(gpu_control);
    assert_eq!(gpu_completion.status, WorkerCompletionStatus::Stale as u16);
    gpu.accept_completion(gpu_completion)
        .expect("accept GPU stale completion");

    let (mut lora, lora_identity) = ready(WorkerRole::Lora, 1);
    let lora_control = WorkerControlRecord::staged(
        1,
        lora_identity,
        WorkerAction::PeftExport,
        WorkerOutcome::Stale,
        10,
        20,
        digests,
    )
    .committed();
    lora.submit_control(lora_control)
        .expect("PEFT stale control");
    lora.accept_peft_receipt(
        PeftReceiptRecord::staged(lora_control)
            .expect("PEFT stale receipt")
            .committed(),
    )
    .expect("accept PEFT stale receipt");
    let lora_completion = completion_for(lora_control);
    assert_eq!(lora_completion.status, WorkerCompletionStatus::Stale as u16);
    lora.accept_completion(lora_completion)
        .expect("accept PEFT stale completion");
}
