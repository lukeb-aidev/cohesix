// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify compiler-owned NineDoor child isolation and passive recovery authority.
// Author: Lukas Bower

use root_task::critical_tcb::{
    passive_service_recovery_contract, PassiveServiceRecoveryContractError,
};
use root_task::generated::{self, TemporalExecution, TemporalTaskKind, TimeoutPolicy};
use root_task::ninedoor_service::{
    generated_config, NineDoorServiceContract, NineDoorServiceObjectPlan, SERVICE_TASK_ID,
};
use secure9p_transport::{
    NAMESPACE_CHILD_RECEIVE_RIGHTS, NAMESPACE_ROOT_CALL_RIGHTS, SEL4_RIGHTS_READ,
    SEL4_RIGHTS_READ_WRITE,
};

#[test]
fn generated_child_inventory_is_exact_and_allocator_free() {
    let config = generated_config();
    let contract = NineDoorServiceContract::from_generated().expect("NineDoor contract");
    let plan = NineDoorServiceObjectPlan::from_generated().expect("NineDoor object plan");

    assert!(config.enabled);
    assert_eq!(contract.revoke_anchor_slot, 16_137);
    assert_eq!(contract.revoke_anchor_bits, 20);
    assert_eq!(contract.child_cspace_slots, 16);
    assert_eq!(contract.endpoint_slot, 2);
    assert_eq!(contract.reply_slot, 3);
    assert_eq!(contract.root_fault_recovery_reply_slot, 10);
    assert_eq!(contract.objects.tcbs, 1);
    assert_eq!(contract.objects.cnodes, 1);
    assert_eq!(contract.objects.vspaces, 1);
    assert_eq!(contract.objects.page_tables, 8);
    assert_eq!(contract.objects.frames, 49);
    assert_eq!(contract.objects.endpoints, 1);
    assert_eq!(contract.objects.notifications, 0);
    assert_eq!(contract.objects.reply_objects, 1);
    assert_eq!(contract.objects.scheduling_contexts, 1);
    assert_eq!(contract.objects.cspace_slots, 80);
    assert_eq!(contract.bootstrap_scheduling_context_bits, 8);
    assert_eq!(contract.bootstrap_budget_us, 3_000);
    assert_eq!(contract.bootstrap_period_us, 10_000);
    assert_eq!(contract.bootstrap_max_refills, 2);
    assert_eq!(plan.image_pages, 35);
    assert_eq!(contract.stack_pages, 8);
    assert_eq!(contract.shared_frame_bytes, 8_192);
    assert_eq!(
        u32::from(plan.image_pages) + u32::from(contract.stack_pages) + 1 + 1 + 4,
        contract.objects.frames
    );
    assert_eq!(config.root_call_rights, NAMESPACE_ROOT_CALL_RIGHTS);
    assert_eq!(config.child_receive_rights, NAMESPACE_CHILD_RECEIVE_RIGHTS);
    assert_eq!(config.root_request_rights, SEL4_RIGHTS_READ_WRITE);
    assert_eq!(config.root_response_rights, SEL4_RIGHTS_READ);
    assert_eq!(config.child_request_rights, SEL4_RIGHTS_READ);
    assert_eq!(config.child_response_rights, SEL4_RIGHTS_READ_WRITE);
}

#[test]
fn recovery_authority_is_ninedoor_only_and_matches_passive_donation() {
    let temporal = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == SERVICE_TASK_ID)
        .expect("NineDoor temporal task");
    assert_eq!(temporal.kind, TemporalTaskKind::Service);
    assert_eq!(temporal.execution, TemporalExecution::Passive);
    assert_eq!(temporal.timeout_policy, TimeoutPolicy::ReturnError);
    assert_eq!(temporal.allowed_donors, ["root-control"]);
    assert_eq!(temporal.reply_objects, 1);
    assert_eq!(temporal.max_donation_depth, 1);
    assert_eq!(temporal.scheduling_context_slot, 0);
    assert_eq!(temporal.scheduling_context_bits, 0);

    let recovery =
        passive_service_recovery_contract(SERVICE_TASK_ID).expect("passive recovery contract");
    assert_eq!(recovery.root_fault_reply_slot, 10);
    assert_eq!(recovery.mailbox, 0);
    assert_eq!(
        passive_service_recovery_contract("console-network-service"),
        Err(PassiveServiceRecoveryContractError::ActiveOrUnownedService)
    );
}
