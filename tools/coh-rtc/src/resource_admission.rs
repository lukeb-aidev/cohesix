// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate Milestone 26e executable-slot, kernel-object, and critical handoff admission.
// Author: Lukas Bower

//! Compiler-owned resource and handoff admission for executable target tasks.
//!
//! Namespace capacity is intentionally not used as executable capacity.  The
//! selected manifest must name each executable role pool and one maximum
//! simultaneous role mix, then prove that mix plus fixed service/driver/root
//! duties fits the selected kernel-object, CSpace, untyped, and fault bounds.

use crate::temporal::{TemporalAuthorityConfig, TemporalExecution, TemporalTaskKind};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const REQUIRED_CRITICAL_TCBS: [&str; 5] = [
    "root-control",
    "root-fault",
    "root-emergency",
    "root-worker-supervisor",
    "root-driver-supervisor",
];
const MAX_EXECUTABLE_ROLES: usize = 8;
const MAX_ROLE_MIXES: usize = 8;
const MAX_CRITICAL_TCBS: usize = 8;
const MAX_ID_BYTES: usize = 64;
const SEL4_16_AARCH64_SMP_MCS: &str = "sel4-16.0.0-aarch64-smp-mcs";
const SEL4_16_AARCH64_SMP_MCS_OBJECT_BITS: KernelObjectBits = KernelObjectBits {
    tcb: 11,
    endpoint: 4,
    notification: 6,
    reply: 5,
    sched_context_min: 7,
    cnode_slot: 5,
    page: 12,
    page_table: 12,
    vspace: 12,
};

/// Exact selected-kernel object sizes used by offline admission.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct KernelObjectBits {
    pub tcb: u8,
    pub endpoint: u8,
    pub notification: u8,
    pub reply: u8,
    pub sched_context_min: u8,
    pub cnode_slot: u8,
    pub page: u8,
    pub page_table: u8,
    pub vspace: u8,
}

impl KernelObjectBits {
    fn validate(&self) -> Result<()> {
        for (name, bits) in [
            ("tcb", self.tcb),
            ("endpoint", self.endpoint),
            ("notification", self.notification),
            ("reply", self.reply),
            ("sched_context_min", self.sched_context_min),
            ("cnode_slot", self.cnode_slot),
            ("page", self.page),
            ("page_table", self.page_table),
            ("vspace", self.vspace),
        ] {
            if !(4..=30).contains(&bits) {
                bail!("worker_resource_admission.object_bits.{name} is outside 4..=30");
            }
        }
        if self.sched_context_min < 7 || self.page < self.cnode_slot {
            bail!("worker_resource_admission selected-kernel object sizes are inconsistent");
        }
        Ok(())
    }
}

/// Count and memory total for one kernel-object resource set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct KernelObjectBudget {
    pub tcbs: u32,
    pub cnodes: u32,
    pub vspaces: u32,
    pub page_tables: u32,
    pub asids: u32,
    pub frames: u32,
    pub endpoints: u32,
    pub notifications: u32,
    pub fault_caps: u32,
    pub timeout_fault_caps: u32,
    pub reply_objects: u32,
    pub scheduling_contexts: u32,
    pub cspace_slots: u32,
    pub untyped_bytes: u64,
}

impl KernelObjectBudget {
    fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            tcbs: self.tcbs.checked_add(other.tcbs)?,
            cnodes: self.cnodes.checked_add(other.cnodes)?,
            vspaces: self.vspaces.checked_add(other.vspaces)?,
            page_tables: self.page_tables.checked_add(other.page_tables)?,
            asids: self.asids.checked_add(other.asids)?,
            frames: self.frames.checked_add(other.frames)?,
            endpoints: self.endpoints.checked_add(other.endpoints)?,
            notifications: self.notifications.checked_add(other.notifications)?,
            fault_caps: self.fault_caps.checked_add(other.fault_caps)?,
            timeout_fault_caps: self
                .timeout_fault_caps
                .checked_add(other.timeout_fault_caps)?,
            reply_objects: self.reply_objects.checked_add(other.reply_objects)?,
            scheduling_contexts: self
                .scheduling_contexts
                .checked_add(other.scheduling_contexts)?,
            cspace_slots: self.cspace_slots.checked_add(other.cspace_slots)?,
            untyped_bytes: self.untyped_bytes.checked_add(other.untyped_bytes)?,
        })
    }

    fn checked_mul(self, count: u32) -> Option<Self> {
        Some(Self {
            tcbs: self.tcbs.checked_mul(count)?,
            cnodes: self.cnodes.checked_mul(count)?,
            vspaces: self.vspaces.checked_mul(count)?,
            page_tables: self.page_tables.checked_mul(count)?,
            asids: self.asids.checked_mul(count)?,
            frames: self.frames.checked_mul(count)?,
            endpoints: self.endpoints.checked_mul(count)?,
            notifications: self.notifications.checked_mul(count)?,
            fault_caps: self.fault_caps.checked_mul(count)?,
            timeout_fault_caps: self.timeout_fault_caps.checked_mul(count)?,
            reply_objects: self.reply_objects.checked_mul(count)?,
            scheduling_contexts: self.scheduling_contexts.checked_mul(count)?,
            cspace_slots: self.cspace_slots.checked_mul(count)?,
            untyped_bytes: self.untyped_bytes.checked_mul(u64::from(count))?,
        })
    }

    fn fits_within(self, capacity: Self) -> bool {
        self.tcbs <= capacity.tcbs
            && self.cnodes <= capacity.cnodes
            && self.vspaces <= capacity.vspaces
            && self.page_tables <= capacity.page_tables
            && self.asids <= capacity.asids
            && self.frames <= capacity.frames
            && self.endpoints <= capacity.endpoints
            && self.notifications <= capacity.notifications
            && self.fault_caps <= capacity.fault_caps
            && self.timeout_fault_caps <= capacity.timeout_fault_caps
            && self.reply_objects <= capacity.reply_objects
            && self.scheduling_contexts <= capacity.scheduling_contexts
            && self.cspace_slots <= capacity.cspace_slots
            && self.untyped_bytes <= capacity.untyped_bytes
    }

    fn checked_available_after(self, reserve: Self) -> Option<Self> {
        Some(Self {
            tcbs: self.tcbs.checked_sub(reserve.tcbs)?,
            cnodes: self.cnodes.checked_sub(reserve.cnodes)?,
            vspaces: self.vspaces.checked_sub(reserve.vspaces)?,
            page_tables: self.page_tables.checked_sub(reserve.page_tables)?,
            asids: self.asids.checked_sub(reserve.asids)?,
            frames: self.frames.checked_sub(reserve.frames)?,
            endpoints: self.endpoints.checked_sub(reserve.endpoints)?,
            notifications: self.notifications.checked_sub(reserve.notifications)?,
            fault_caps: self.fault_caps.checked_sub(reserve.fault_caps)?,
            timeout_fault_caps: self
                .timeout_fault_caps
                .checked_sub(reserve.timeout_fault_caps)?,
            reply_objects: self.reply_objects.checked_sub(reserve.reply_objects)?,
            scheduling_contexts: self
                .scheduling_contexts
                .checked_sub(reserve.scheduling_contexts)?,
            cspace_slots: self.cspace_slots.checked_sub(reserve.cspace_slots)?,
            untyped_bytes: self.untyped_bytes.checked_sub(reserve.untyped_bytes)?,
        })
    }
}

/// Compiler-owned executable pool for one Worker role.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExecutableRoleAdmission {
    pub role: String,
    pub task_prefix: String,
    pub namespace_capacity: u16,
    pub executable_slots: u16,
    pub core: u8,
    pub revoke_anchor_slot: u32,
    pub per_slot: KernelObjectBudget,
}

/// Count of one role in a simultaneous execution mix.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RoleMixCount {
    pub role: String,
    pub count: u16,
}

/// One compiler-accepted simultaneous executable-role mix.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExecutableRoleMix {
    pub id: String,
    pub maximum: bool,
    pub roles: Vec<RoleMixCount>,
}

/// Least-authority CSpace and permanent retention slot for one critical TCB.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CriticalTcbResource {
    pub id: String,
    pub cnode_radix_bits: u8,
    pub cspace_cap_count: u16,
    /// Reserved slot retaining the permanent critical domain's CNode cap.
    ///
    /// Critical objects are not claimed as a grouped, reclaimable untyped
    /// allocation; executable-role anchors use that stronger contract.
    pub revoke_anchor_slot: u32,
    pub ipc_buffer_pages: u8,
    pub stack_pages: u8,
    pub fault_reply_lanes: u8,
}

/// Exact seL4 capability rights; all unspecified rights are false.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CapabilityRights {
    pub read: bool,
    pub write: bool,
    pub grant: bool,
    pub grant_reply: bool,
}

impl CapabilityRights {
    const READ: Self = Self {
        read: true,
        write: false,
        grant: false,
        grant_reply: false,
    };
    const WRITE: Self = Self {
        read: false,
        write: true,
        grant: false,
        grant_reply: false,
    };
    const WRITE_GRANT_REPLY: Self = Self {
        read: false,
        write: true,
        grant: false,
        grant_reply: true,
    };
}

/// Reserved identity-badge domain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BadgeRange {
    pub base: u64,
    pub count: u16,
    pub stride: u16,
}

impl BadgeRange {
    fn end_exclusive(self) -> Option<u64> {
        self.base
            .checked_add(u64::from(self.count).checked_mul(u64::from(self.stride))?)
    }

    fn overlaps(self, other: Self) -> bool {
        let Some(self_end) = self.end_exclusive() else {
            return true;
        };
        let Some(other_end) = other.end_exclusive() else {
            return true;
        };
        self.base < other_end && other.base < self_end
    }
}

/// Saturation behavior for a bounded critical handoff.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SaturationPolicy {
    /// Reject new policy work without blocking the producer.
    #[default]
    RefuseNew,
    /// Escalate because losing a containment record is unsafe.
    Fatal,
}

/// Handoff classes in generated coalesced-drain order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HandoffClass {
    #[default]
    WorkerFault,
    WorkerControl,
    DriverFault,
}

/// Bounded critical-TCB handoff and rights contract.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CriticalHandoffConfig {
    pub worker_control_queue_capacity: u16,
    pub worker_fault_mailboxes: u16,
    pub driver_fault_records: u16,
    pub worker_wake_badge: u64,
    pub driver_wake_badge: u64,
    pub emergency_wake_badge: u64,
    pub worker_fault_badges: BadgeRange,
    pub driver_fault_badges: BadgeRange,
    pub critical_fault_badges: BadgeRange,
    pub service_fault_badges: BadgeRange,
    pub timeout_fault_badges: BadgeRange,
    pub supervisor_signal_rights: CapabilityRights,
    pub supervisor_wait_rights: CapabilityRights,
    pub fault_sender_rights: CapabilityRights,
    pub fault_receiver_rights: CapabilityRights,
    pub worker_control_saturation: SaturationPolicy,
    pub worker_fault_saturation: SaturationPolicy,
    pub service_fault_saturation: SaturationPolicy,
    pub driver_fault_saturation: SaturationPolicy,
    pub worker_drain_precedence: Vec<HandoffClass>,
    pub driver_drain_precedence: Vec<HandoffClass>,
}

/// Exact registry and MCS fault-Reply lane cardinality.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FaultRegistryAdmission {
    pub critical_tcbs: u16,
    pub service_tcbs: u16,
    pub worker_tcbs: u16,
    pub driver_tcbs: u16,
    pub capacity: u16,
    pub root_fault_tcb_control_slot_base: u16,
    pub standard_reply_lanes: u8,
    pub timeout_reply_lanes: u8,
    pub recoverable_timeout_tasks: Vec<String>,
}

/// Full Milestone 26e executable-resource admission contract.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WorkerResourceAdmissionConfig {
    pub enabled: bool,
    pub selected_kernel: String,
    pub object_bits: KernelObjectBits,
    pub capacity: KernelObjectBudget,
    pub post_construction_reserve: KernelObjectBudget,
    pub fixed_objects: KernelObjectBudget,
    pub executable_roles: Vec<ExecutableRoleAdmission>,
    pub allowed_role_mixes: Vec<ExecutableRoleMix>,
    pub critical_tcbs: Vec<CriticalTcbResource>,
    pub handoff: CriticalHandoffConfig,
    pub fault_registry: FaultRegistryAdmission,
}

impl WorkerResourceAdmissionConfig {
    /// Return the exact fixed-plus-maximum-role object inventory admitted by
    /// this compiler contract.
    ///
    /// Disabled profiles have no Milestone 26e executable topology and return
    /// an all-zero inventory. Enabled profiles fail closed if their role table
    /// or maximum mix is not canonical, even when a caller invokes this helper
    /// independently of full manifest validation.
    pub fn maximum_inventory(&self) -> Result<KernelObjectBudget> {
        if !self.enabled {
            return Ok(KernelObjectBudget::default());
        }
        let mut roles = BTreeMap::new();
        for role in &self.executable_roles {
            if roles.insert(role.role.as_str(), role).is_some() {
                bail!("duplicate executable role admission {}", role.role);
            }
        }
        self.validate_mixes(&roles)
    }

    /// Validate resources against the exact generated temporal topology.
    pub fn validate(&self, temporal: &TemporalAuthorityConfig) -> Result<()> {
        if !self.enabled {
            if temporal.enabled {
                bail!("enabled temporal_authority requires worker_resource_admission.enabled=true");
            }
            return Ok(());
        }
        if !temporal.enabled {
            bail!("worker_resource_admission requires temporal_authority.enabled=true");
        }
        validate_id(
            "worker_resource_admission.selected_kernel",
            &self.selected_kernel,
        )?;
        self.object_bits.validate()?;
        if self.selected_kernel != SEL4_16_AARCH64_SMP_MCS
            || self.object_bits != SEL4_16_AARCH64_SMP_MCS_OBJECT_BITS
        {
            bail!(
                "worker_resource_admission selected-kernel object sizes do not match seL4 16 AArch64 SMP+MCS"
            );
        }
        if self.executable_roles.is_empty()
            || self.executable_roles.len() > MAX_EXECUTABLE_ROLES
            || self.allowed_role_mixes.is_empty()
            || self.allowed_role_mixes.len() > MAX_ROLE_MIXES
        {
            bail!("worker_resource_admission role pools or role mixes are empty/oversized");
        }

        let available = self
            .capacity
            .checked_available_after(self.post_construction_reserve)
            .ok_or_else(|| {
                anyhow::anyhow!("post-construction reserve exceeds resource capacity")
            })?;
        let roles = self.validate_roles(temporal)?;
        let required = self.validate_mixes(&roles)?;
        if !required.fits_within(available) {
            bail!("maximum executable role mix exceeds admitted kernel resources after reserve");
        }
        self.validate_critical_tcbs(temporal)?;
        self.validate_fault_registry(temporal)?;
        self.validate_handoff(temporal)?;
        Ok(())
    }

    fn validate_roles<'a>(
        &'a self,
        temporal: &TemporalAuthorityConfig,
    ) -> Result<BTreeMap<&'a str, &'a ExecutableRoleAdmission>> {
        let mut roles = BTreeMap::new();
        let critical_anchors: BTreeSet<u32> = self
            .critical_tcbs
            .iter()
            .map(|resource| resource.revoke_anchor_slot)
            .collect();
        let mut worker_anchors = BTreeSet::new();
        let mut mapped_worker_tasks = BTreeSet::new();
        for role in &self.executable_roles {
            validate_id(
                "worker_resource_admission.executable_roles.role",
                &role.role,
            )?;
            validate_id(
                "worker_resource_admission.executable_roles.task_prefix",
                &role.task_prefix,
            )?;
            if role.namespace_capacity == 0
                || role.executable_slots == 0
                || role.executable_slots > role.namespace_capacity
            {
                bail!(
                    "executable role {} has invalid namespace/executable capacity",
                    role.role
                );
            }
            if role.revoke_anchor_slot == 0
                || role.revoke_anchor_slot >= self.capacity.cspace_slots
                || critical_anchors.contains(&role.revoke_anchor_slot)
                || !worker_anchors.insert(role.revoke_anchor_slot)
            {
                bail!(
                    "executable role {} has a zero, out-of-range, duplicate, or critical revoke-anchor slot",
                    role.role
                );
            }
            if roles.insert(role.role.as_str(), role).is_some() {
                bail!("duplicate executable role admission {}", role.role);
            }
            let tasks: Vec<_> = temporal
                .tasks
                .iter()
                .filter(|task| task.id.starts_with(&role.task_prefix))
                .collect();
            if tasks.len() != usize::from(role.executable_slots) {
                bail!(
                    "executable role {} declares {} slots but temporal topology has {}",
                    role.role,
                    role.executable_slots,
                    tasks.len()
                );
            }
            if tasks.iter().any(|task| {
                task.kind != TemporalTaskKind::Worker
                    || task.execution != TemporalExecution::Active
                    || task.core != role.core
                    || !task.allowed_donors.is_empty()
            }) {
                bail!(
                    "executable role {} does not map to dedicated active Worker SCs",
                    role.role
                );
            }
            for task in tasks {
                if !mapped_worker_tasks.insert(task.id.as_str()) {
                    bail!(
                        "executable Worker task {} matches more than one role prefix",
                        task.id
                    );
                }
            }
            if role.executable_slots > 0
                && (role.per_slot.tcbs != 1
                    || role.per_slot.cnodes != 1
                    || role.per_slot.vspaces != 1
                    || role.per_slot.asids != 1
                    || role.per_slot.scheduling_contexts != 1
                    || role.per_slot.fault_caps != 1
                    || role.per_slot.timeout_fault_caps != 1
                    || role.per_slot.cspace_slots == 0
                    || role.per_slot.untyped_bytes == 0
                    || !role.per_slot.untyped_bytes.is_power_of_two())
            {
                bail!(
                    "executable role {} has an incomplete per-slot object bundle",
                    role.role
                );
            }
        }
        let temporal_worker_tasks: BTreeSet<_> = temporal
            .tasks
            .iter()
            .filter(|task| task.kind == TemporalTaskKind::Worker)
            .map(|task| task.id.as_str())
            .collect();
        if mapped_worker_tasks != temporal_worker_tasks {
            bail!("executable role pools do not cover the exact temporal Worker task set");
        }
        Ok(roles)
    }

    fn validate_mixes(
        &self,
        roles: &BTreeMap<&str, &ExecutableRoleAdmission>,
    ) -> Result<KernelObjectBudget> {
        let maximum_mix_count = self
            .allowed_role_mixes
            .iter()
            .filter(|mix| mix.maximum)
            .count();
        if maximum_mix_count != 1 {
            bail!("worker_resource_admission requires exactly one maximum role mix");
        }
        let mut maximum_required = None;
        let mut mix_ids = BTreeSet::new();
        for mix in &self.allowed_role_mixes {
            validate_id("worker_resource_admission.allowed_role_mixes.id", &mix.id)?;
            if !mix_ids.insert(mix.id.as_str()) {
                bail!("duplicate executable role mix {}", mix.id);
            }
            let mut seen = BTreeSet::new();
            let mut required = self.fixed_objects;
            for count in &mix.roles {
                let role = roles.get(count.role.as_str()).ok_or_else(|| {
                    anyhow::anyhow!("role mix {} references unknown role {}", mix.id, count.role)
                })?;
                if !seen.insert(count.role.as_str()) || count.count > role.executable_slots {
                    bail!(
                        "role mix {} duplicates or overcommits role {}",
                        mix.id,
                        count.role
                    );
                }
                required = required
                    .checked_add(
                        role.per_slot
                            .checked_mul(u32::from(count.count))
                            .ok_or_else(|| {
                                anyhow::anyhow!("role mix resource arithmetic overflow")
                            })?,
                    )
                    .ok_or_else(|| anyhow::anyhow!("role mix resource arithmetic overflow"))?;
            }
            if mix.maximum {
                if roles.iter().any(|(name, role)| {
                    !mix.roles
                        .iter()
                        .any(|count| count.role == **name && count.count == role.executable_slots)
                }) {
                    bail!("maximum role mix must contain every executable role at its slot bound");
                }
                maximum_required = Some(required);
            }
        }
        maximum_required.ok_or_else(|| anyhow::anyhow!("missing maximum role mix"))
    }

    fn validate_critical_tcbs(&self, temporal: &TemporalAuthorityConfig) -> Result<()> {
        if self.critical_tcbs.len() != REQUIRED_CRITICAL_TCBS.len()
            || self.critical_tcbs.len() > MAX_CRITICAL_TCBS
        {
            bail!("critical TCB resource table must contain exactly five records");
        }
        let mut ids = BTreeSet::new();
        let mut anchors = BTreeSet::new();
        for critical in &self.critical_tcbs {
            if !ids.insert(critical.id.as_str())
                || !anchors.insert(critical.revoke_anchor_slot)
                || critical.revoke_anchor_slot == 0
                || critical.revoke_anchor_slot >= self.capacity.cspace_slots
            {
                bail!("critical TCB records duplicate an id or use an invalid retention slot");
            }
            if critical.cnode_radix_bits < 4 || critical.cnode_radix_bits > 16 {
                bail!("critical TCB {} has invalid CNode radix", critical.id);
            }
            let slots = 1u32 << critical.cnode_radix_bits;
            if u32::from(critical.cspace_cap_count) > slots
                || critical.cspace_cap_count == 0
                || critical.ipc_buffer_pages != 1
                || critical.stack_pages == 0
                || critical.fault_reply_lanes == 0
            {
                bail!(
                    "critical TCB {} has an incomplete least-authority resource view",
                    critical.id
                );
            }
            let task = temporal
                .tasks
                .iter()
                .find(|task| task.id == critical.id)
                .ok_or_else(|| {
                    anyhow::anyhow!("critical TCB {} is missing temporal state", critical.id)
                })?;
            if task.execution != TemporalExecution::Active
                || !task.critical_reserve
                || !task.allowed_donors.is_empty()
                || task.reply_objects != 0
            {
                bail!(
                    "critical TCB {} does not own an independent active reserve",
                    critical.id
                );
            }
        }
        if REQUIRED_CRITICAL_TCBS
            .iter()
            .any(|required| !ids.contains(required))
        {
            bail!("critical TCB resource table is incomplete");
        }
        Ok(())
    }

    fn validate_fault_registry(&self, temporal: &TemporalAuthorityConfig) -> Result<()> {
        let critical = temporal
            .tasks
            .iter()
            .filter(|task| task.critical_reserve)
            .count();
        let workers = temporal
            .tasks
            .iter()
            .filter(|task| task.kind == TemporalTaskKind::Worker)
            .count();
        let drivers = temporal
            .tasks
            .iter()
            .filter(|task| task.kind == TemporalTaskKind::Driver)
            .count();
        let services = temporal.tasks.len() - critical - workers - drivers;
        let active_nonworkers = temporal
            .tasks
            .iter()
            .filter(|task| {
                task.kind != TemporalTaskKind::Worker && task.execution == TemporalExecution::Active
            })
            .count();
        let critical_reply_objects = self
            .critical_tcbs
            .iter()
            .try_fold(0u32, |total, critical| {
                total.checked_add(u32::from(critical.fault_reply_lanes))
            })
            .ok_or_else(|| anyhow::anyhow!("critical Reply-object total overflows"))?;
        let passive_reply_objects = temporal
            .tasks
            .iter()
            .filter(|task| task.execution == TemporalExecution::Passive)
            .try_fold(0u32, |total, task| {
                total.checked_add(u32::from(task.reply_objects))
            })
            .ok_or_else(|| anyhow::anyhow!("passive Reply-object total overflows"))?;
        let driver_command_replies = u32::try_from(drivers)
            .map_err(|_| anyhow::anyhow!("driver Reply-object total overflows"))?;
        let expected_reply_objects = critical_reply_objects
            .checked_add(passive_reply_objects)
            .and_then(|total| total.checked_add(driver_command_replies))
            .ok_or_else(|| anyhow::anyhow!("fixed Reply-object total overflows"))?;
        let declared = &self.fault_registry;
        if usize::from(declared.critical_tcbs) != critical
            || usize::from(declared.worker_tcbs) != workers
            || usize::from(declared.driver_tcbs) != drivers
            || usize::from(declared.service_tcbs) != services
            || usize::from(declared.capacity) != temporal.tasks.len()
        {
            bail!("fault registry capacity does not equal every admitted temporal TCB");
        }
        let fixed_tcb_count = u32::try_from(temporal.tasks.len() - workers)
            .map_err(|_| anyhow::anyhow!("fixed TCB total overflows"))?;
        if self.fixed_objects.tcbs != fixed_tcb_count
            || self.fixed_objects.cnodes != fixed_tcb_count
            || self.fixed_objects.vspaces != fixed_tcb_count
            || self.fixed_objects.asids != fixed_tcb_count
            || self.fixed_objects.fault_caps != fixed_tcb_count
            || self.fixed_objects.timeout_fault_caps != fixed_tcb_count
            || self.fixed_objects.scheduling_contexts != active_nonworkers as u32
        {
            bail!("fixed object totals do not match non-Worker temporal duties");
        }
        if self.fixed_objects.reply_objects != expected_reply_objects {
            bail!(
                "fixed Reply-object total {} does not match critical lanes + passive receivers + driver command lanes {}",
                self.fixed_objects.reply_objects,
                expected_reply_objects
            );
        }
        if declared.standard_reply_lanes == 0 || declared.timeout_reply_lanes == 0 {
            bail!("fault registry requires owned standard and timeout Reply lanes");
        }
        let passive_recovery_caps = temporal
            .tasks
            .iter()
            .filter(|task| {
                task.execution == TemporalExecution::Passive
                    && task.timeout_policy == crate::temporal::TimeoutPolicy::ReturnError
            })
            .count();
        let root_fault = self
            .critical_tcbs
            .iter()
            .find(|critical| critical.id == "root-fault")
            .ok_or_else(|| anyhow::anyhow!("root-fault resource record is missing"))?;
        // Slots 1..=9 are the two self-fault caps, two receive endpoints, two
        // Reply objects, and three notification signals. Passive recovery caps
        // follow in their compiler-selected slots; every registered temporal
        // TCB additionally needs one root-fault-local control cap because a
        // root-Cspace CPtr is meaningless inside the restricted handler CSpace.
        let expected_root_fault_caps = 9usize
            .checked_add(passive_recovery_caps)
            .and_then(|total| total.checked_add(temporal.tasks.len()))
            .ok_or_else(|| anyhow::anyhow!("root-fault CSpace arithmetic overflows"))?;
        if usize::from(root_fault.cspace_cap_count) != expected_root_fault_caps {
            bail!(
                "root-fault CSpace cap count {} does not match handler lanes + passive recovery + temporal TCB controls {}",
                root_fault.cspace_cap_count,
                expected_root_fault_caps
            );
        }
        let root_fault_slots = 1usize << root_fault.cnode_radix_bits;
        let tcb_control_slot_base = usize::from(declared.root_fault_tcb_control_slot_base);
        let first_slot_after_fixed_and_recovery = 10usize
            .checked_add(passive_recovery_caps)
            .ok_or_else(|| anyhow::anyhow!("root-fault recovery-slot arithmetic overflows"))?;
        if tcb_control_slot_base < first_slot_after_fixed_and_recovery {
            bail!(
                "root-fault TCB control slot range overlaps fixed handler or passive recovery caps"
            );
        }
        let highest_tcb_control_slot = tcb_control_slot_base
            .checked_add(temporal.tasks.len())
            .ok_or_else(|| anyhow::anyhow!("root-fault control-slot arithmetic overflows"))?;
        if highest_tcb_control_slot > root_fault_slots {
            bail!("root-fault CNode cannot contain every temporal TCB control cap");
        }
        let mut recoverable = BTreeSet::new();
        for id in &declared.recoverable_timeout_tasks {
            if !recoverable.insert(id.as_str())
                || !temporal.tasks.iter().any(|task| {
                    task.id == *id
                        && matches!(
                            task.timeout_policy,
                            crate::temporal::TimeoutPolicy::ReplenishOnce
                        )
                })
            {
                bail!("recoverable timeout allowlist contains invalid task {id}");
            }
        }
        let required_recoverable: BTreeSet<_> = temporal
            .tasks
            .iter()
            .filter(|task| {
                matches!(
                    task.timeout_policy,
                    crate::temporal::TimeoutPolicy::ReplenishOnce
                )
            })
            .map(|task| task.id.as_str())
            .collect();
        if recoverable != required_recoverable {
            bail!("recoverable timeout allowlist omits or adds a temporal task");
        }
        Ok(())
    }

    fn validate_handoff(&self, temporal: &TemporalAuthorityConfig) -> Result<()> {
        let workers = self.fault_registry.worker_tcbs;
        let drivers = self.fault_registry.driver_tcbs;
        let handoff = &self.handoff;
        if handoff.worker_control_queue_capacity == 0
            || handoff.worker_fault_mailboxes != workers
            || handoff.driver_fault_records != drivers
        {
            bail!("critical handoff capacities do not match admitted Worker/driver slots");
        }
        for badge in [
            handoff.worker_wake_badge,
            handoff.driver_wake_badge,
            handoff.emergency_wake_badge,
        ] {
            if badge == 0 || !badge.is_power_of_two() {
                bail!("critical supervisor wake badges must be non-zero one-hot values");
            }
        }
        if handoff.worker_wake_badge == handoff.driver_wake_badge
            || handoff.worker_wake_badge == handoff.emergency_wake_badge
            || handoff.driver_wake_badge == handoff.emergency_wake_badge
        {
            bail!("critical supervisor wake badges must be disjoint");
        }
        let ranges = [
            handoff.worker_fault_badges,
            handoff.driver_fault_badges,
            handoff.critical_fault_badges,
            handoff.service_fault_badges,
            handoff.timeout_fault_badges,
        ];
        if handoff.worker_fault_badges.count != workers
            || handoff.driver_fault_badges.count != drivers
            || usize::from(handoff.critical_fault_badges.count) != REQUIRED_CRITICAL_TCBS.len()
            || handoff.service_fault_badges.count != self.fault_registry.service_tcbs
            || usize::from(handoff.timeout_fault_badges.count) != temporal.tasks.len()
        {
            bail!("critical fault badge ranges do not exactly cover their admitted classes");
        }
        for (range, required_count) in ranges.into_iter().zip([
            workers,
            drivers,
            self.fault_registry.critical_tcbs,
            self.fault_registry.service_tcbs,
            self.fault_registry.capacity,
        ]) {
            if range.base == 0
                || range.stride == 0
                || (required_count != 0 && range.count == 0)
                || range.end_exclusive().is_none()
            {
                bail!("critical fault badge range is empty or overflows");
            }
        }
        for left in 0..ranges.len() {
            for right in (left + 1)..ranges.len() {
                if ranges[left].overlaps(ranges[right]) {
                    bail!("critical fault badge domains overlap");
                }
            }
        }
        if handoff.supervisor_signal_rights != CapabilityRights::WRITE
            || handoff.supervisor_wait_rights != CapabilityRights::READ
            || handoff.fault_sender_rights != CapabilityRights::WRITE_GRANT_REPLY
            || handoff.fault_receiver_rights != CapabilityRights::READ
        {
            bail!("critical handoff capability rights are broader than the generated contract");
        }
        if handoff.worker_control_saturation != SaturationPolicy::RefuseNew
            || handoff.worker_fault_saturation != SaturationPolicy::Fatal
            || handoff.service_fault_saturation != SaturationPolicy::Fatal
            || handoff.driver_fault_saturation != SaturationPolicy::Fatal
        {
            bail!("critical handoff saturation policy may block or drop containment work");
        }
        if handoff.worker_drain_precedence
            != [HandoffClass::WorkerFault, HandoffClass::WorkerControl]
            || handoff.driver_drain_precedence != [HandoffClass::DriverFault]
        {
            bail!("critical supervisor coalesced-drain precedence is invalid");
        }
        for (index, task) in temporal.tasks.iter().enumerate() {
            let expected = handoff
                .timeout_fault_badges
                .base
                .checked_add(
                    u64::try_from(index)
                        .ok()
                        .and_then(|value| {
                            value.checked_mul(u64::from(handoff.timeout_fault_badges.stride))
                        })
                        .ok_or_else(|| anyhow::anyhow!("timeout badge arithmetic overflow"))?,
                )
                .ok_or_else(|| anyhow::anyhow!("timeout badge arithmetic overflow"))?;
            if task.timeout_badge != expected {
                bail!(
                    "temporal task {} timeout badge does not match exact admitted range",
                    task.id
                );
            }
        }
        Ok(())
    }
}

fn validate_id(field: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
    {
        bail!("{field} is empty, oversized, or contains unsupported characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::temporal::{
        SchedulerArchitecture, TemporalCoreAdmission, TemporalTaskConfig, TimeoutPolicy,
    };

    fn temporal_task(
        id: &str,
        kind: TemporalTaskKind,
        core: u8,
        critical: bool,
    ) -> TemporalTaskConfig {
        TemporalTaskConfig {
            id: id.to_owned(),
            kind,
            execution: TemporalExecution::Active,
            core,
            scheduling_context_slot: u32::from(core) + 1 + id.len() as u32,
            scheduling_context_bits: if id == "root-control" { 7 } else { 8 },
            sched_control_core: core,
            budget_us: 100,
            period_us: 10_000,
            deadline_us: 10_000,
            blocking_us: 0,
            jitter_us: 0,
            max_refills: 2,
            priority: 100,
            mcp: 200,
            timeout_badge: 0x26ee_0000 + id.len() as u64 + u64::from(core) * 0x100,
            timeout_policy: if id == "root-emergency" {
                TimeoutPolicy::FailStop
            } else {
                TimeoutPolicy::Terminal
            },
            consumed_time_evidence: true,
            wcet_us: 80,
            response_time_us: 80,
            admitted: true,
            wcet_provenance: "unit-test".to_owned(),
            allowed_donors: Vec::new(),
            reply_objects: 0,
            max_donation_depth: 0,
            fault_handler: if id == "root-emergency" {
                String::new()
            } else if id == "root-fault" {
                "root-emergency".to_owned()
            } else {
                "root-fault".to_owned()
            },
            critical_reserve: critical,
            locality_bound: false,
        }
    }

    fn temporal() -> TemporalAuthorityConfig {
        let mut tasks = vec![
            temporal_task("root-control", TemporalTaskKind::RootControl, 0, true),
            temporal_task("root-fault", TemporalTaskKind::RootFault, 0, true),
            temporal_task("root-emergency", TemporalTaskKind::RootEmergency, 0, true),
            temporal_task(
                "root-worker-supervisor",
                TemporalTaskKind::WorkerSupervisor,
                1,
                true,
            ),
            temporal_task(
                "root-driver-supervisor",
                TemporalTaskKind::DriverSupervisor,
                1,
                true,
            ),
            temporal_task(
                "console-network-service",
                TemporalTaskKind::Service,
                0,
                false,
            ),
            temporal_task("driver-net", TemporalTaskKind::Driver, 3, false),
            temporal_task("worker-heart-slot-0", TemporalTaskKind::Worker, 3, false),
        ];
        for (index, task) in tasks.iter_mut().enumerate() {
            task.scheduling_context_slot = index as u32 + 1;
            task.timeout_badge = 0x26ee_0000 + index as u64;
        }
        for index in 0..tasks.len() {
            let peers = tasks
                .iter()
                .filter(|task| task.core == tasks[index].core)
                .count();
            tasks[index].response_time_us = u32::try_from(peers).expect("bounded peer count") * 80;
        }
        TemporalAuthorityConfig {
            enabled: true,
            architecture: SchedulerArchitecture::SmpMcs,
            cores: 4,
            domains: 1,
            admission_window_us: 10_000,
            core_admission: (0..4)
                .map(|core| TemporalCoreAdmission {
                    core,
                    capacity_us: 10_000,
                    reserve_us: 1_000,
                })
                .collect(),
            tasks,
        }
    }

    fn budget(tcbs: u32) -> KernelObjectBudget {
        KernelObjectBudget {
            tcbs,
            cnodes: tcbs,
            vspaces: tcbs,
            page_tables: tcbs * 4,
            asids: tcbs,
            frames: tcbs * 8,
            endpoints: tcbs * 2,
            notifications: tcbs * 2,
            fault_caps: tcbs,
            timeout_fault_caps: tcbs,
            reply_objects: tcbs,
            scheduling_contexts: tcbs,
            cspace_slots: tcbs * 32,
            untyped_bytes: u64::from(tcbs) * 128 * 1024,
        }
    }

    fn admission() -> WorkerResourceAdmissionConfig {
        let temporal = temporal();
        let mut fixed_objects = budget(7);
        fixed_objects.reply_objects = 6;
        WorkerResourceAdmissionConfig {
            enabled: true,
            selected_kernel: SEL4_16_AARCH64_SMP_MCS.to_owned(),
            object_bits: KernelObjectBits {
                tcb: 11,
                endpoint: 4,
                notification: 6,
                reply: 5,
                sched_context_min: 7,
                cnode_slot: 5,
                page: 12,
                page_table: 12,
                vspace: 12,
            },
            capacity: budget(64),
            post_construction_reserve: KernelObjectBudget::default(),
            fixed_objects,
            executable_roles: vec![ExecutableRoleAdmission {
                role: "worker-heartbeat".to_owned(),
                task_prefix: "worker-heart-slot-".to_owned(),
                namespace_capacity: 8,
                executable_slots: 1,
                core: 3,
                revoke_anchor_slot: 16,
                per_slot: budget(1),
            }],
            allowed_role_mixes: vec![ExecutableRoleMix {
                id: "maximum".to_owned(),
                maximum: true,
                roles: vec![RoleMixCount {
                    role: "worker-heartbeat".to_owned(),
                    count: 1,
                }],
            }],
            critical_tcbs: REQUIRED_CRITICAL_TCBS
                .iter()
                .enumerate()
                .map(|(index, id)| CriticalTcbResource {
                    id: (*id).to_owned(),
                    cnode_radix_bits: 5,
                    cspace_cap_count: if *id == "root-fault" { 17 } else { 8 },
                    revoke_anchor_slot: index as u32 + 1,
                    ipc_buffer_pages: 1,
                    stack_pages: 2,
                    fault_reply_lanes: 1,
                })
                .collect(),
            handoff: CriticalHandoffConfig {
                worker_control_queue_capacity: 4,
                worker_fault_mailboxes: 1,
                driver_fault_records: 1,
                worker_wake_badge: 1,
                driver_wake_badge: 2,
                emergency_wake_badge: 4,
                worker_fault_badges: BadgeRange {
                    base: 0x26e1_0000,
                    count: 1,
                    stride: 1,
                },
                driver_fault_badges: BadgeRange {
                    base: 0x26e2_0000,
                    count: 1,
                    stride: 1,
                },
                critical_fault_badges: BadgeRange {
                    base: 0x26e3_0000,
                    count: 5,
                    stride: 1,
                },
                service_fault_badges: BadgeRange {
                    base: 0x26e4_0000,
                    count: 1,
                    stride: 1,
                },
                timeout_fault_badges: BadgeRange {
                    base: 0x26ee_0000,
                    count: temporal.tasks.len() as u16,
                    stride: 1,
                },
                supervisor_signal_rights: CapabilityRights::WRITE,
                supervisor_wait_rights: CapabilityRights::READ,
                fault_sender_rights: CapabilityRights::WRITE_GRANT_REPLY,
                fault_receiver_rights: CapabilityRights::READ,
                worker_control_saturation: SaturationPolicy::RefuseNew,
                worker_fault_saturation: SaturationPolicy::Fatal,
                service_fault_saturation: SaturationPolicy::Fatal,
                driver_fault_saturation: SaturationPolicy::Fatal,
                worker_drain_precedence: vec![
                    HandoffClass::WorkerFault,
                    HandoffClass::WorkerControl,
                ],
                driver_drain_precedence: vec![HandoffClass::DriverFault],
            },
            fault_registry: FaultRegistryAdmission {
                critical_tcbs: 5,
                service_tcbs: 1,
                worker_tcbs: 1,
                driver_tcbs: 1,
                capacity: temporal.tasks.len() as u16,
                root_fault_tcb_control_slot_base: 16,
                standard_reply_lanes: 1,
                timeout_reply_lanes: 1,
                recoverable_timeout_tasks: Vec::new(),
            },
        }
    }

    #[test]
    fn worker_admission_separates_namespace_and_executable_capacity() {
        let config = admission();
        config.validate(&temporal()).expect("valid admission");
        assert_eq!(config.executable_roles[0].namespace_capacity, 8);
        assert_eq!(config.executable_roles[0].executable_slots, 1);
    }

    #[test]
    fn worker_admission_rejects_maximum_mix_overcommit() {
        let mut config = admission();
        config.allowed_role_mixes[0].roles[0].count = 2;
        let error = config.validate(&temporal()).expect_err("overcommit");
        assert!(error.to_string().contains("overcommits"));
    }

    #[test]
    fn worker_admission_rejects_unbudgeted_temporal_worker() {
        let mut temporal = temporal();
        let mut extra = temporal_task("worker-extra-slot-0", TemporalTaskKind::Worker, 2, false);
        extra.scheduling_context_slot = 99;
        extra.timeout_badge = 0x26ee_0100;
        temporal.tasks.push(extra);
        let error = admission()
            .validate(&temporal)
            .expect_err("every temporal Worker requires one executable role pool");
        assert!(error.to_string().contains("exact temporal Worker task set"));
    }

    #[test]
    fn worker_admission_rejects_critical_revoke_anchor_alias() {
        let mut config = admission();
        config.executable_roles[0].revoke_anchor_slot = config.critical_tcbs[0].revoke_anchor_slot;
        let error = config
            .validate(&temporal())
            .expect_err("critical anchor alias");
        assert!(error.to_string().contains("revoke-anchor slot"));
    }

    #[test]
    fn worker_admission_rejects_fault_registry_omission() {
        let mut config = admission();
        config.fault_registry.capacity -= 1;
        let error = config.validate(&temporal()).expect_err("fault omission");
        assert!(error.to_string().contains("every admitted temporal TCB"));
    }

    #[test]
    fn worker_admission_rejects_root_fault_tcb_control_slot_overlap() {
        let mut config = admission();
        config.fault_registry.root_fault_tcb_control_slot_base = 9;
        let error = config
            .validate(&temporal())
            .expect_err("TCB control caps must not overlap fixed handler slots");
        assert!(error.to_string().contains("overlaps fixed handler"));
    }

    #[test]
    fn worker_admission_rejects_root_fault_tcb_control_slot_overflow() {
        let mut config = admission();
        config.fault_registry.root_fault_tcb_control_slot_base = 25;
        let error = config
            .validate(&temporal())
            .expect_err("TCB control caps must fit the root-fault CNode");
        assert!(error
            .to_string()
            .contains("cannot contain every temporal TCB"));
    }

    #[test]
    fn worker_admission_rejects_droppable_fault_handoff() {
        let mut config = admission();
        config.handoff.worker_fault_saturation = SaturationPolicy::RefuseNew;
        let error = config.validate(&temporal()).expect_err("droppable fault");
        assert!(error.to_string().contains("containment work"));

        let mut service = admission();
        service.handoff.service_fault_saturation = SaturationPolicy::RefuseNew;
        let error = service
            .validate(&temporal())
            .expect_err("droppable service fault");
        assert!(error.to_string().contains("containment work"));
    }

    #[test]
    fn worker_admission_rejects_missing_fixed_reply_object() {
        let mut config = admission();
        config.fixed_objects.reply_objects -= 1;
        let error = config
            .validate(&temporal())
            .expect_err("every Reply lane requires one fixed object");
        assert!(error.to_string().contains("fixed Reply-object total"));
    }

    #[test]
    fn worker_admission_rejects_selected_kernel_object_size_drift() {
        let mut config = admission();
        config.object_bits.notification -= 1;
        let error = config
            .validate(&temporal())
            .expect_err("classic notification size cannot describe MCS");
        assert!(error.to_string().contains("selected-kernel object sizes"));
    }
}
