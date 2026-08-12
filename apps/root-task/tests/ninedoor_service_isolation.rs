// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify compiler-owned NineDoor child isolation and passive recovery authority.
// Author: Lukas Bower

use root_task::critical_tcb::{
    passive_service_recovery_contract, PassiveServiceRecoveryContractError,
};
use root_task::generated::{self, TemporalExecution, TemporalTaskKind, TimeoutPolicy};
use root_task::ninedoor_service::{
    generated_config, NineDoorContainmentCursor, NineDoorContainmentProof, NineDoorContainmentUnit,
    NineDoorServiceContract, NineDoorServiceObjectPlan, SERVICE_TASK_ID,
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

#[test]
fn containment_cursor_commits_the_exact_passive_teardown_order() {
    let expected = [
        NineDoorContainmentUnit::SuspendTcb,
        NineDoorContainmentUnit::ScrubCleanRequestFrame(0),
        NineDoorContainmentUnit::UnmapRequestFrame(0),
        NineDoorContainmentUnit::ScrubCleanRequestFrame(1),
        NineDoorContainmentUnit::UnmapRequestFrame(1),
        NineDoorContainmentUnit::UnmapResponseRead(0),
        NineDoorContainmentUnit::MapResponseWritable(0),
        NineDoorContainmentUnit::ScrubCleanResponseWritable(0),
        NineDoorContainmentUnit::UnmapResponseWritable(0),
        NineDoorContainmentUnit::UnmapResponseRead(1),
        NineDoorContainmentUnit::MapResponseWritable(1),
        NineDoorContainmentUnit::ScrubCleanResponseWritable(1),
        NineDoorContainmentUnit::UnmapResponseWritable(1),
        NineDoorContainmentUnit::RevokeRecoveryReply,
        NineDoorContainmentUnit::DeleteFaultCap(0),
        NineDoorContainmentUnit::DeleteFaultCap(1),
        NineDoorContainmentUnit::RevokeAnchor,
        NineDoorContainmentUnit::Finalize,
    ];
    let mut cursor = NineDoorContainmentCursor::new();

    for (index, expected_unit) in expected.into_iter().enumerate() {
        assert_eq!(cursor.unit(), expected_unit, "selected unit {index}");
        assert_eq!(cursor.select_next(), expected_unit);
        let expected_successor = expected
            .get(index + 1)
            .copied()
            .unwrap_or(NineDoorContainmentUnit::Complete);
        assert_eq!(
            cursor.unit(),
            expected_successor,
            "successor must be externally visible before unit {index} work",
        );
    }
    assert_eq!(cursor.unit(), NineDoorContainmentUnit::Complete);
}

#[test]
fn containment_cursor_restores_only_the_selected_synchronous_failure() {
    let mut cursor = NineDoorContainmentCursor::new();
    assert_eq!(cursor.select_next(), NineDoorContainmentUnit::SuspendTcb,);
    assert_eq!(
        cursor.unit(),
        NineDoorContainmentUnit::ScrubCleanRequestFrame(0),
    );

    cursor.restore_selected(NineDoorContainmentUnit::SuspendTcb);

    assert_eq!(cursor.unit(), NineDoorContainmentUnit::SuspendTcb);
    assert_eq!(
        cursor.select_next(),
        NineDoorContainmentUnit::SuspendTcb,
        "a typed synchronous failure must retry on a later Recovery turn",
    );
}

#[test]
fn only_a_complete_proof_authorizes_runtime_removal() {
    let incomplete = NineDoorContainmentProof {
        tcb_suspended: true,
        mappings_scrubbed: true,
        recovery_reply_revoked: false,
        capabilities_revoked: true,
        generation_fenced: true,
    };
    let complete = NineDoorContainmentProof {
        recovery_reply_revoked: true,
        ..incomplete
    };

    assert!(!incomplete.complete());
    assert!(complete.complete());
}

#[test]
fn passive_fault_reply_precedes_durable_containment_publication() {
    let source = include_str!("../src/hal/critical_tcb.rs");
    let start = source
        .find("extern \"C\" fn root_fault_entry")
        .expect("root-fault entrypoint");
    let end = source[start..]
        .find("extern \"C\" fn root_emergency_entry")
        .map(|offset| start + offset)
        .expect("bounded root-fault entrypoint");
    let entry = &source[start..end];
    let recover_turn = entry
        .find("RootFaultCriticalTurn::RecoverPassiveService =>")
        .expect("passive donor recovery turn");
    let publish_turn = entry
        .find("RootFaultCriticalTurn::PublishService =>")
        .expect("durable service publication turn");
    let recover = &entry[recover_turn..publish_turn];
    let publish = &entry[publish_turn..];
    let commit_publish = recover
        .find("commit_root_fault_turn(RootFaultCriticalTurn::PublishService);")
        .expect("publish successor before donor recovery");
    let passive_recovery = recover
        .find("recover_target_passive_service_call(pending.record.task_index)")
        .expect("passive donor recovery action");
    let mailbox = publish
        .find("publish_target_service_fault(pending.record)")
        .expect("durable service mailbox publication");

    assert!(recover_turn < publish_turn);
    assert!(commit_publish < passive_recovery);
    assert!(passive_recovery < recover.len() && mailbox < publish.len());
    assert_eq!(
        recover
            .matches("recover_target_passive_service_call(")
            .count(),
        1
    );
    assert_eq!(publish.matches("publish_target_service_fault(").count(), 1);

    let helper_start = source
        .find("fn recover_target_passive_service_call(")
        .expect("passive donor recovery helper");
    let helper_end = source[helper_start..]
        .find("enum FaultReplyDisposition")
        .map(|offset| helper_start + offset)
        .expect("bounded passive donor recovery helper");
    let helper = &source[helper_start..helper_end];
    assert_eq!(helper.matches("sel4::reply_to(").count(), 1);
    assert!(
        helper.find("if sequence == 0").expect("zero-Reply path")
            < helper.find("sel4::reply_to(").expect("one-Reply path")
    );
}

#[test]
fn qemu_local_revoke_seam_runs_only_after_a_completed_donated_call() {
    let source = include_str!("../src/ninedoor.rs");
    let start = source
        .find("fn prepare_namespace<'a>(")
        .expect("NineDoor preparation boundary");
    let end = source[start..]
        .find("/// Reset per-session state")
        .map(|offset| start + offset)
        .expect("bounded NineDoor preparation boundary");
    let prepare = &source[start..end];
    let completed_call = prepare
        .find(".map_err(namespace_transport_error)?;")
        .expect("completed donated Call");
    let post_prepare = prepare
        .find("cohesix_ninedoor_qemu_evidence_post_prepare();")
        .expect("post-Call observation point");
    let consume_request = prepare
        .find("take_ninedoor_qemu_evidence_local_revoke_request()")
        .expect("one-shot local-revoke request");
    let local_revoke = prepare
        .find("self.namespace_service.revoke();")
        .expect("ordinary root-local revoke");

    assert!(completed_call < post_prepare);
    assert!(post_prepare < consume_request);
    assert!(consume_request < local_revoke);

    let setter_start = source
        .find("pub extern \"C\" fn cohesix_ninedoor_qemu_evidence_request_local_revoke()")
        .expect("external QEMU request setter");
    let setter_end = source[setter_start..]
        .find("\n}\n")
        .map(|offset| setter_start + offset)
        .expect("bounded QEMU request setter");
    let setter = &source[setter_start..setter_end];
    assert!(setter.contains(".store(true, Ordering::Release);"));
    assert!(!setter.contains(".revoke("));
    assert!(!source
        .contains("#[no_mangle]\npub extern \"C\" fn cohesix_ninedoor_qemu_evidence_post_prepare"));
    assert!(!source.contains(
        "#[no_mangle]\npub extern \"C\" fn cohesix_ninedoor_qemu_evidence_request_local_revoke"
    ));
}
