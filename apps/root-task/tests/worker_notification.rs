// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify generated one-hot Worker notification and shutdown behavior.
// Author: Lukas Bower

mod support {
    pub mod worker_supervisor_fixture;
}

use root_task::generated;
use root_task::worker_supervisor::{WorkerLifecycleState, WorkerTerminalReason};
use support::worker_supervisor_fixture::{ready, Event};
use worker_task_abi::{WorkerCompletionRecord, WorkerCompletionStatus, WorkerRole};

#[test]
fn lifecycle_bits_are_disjoint_and_shutdown_is_idempotent() {
    let config = generated::worker_runtime_config().task_abi;
    let bits = [
        config.lifecycle_control_bit,
        config.lifecycle_timeout_bit,
        config.lifecycle_shutdown_bit,
        config.lifecycle_revoke_bit,
        config.heartbeat_wake_bit,
        config.gpu_wake_bit,
        config.lora_wake_bit,
    ];
    let mut combined = 0u64;
    for bit in bits {
        assert!(bit.is_power_of_two());
        assert_eq!(combined & bit, 0);
        combined |= bit;
    }

    let (mut supervisor, identity) = ready(WorkerRole::Heartbeat, 1);
    let first = supervisor
        .begin_shutdown(WorkerRole::Heartbeat, 0, 100)
        .expect("shutdown accepted");
    let second = supervisor
        .begin_shutdown(WorkerRole::Heartbeat, 0, 101)
        .expect("repeat shutdown is idempotent");
    assert_eq!(first.lifecycle, WorkerLifecycleState::Closing);
    assert_eq!(first, second);
    assert!(supervisor
        .backend()
        .events
        .contains(&Event::Signal(config.lifecycle_shutdown_bit)));
    let init = supervisor.backend().init.expect("init");
    let completion =
        WorkerCompletionRecord::staged_terminal(2, identity, WorkerCompletionStatus::Shutdown)
            .committed();
    completion.validate_for(init).expect("terminal ABI record");
    let terminal = supervisor
        .accept_completion(completion)
        .expect("shutdown completion contains child");
    assert_eq!(terminal.lifecycle, WorkerLifecycleState::Terminal);
    assert_eq!(
        terminal.terminal_reason,
        Some(WorkerTerminalReason::Shutdown)
    );
}
