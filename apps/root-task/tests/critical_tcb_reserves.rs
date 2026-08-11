// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify the five critical root duties own distinct generated temporal reserves.
// Author: Lukas Bower

use root_task::critical_tcb::{
    validate_critical_temporal_graph, CriticalTcbHandle, CriticalTcbInventory, CriticalTcbOrigin,
    CriticalTopologyError, CRITICAL_TCB_COUNT,
};
use root_task::generated;

#[test]
fn bootstrap_slot_cursor_advances_before_critical_object_allocation() {
    let source = include_str!("../src/kernel.rs");
    let consume = source
        .find("hal.consume_bootstrap_slots(consumed_slots)")
        .expect("bootstrap slot consumption must remain explicit");
    let critical = source
        .find("construct_critical_tcb_runtime(")
        .expect("critical MCS construction must remain explicit");

    assert!(
        consume < critical,
        "HAL slot cursor must advance before critical MCS objects allocate",
    );
    assert_eq!(
        source
            .match_indices("hal.consume_bootstrap_slots(consumed_slots)")
            .count(),
        1,
        "bootstrap slots must be consumed exactly once",
    );
}

#[test]
fn bootstrap_shared_untyped_is_quarantined_before_mcs_allocation() {
    let source = include_str!("../src/kernel.rs");
    let record_start = source
        .find("kernel_env.record_untyped_bytes(")
        .expect("bootstrap untyped handoff must remain explicit");
    let record = &source[record_start..record_start + 180];

    assert!(record.contains("notification_selection.index"));
    assert!(record.contains("notification_selection.capacity_bytes()"));
    assert!(!record.contains("notification_selection.used_bytes"));
}

#[test]
fn all_five_named_duties_finish_with_distinct_kernel_objects() {
    validate_critical_temporal_graph().expect("critical temporal graph");
    let resources = generated::worker_resource_admission_config().critical_tcbs;
    assert_eq!(resources.len(), CRITICAL_TCB_COUNT);
    let mut inventory = CriticalTcbInventory::default();
    for (index, resource) in resources.iter().enumerate() {
        let task = generated::temporal_tasks()
            .iter()
            .find(|task| task.id == resource.id)
            .expect("temporal duty");
        let base = 0x1000 + index * 0x20;
        inventory
            .register(CriticalTcbHandle {
                id: resource.id,
                origin: if resource.id == "root-control" {
                    CriticalTcbOrigin::InitRootControl
                } else {
                    CriticalTcbOrigin::RestrictedChild
                },
                tcb_cap: base + 1,
                cnode_cap: base + 2,
                sched_context_cap: base + 3,
                sched_control_cap: base + 4,
                fault_endpoint_cap: base + 5,
                timeout_endpoint_cap: base + 6,
                reply_cap: base + 7,
                wake_notification_cap: base + 8,
                revoke_anchor_cap: base + 9,
                core: task.core,
            })
            .expect("complete distinct reserve");
    }
    let handles = inventory.finish().expect("five-duty inventory");
    assert_eq!(handles.len(), CRITICAL_TCB_COUNT);
    assert!(handles
        .iter()
        .all(|handle| handle.sched_context_cap != handle.sched_control_cap));
}

#[test]
fn partial_inventory_is_fatal() {
    let task = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == "root-control")
        .expect("root-control");
    let handle = CriticalTcbHandle {
        id: "root-control",
        origin: CriticalTcbOrigin::InitRootControl,
        tcb_cap: 1,
        cnode_cap: 2,
        sched_context_cap: 3,
        sched_control_cap: 4,
        fault_endpoint_cap: 5,
        timeout_endpoint_cap: 6,
        reply_cap: 7,
        wake_notification_cap: 8,
        revoke_anchor_cap: 9,
        core: task.core,
    };
    let mut inventory = CriticalTcbInventory::default();
    inventory.register(handle).expect("first");
    assert_eq!(inventory.finish(), Err(CriticalTopologyError::Incomplete));
}

#[test]
fn duplicate_sched_context_is_fatal() {
    let root_control = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == "root-control")
        .expect("root-control");
    let root_fault = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == "root-fault")
        .expect("root-fault");
    let mut inventory = CriticalTcbInventory::default();
    inventory
        .register(CriticalTcbHandle {
            id: "root-control",
            origin: CriticalTcbOrigin::InitRootControl,
            tcb_cap: 1,
            cnode_cap: 2,
            sched_context_cap: 3,
            sched_control_cap: 4,
            fault_endpoint_cap: 5,
            timeout_endpoint_cap: 6,
            reply_cap: 7,
            wake_notification_cap: 8,
            revoke_anchor_cap: 9,
            core: root_control.core,
        })
        .expect("root-control");
    assert_eq!(
        inventory.register(CriticalTcbHandle {
            id: "root-fault",
            origin: CriticalTcbOrigin::RestrictedChild,
            tcb_cap: 11,
            cnode_cap: 12,
            sched_context_cap: 3,
            sched_control_cap: 14,
            fault_endpoint_cap: 15,
            timeout_endpoint_cap: 16,
            reply_cap: 17,
            wake_notification_cap: 18,
            revoke_anchor_cap: 19,
            core: root_fault.core,
        }),
        Err(CriticalTopologyError::DuplicateKernelObject)
    );
}

#[test]
fn phantom_child_root_control_is_rejected() {
    let mut inventory = CriticalTcbInventory::default();
    assert_eq!(
        inventory.register(CriticalTcbHandle {
            id: "root-control",
            origin: CriticalTcbOrigin::RestrictedChild,
            tcb_cap: 1,
            cnode_cap: 2,
            sched_context_cap: 3,
            sched_control_cap: 4,
            fault_endpoint_cap: 5,
            timeout_endpoint_cap: 6,
            reply_cap: 7,
            wake_notification_cap: 8,
            revoke_anchor_cap: 9,
            core: 0,
        }),
        Err(CriticalTopologyError::TemporalMismatch)
    );
}
