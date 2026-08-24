// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify bounded nonblocking Worker-supervisor handoff and fault-first coalescence.
// Author: Lukas Bower

use root_task::critical_tcb::{
    service_fault_mailbox_index, validate_worker_supervisor_wake, worker_fault_mailbox_index,
    CriticalHandoff, FaultClass, FaultHandoffError, FaultHandoffRecord, GenerationIdentity,
    PublishResult, WorkerControlOperation, WorkerControlQueue, WorkerControlRecord,
    WORKER_CONTROL_QUEUE_CAPACITY,
};
use root_task::generated;

fn identity(slot: u16) -> GenerationIdentity {
    GenerationIdentity {
        slot,
        lease_epoch: 1,
        supervisor_generation: 2,
        cap_generation: 3,
    }
}

fn fault(worker_ordinal: u16, sequence: u64) -> FaultHandoffRecord {
    let task_index = worker_task_index(worker_ordinal);
    FaultHandoffRecord {
        sequence,
        task_index,
        identity: identity(0),
        fault_badge: 0x26e1_0000 + u64::from(worker_ordinal),
        fault_class: FaultClass::Standard,
        fault_label: 1,
        fault_length: 2,
        fault_mr0: 0,
        fault_mr1: 0,
        tcb_cap: 0x100 + usize::from(worker_ordinal),
    }
}

fn worker_task_index(worker_ordinal: u16) -> u16 {
    generated::temporal_tasks()
        .iter()
        .enumerate()
        .filter(|(_, task)| task.kind == generated::TemporalTaskKind::Worker)
        .nth(usize::from(worker_ordinal))
        .and_then(|(index, _)| u16::try_from(index).ok())
        .expect("generated Worker temporal task")
}

fn service_fault(task_index: u16, sequence: u64) -> FaultHandoffRecord {
    FaultHandoffRecord {
        sequence,
        task_index,
        identity: identity(0),
        fault_badge: 0x26e2_0000 + u64::from(task_index),
        fault_class: FaultClass::Standard,
        fault_label: 1,
        fault_length: 2,
        fault_mr0: 0,
        fault_mr1: 0,
        tcb_cap: 0x200 + usize::from(task_index),
    }
}

#[test]
fn role_local_worker_slots_map_to_distinct_temporal_mailboxes() {
    let records = [fault(0, 1), fault(1, 2), fault(2, 3)];
    let task_indices = generated::temporal_tasks()
        .iter()
        .enumerate()
        .filter(|(_, task)| task.kind == generated::TemporalTaskKind::Worker)
        .take(records.len())
        .map(|(index, _)| u16::try_from(index).expect("bounded temporal index"))
        .collect::<heapless::Vec<_, 3>>();
    assert_eq!(task_indices.len(), 3);
    assert!(records.iter().all(|record| record.identity.slot == 0));
    for (mailbox, task_index) in task_indices.into_iter().enumerate() {
        assert_eq!(worker_fault_mailbox_index(task_index), Some(mailbox));
        assert_eq!(records[mailbox].task_index, task_index);
    }
}

#[test]
fn simultaneous_faults_are_durable_and_precede_policy() {
    CriticalHandoff::validate_generated_contract().expect("generated handoff");
    let mut handoff = CriticalHandoff::default();
    let controls = WorkerControlQueue::new();
    assert_eq!(
        controls.publish(WorkerControlRecord {
            sequence: 1,
            task_index: worker_task_index(0),
            identity: identity(0),
            operation: WorkerControlOperation::Admit,
        }),
        PublishResult::Published
    );
    for slot in 0..3 {
        handoff
            .publish_worker_fault(fault(slot, 10 + u64::from(slot)))
            .expect("one durable mailbox per executable slot");
    }
    for mailbox in 0..3usize {
        assert!(matches!(
            handoff.drain_worker_fault(),
            Some(record)
                if record.identity.slot == 0
                    && worker_fault_mailbox_index(record.task_index) == Some(mailbox)
        ));
    }
    assert!(matches!(
        controls.validate_next().expect("critical validation"),
        Some(record) if record.sequence == 1
    ));
    assert!(matches!(
        controls.drain_validated().expect("root consume"),
        Some(record) if record.sequence == 1
    ));
}

#[test]
fn critical_validation_exposes_policy_without_releasing_capacity() {
    let controls = WorkerControlQueue::new();
    let make_record = |sequence| WorkerControlRecord {
        sequence,
        task_index: worker_task_index(0),
        identity: identity(0),
        operation: WorkerControlOperation::Admit,
    };

    assert_eq!(controls.publish(make_record(1)), PublishResult::Published);
    assert_eq!(controls.len(), 1);
    assert_eq!(controls.unvalidated_len(), 1);
    assert_eq!(controls.validated_len(), 0);
    assert_eq!(controls.validate_next(), Ok(Some(make_record(1))));
    assert_eq!(controls.len(), 1);
    assert_eq!(controls.unvalidated_len(), 0);
    assert_eq!(controls.validated_len(), 1);

    for sequence in 2..=WORKER_CONTROL_QUEUE_CAPACITY as u64 {
        assert_eq!(
            controls.publish(make_record(sequence)),
            PublishResult::Published
        );
    }
    assert_eq!(
        controls.publish(make_record(WORKER_CONTROL_QUEUE_CAPACITY as u64 + 1)),
        PublishResult::Refused,
        "critical validation must not release root-owned queue capacity",
    );
    assert_eq!(controls.drain_validated(), Ok(Some(make_record(1))));
    assert_eq!(
        controls.publish(make_record(WORKER_CONTROL_QUEUE_CAPACITY as u64 + 1)),
        PublishResult::Published,
    );
}

#[test]
fn policy_saturation_refuses_but_fault_overwrite_is_fatal() {
    let mut handoff = CriticalHandoff::default();
    let controls = WorkerControlQueue::new();
    for sequence in 1..=WORKER_CONTROL_QUEUE_CAPACITY as u64 {
        assert_eq!(
            controls.publish(WorkerControlRecord {
                sequence,
                task_index: worker_task_index(0),
                identity: identity(0),
                operation: WorkerControlOperation::Shutdown,
            }),
            PublishResult::Published
        );
    }
    assert_eq!(
        controls.publish(WorkerControlRecord {
            sequence: WORKER_CONTROL_QUEUE_CAPACITY as u64 + 1,
            task_index: worker_task_index(0),
            identity: identity(0),
            operation: WorkerControlOperation::Revoke,
        }),
        PublishResult::Refused
    );
    handoff.publish_worker_fault(fault(0, 100)).expect("first");
    assert_eq!(
        handoff.publish_worker_fault(fault(0, 101)),
        Err(FaultHandoffError::MailboxOccupied)
    );
    assert!(handoff.fatal_fault_handoff());
}

#[test]
fn non_worker_temporal_index_cannot_alias_a_worker_mailbox() {
    let mut handoff = CriticalHandoff::default();
    let mut invalid = fault(0, 1);
    invalid.task_index = 0;
    assert_eq!(
        handoff.publish_worker_fault(invalid),
        Err(FaultHandoffError::SlotOutOfRange)
    );
    assert!(handoff.fatal_fault_handoff());
}

#[test]
fn non_worker_temporal_index_cannot_alias_a_worker_control_record() {
    let controls = WorkerControlQueue::new();
    assert_eq!(
        controls.publish(WorkerControlRecord {
            sequence: 1,
            task_index: 0,
            identity: identity(0),
            operation: WorkerControlOperation::Admit,
        }),
        PublishResult::Refused
    );
    assert_eq!(controls.validate_next().expect("critical validation"), None);
    assert_eq!(controls.drain_validated().expect("root consume"), None);
}

#[test]
fn service_faults_are_durable_and_taken_only_by_exact_owner() {
    let service_indices = generated::temporal_tasks()
        .iter()
        .enumerate()
        .filter(|(_, task)| {
            matches!(
                task.kind,
                generated::TemporalTaskKind::Service | generated::TemporalTaskKind::Drain
            )
        })
        .map(|(index, _)| u16::try_from(index).expect("bounded temporal index"))
        .collect::<heapless::Vec<_, 2>>();
    assert_eq!(service_indices.len(), 2);

    let mut handoff = CriticalHandoff::default();
    for (mailbox, task_index) in service_indices.iter().copied().enumerate() {
        assert_eq!(service_fault_mailbox_index(task_index), Some(mailbox));
        handoff
            .publish_service_fault(service_fault(task_index, mailbox as u64 + 1))
            .expect("distinct service mailbox");
    }

    let second = handoff
        .drain_service(service_indices[1])
        .expect("generated service")
        .expect("second service fault");
    assert_eq!(second.task_index, service_indices[1]);
    let first = handoff
        .drain_service(service_indices[0])
        .expect("generated service")
        .expect("first service fault");
    assert_eq!(first.task_index, service_indices[0]);
    assert!(!handoff.service_pending());
}

#[test]
fn non_service_index_and_service_overwrite_fail_closed() {
    let service_index = generated::temporal_tasks()
        .iter()
        .position(|task| task.kind == generated::TemporalTaskKind::Service)
        .and_then(|index| u16::try_from(index).ok())
        .expect("generated service");
    let mut handoff = CriticalHandoff::default();
    handoff
        .publish_service_fault(service_fault(service_index, 1))
        .expect("first service fault");
    assert_eq!(
        handoff.publish_service_fault(service_fault(service_index, 2)),
        Err(FaultHandoffError::MailboxOccupied)
    );
    assert!(handoff.fatal_fault_handoff());

    let mut invalid = CriticalHandoff::default();
    assert_eq!(
        invalid.publish_service_fault(service_fault(worker_task_index(0), 3)),
        Err(FaultHandoffError::SlotOutOfRange)
    );
    assert!(invalid.fatal_fault_handoff());
}

#[test]
fn coalesced_worker_wake_accepts_only_generated_bits() {
    let handoff = generated::worker_resource_admission_config()
        .handoff
        .worker_wake_badge;
    let abi = generated::worker_runtime_config().task_abi;
    let child_mask = abi.heartbeat_wake_bit | abi.gpu_wake_bit | abi.lora_wake_bit;
    assert_eq!(validate_worker_supervisor_wake(child_mask), Some(false));
    assert_eq!(
        validate_worker_supervisor_wake(handoff | child_mask),
        Some(true)
    );
    assert_eq!(validate_worker_supervisor_wake(0), None);
    let unknown = (handoff | child_mask)
        .checked_next_power_of_two()
        .expect("bounded generated mask");
    assert_eq!(validate_worker_supervisor_wake(unknown), None);
}
