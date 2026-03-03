// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Apply manifest-driven SMP affinity hints for root-task TCBs.
// Author: Lukas Bower
#![allow(dead_code)]

use core::fmt;

use crate::generated;

#[cfg(feature = "kernel")]
use crate::sel4::seL4_CPtr;
#[cfg(feature = "kernel")]
use crate::sel4::BootInfoExt;
#[cfg(feature = "kernel")]
use sel4_sys;

/// Logical affinity roles mapped to manifest core pools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AffinityRole {
    Authority,
    NineDoor,
    Provider,
    Worker,
}

impl AffinityRole {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Authority => "authority",
            Self::NineDoor => "ninedoor",
            Self::Provider => "provider",
            Self::Worker => "worker",
        }
    }
}

/// Errors surfaced when validating or applying affinity hints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AffinityError {
    NodesMismatch {
        expected: u8,
        observed: u8,
    },
    InvalidCore {
        role: AffinityRole,
        core: u8,
        max_cores: u8,
    },
    Syscall {
        role: AffinityRole,
        core: u8,
        err: i32,
    },
}

impl fmt::Display for AffinityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NodesMismatch { expected, observed } => write!(
                f,
                "affinity max_cores {} does not match kernel nodes {}",
                expected, observed
            ),
            Self::InvalidCore {
                role,
                core,
                max_cores,
            } => write!(
                f,
                "affinity core {} for role {} exceeds max_cores {}",
                core,
                role.label(),
                max_cores
            ),
            Self::Syscall { role, core, err } => write!(
                f,
                "affinity syscall failed role={} core={} err={}",
                role.label(),
                core,
                err
            ),
        }
    }
}

pub fn policy() -> generated::AffinityPolicy {
    generated::affinity_policy()
}

pub fn validate_policy(
    policy: &generated::AffinityPolicy,
    observed_nodes: u8,
) -> Result<(), AffinityError> {
    if !policy.enabled {
        return Ok(());
    }
    if policy.max_cores != observed_nodes {
        return Err(AffinityError::NodesMismatch {
            expected: policy.max_cores,
            observed: observed_nodes,
        });
    }
    if let Some(core) = policy.authority_core {
        if core >= policy.max_cores {
            return Err(AffinityError::InvalidCore {
                role: AffinityRole::Authority,
                core,
                max_cores: policy.max_cores,
            });
        }
    }
    for &core in policy.ninedoor_cores {
        if core >= policy.max_cores {
            return Err(AffinityError::InvalidCore {
                role: AffinityRole::NineDoor,
                core,
                max_cores: policy.max_cores,
            });
        }
    }
    for &core in policy.provider_cores {
        if core >= policy.max_cores {
            return Err(AffinityError::InvalidCore {
                role: AffinityRole::Provider,
                core,
                max_cores: policy.max_cores,
            });
        }
    }
    for &core in policy.worker_cores {
        if core >= policy.max_cores {
            return Err(AffinityError::InvalidCore {
                role: AffinityRole::Worker,
                core,
                max_cores: policy.max_cores,
            });
        }
    }
    Ok(())
}

pub fn select_core(
    policy: &generated::AffinityPolicy,
    role: AffinityRole,
    index: usize,
) -> Option<u8> {
    if !policy.enabled {
        return None;
    }
    match role {
        AffinityRole::Authority => policy.authority_core,
        AffinityRole::NineDoor => pick_core(policy.ninedoor_cores, index),
        AffinityRole::Provider => pick_core(policy.provider_cores, index),
        AffinityRole::Worker => pick_core(policy.worker_cores, index),
    }
}

fn pick_core(cores: &[u8], index: usize) -> Option<u8> {
    if cores.is_empty() {
        None
    } else {
        Some(cores[index % cores.len()])
    }
}

#[cfg(feature = "kernel")]
fn apply_role_affinity(
    tcb: seL4_CPtr,
    role: AffinityRole,
    index: usize,
    policy: &generated::AffinityPolicy,
) -> Option<u8> {
    if !policy.enabled {
        return None;
    }
    let Some(core) = select_core(policy, role, index) else {
        return None;
    };
    if core >= policy.max_cores {
        ::log::error!(
            "[affinity] role={} index={} core={} exceeds max_cores={}",
            role.label(),
            index,
            core,
            policy.max_cores
        );
        return None;
    }
    if let Err(err) = crate::sel4::set_tcb_affinity(tcb, core) {
        ::log::error!(
            "[affinity] role={} index={} core={} apply failed err={}",
            role.label(),
            index,
            core,
            err
        );
        return None;
    }
    ::log::info!(
        "[affinity] role={} index={} core={} applied",
        role.label(),
        index,
        core
    );
    Some(core)
}

#[cfg(feature = "kernel")]
pub fn with_role_affinity<T>(
    role: AffinityRole,
    index: usize,
    policy: &generated::AffinityPolicy,
    f: impl FnOnce() -> T,
) -> T {
    let tcb = sel4_sys::seL4_CapInitThreadTCB;
    let applied = apply_role_affinity(tcb, role, index, policy);
    let result = f();
    if let Some(authority_core) = policy.authority_core {
        if applied.is_some() && Some(authority_core) != applied {
            if let Err(err) = crate::sel4::set_tcb_affinity(tcb, authority_core) {
                ::log::error!(
                    "[affinity] restore authority core={} failed err={}",
                    authority_core,
                    err
                );
            }
        }
    }
    result
}

#[cfg(not(feature = "kernel"))]
pub fn with_role_affinity<T>(
    _role: AffinityRole,
    _index: usize,
    _policy: &generated::AffinityPolicy,
    f: impl FnOnce() -> T,
) -> T {
    f()
}

#[cfg(all(feature = "kernel", sel4_config_debug_build))]
pub fn debug_dump_per_core<F>(policy: &generated::AffinityPolicy, mut emit: F)
where
    F: FnMut(&str),
{
    use heapless::String as HeaplessString;

    let tcb = sel4_sys::seL4_CapInitThreadTCB;
    if !policy.enabled {
        emit("[smp] affinity disabled; dumping scheduler once");
        emit("[smp] note: kernel scheduler/CPU dump text is UART-only");
        crate::sel4::debug_dump_scheduler();
        crate::sel4::debug_dump_cpu_info();
        return;
    }

    emit("[smp] note: kernel scheduler/CPU dump text is UART-only");

    let mut seen = [false; 64];
    let mut probe = |core: u8, label: &'static str| {
        if core as usize >= seen.len() {
            return;
        }
        if seen[core as usize] {
            return;
        }
        seen[core as usize] = true;
        let mut line = HeaplessString::<96>::new();
        let _ = fmt::write(
            &mut line,
            format_args!("[smp] affinity probe role={} core={}", label, core),
        );
        emit(line.as_str());
        if let Err(err) = crate::sel4::set_tcb_affinity_silent(tcb, core) {
            let mut err_line = HeaplessString::<96>::new();
            let _ = fmt::write(
                &mut err_line,
                format_args!("[smp] affinity core={} set failed err={}", core, err),
            );
            emit(err_line.as_str());
            return;
        }
        // Yield to give the scheduler a deterministic window to migrate the TCB.
        for _ in 0..2 {
            crate::sel4::yield_now();
        }
        crate::sel4::debug_dump_scheduler();
        crate::sel4::debug_dump_cpu_info();
    };

    if let Some(core) = policy.authority_core {
        probe(core, "authority");
    }
    for &core in policy.ninedoor_cores {
        probe(core, "ninedoor");
    }
    for &core in policy.provider_cores {
        probe(core, "provider");
    }
    for &core in policy.worker_cores {
        probe(core, "worker");
    }

    if let Some(core) = policy.authority_core {
        let _ = crate::sel4::set_tcb_affinity_silent(tcb, core);
    }
}

#[cfg(not(all(feature = "kernel", sel4_config_debug_build)))]
pub fn debug_dump_per_core<F>(_policy: &generated::AffinityPolicy, mut emit: F)
where
    F: FnMut(&str),
{
    emit("ERR reason=unsupported");
}

#[cfg(feature = "kernel")]
pub fn apply_tcb_affinity(
    tcb: crate::sel4::seL4_CPtr,
    role: AffinityRole,
    index: usize,
    policy: &generated::AffinityPolicy,
) -> Result<Option<u8>, AffinityError> {
    let core = match select_core(policy, role, index) {
        Some(core) => core,
        None => return Ok(None),
    };
    if core >= policy.max_cores {
        return Err(AffinityError::InvalidCore {
            role,
            core,
            max_cores: policy.max_cores,
        });
    }
    if let Err(err) = crate::sel4::set_tcb_affinity(tcb, core) {
        return Err(AffinityError::Syscall {
            role,
            core,
            err: err as i32,
        });
    }
    Ok(Some(core))
}

#[cfg(feature = "kernel")]
pub fn apply_boot_policy(view: &crate::sel4::BootInfoView) -> Result<Option<u8>, AffinityError> {
    let policy = policy();
    if !policy.enabled {
        return Ok(None);
    }
    let observed_nodes = view.header().numNodes as u8;
    validate_policy(&policy, observed_nodes)?;
    let tcb = view.header().init_tcb_cap();
    apply_tcb_affinity(tcb, AffinityRole::Authority, 0, &policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    static NINEDOOR: [u8; 2] = [1, 2];
    static PROVIDER: [u8; 1] = [3];
    static WORKER: [u8; 2] = [2, 3];

    fn sample_policy() -> generated::AffinityPolicy {
        generated::AffinityPolicy {
            enabled: true,
            max_cores: 4,
            authority_core: Some(0),
            ninedoor_cores: &NINEDOOR,
            provider_cores: &PROVIDER,
            worker_cores: &WORKER,
        }
    }

    #[test]
    fn select_core_round_robin() {
        let policy = sample_policy();
        assert_eq!(select_core(&policy, AffinityRole::NineDoor, 0), Some(1));
        assert_eq!(select_core(&policy, AffinityRole::NineDoor, 1), Some(2));
        assert_eq!(select_core(&policy, AffinityRole::NineDoor, 2), Some(1));
        assert_eq!(select_core(&policy, AffinityRole::Provider, 7), Some(3));
        assert_eq!(select_core(&policy, AffinityRole::Worker, 0), Some(2));
        assert_eq!(select_core(&policy, AffinityRole::Worker, 1), Some(3));
        assert_eq!(select_core(&policy, AffinityRole::Worker, 2), Some(2));
    }

    #[test]
    fn validate_policy_requires_node_match() {
        let policy = sample_policy();
        let err = validate_policy(&policy, 2).unwrap_err();
        assert!(matches!(err, AffinityError::NodesMismatch { .. }));
    }
}
