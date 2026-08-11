// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify Worker fault, timeout, deadline, and containment semantics.
// Author: Lukas Bower

mod support {
    pub mod worker_supervisor_fixture;
}

use root_task::worker_supervisor::{
    WorkerLifecycleState, WorkerSupervisor, WorkerSupervisorError, WorkerTerminalReason,
};
use support::worker_supervisor_fixture::{image_fixture, ready, starting, FakeBackend};
use worker_task_abi::WorkerRole;

#[test]
fn faults_before_and_after_ready_contain_exact_generation() {
    let (mut starting, _image) = starting(WorkerRole::Gpu, 1);
    let identity = starting
        .snapshot(WorkerRole::Gpu)
        .expect("snapshot")
        .identity
        .expect("identity");
    let terminal = starting.fault(identity, false).expect("fault containment");
    assert_eq!(terminal.lifecycle, WorkerLifecycleState::Terminal);
    assert_eq!(terminal.terminal_reason, Some(WorkerTerminalReason::Fault));

    let (mut ready, identity) = ready(WorkerRole::Lora, 1);
    let terminal = ready.fault(identity, true).expect("timeout containment");
    assert_eq!(
        terminal.terminal_reason,
        Some(WorkerTerminalReason::Timeout)
    );
    let mut stale = identity;
    stale.cap_generation += 1;
    assert_eq!(
        ready.fault(stale, false),
        Err(WorkerSupervisorError::InvalidGeneration)
    );
}

#[test]
fn ready_and_shutdown_deadlines_force_terminal_containment() {
    let (mut supervisor, _image) = starting(WorkerRole::Heartbeat, 1);
    assert_eq!(
        supervisor.enforce_deadlines(5_099).expect("before bound"),
        0
    );
    assert_eq!(supervisor.enforce_deadlines(5_100).expect("at bound"), 1);
    let snapshot = supervisor
        .snapshot(WorkerRole::Heartbeat)
        .expect("snapshot");
    assert_eq!(
        snapshot.terminal_reason,
        Some(WorkerTerminalReason::ReadyTimeout)
    );
}

#[test]
fn preconstructed_ready_deadline_starts_at_actual_admission() {
    let (mut supervisor, _image) = starting(WorkerRole::Heartbeat, 1);
    let receipt = supervisor
        .arm_preconstructed_ready_deadline(WorkerRole::Heartbeat, 10_000)
        .expect("arm deferred READY deadline");
    assert_eq!(receipt.ready_deadline_ms, 15_000);
    assert!(!supervisor
        .enforce_role_deadline(WorkerRole::Heartbeat, 14_999)
        .expect("before refreshed bound"));
    assert!(supervisor
        .enforce_role_deadline(WorkerRole::Heartbeat, 15_000)
        .expect("at refreshed bound"));
}

#[test]
fn incomplete_containment_strands_slot_and_blocks_reuse() {
    let (image, plan) = image_fixture(WorkerRole::Heartbeat);
    let mut supervisor = WorkerSupervisor::new(FakeBackend {
        containment_complete: false,
        ..FakeBackend::default()
    });
    supervisor
        .spawn(WorkerRole::Heartbeat, 1, &plan, &image, 0)
        .expect("construction");
    assert_eq!(
        supervisor.revoke(WorkerRole::Heartbeat),
        Err(WorkerSupervisorError::ContainmentIncomplete)
    );
    assert_eq!(
        supervisor.spawn(WorkerRole::Heartbeat, 2, &plan, &image, 0),
        Err(WorkerSupervisorError::SlotBusy)
    );
}
