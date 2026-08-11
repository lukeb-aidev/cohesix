// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify driver faults use exact durable supervisor records without blocking or drops.
// Author: Lukas Bower

use root_task::critical_tcb::{
    CriticalHandoff, FaultClass, FaultHandoffError, FaultHandoffRecord, GenerationIdentity,
    DRIVER_FAULT_RECORD_CAPACITY,
};
use root_task::generated;

fn fault(slot: u16) -> FaultHandoffRecord {
    FaultHandoffRecord {
        sequence: u64::from(slot) + 1,
        task_index: slot + 10,
        identity: GenerationIdentity {
            slot,
            lease_epoch: 1,
            supervisor_generation: 2,
            cap_generation: 3,
        },
        fault_badge: 0x26e2_0000 + u64::from(slot),
        fault_class: FaultClass::Standard,
        tcb_cap: 0x500 + usize::from(slot),
    }
}

#[test]
fn all_runtime_faults_survive_one_coalesced_wake() {
    let generated_count = generated::worker_resource_admission_config()
        .handoff
        .driver_fault_records;
    assert!(usize::from(generated_count) <= DRIVER_FAULT_RECORD_CAPACITY);
    let mut handoff = CriticalHandoff::default();
    for slot in 0..generated_count {
        handoff
            .publish_driver_fault(slot, fault(slot))
            .expect("exact runtime fault record");
    }
    for slot in 0..generated_count {
        assert_eq!(handoff.drain_driver(), Some(fault(slot)));
    }
    assert_eq!(handoff.drain_driver(), None);
}

#[test]
fn duplicate_or_out_of_range_runtime_fault_is_fatal() {
    let generated_count = generated::worker_resource_admission_config()
        .handoff
        .driver_fault_records;
    let mut handoff = CriticalHandoff::default();
    if generated_count == 0 {
        assert_eq!(
            handoff.publish_driver_fault(0, fault(0)),
            Err(FaultHandoffError::SlotOutOfRange)
        );
        assert!(handoff.fatal_fault_handoff());
    } else {
        handoff.publish_driver_fault(0, fault(0)).expect("first");
        assert_eq!(
            handoff.publish_driver_fault(0, fault(0)),
            Err(FaultHandoffError::MailboxOccupied)
        );
        assert!(handoff.fatal_fault_handoff());
    }

    let mut out_of_range = CriticalHandoff::default();
    assert_eq!(
        out_of_range.publish_driver_fault(generated_count, fault(generated_count)),
        Err(FaultHandoffError::SlotOutOfRange)
    );
    assert!(out_of_range.fatal_fault_handoff());
}
