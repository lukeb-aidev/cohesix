// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify one-in-flight durable Worker control and completion validation.
// Author: Lukas Bower

mod support {
    pub mod worker_supervisor_fixture;
}

use root_task::worker_supervisor::{WorkerLifecycleState, WorkerSupervisorError};
use support::worker_supervisor_fixture::{completion_for, ready};
use worker_task_abi::{
    ReceiptDigests, WorkerAction, WorkerControlRecord, WorkerOutcome, WorkerRole,
};

#[test]
fn one_inflight_control_completes_once_and_stale_completion_fails() {
    let (mut supervisor, identity) = ready(WorkerRole::Heartbeat, 1);
    let control = WorkerControlRecord::staged(
        1,
        identity,
        WorkerAction::HeartbeatPublish,
        WorkerOutcome::NotApplicable,
        100,
        200,
        ReceiptDigests::EMPTY,
    )
    .committed();
    supervisor
        .submit_control(control)
        .expect("first control admitted");
    assert_eq!(
        supervisor.submit_control(control),
        Err(WorkerSupervisorError::ControlBusy)
    );
    let completion = completion_for(control);
    let snapshot = supervisor
        .accept_completion(completion)
        .expect("exact completion");
    assert_eq!(snapshot.lifecycle, WorkerLifecycleState::Ready);
    assert_eq!(snapshot.control_sequence, 0);
    assert_eq!(
        supervisor.submit_control(control),
        Err(WorkerSupervisorError::InvalidRecord)
    );
    assert_eq!(
        supervisor.accept_completion(completion),
        Err(WorkerSupervisorError::NoControlPending)
    );
}

#[test]
fn forged_role_control_is_rejected_before_backend_signal() {
    let (mut supervisor, identity) = ready(WorkerRole::Heartbeat, 1);
    let control = WorkerControlRecord::staged(
        1,
        identity,
        WorkerAction::GpuLeaseGrant,
        WorkerOutcome::Confirmed,
        100,
        200,
        ReceiptDigests::EMPTY,
    )
    .committed();
    assert_eq!(
        supervisor.submit_control(control),
        Err(WorkerSupervisorError::InvalidRecord)
    );
}
