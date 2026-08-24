// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify generated executable Worker resources remain distinct from namespace capacity.
// Author: Lukas Bower

use root_task::critical_tcb::GenerationIdentity;
use root_task::generated;
use root_task::hal::resource_pool::{
    ResourcePoolError, SupervisorResourcePool, WORKER_RESOURCE_POOL_CAPACITY,
};

fn identity(slot: u16, generation: u32) -> GenerationIdentity {
    GenerationIdentity {
        slot,
        lease_epoch: 1,
        supervisor_generation: generation,
        cap_generation: generation,
    }
}

fn add_budget(
    left: generated::KernelObjectBudget,
    right: generated::KernelObjectBudget,
) -> generated::KernelObjectBudget {
    generated::KernelObjectBudget {
        tcbs: left.tcbs.checked_add(right.tcbs).expect("TCB total"),
        cnodes: left.cnodes.checked_add(right.cnodes).expect("CNode total"),
        vspaces: left
            .vspaces
            .checked_add(right.vspaces)
            .expect("VSpace total"),
        page_tables: left
            .page_tables
            .checked_add(right.page_tables)
            .expect("page-table total"),
        asids: left.asids.checked_add(right.asids).expect("ASID total"),
        frames: left.frames.checked_add(right.frames).expect("frame total"),
        endpoints: left
            .endpoints
            .checked_add(right.endpoints)
            .expect("endpoint total"),
        notifications: left
            .notifications
            .checked_add(right.notifications)
            .expect("notification total"),
        fault_caps: left
            .fault_caps
            .checked_add(right.fault_caps)
            .expect("fault-cap total"),
        timeout_fault_caps: left
            .timeout_fault_caps
            .checked_add(right.timeout_fault_caps)
            .expect("timeout-fault-cap total"),
        reply_objects: left
            .reply_objects
            .checked_add(right.reply_objects)
            .expect("Reply total"),
        scheduling_contexts: left
            .scheduling_contexts
            .checked_add(right.scheduling_contexts)
            .expect("SC total"),
        cspace_slots: left
            .cspace_slots
            .checked_add(right.cspace_slots)
            .expect("CSpace total"),
        untyped_bytes: left
            .untyped_bytes
            .checked_add(right.untyped_bytes)
            .expect("untyped total"),
    }
}

#[test]
fn maximum_role_mix_reserves_exact_generated_worker_population() {
    let admission = generated::worker_resource_admission_config();
    assert!(admission.enabled);
    assert_eq!(admission.executable_roles.len(), 3);
    let expected_roles = [
        ("worker-heartbeat", 64, 1),
        ("worker-gpu", 64, 15),
        ("worker-lora", 64, 21),
    ];
    for (role, namespace_capacity, executable_slots) in expected_roles {
        let generated = admission
            .executable_roles
            .iter()
            .find(|generated| generated.role == role)
            .expect("generated executable role");
        assert_eq!(generated.namespace_capacity, namespace_capacity);
        assert_eq!(generated.executable_slots, executable_slots);
    }
    let maximum = admission
        .allowed_role_mixes
        .iter()
        .find(|role_mix| role_mix.maximum)
        .expect("one compiler-selected maximum role mix");
    assert_eq!(maximum.roles.len(), expected_roles.len());
    assert_eq!(
        maximum
            .roles
            .iter()
            .map(|role| usize::from(role.count))
            .sum::<usize>(),
        37
    );

    let mut admitted = admission.fixed_objects;
    for mix_role in maximum.roles {
        let role = admission
            .executable_roles
            .iter()
            .find(|role| role.role == mix_role.role)
            .expect("maximum mix role");
        assert_eq!(mix_role.count, role.executable_slots);
        for _ in 0..mix_role.count {
            admitted = add_budget(admitted, role.per_slot);
        }
    }
    let total = add_budget(admitted, admission.post_construction_reserve);
    assert_eq!(
        (
            total.tcbs,
            total.cnodes,
            total.vspaces,
            total.asids,
            total.fault_caps,
            total.timeout_fault_caps,
        ),
        (52, 52, 52, 52, 52, 52)
    );
    assert_eq!(total.page_tables, 616);
    assert_eq!(total.frames, 3_122);
    assert_eq!(total.endpoints, 31);
    assert_eq!(total.notifications, 69);
    assert_eq!(total.reply_objects, 14);
    assert_eq!(total.scheduling_contexts, 52);
    assert_eq!(total.cspace_slots, 8_474);
    assert_eq!(total.untyped_bytes, 105_512_960);
    assert!(total.tcbs <= admission.capacity.tcbs);
    assert!(total.page_tables <= admission.capacity.page_tables);
    assert!(total.frames <= admission.capacity.frames);
    assert!(total.cspace_slots <= admission.capacity.cspace_slots);
    assert!(total.untyped_bytes <= admission.capacity.untyped_bytes);
    assert_eq!(admission.fault_registry.capacity, 44);
    assert_eq!(admission.fault_registry.driver_tcbs, 0);

    let mut pool =
        SupervisorResourcePool::<WORKER_RESOURCE_POOL_CAPACITY>::from_generated().expect("pool");
    let mut generation = 0u32;
    for role in admission.executable_roles {
        for slot in 0..role.executable_slots {
            generation += 1;
            pool.reserve(
                role.role,
                identity(slot, generation),
                0x4000 + generation as usize,
            )
            .expect("complete executable bundle");
        }
    }
    assert_eq!(pool.len(), 37);
    assert_eq!(
        pool.reserve("worker-heartbeat", identity(1, 99), 0x5000),
        Err(ResourcePoolError::InvalidIdentity)
    );
}

#[test]
fn namespace_capacity_never_authorizes_an_extra_kernel_slot() {
    let mut pool =
        SupervisorResourcePool::<WORKER_RESOURCE_POOL_CAPACITY>::from_generated().expect("pool");
    assert_eq!(
        pool.reserve("worker-heartbeat", identity(1, 1), 0x4100),
        Err(ResourcePoolError::InvalidIdentity)
    );
}
