// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify acyclic MCS fault routing and single-owner Reply-lane transitions.
// Author: Lukas Bower

use root_task::critical_tcb::{
    mcs_extra_refills, validate_critical_temporal_graph, FaultClass, FaultRegistration,
    FaultReplyLane, FaultReplyLaneError, FaultReplyLaneState, GenerationIdentity,
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
fn target_runtime_blocks_on_one_shared_endpoint_and_one_reply() {
    let source = include_str!("../src/hal/critical_tcb.rs");

    assert!(
        source.contains("sel4::recv_with_reply(CHILD_INBOX_SLOT, &mut badge, CHILD_REPLY_SLOT)")
    );
    assert!(source.contains("let (registration, fault_class) = resolve_target_fault(badge)?;"));
    assert!(source.contains("sel4::wait(CHILD_DRIVER_RELEASE_SLOT, &mut observed_badge)"));
    assert!(source.contains("sel4::signal_unchecked(CHILD_DRIVER_RELEASE_SIGNAL_SLOT)"));
    assert!(!source.contains("nb_recv_with_reply"));
    assert!(!source.contains("CHILD_TIMEOUT_INBOX_SLOT"));
    assert!(!source.contains("CHILD_TIMEOUT_REPLY_SLOT"));
}

#[test]
fn driver_containment_clears_the_fault_association_before_reply_release() {
    let containment_source = include_str!("../src/hal/driver_task.rs");
    let containment_start = containment_source
        .find("pub fn root_driver_supervisor_contain_fault(")
        .expect("MCS driver containment entrypoint");
    let containment = &containment_source[containment_start..];
    let suspend = containment
        .find("crate::sel4::suspend_tcb(")
        .expect("driver TCB suspension");
    let association_clear = containment
        .find("DRIVER_TASK_MCS_CALL_ASSOCIATIONS_CLEAR")
        .expect("driver fault-association clear state");
    let success = containment.find("Ok(())").expect("successful containment");
    assert!(suspend < association_clear);
    assert!(association_clear < success);

    let root_fault_source = include_str!("../src/hal/critical_tcb.rs");
    let supervisor_start = root_fault_source
        .find("extern \"C\" fn root_driver_supervisor_entry")
        .expect("driver supervisor entrypoint");
    let supervisor = &root_fault_source[supervisor_start..];
    let contain = supervisor
        .find("root_driver_supervisor_contain_fault(record)")
        .expect("driver containment call");
    let clear_busy = supervisor
        .find(".compare_exchange(true, false")
        .expect("Reply association busy clear");
    let signal_release = supervisor
        .find("sel4::signal_unchecked(CHILD_DRIVER_RELEASE_SIGNAL_SLOT)")
        .expect("root-fault Reply release signal");
    assert!(contain < clear_busy);
    assert!(clear_busy < signal_release);
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
