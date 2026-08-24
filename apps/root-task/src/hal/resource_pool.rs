// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Reserve compiler-bounded child resource bundles and enforce revoke-before-reuse.
// Author: Lukas Bower

//! HAL-owned executable-slot resource ledger.
//!
//! The ledger is intentionally separate from namespace capacity.  It reserves
//! one complete compiler-generated object bundle before construction and does
//! not return it until admission is closed, descendants are revoked, and old
//! mappings are cleared.

use crate::critical_tcb::GenerationIdentity;
use crate::generated::{self, KernelObjectBudget};
use crate::worker_supervisor::MAX_EXECUTABLE_WORKER_SLOTS;

/// Maximum simultaneous Worker bundles in the selected 26e profile.
pub const WORKER_RESOURCE_POOL_CAPACITY: usize = MAX_EXECUTABLE_WORKER_SLOTS;

/// Resource-pool lifecycle for one exact generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceSlotState {
    Reserved,
    AdmissionClosed,
    DescendantsRevoked,
    MappingsCleared,
}

/// One supervisor-owned complete slot reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceReservation {
    pub role: &'static str,
    pub identity: GenerationIdentity,
    pub revoke_anchor_cap: usize,
    pub budget: KernelObjectBudget,
    pub state: ResourceSlotState,
}

/// Fail-closed resource reservation or teardown error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourcePoolError {
    GeneratedCapacityMismatch,
    UnknownRole,
    DuplicateIdentity,
    DuplicateRevokeAnchor,
    InvalidIdentity,
    PoolFull,
    ResourceCapacity,
    UnknownReservation,
    InvalidTransition,
    Arithmetic,
}

/// Fixed HAL resource pool with no allocation after construction.
pub struct SupervisorResourcePool<const N: usize> {
    slots: [Option<ResourceReservation>; N],
    capacity: KernelObjectBudget,
    protected_reserve: KernelObjectBudget,
    used: KernelObjectBudget,
}

impl<const N: usize> SupervisorResourcePool<N> {
    /// Construct from generated capacity, reserve, and fixed-object truth.
    pub fn from_generated() -> Result<Self, ResourcePoolError> {
        let config = generated::worker_resource_admission_config();
        let executable_slots: usize = config
            .executable_roles
            .iter()
            .map(|role| usize::from(role.executable_slots))
            .sum();
        if !config.enabled
            || executable_slots == 0
            || executable_slots > N
            || !fits_after_reserve(
                config.fixed_objects,
                config.capacity,
                config.post_construction_reserve,
            )?
        {
            return Err(ResourcePoolError::GeneratedCapacityMismatch);
        }
        Ok(Self {
            slots: [None; N],
            capacity: config.capacity,
            protected_reserve: config.post_construction_reserve,
            used: config.fixed_objects,
        })
    }

    /// Reserve a complete generated per-role bundle before any object creation.
    pub fn reserve(
        &mut self,
        role: &'static str,
        identity: GenerationIdentity,
        revoke_anchor_cap: usize,
    ) -> Result<ResourceReservation, ResourcePoolError> {
        if identity.supervisor_generation == 0
            || identity.cap_generation == 0
            || revoke_anchor_cap == 0
        {
            return Err(ResourcePoolError::InvalidIdentity);
        }
        if self
            .slots
            .iter()
            .flatten()
            .any(|slot| slot.identity == identity)
        {
            return Err(ResourcePoolError::DuplicateIdentity);
        }
        if self
            .slots
            .iter()
            .flatten()
            .any(|slot| slot.revoke_anchor_cap == revoke_anchor_cap)
        {
            return Err(ResourcePoolError::DuplicateRevokeAnchor);
        }
        let role_config = generated::worker_resource_admission_config()
            .executable_roles
            .iter()
            .find(|entry| entry.role == role)
            .ok_or(ResourcePoolError::UnknownRole)?;
        if usize::from(identity.slot) >= usize::from(role_config.executable_slots) {
            return Err(ResourcePoolError::InvalidIdentity);
        }
        let next_used = checked_add(self.used, role_config.per_slot)?;
        if !fits_after_reserve(next_used, self.capacity, self.protected_reserve)? {
            return Err(ResourcePoolError::ResourceCapacity);
        }
        let reservation = ResourceReservation {
            role,
            identity,
            revoke_anchor_cap,
            budget: role_config.per_slot,
            state: ResourceSlotState::Reserved,
        };
        let target = self
            .slots
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(ResourcePoolError::PoolFull)?;
        *target = Some(reservation);
        self.used = next_used;
        Ok(reservation)
    }

    /// Close new control admission before revoking an old generation.
    pub fn close_admission(
        &mut self,
        identity: GenerationIdentity,
    ) -> Result<(), ResourcePoolError> {
        transition(
            self.lookup_mut(identity)?,
            ResourceSlotState::Reserved,
            ResourceSlotState::AdmissionClosed,
        )
    }

    /// Record successful revoke of every descendant from the retained anchor.
    pub fn mark_descendants_revoked(
        &mut self,
        identity: GenerationIdentity,
    ) -> Result<(), ResourcePoolError> {
        transition(
            self.lookup_mut(identity)?,
            ResourceSlotState::AdmissionClosed,
            ResourceSlotState::DescendantsRevoked,
        )
    }

    /// Record removal of every old-generation mapping/notification association.
    pub fn mark_mappings_cleared(
        &mut self,
        identity: GenerationIdentity,
    ) -> Result<(), ResourcePoolError> {
        transition(
            self.lookup_mut(identity)?,
            ResourceSlotState::DescendantsRevoked,
            ResourceSlotState::MappingsCleared,
        )
    }

    /// Return a bundle only after the full revoke/clear sequence.
    pub fn release(&mut self, identity: GenerationIdentity) -> Result<(), ResourcePoolError> {
        let index = self
            .slots
            .iter()
            .position(|slot| slot.is_some_and(|entry| entry.identity == identity))
            .ok_or(ResourcePoolError::UnknownReservation)?;
        let reservation = self.slots[index].ok_or(ResourcePoolError::UnknownReservation)?;
        if reservation.state != ResourceSlotState::MappingsCleared {
            return Err(ResourcePoolError::InvalidTransition);
        }
        self.used = checked_sub(self.used, reservation.budget)?;
        self.slots[index] = None;
        Ok(())
    }

    /// Current exact reservation for an identity.
    #[must_use]
    pub fn get(&self, identity: GenerationIdentity) -> Option<ResourceReservation> {
        self.slots
            .iter()
            .flatten()
            .find(|entry| entry.identity == identity)
            .copied()
    }

    /// Current object/memory total including fixed critical/service/driver state.
    #[must_use]
    pub const fn used(&self) -> KernelObjectBudget {
        self.used
    }

    /// Number of live supervisor reservations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// Whether no executable-role bundle is reserved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    fn lookup_mut(
        &mut self,
        identity: GenerationIdentity,
    ) -> Result<&mut ResourceReservation, ResourcePoolError> {
        self.slots
            .iter_mut()
            .flatten()
            .find(|entry| entry.identity == identity)
            .ok_or(ResourcePoolError::UnknownReservation)
    }
}

fn transition(
    reservation: &mut ResourceReservation,
    expected: ResourceSlotState,
    next: ResourceSlotState,
) -> Result<(), ResourcePoolError> {
    if reservation.state != expected {
        return Err(ResourcePoolError::InvalidTransition);
    }
    reservation.state = next;
    Ok(())
}

fn checked_add(
    left: KernelObjectBudget,
    right: KernelObjectBudget,
) -> Result<KernelObjectBudget, ResourcePoolError> {
    Ok(KernelObjectBudget {
        tcbs: left
            .tcbs
            .checked_add(right.tcbs)
            .ok_or(ResourcePoolError::Arithmetic)?,
        cnodes: left
            .cnodes
            .checked_add(right.cnodes)
            .ok_or(ResourcePoolError::Arithmetic)?,
        vspaces: left
            .vspaces
            .checked_add(right.vspaces)
            .ok_or(ResourcePoolError::Arithmetic)?,
        page_tables: left
            .page_tables
            .checked_add(right.page_tables)
            .ok_or(ResourcePoolError::Arithmetic)?,
        asids: left
            .asids
            .checked_add(right.asids)
            .ok_or(ResourcePoolError::Arithmetic)?,
        frames: left
            .frames
            .checked_add(right.frames)
            .ok_or(ResourcePoolError::Arithmetic)?,
        endpoints: left
            .endpoints
            .checked_add(right.endpoints)
            .ok_or(ResourcePoolError::Arithmetic)?,
        notifications: left
            .notifications
            .checked_add(right.notifications)
            .ok_or(ResourcePoolError::Arithmetic)?,
        fault_caps: left
            .fault_caps
            .checked_add(right.fault_caps)
            .ok_or(ResourcePoolError::Arithmetic)?,
        timeout_fault_caps: left
            .timeout_fault_caps
            .checked_add(right.timeout_fault_caps)
            .ok_or(ResourcePoolError::Arithmetic)?,
        reply_objects: left
            .reply_objects
            .checked_add(right.reply_objects)
            .ok_or(ResourcePoolError::Arithmetic)?,
        scheduling_contexts: left
            .scheduling_contexts
            .checked_add(right.scheduling_contexts)
            .ok_or(ResourcePoolError::Arithmetic)?,
        cspace_slots: left
            .cspace_slots
            .checked_add(right.cspace_slots)
            .ok_or(ResourcePoolError::Arithmetic)?,
        untyped_bytes: left
            .untyped_bytes
            .checked_add(right.untyped_bytes)
            .ok_or(ResourcePoolError::Arithmetic)?,
    })
}

fn checked_sub(
    left: KernelObjectBudget,
    right: KernelObjectBudget,
) -> Result<KernelObjectBudget, ResourcePoolError> {
    Ok(KernelObjectBudget {
        tcbs: left
            .tcbs
            .checked_sub(right.tcbs)
            .ok_or(ResourcePoolError::Arithmetic)?,
        cnodes: left
            .cnodes
            .checked_sub(right.cnodes)
            .ok_or(ResourcePoolError::Arithmetic)?,
        vspaces: left
            .vspaces
            .checked_sub(right.vspaces)
            .ok_or(ResourcePoolError::Arithmetic)?,
        page_tables: left
            .page_tables
            .checked_sub(right.page_tables)
            .ok_or(ResourcePoolError::Arithmetic)?,
        asids: left
            .asids
            .checked_sub(right.asids)
            .ok_or(ResourcePoolError::Arithmetic)?,
        frames: left
            .frames
            .checked_sub(right.frames)
            .ok_or(ResourcePoolError::Arithmetic)?,
        endpoints: left
            .endpoints
            .checked_sub(right.endpoints)
            .ok_or(ResourcePoolError::Arithmetic)?,
        notifications: left
            .notifications
            .checked_sub(right.notifications)
            .ok_or(ResourcePoolError::Arithmetic)?,
        fault_caps: left
            .fault_caps
            .checked_sub(right.fault_caps)
            .ok_or(ResourcePoolError::Arithmetic)?,
        timeout_fault_caps: left
            .timeout_fault_caps
            .checked_sub(right.timeout_fault_caps)
            .ok_or(ResourcePoolError::Arithmetic)?,
        reply_objects: left
            .reply_objects
            .checked_sub(right.reply_objects)
            .ok_or(ResourcePoolError::Arithmetic)?,
        scheduling_contexts: left
            .scheduling_contexts
            .checked_sub(right.scheduling_contexts)
            .ok_or(ResourcePoolError::Arithmetic)?,
        cspace_slots: left
            .cspace_slots
            .checked_sub(right.cspace_slots)
            .ok_or(ResourcePoolError::Arithmetic)?,
        untyped_bytes: left
            .untyped_bytes
            .checked_sub(right.untyped_bytes)
            .ok_or(ResourcePoolError::Arithmetic)?,
    })
}

fn fits_after_reserve(
    used: KernelObjectBudget,
    capacity: KernelObjectBudget,
    reserve: KernelObjectBudget,
) -> Result<bool, ResourcePoolError> {
    let available = checked_sub(capacity, reserve)?;
    Ok(used.tcbs <= available.tcbs
        && used.cnodes <= available.cnodes
        && used.vspaces <= available.vspaces
        && used.page_tables <= available.page_tables
        && used.asids <= available.asids
        && used.frames <= available.frames
        && used.endpoints <= available.endpoints
        && used.notifications <= available.notifications
        && used.fault_caps <= available.fault_caps
        && used.timeout_fault_caps <= available.timeout_fault_caps
        && used.reply_objects <= available.reply_objects
        && used.scheduling_contexts <= available.scheduling_contexts
        && used.cspace_slots <= available.cspace_slots
        && used.untyped_bytes <= available.untyped_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(slot: u16, generation: u32) -> GenerationIdentity {
        GenerationIdentity {
            slot,
            lease_epoch: 1,
            supervisor_generation: generation,
            cap_generation: generation,
        }
    }

    #[test]
    fn complete_bundle_is_reserved_and_revoke_precedes_reuse() {
        let mut pool = SupervisorResourcePool::<WORKER_RESOURCE_POOL_CAPACITY>::from_generated()
            .expect("generated pool");
        let first = identity(0, 1);
        pool.reserve("worker-heartbeat", first, 0x5000)
            .expect("reserve first generation");
        assert_eq!(
            pool.release(first),
            Err(ResourcePoolError::InvalidTransition)
        );
        pool.close_admission(first).expect("close admission");
        pool.mark_descendants_revoked(first)
            .expect("revoke descendants");
        pool.mark_mappings_cleared(first).expect("clear mappings");
        pool.release(first).expect("release complete bundle");
        assert!(pool.is_empty());
        pool.reserve("worker-heartbeat", identity(0, 2), 0x5001)
            .expect("fresh generation after complete teardown");
    }

    #[test]
    fn duplicate_anchor_and_unknown_role_fail_closed() {
        let mut pool = SupervisorResourcePool::<WORKER_RESOURCE_POOL_CAPACITY>::from_generated()
            .expect("generated pool");
        pool.reserve("worker-heartbeat", identity(0, 1), 0x5000)
            .expect("first reservation");
        assert_eq!(
            pool.reserve("worker-gpu", identity(0, 2), 0x5000),
            Err(ResourcePoolError::DuplicateRevokeAnchor)
        );
        assert_eq!(
            pool.reserve("worker-bus", identity(0, 2), 0x5001),
            Err(ResourcePoolError::UnknownRole)
        );
    }
}
