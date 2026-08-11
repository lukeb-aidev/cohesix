// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify every admitted temporal TCB has one exact non-aliasing fault registration.
// Author: Lukas Bower

use root_task::critical_tcb::{
    generated_standard_fault_badge, FaultClass, FaultRegistration, FaultRegistry,
    FaultRegistryError, GenerationIdentity, FAULT_REGISTRY_CAPACITY,
};
use root_task::generated;

fn registration(index: u16) -> FaultRegistration {
    let task = &generated::temporal_tasks()[usize::from(index)];
    FaultRegistration {
        task_index: index,
        identity: GenerationIdentity {
            slot: index,
            lease_epoch: 1,
            supervisor_generation: 1,
            cap_generation: 1,
        },
        standard_badge: generated_standard_fault_badge(task.id).expect("generated standard badge"),
        timeout_badge: task.timeout_badge,
        tcb_cap: 0x3000 + usize::from(index),
        terminal: task.timeout_policy != generated::TimeoutPolicy::ReplenishOnce,
    }
}

#[test]
fn exact_registry_resolves_every_standard_and_timeout_badge() {
    FaultRegistry::validate_generated_capacity().expect("generated registry bound");
    let generated_capacity = usize::from(
        generated::worker_resource_admission_config()
            .fault_registry
            .capacity,
    );
    assert!(generated_capacity <= FAULT_REGISTRY_CAPACITY);
    assert_eq!(generated::temporal_tasks().len(), generated_capacity);
    let mut registry = FaultRegistry::default();
    for index in 0..generated_capacity as u16 {
        registry
            .register(registration(index))
            .expect("unique registration");
    }
    let registry = registry.finish().expect("complete registry");
    for index in 0..generated_capacity as u16 {
        let registration = registration(index);
        assert_eq!(
            registry.resolve(registration.standard_badge),
            Ok((registration, FaultClass::Standard))
        );
        assert_eq!(
            registry.resolve(registration.timeout_badge),
            Ok((registration, FaultClass::Timeout))
        );
    }
}

#[test]
fn duplicate_alias_and_partial_registries_fail_closed() {
    let mut registry = FaultRegistry::default();
    registry.register(registration(0)).expect("first");
    assert_eq!(
        registry.register(registration(0)),
        Err(FaultRegistryError::DuplicateTask)
    );
    assert_eq!(
        registry.register(FaultRegistration {
            task_index: 1,
            identity: registration(1).identity,
            standard_badge: 0x5000,
            timeout_badge: 0x6000,
            tcb_cap: registration(0).tcb_cap,
            terminal: true,
        }),
        Err(FaultRegistryError::DuplicateTcb)
    );
    assert_eq!(
        registry.register(FaultRegistration {
            task_index: 1,
            identity: registration(1).identity,
            standard_badge: registration(0).timeout_badge,
            timeout_badge: registration(1).timeout_badge,
            tcb_cap: 0x5000,
            terminal: true,
        }),
        Err(FaultRegistryError::DuplicateBadge)
    );
    assert!(matches!(
        registry.finish(),
        Err(FaultRegistryError::Incomplete)
    ));

    let mut invalid = FaultRegistry::default();
    assert_eq!(
        invalid.register(FaultRegistration {
            task_index: 0,
            identity: registration(0).identity,
            standard_badge: 1,
            timeout_badge: 1,
            tcb_cap: 2,
            terminal: true,
        }),
        Err(FaultRegistryError::InvalidRegistration)
    );

    let mut wrong_generated_pair = FaultRegistry::default();
    assert_eq!(
        wrong_generated_pair.register(FaultRegistration {
            standard_badge: registration(0).standard_badge + 1,
            ..registration(0)
        }),
        Err(FaultRegistryError::InvalidRegistration)
    );
}

#[test]
fn generated_standard_badges_cover_every_task_without_aliasing() {
    let mut badges = heapless::Vec::<u64, FAULT_REGISTRY_CAPACITY>::new();
    for task in generated::temporal_tasks() {
        let badge = generated_standard_fault_badge(task.id).expect("generated badge");
        assert!(
            !badges.contains(&badge),
            "duplicate standard badge for {}",
            task.id
        );
        badges.push(badge).expect("exact registry capacity");
    }
    assert_eq!(badges.len(), generated::temporal_tasks().len());
}

#[test]
fn sealed_registry_replaces_only_the_exact_newer_generation() {
    let generated_capacity = usize::from(
        generated::worker_resource_admission_config()
            .fault_registry
            .capacity,
    );
    let mut registry = FaultRegistry::default();
    for index in 0..generated_capacity as u16 {
        registry
            .register(registration(index))
            .expect("initial exact registration");
    }
    let mut registry = registry.finish().expect("sealed exact registry");
    let prior = registration(0);
    let replacement = FaultRegistration {
        identity: GenerationIdentity {
            slot: prior.identity.slot,
            lease_epoch: 2,
            supervisor_generation: 2,
            cap_generation: 2,
        },
        tcb_cap: 0x7000,
        ..prior
    };
    registry
        .replace(prior.identity, replacement)
        .expect("strictly newer exact replacement");
    assert_eq!(registry.len(), generated_capacity);
    assert_eq!(
        registry.resolve(prior.standard_badge),
        Ok((replacement, FaultClass::Standard))
    );
    assert_eq!(
        registry.replace(
            prior.identity,
            FaultRegistration {
                identity: GenerationIdentity {
                    supervisor_generation: 3,
                    cap_generation: 3,
                    ..replacement.identity
                },
                tcb_cap: 0x7001,
                ..replacement
            }
        ),
        Err(FaultRegistryError::IdentityMismatch)
    );

    let stale = FaultRegistration {
        identity: GenerationIdentity {
            supervisor_generation: replacement.identity.supervisor_generation,
            cap_generation: replacement.identity.cap_generation + 1,
            ..replacement.identity
        },
        tcb_cap: 0x7001,
        ..replacement
    };
    assert_eq!(
        registry.replace(replacement.identity, stale),
        Err(FaultRegistryError::GenerationNotNewer)
    );

    let wrong_badges = FaultRegistration {
        identity: GenerationIdentity {
            supervisor_generation: replacement.identity.supervisor_generation + 1,
            cap_generation: replacement.identity.cap_generation + 1,
            ..replacement.identity
        },
        standard_badge: 0x9000,
        timeout_badge: 0xa000,
        tcb_cap: 0x7001,
        ..replacement
    };
    assert_eq!(
        registry.replace(replacement.identity, wrong_badges),
        Err(FaultRegistryError::InvalidRegistration)
    );

    let wrong_task = FaultRegistration {
        task_index: 1,
        identity: GenerationIdentity {
            slot: 1,
            lease_epoch: 2,
            supervisor_generation: 2,
            cap_generation: 2,
        },
        standard_badge: registration(1).standard_badge,
        timeout_badge: registration(1).timeout_badge,
        tcb_cap: 0x7002,
        terminal: registration(1).terminal,
    };
    assert_eq!(
        registry.replace(replacement.identity, wrong_task),
        Err(FaultRegistryError::IdentityMismatch)
    );
}

#[test]
fn unsealed_registry_rejects_generation_replacement() {
    let mut registry = FaultRegistry::default();
    let prior = registration(0);
    registry.register(prior).expect("initial registration");
    let replacement = FaultRegistration {
        identity: GenerationIdentity {
            supervisor_generation: 2,
            cap_generation: 2,
            ..prior.identity
        },
        tcb_cap: 0x7000,
        ..prior
    };
    assert_eq!(
        registry.replace(prior.identity, replacement),
        Err(FaultRegistryError::NotSealed)
    );
}
