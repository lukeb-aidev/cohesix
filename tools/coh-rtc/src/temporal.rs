// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define and validate compiler-owned SMP+MCS temporal-authority records.
// Author: Lukas Bower

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const M26E_CORE_COUNT: u8 = 4;
const M26E_DOMAIN_COUNT: u8 = 1;
const MIN_SCHED_CONTEXT_BITS: u8 = 7;
const MAX_TEMPORAL_TASKS: usize = 64;
const MAX_RESPONSE_TIME_ITERATIONS: usize = 128;
const MAX_TASK_ID_BYTES: usize = 64;
const MAX_WCET_PROVENANCE_BYTES: usize = 160;
const REQUIRED_CRITICAL_TASKS: [&str; 5] = [
    "root-control",
    "root-fault",
    "root-emergency",
    "root-worker-supervisor",
    "root-driver-supervisor",
];

/// Kernel scheduler architecture selected by an operational manifest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchedulerArchitecture {
    /// Pre-26e compatibility state. It is never valid when temporal authority is enabled.
    #[default]
    Classic,
    /// Four-core, one-domain seL4 SMP+MCS.
    SmpMcs,
}

/// How a task obtains CPU-time authority.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemporalExecution {
    /// A scheduling context is bound directly to the task TCB.
    #[default]
    Active,
    /// A bounded synchronous server runs only with an allowlisted donated SC.
    Passive,
}

/// Security-relevant task classification used by admission and evidence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemporalTaskKind {
    /// Root/Queen authoritative mutation path.
    #[default]
    RootControl,
    /// Independent root fault supervisor.
    RootFault,
    /// Minimal root emergency terminal handler.
    RootEmergency,
    /// Worker lifecycle supervisor.
    WorkerSupervisor,
    /// Linked-driver containment supervisor.
    DriverSupervisor,
    /// Restricted namespace or protocol service.
    Service,
    /// Manifest-declared isolated device runtime.
    Driver,
    /// Executable Worker task.
    Worker,
    /// Low-priority bounded drain.
    Drain,
}

/// Generated timeout or overrun action.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimeoutPolicy {
    /// Suspend and tear down the exact task generation.
    #[default]
    Terminal,
    /// Install no timeout endpoint and let seL4 postpone until replenishment.
    NaturalPostpone,
    /// Apply one generated replenishment and reply exactly once.
    ReplenishOnce,
    /// Return one typed failure to a blocked caller, then contain the task.
    ReturnError,
    /// Stop without attempting recursive recovery.
    FailStop,
}

/// One compiler-owned live-TCB scheduling and fault-routing record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TemporalTaskConfig {
    pub id: String,
    pub kind: TemporalTaskKind,
    pub execution: TemporalExecution,
    pub core: u8,
    pub scheduling_context_slot: u32,
    pub scheduling_context_bits: u8,
    pub sched_control_core: u8,
    pub budget_us: u32,
    pub period_us: u32,
    pub deadline_us: u32,
    pub blocking_us: u32,
    pub jitter_us: u32,
    pub max_refills: u8,
    pub priority: u8,
    pub mcp: u8,
    /// Non-zero identity reserved by SC configuration. The badge is delivered
    /// only when the TCB has a timeout endpoint installed.
    pub timeout_badge: u64,
    pub timeout_policy: TimeoutPolicy,
    /// Whether the active SC exposes kernel consumed-time accounting. This
    /// does not assert that budget exhaustion delivers timeout IPC.
    pub consumed_time_evidence: bool,
    pub wcet_us: u32,
    pub response_time_us: u32,
    pub admitted: bool,
    pub wcet_provenance: String,
    pub virtio_operator_serial_io_bytes_per_turn: u32,
    pub allowed_donors: Vec<String>,
    pub reply_objects: u8,
    pub max_donation_depth: u8,
    pub fault_handler: String,
    pub critical_reserve: bool,
    pub locality_bound: bool,
}

impl TemporalTaskConfig {
    fn validate_local(&self, cores: u8) -> Result<()> {
        validate_name("temporal_authority.tasks.id", &self.id, MAX_TASK_ID_BYTES)?;
        validate_name(
            "temporal_authority.tasks.wcet_provenance",
            &self.wcet_provenance,
            MAX_WCET_PROVENANCE_BYTES,
        )?;
        if self.core >= cores {
            bail!(
                "temporal task {} selects core {} outside 0..{}",
                self.id,
                self.core,
                cores
            );
        }
        if self.priority > self.mcp {
            bail!(
                "temporal task {} priority {} exceeds MCP {}",
                self.id,
                self.priority,
                self.mcp
            );
        }
        if self.timeout_badge == 0 {
            bail!(
                "temporal task {} requires a non-zero timeout badge",
                self.id
            );
        }
        if self.kind != TemporalTaskKind::RootControl
            && self.virtio_operator_serial_io_bytes_per_turn != 0
        {
            bail!(
                "non-root-control temporal task {} must not declare a VirtIO Operator serial I/O byte bound",
                self.id
            );
        }
        match self.execution {
            TemporalExecution::Active => {
                if self.scheduling_context_slot == 0 {
                    bail!("active temporal task {} has no SC slot", self.id);
                }
                if self.scheduling_context_bits < MIN_SCHED_CONTEXT_BITS {
                    bail!(
                        "active temporal task {} SC bits {} are below {}",
                        self.id,
                        self.scheduling_context_bits,
                        MIN_SCHED_CONTEXT_BITS
                    );
                }
                if self.sched_control_core != self.core {
                    bail!(
                        "active temporal task {} uses SchedControl core {} on core {}",
                        self.id,
                        self.sched_control_core,
                        self.core
                    );
                }
                if self.budget_us == 0 || self.period_us == 0 || self.budget_us > self.period_us {
                    bail!(
                        "active temporal task {} requires 0 < budget <= period",
                        self.id
                    );
                }
                if self.deadline_us == 0 || self.deadline_us > self.period_us {
                    bail!(
                        "active temporal task {} requires 0 < deadline <= period",
                        self.id
                    );
                }
                if self.jitter_us >= self.deadline_us {
                    bail!(
                        "active temporal task {} jitter must be below its deadline",
                        self.id
                    );
                }
                // seL4 always provides two base replenishments and accepts an
                // `extra_refills` syscall argument.  The manifest records the
                // total bound, so values below the base cardinality are invalid.
                if self.max_refills < 2 {
                    bail!("active temporal task {} requires max_refills >= 2", self.id);
                }
                if !self.consumed_time_evidence {
                    bail!(
                        "active temporal task {} requires kernel SC consumed-time accounting",
                        self.id
                    );
                }
                if self.wcet_us == 0 || self.wcet_us > self.budget_us {
                    bail!(
                        "active temporal task {} requires 0 < WCET <= budget",
                        self.id
                    );
                }
                if self.response_time_us == 0 {
                    bail!(
                        "active temporal task {} requires a compiler-checked response-time result",
                        self.id
                    );
                }
                self.wcet_us.checked_add(self.blocking_us).ok_or_else(|| {
                    anyhow::anyhow!(
                        "active temporal task {} WCET plus blocking overflows",
                        self.id
                    )
                })?;
                if !self.allowed_donors.is_empty()
                    || self.reply_objects != 0
                    || self.max_donation_depth != 0
                {
                    bail!(
                        "active temporal task {} must not declare passive donation state",
                        self.id
                    );
                }
            }
            TemporalExecution::Passive => {
                if self.timeout_policy == TimeoutPolicy::NaturalPostpone {
                    bail!(
                        "passive temporal task {} cannot select natural MCS postponement",
                        self.id
                    );
                }
                if self.scheduling_context_slot != 0
                    || self.scheduling_context_bits != 0
                    || self.budget_us != 0
                    || self.period_us != 0
                    || self.deadline_us != 0
                    || self.blocking_us != 0
                    || self.jitter_us != 0
                    || self.max_refills != 0
                    || self.consumed_time_evidence
                    || self.wcet_us != 0
                    || self.response_time_us != 0
                    || self.admitted
                {
                    bail!(
                        "passive temporal task {} must not own active scheduling state",
                        self.id
                    );
                }
                if self.allowed_donors.is_empty()
                    || self.reply_objects == 0
                    || self.max_donation_depth == 0
                {
                    bail!(
                        "passive temporal task {} requires donors, Reply objects, and donation depth",
                        self.id
                    );
                }
            }
        }
        Ok(())
    }
}

/// Offline per-core admission ceiling and reserved slack.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TemporalCoreAdmission {
    pub core: u8,
    pub capacity_us: u32,
    pub reserve_us: u32,
}

/// Complete generated scheduling, donation, and fault-routing contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TemporalAuthorityConfig {
    pub enabled: bool,
    pub architecture: SchedulerArchitecture,
    pub cores: u8,
    pub domains: u8,
    pub admission_window_us: u32,
    pub core_admission: Vec<TemporalCoreAdmission>,
    pub tasks: Vec<TemporalTaskConfig>,
}

impl Default for TemporalAuthorityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            architecture: SchedulerArchitecture::Classic,
            cores: M26E_CORE_COUNT,
            domains: M26E_DOMAIN_COUNT,
            admission_window_us: 0,
            core_admission: Vec::new(),
            tasks: Vec::new(),
        }
    }
}

impl TemporalAuthorityConfig {
    /// Validate the complete topology and offline admission arithmetic.
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            if self.architecture != SchedulerArchitecture::Classic
                || self.admission_window_us != 0
                || !self.core_admission.is_empty()
                || !self.tasks.is_empty()
            {
                bail!(
                    "disabled temporal_authority must not claim an architecture, admission, or live tasks"
                );
            }
            return Ok(());
        }
        if self.architecture != SchedulerArchitecture::SmpMcs {
            bail!("enabled temporal_authority requires architecture=smp-mcs");
        }
        if self.cores != M26E_CORE_COUNT || self.domains != M26E_DOMAIN_COUNT {
            bail!("Milestone 26e temporal_authority requires four cores and one domain");
        }
        if self.admission_window_us == 0 {
            bail!("temporal_authority.admission_window_us must be > 0");
        }
        if self.tasks.is_empty() || self.tasks.len() > MAX_TEMPORAL_TASKS {
            bail!(
                "temporal_authority.tasks count must be in 1..={} (got {})",
                MAX_TEMPORAL_TASKS,
                self.tasks.len()
            );
        }

        let mut admission = BTreeMap::<u8, &TemporalCoreAdmission>::new();
        for entry in &self.core_admission {
            if entry.core >= self.cores {
                bail!(
                    "temporal core-admission core {} is out of range",
                    entry.core
                );
            }
            if entry.capacity_us != self.admission_window_us {
                bail!(
                    "temporal core {} capacity {} does not match admission window {}",
                    entry.core,
                    entry.capacity_us,
                    self.admission_window_us
                );
            }
            if entry.reserve_us >= entry.capacity_us {
                bail!("temporal core {} reserve exhausts its capacity", entry.core);
            }
            if admission.insert(entry.core, entry).is_some() {
                bail!("temporal core-admission duplicates core {}", entry.core);
            }
        }
        if admission.len() != usize::from(self.cores) {
            bail!("temporal_authority requires one admission record per core");
        }

        let mut ids = BTreeSet::<&str>::new();
        let mut sc_slots = BTreeSet::<u32>::new();
        let mut timeout_badges = BTreeSet::<u64>::new();
        let mut demand = vec![0u32; usize::from(self.cores)];
        for task in &self.tasks {
            task.validate_local(self.cores)?;
            if !ids.insert(task.id.as_str()) {
                bail!("temporal_authority duplicates task id {}", task.id);
            }
            if !timeout_badges.insert(task.timeout_badge) {
                bail!(
                    "temporal_authority duplicates timeout badge 0x{:x}",
                    task.timeout_badge
                );
            }
            if task.execution == TemporalExecution::Active {
                if !sc_slots.insert(task.scheduling_context_slot) {
                    bail!(
                        "temporal_authority duplicates SC slot {}",
                        task.scheduling_context_slot
                    );
                }
                if !self.admission_window_us.is_multiple_of(task.period_us) {
                    bail!(
                        "temporal task {} period {} does not divide admission window {}",
                        task.id,
                        task.period_us,
                        self.admission_window_us
                    );
                }
                let releases = self.admission_window_us / task.period_us;
                let contribution = task.budget_us.checked_mul(releases).ok_or_else(|| {
                    anyhow::anyhow!("temporal task {} admission demand overflows", task.id)
                })?;
                demand[usize::from(task.core)] = demand[usize::from(task.core)]
                    .checked_add(contribution)
                    .ok_or_else(|| {
                        anyhow::anyhow!("temporal core {} admission demand overflows", task.core)
                    })?;
            }
        }

        for required in REQUIRED_CRITICAL_TASKS {
            let task = self
                .tasks
                .iter()
                .find(|task| task.id == required)
                .ok_or_else(|| anyhow::anyhow!("missing critical temporal task {required}"))?;
            let expected_kind = match required {
                "root-control" => TemporalTaskKind::RootControl,
                "root-fault" => TemporalTaskKind::RootFault,
                "root-emergency" => TemporalTaskKind::RootEmergency,
                "root-worker-supervisor" => TemporalTaskKind::WorkerSupervisor,
                "root-driver-supervisor" => TemporalTaskKind::DriverSupervisor,
                _ => bail!("unsupported critical temporal task {required}"),
            };
            let expected_sc_bits = if required == "root-control" {
                MIN_SCHED_CONTEXT_BITS
            } else {
                8
            };
            if task.kind != expected_kind
                || task.execution != TemporalExecution::Active
                || !task.critical_reserve
                || task.scheduling_context_bits != expected_sc_bits
            {
                bail!(
                    "critical temporal task {} requires its exact kind, SC size {}, and an independent active reserve",
                    required,
                    expected_sc_bits
                );
            }
        }

        self.validate_fault_graph(&ids)?;
        self.validate_donors(&ids)?;
        self.validate_response_times()?;
        for core in 0..self.cores {
            let record = admission[&core];
            let usable = record.capacity_us - record.reserve_us;
            if demand[usize::from(core)] > usable {
                bail!(
                    "temporal core {} demand {} exceeds admitted capacity {} after reserve",
                    core,
                    demand[usize::from(core)],
                    usable
                );
            }
        }
        Ok(())
    }

    /// Recompute each active task's fixed-priority response-time result.
    ///
    /// The recurrence includes declared lower-priority blocking, release
    /// jitter, and interference from every other task on the same core at an
    /// equal or higher priority. Equal-priority peers are included because
    /// seL4's FIFO ordering permits each peer to be ahead of the task under
    /// analysis. The manifest result is evidence, not an input: any mismatch
    /// or missed deadline fails compilation.
    fn validate_response_times(&self) -> Result<()> {
        for task in self
            .tasks
            .iter()
            .filter(|task| task.execution == TemporalExecution::Active)
        {
            let base = task.wcet_us.checked_add(task.blocking_us).ok_or_else(|| {
                anyhow::anyhow!("temporal task {} response-time base overflows", task.id)
            })?;
            let mut window = base;
            let mut converged = false;
            for _ in 0..MAX_RESPONSE_TIME_ITERATIONS {
                let mut interference = 0u32;
                for interferer in self.tasks.iter().filter(|candidate| {
                    candidate.execution == TemporalExecution::Active
                        && candidate.core == task.core
                        && candidate.id != task.id
                        && candidate.priority >= task.priority
                }) {
                    let release_window =
                        window.checked_add(interferer.jitter_us).ok_or_else(|| {
                            anyhow::anyhow!(
                                "temporal task {} interference window overflows",
                                task.id
                            )
                        })?;
                    let releases = release_window
                        .checked_add(interferer.period_us - 1)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "temporal task {} release calculation overflows",
                                task.id
                            )
                        })?
                        / interferer.period_us;
                    let demand = releases.checked_mul(interferer.wcet_us).ok_or_else(|| {
                        anyhow::anyhow!("temporal task {} interference demand overflows", task.id)
                    })?;
                    interference = interference.checked_add(demand).ok_or_else(|| {
                        anyhow::anyhow!(
                            "temporal task {} cumulative interference overflows",
                            task.id
                        )
                    })?;
                }
                let next = base.checked_add(interference).ok_or_else(|| {
                    anyhow::anyhow!("temporal task {} response time overflows", task.id)
                })?;
                if next == window {
                    converged = true;
                    break;
                }
                if next < window {
                    bail!(
                        "temporal task {} response-time recurrence regressed",
                        task.id
                    );
                }
                window = next;
                if window
                    .checked_add(task.jitter_us)
                    .is_none_or(|response| response > task.deadline_us)
                {
                    bail!(
                        "temporal task {} response-time recurrence exceeds deadline {}",
                        task.id,
                        task.deadline_us
                    );
                }
            }
            if !converged {
                bail!(
                    "temporal task {} response-time recurrence did not converge",
                    task.id
                );
            }
            let response = window.checked_add(task.jitter_us).ok_or_else(|| {
                anyhow::anyhow!("temporal task {} response time overflows", task.id)
            })?;
            let admitted = response <= task.deadline_us;
            if task.response_time_us != response || task.admitted != admitted {
                bail!(
                    "temporal task {} response-time result mismatch: declared response={} admitted={}, computed response={} admitted={}",
                    task.id,
                    task.response_time_us,
                    task.admitted,
                    response,
                    admitted
                );
            }
            if !admitted {
                bail!(
                    "temporal task {} response time {} exceeds deadline {}",
                    task.id,
                    response,
                    task.deadline_us
                );
            }
        }
        Ok(())
    }

    fn validate_donors(&self, ids: &BTreeSet<&str>) -> Result<()> {
        for task in &self.tasks {
            for donor in &task.allowed_donors {
                if donor == &task.id || !ids.contains(donor.as_str()) {
                    bail!(
                        "passive temporal task {} has invalid donor {}",
                        task.id,
                        donor
                    );
                }
                let donor_task = self
                    .tasks
                    .iter()
                    .find(|candidate| candidate.id == *donor)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "passive temporal task {} has unknown donor {}",
                            task.id,
                            donor
                        )
                    })?;
                if donor_task.execution != TemporalExecution::Active {
                    bail!(
                        "passive temporal task {} donor {} is not active",
                        task.id,
                        donor
                    );
                }
                if task.locality_bound && donor_task.core != task.core {
                    bail!(
                        "locality-bound temporal task {} has cross-core donor {}",
                        task.id,
                        donor
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_fault_graph(&self, ids: &BTreeSet<&str>) -> Result<()> {
        let root_fault = self
            .tasks
            .iter()
            .find(|task| task.id == "root-fault")
            .ok_or_else(|| anyhow::anyhow!("missing critical temporal task root-fault"))?;
        if root_fault.fault_handler != "root-emergency" {
            bail!("root-fault must route faults to root-emergency");
        }
        let root_emergency = self
            .tasks
            .iter()
            .find(|task| task.id == "root-emergency")
            .ok_or_else(|| anyhow::anyhow!("missing critical temporal task root-emergency"))?;
        if !root_emergency.fault_handler.is_empty()
            || root_emergency.timeout_policy != TimeoutPolicy::FailStop
        {
            bail!("root-emergency must have no handler and fail stop");
        }
        for task in &self.tasks {
            if task.id == "root-emergency" {
                continue;
            }
            if task.fault_handler.is_empty() || !ids.contains(task.fault_handler.as_str()) {
                bail!(
                    "temporal task {} has missing or unknown fault handler {}",
                    task.id,
                    task.fault_handler
                );
            }
            if task.id != "root-fault" && task.fault_handler != "root-fault" {
                bail!(
                    "temporal task {} must route faults directly to root-fault",
                    task.id
                );
            }
        }
        for task in &self.tasks {
            let mut current = task;
            let mut seen = BTreeSet::new();
            while !current.fault_handler.is_empty() {
                if !seen.insert(current.id.as_str()) {
                    bail!("temporal fault graph contains a cycle at {}", current.id);
                }
                current = self
                    .tasks
                    .iter()
                    .find(|candidate| candidate.id == current.fault_handler)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "temporal task {} has unknown fault handler {}",
                            current.id,
                            current.fault_handler
                        )
                    })?;
            }
            if current.id != "root-emergency" {
                bail!(
                    "temporal task {} fault path does not terminate at root-emergency",
                    task.id
                );
            }
        }
        Ok(())
    }
}

fn validate_name(field: &str, value: &str, max_bytes: usize) -> Result<()> {
    if value.is_empty() || value.len() > max_bytes {
        bail!("{} length must be in 1..={}", field, max_bytes);
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
    }) {
        bail!("{} contains unsupported characters", field);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(id: &str, kind: TemporalTaskKind, core: u8, slot: u32) -> TemporalTaskConfig {
        TemporalTaskConfig {
            id: id.to_owned(),
            kind,
            execution: TemporalExecution::Active,
            core,
            scheduling_context_slot: slot,
            scheduling_context_bits: MIN_SCHED_CONTEXT_BITS,
            sched_control_core: core,
            budget_us: 500,
            period_us: 10_000,
            deadline_us: 10_000,
            blocking_us: 0,
            jitter_us: 0,
            max_refills: 2,
            priority: 100,
            mcp: 100,
            timeout_badge: 0x260e_0000 + u64::from(slot),
            timeout_policy: if id == "root-emergency" {
                TimeoutPolicy::FailStop
            } else {
                TimeoutPolicy::Terminal
            },
            consumed_time_evidence: true,
            wcet_us: 400,
            response_time_us: 400,
            admitted: true,
            wcet_provenance: "m26e-qemu-probe-v1".to_owned(),
            virtio_operator_serial_io_bytes_per_turn: if kind == TemporalTaskKind::RootControl {
                64
            } else {
                0
            },
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
            critical_reserve: id.starts_with("root-"),
            locality_bound: false,
        }
    }

    fn valid_config() -> TemporalAuthorityConfig {
        let mut tasks = vec![
            active("root-control", TemporalTaskKind::RootControl, 0, 1),
            active("root-fault", TemporalTaskKind::RootFault, 0, 2),
            active("root-emergency", TemporalTaskKind::RootEmergency, 0, 3),
            active(
                "root-worker-supervisor",
                TemporalTaskKind::WorkerSupervisor,
                1,
                4,
            ),
            active(
                "root-driver-supervisor",
                TemporalTaskKind::DriverSupervisor,
                1,
                5,
            ),
        ];
        for task in &mut tasks[1..] {
            task.scheduling_context_bits = 8;
        }
        // All five test duties share one priority and period. Equal-priority
        // peers on the same core are conservatively included as interference.
        tasks[0].response_time_us = 1_200;
        tasks[1].response_time_us = 1_200;
        tasks[2].response_time_us = 1_200;
        tasks[3].response_time_us = 800;
        tasks[4].response_time_us = 800;
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
                    reserve_us: 500,
                })
                .collect(),
            tasks,
        }
    }

    #[test]
    fn exact_critical_topology_is_admitted() {
        valid_config().validate().expect("valid MCS topology");
    }

    #[test]
    fn natural_postpone_retains_kernel_consumed_time_accounting_contract() {
        let mut config = valid_config();
        config.tasks[0].timeout_policy = TimeoutPolicy::NaturalPostpone;
        config
            .validate()
            .expect("natural postponement retains SC accounting without timeout IPC");

        config.tasks[0].consumed_time_evidence = false;
        assert!(config
            .validate()
            .expect_err("active SC accounting remains required")
            .to_string()
            .contains("requires kernel SC consumed-time accounting"));
    }

    #[test]
    fn virtio_operator_serial_io_bound_is_root_control_only() {
        let mut misplaced = valid_config();
        misplaced.tasks[1].virtio_operator_serial_io_bytes_per_turn = 64;
        assert!(misplaced
            .validate()
            .expect_err("serial bound is root-control-only")
            .to_string()
            .contains("must not declare a VirtIO Operator serial I/O byte bound"));
    }

    #[test]
    fn missing_independent_critical_reserve_fails() {
        let mut config = valid_config();
        config.tasks[0].critical_reserve = false;
        let error = config.validate().expect_err("critical reserve required");
        assert!(error.to_string().contains("independent active reserve"));
    }

    #[test]
    fn duplicate_sc_and_invalid_fault_routes_fail_closed() {
        let mut duplicate = valid_config();
        duplicate.tasks[1].scheduling_context_slot = 1;
        assert!(duplicate
            .validate()
            .expect_err("duplicate SC")
            .to_string()
            .contains("duplicates SC slot"));

        let mut cycle = valid_config();
        cycle.tasks[0].fault_handler = "root-control".to_owned();
        assert!(cycle
            .validate()
            .expect_err("invalid fault route")
            .to_string()
            .contains("directly to root-fault"));
    }

    #[test]
    fn passive_locality_requires_every_donor_on_the_same_core() {
        let mut config = valid_config();
        let mut service = active("passive-service", TemporalTaskKind::Service, 0, 0);
        service.execution = TemporalExecution::Passive;
        service.scheduling_context_slot = 0;
        service.scheduling_context_bits = 0;
        service.budget_us = 0;
        service.period_us = 0;
        service.deadline_us = 0;
        service.blocking_us = 0;
        service.jitter_us = 0;
        service.max_refills = 0;
        service.timeout_policy = TimeoutPolicy::ReturnError;
        service.consumed_time_evidence = false;
        service.wcet_us = 0;
        service.response_time_us = 0;
        service.admitted = false;
        service.virtio_operator_serial_io_bytes_per_turn = 0;
        service.allowed_donors = vec!["root-control".to_owned()];
        service.reply_objects = 1;
        service.max_donation_depth = 1;
        service.critical_reserve = false;
        service.locality_bound = true;
        config.tasks.push(service);

        config
            .validate()
            .expect("passive service accepts a co-located allowlisted donor");

        config.tasks.last_mut().expect("passive service").core = 1;
        assert!(config
            .validate()
            .expect_err("cross-core passive donor must fail locality")
            .to_string()
            .contains("has cross-core donor root-control"));
    }

    #[test]
    fn per_core_overcommit_fails_closed() {
        let mut config = valid_config();
        config.tasks[0].budget_us = 9_500;
        let error = config.validate().expect_err("overcommit");
        assert!(error.to_string().contains("demand") && error.to_string().contains("capacity"));
    }

    #[test]
    fn response_time_results_are_recomputed_not_trusted() {
        let mut config = valid_config();
        config.tasks[0].response_time_us = 1_199;
        let error = config.validate().expect_err("stale result must fail");
        assert!(error.to_string().contains("response-time result mismatch"));
    }

    #[test]
    fn blocking_and_jitter_can_make_an_admission_miss_its_deadline() {
        let mut config = valid_config();
        config.tasks[0].deadline_us = 1_300;
        config.tasks[0].blocking_us = 200;
        config.tasks[0].jitter_us = 100;
        let error = config.validate().expect_err("deadline miss must fail");
        assert!(error.to_string().contains("exceeds deadline"));
    }
}
