// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify acyclic MCS fault routing and single-owner Reply-lane transitions.
// Author: Lukas Bower

use root_task::critical_tcb::{
    fault_nbrecv_delivered, mcs_extra_refills, validate_critical_temporal_graph, FaultClass,
    FaultRegistration, FaultReplyLane, FaultReplyLaneError, FaultReplyLaneState,
    GenerationIdentity,
};
use root_task::generated;

fn registration(terminal: bool) -> FaultRegistration {
    FaultRegistration {
        task_index: 1,
        identity: GenerationIdentity {
            slot: 1,
            lease_epoch: 1,
            supervisor_generation: 1,
            cap_generation: 1,
        },
        standard_badge: 0x1001,
        timeout_badge: 0x2001,
        tcb_cap: 0x3001,
        terminal,
    }
}

#[test]
fn empty_nbrecv_ignores_undefined_message_info_and_requires_a_badge() {
    assert!(!fault_nbrecv_delivered(0));
    for badge in [1, 0x26e3_0001, 0x26ee_0001, u64::MAX] {
        assert!(fault_nbrecv_delivered(badge));
    }
}

#[test]
fn terminal_fault_suspends_without_reply_or_early_reuse() {
    let mut lane = FaultReplyLane::default();
    lane.begin(registration(true), FaultClass::Standard)
        .expect("associate");
    assert_eq!(
        lane.reply_recoverable_timeout(true),
        Err(FaultReplyLaneError::ReplyForbidden)
    );
    assert_eq!(
        lane.finish_terminal(false, true),
        Err(FaultReplyLaneError::AssociationNotCleared)
    );
    assert_eq!(
        lane.begin(registration(true), FaultClass::Standard),
        Err(FaultReplyLaneError::Busy)
    );
    lane.finish_terminal(true, true)
        .expect("suspend cleared lane");
    assert_eq!(lane.state(), FaultReplyLaneState::Free);
}

#[test]
fn recoverable_timeout_replies_exactly_once_and_graph_is_acyclic() {
    validate_critical_temporal_graph().expect("acyclic generated graph");
    let root_fault = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == "root-fault")
        .expect("root-fault");
    let root_emergency = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == "root-emergency")
        .expect("root-emergency");
    assert_eq!(root_fault.fault_handler, "root-emergency");
    assert!(root_emergency.fault_handler.is_empty());
    assert_eq!(mcs_extra_refills(root_fault.max_refills), Ok(0));

    let mut lane = FaultReplyLane::default();
    lane.begin(registration(false), FaultClass::Timeout)
        .expect("associate");
    assert_eq!(
        lane.reply_recoverable_timeout(false),
        Err(FaultReplyLaneError::ReplyForbidden)
    );
    lane.reply_recoverable_timeout(true).expect("one reply");
    assert_eq!(
        lane.reply_recoverable_timeout(true),
        Err(FaultReplyLaneError::ReplyAlreadyIssued)
    );
    assert_eq!(
        lane.finish_recoverable(false),
        Err(FaultReplyLaneError::AssociationNotCleared)
    );
    lane.finish_recoverable(true).expect("association clear");
}
