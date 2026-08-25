// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify the generated four-core SMP+MCS scheduling contract as one gate.
// Author: Lukas Bower

use std::collections::{BTreeMap, BTreeSet};

use root_task::critical_tcb::validate_critical_temporal_graph;
use root_task::generated::{self, SchedulerArchitecture, TemporalExecution};

#[test]
fn selected_qemu_schedule_is_four_core_smp_mcs_and_offline_admitted() {
    let authority = generated::temporal_authority_config();
    let resources = generated::worker_resource_admission_config();
    assert!(authority.enabled);
    assert_eq!(authority.architecture, SchedulerArchitecture::SmpMcs);
    assert_eq!(authority.cores, 4);
    assert_eq!(authority.domains, 1);
    assert_eq!(authority.core_admission.len(), usize::from(authority.cores));
    assert_eq!(
        authority.tasks.len(),
        usize::from(resources.fault_registry.capacity)
    );

    let mut admission_by_core = BTreeMap::new();
    for (expected_core, admission) in authority.core_admission.iter().enumerate() {
        assert_eq!(usize::from(admission.core), expected_core);
        assert!(admission.reserve_us > 0);
        assert!(admission.reserve_us < admission.capacity_us);
        assert_eq!(admission.capacity_us, authority.admission_window_us);
        assert!(admission_by_core
            .insert(admission.core, *admission)
            .is_none());
    }

    let mut active_budget_by_core = BTreeMap::<u8, u32>::new();
    let mut scheduling_context_slots = BTreeSet::new();
    let mut timeout_badges = BTreeSet::new();
    let mut active_count = 0usize;
    let mut passive_count = 0usize;
    for task in authority.tasks {
        assert!(task.core < authority.cores);
        assert!(task.priority <= task.mcp);
        assert!(task.timeout_badge != 0);
        assert!(timeout_badges.insert(task.timeout_badge));
        if !task.fault_handler.is_empty() {
            assert_ne!(task.fault_handler, task.id);
            assert!(authority
                .tasks
                .iter()
                .any(|candidate| candidate.id == task.fault_handler));
        }

        match task.execution {
            TemporalExecution::Active => {
                active_count += 1;
                assert!(task.admitted);
                assert!(task.scheduling_context_slot != 0);
                assert!(scheduling_context_slots.insert(task.scheduling_context_slot));
                assert!(task.scheduling_context_bits >= resources.object_bits.sched_context_min);
                assert_eq!(task.sched_control_core, task.core);
                assert!(task.budget_us > 0 && task.budget_us <= task.period_us);
                assert!(task.wcet_us > 0 && task.wcet_us <= task.budget_us);
                assert!(task.response_time_us > 0 && task.response_time_us <= task.deadline_us);
                assert!(task.deadline_us <= task.period_us);
                assert!(task.max_refills >= 2);
                assert!(task.consumed_time_evidence);
                assert!(task.allowed_donors.is_empty());
                assert_eq!(task.reply_objects, 0);
                assert_eq!(task.max_donation_depth, 0);
                let total = active_budget_by_core.entry(task.core).or_default();
                *total = total
                    .checked_add(task.budget_us)
                    .expect("per-core admitted budget does not overflow");
            }
            TemporalExecution::Passive => {
                passive_count += 1;
                assert!(!task.admitted);
                assert_eq!(task.scheduling_context_slot, 0);
                assert_eq!(task.scheduling_context_bits, 0);
                assert_eq!(task.budget_us, 0);
                assert_eq!(task.period_us, 0);
                assert_eq!(task.deadline_us, 0);
                assert!(!task.consumed_time_evidence);
                assert!(!task.allowed_donors.is_empty());
                assert!(task.reply_objects > 0);
                assert!(task.max_donation_depth > 0);
                for donor in task.allowed_donors {
                    let donor = authority
                        .tasks
                        .iter()
                        .find(|candidate| candidate.id == *donor)
                        .expect("passive donor is generated");
                    assert_eq!(donor.execution, TemporalExecution::Active);
                }
            }
        }
    }

    assert_eq!(
        active_count + passive_count,
        usize::from(resources.fault_registry.capacity)
    );
    assert_eq!(
        passive_count,
        usize::from(generated::worker_runtime_config().max_workers) + 1,
        "the passive set is one NineDoor service plus every isolated Worker",
    );
    assert_eq!(
        authority
            .tasks
            .iter()
            .filter(|task| task.id.starts_with("root-worker-executor-"))
            .count(),
        2,
        "the 256 passive Workers share exactly two bounded executor lanes",
    );
    for (core, admission) in admission_by_core {
        let admitted = active_budget_by_core.get(&core).copied().unwrap_or(0);
        assert!(admitted <= admission.capacity_us - admission.reserve_us);
    }
}

#[test]
fn passive_reply_chain_and_critical_fault_graph_are_acyclic() {
    validate_critical_temporal_graph().expect("generated critical MCS graph is acyclic");
    let tasks = generated::temporal_tasks();
    let service = tasks
        .iter()
        .find(|task| task.id == "ninedoor-service")
        .expect("generated passive NineDoor service");
    assert_eq!(service.execution, TemporalExecution::Passive);
    assert_eq!(service.allowed_donors, ["root-control"]);
    assert_eq!(service.reply_objects, 1);
    assert_eq!(service.max_donation_depth, 1);

    let root_fault = tasks
        .iter()
        .find(|task| task.id == "root-fault")
        .expect("generated root-fault duty");
    let root_emergency = tasks
        .iter()
        .find(|task| task.id == "root-emergency")
        .expect("generated root-emergency duty");
    assert_eq!(root_fault.fault_handler, root_emergency.id);
    assert!(root_emergency.fault_handler.is_empty());
}
