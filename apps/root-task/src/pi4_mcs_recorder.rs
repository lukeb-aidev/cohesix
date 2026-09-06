// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Retain bounded Pi 4 composer-quantum and scheduler-Yield timing without perturbing scheduling-context accounting.
// Author: Lukas Bower

use core::fmt::Write;

use heapless::String as HeaplessString;
use spin::Mutex;

use crate::serial::DEFAULT_LINE_CAPACITY;

const LATENCY_BUCKETS: usize = 7;
const PI4_COUNTER_HZ: u64 = 54_000_000;
const PI4_LATENCY_BUCKET_EDGES: [u64; LATENCY_BUCKETS - 1] =
    [54_000, 162_000, 324_000, 486_000, 648_000, 1_080_000];

/// The physical backend whose root-control composer quantum is being measured.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PiMcsLane {
    Wifi,
    Genet,
}

impl PiMcsLane {
    const fn index(self) -> usize {
        match self {
            Self::Wifi => 0,
            Self::Genet => 1,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Wifi => "wifi",
            Self::Genet => "genet",
        }
    }
}

/// Why the measured root-control unit returned to its caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PiMcsExit {
    Yield,
    Retain,
    Fence,
    Fault,
}

impl PiMcsExit {
    const COUNT: usize = 4;

    const fn index(self) -> usize {
        match self {
            Self::Yield => 0,
            Self::Retain => 1,
            Self::Fence => 2,
            Self::Fault => 3,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Yield => "YIELD",
            Self::Retain => "RETAIN",
            Self::Fence => "FENCE",
            Self::Fault => "FAULT",
        }
    }
}

/// One bounded physical-Pi root-control composer-quantum observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PiMcsQuantumRecord {
    pub lane: PiMcsLane,
    pub started_ticks: u64,
    pub finished_ticks: u64,
    pub counter_hz: u64,
    pub generation: u64,
    pub connection_id: u64,
    pub pending_before: u32,
    pub pending_after: u32,
    pub progress_mask: u32,
    pub exit: PiMcsExit,
}

/// Exact scheduler hiatus around one physical-Pi `Yield` boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PiMcsYieldRecord {
    pub lane: PiMcsLane,
    pub entered_ticks: u64,
    pub resumed_ticks: u64,
    pub counter_hz: u64,
    pub generation: u64,
    pub connection_id: u64,
    pub pending_mask: u32,
    pub trigger: PiMcsYieldTrigger,
    pub context: Option<PiMcsYieldContext>,
}

/// Existing durable levels at the pre-Yield cut, without additional clocks or IPC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PiMcsYieldContext {
    pub accepted_commands: u64,
    pub stages: u64,
    pub drains: u64,
    pub phase: u8,
    /// 0 unknown, 1 observed empty, 2 observed durable child publication.
    pub child_publication: u8,
}

/// Exclusive reason the root-control path crossed one explicit MCS Yield.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PiMcsYieldTrigger {
    ReserveGuard,
    NoProductiveSuccessor,
    PassiveAdmission,
    RecoveryFence,
    OperatorRotation,
    OtherBoundary,
}

impl PiMcsYieldTrigger {
    const COUNT: usize = 6;

    const fn index(self) -> usize {
        match self {
            Self::ReserveGuard => 0,
            Self::NoProductiveSuccessor => 1,
            Self::PassiveAdmission => 2,
            Self::RecoveryFence => 3,
            Self::OperatorRotation => 4,
            Self::OtherBoundary => 5,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::ReserveGuard => "RESERVE_GUARD",
            Self::NoProductiveSuccessor => "NO_PRODUCTIVE_SUCCESSOR",
            Self::PassiveAdmission => "PASSIVE_ADMISSION",
            Self::RecoveryFence => "RECOVERY_FENCE",
            Self::OperatorRotation => "OPERATOR_ROTATION",
            Self::OtherBoundary => "OTHER_BOUNDARY",
        }
    }
}

/// Reserve-admission cut that forced the WiFi supervisor to yield before a
/// composer or bootstrap unit could run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PiMcsBudgetGuardStage {
    Activation,
    Attached,
    BootstrapOperator,
    BootstrapDriver,
}

impl PiMcsBudgetGuardStage {
    const COUNT: usize = 4;

    const fn index(self) -> usize {
        match self {
            Self::Activation => 0,
            Self::Attached => 1,
            Self::BootstrapOperator => 2,
            Self::BootstrapDriver => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PiMcsTimingSummary {
    previous_started_ticks: u64,
    counter_hz: u64,
    samples: u64,
    material_samples: u64,
    period_samples: u64,
    invalid_period_samples: u64,
    pending_samples: u64,
    stalled_samples: u64,
    invalid_samples: u64,
    lane_samples: [u64; 2],
    exits: [u64; PiMcsExit::COUNT],
    period_ticks_total: u64,
    period_ticks_max: u64,
    run_ticks_total: u64,
    run_ticks_max: u64,
    bucket_edges_ticks: [u64; LATENCY_BUCKETS - 1],
    gap_buckets: [u64; LATENCY_BUCKETS],
    run_buckets: [u64; LATENCY_BUCKETS],
    last_lane: PiMcsLane,
    last_generation: u64,
    last_connection_id: u64,
    last_progress_mask: u32,
    last_pending_before: u32,
    last_pending_after: u32,
    last_period_ticks: u64,
    last_run_ticks: u64,
    last_exit: PiMcsExit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PiMcsCommandDispatchSummary {
    samples: u64,
    total_ms: u64,
    last_ms: u64,
    max_ms: u64,
    invalid_samples: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PiMcsYieldSummary {
    samples: u64,
    invalid_samples: u64,
    pending_samples: u64,
    lane_samples: [u64; 2],
    triggers: [u64; PiMcsYieldTrigger::COUNT],
    ticks_total: u64,
    ticks_max: u64,
    buckets: [u64; LATENCY_BUCKETS],
    bucket_edges_ticks: [u64; LATENCY_BUCKETS - 1],
    counter_hz: u64,
    last_lane: PiMcsLane,
    last_ticks: u64,
    last_generation: u64,
    last_connection_id: u64,
    last_pending_mask: u32,
    last_trigger: PiMcsYieldTrigger,
}

impl PiMcsYieldSummary {
    const fn new() -> Self {
        Self {
            samples: 0,
            invalid_samples: 0,
            pending_samples: 0,
            lane_samples: [0; 2],
            triggers: [0; PiMcsYieldTrigger::COUNT],
            ticks_total: 0,
            ticks_max: 0,
            buckets: [0; LATENCY_BUCKETS],
            bucket_edges_ticks: PI4_LATENCY_BUCKET_EDGES,
            counter_hz: 0,
            last_lane: PiMcsLane::Wifi,
            last_ticks: 0,
            last_generation: 0,
            last_connection_id: 0,
            last_pending_mask: 0,
            last_trigger: PiMcsYieldTrigger::OtherBoundary,
        }
    }

    fn record(&mut self, record: PiMcsYieldRecord) {
        let Some(hiatus_ticks) = record
            .resumed_ticks
            .checked_sub(record.entered_ticks)
            .filter(|_| {
                record.entered_ticks != 0
                    && record.resumed_ticks != 0
                    && record.counter_hz == PI4_COUNTER_HZ
            })
        else {
            self.invalid_samples = self.invalid_samples.saturating_add(1);
            return;
        };
        if self.counter_hz != 0 && self.counter_hz != record.counter_hz {
            self.invalid_samples = self.invalid_samples.saturating_add(1);
            return;
        }
        let bucket = latency_bucket_from_edges(hiatus_ticks, self.bucket_edges_ticks);
        self.samples = self.samples.saturating_add(1);
        self.lane_samples[record.lane.index()] =
            self.lane_samples[record.lane.index()].saturating_add(1);
        self.triggers[record.trigger.index()] =
            self.triggers[record.trigger.index()].saturating_add(1);
        self.pending_samples = self
            .pending_samples
            .saturating_add(u64::from(record.pending_mask != 0));
        self.ticks_total = self.ticks_total.saturating_add(hiatus_ticks);
        self.ticks_max = self.ticks_max.max(hiatus_ticks);
        self.buckets[bucket] = self.buckets[bucket].saturating_add(1);
        self.counter_hz = record.counter_hz;
        self.last_lane = record.lane;
        self.last_ticks = hiatus_ticks;
        self.last_generation = record.generation;
        self.last_connection_id = record.connection_id;
        self.last_pending_mask = record.pending_mask;
        self.last_trigger = record.trigger;
    }

    fn lines(self) -> [HeaplessString<DEFAULT_LINE_CAPACITY>; 5] {
        let average_ticks = if self.samples == 0 {
            0
        } else {
            self.ticks_total / self.samples
        };
        let total_us = ticks_to_us(self.ticks_total, self.counter_hz).unwrap_or(0);
        let average_us = ticks_to_us(average_ticks, self.counter_hz).unwrap_or(0);
        let max_us = ticks_to_us(self.ticks_max, self.counter_hz).unwrap_or(0);
        let last_us = ticks_to_us(self.last_ticks, self.counter_hz).unwrap_or(0);
        let mut counts = HeaplessString::new();
        let _ = write!(
            counts,
            "netstats: mcs_yield schema=v1 hz={} samples={} invalid={} pending={} wifi={} genet={}",
            self.counter_hz,
            self.samples,
            self.invalid_samples,
            self.pending_samples,
            self.lane_samples[PiMcsLane::Wifi.index()],
            self.lane_samples[PiMcsLane::Genet.index()],
        );
        let mut timing = HeaplessString::new();
        let _ = write!(
            timing,
            "netstats: mcs_yield_timing schema=v1 total_us={} avg_us={} max_us={}",
            total_us, average_us, max_us,
        );
        let mut buckets = HeaplessString::new();
        let _ = write!(
            buckets,
            "netstats: mcs_yield_hist schema=v1 bounds_us=1000,3000,6000,9000,12000,20000 buckets={},{},{},{},{},{},{}",
            self.buckets[0],
            self.buckets[1],
            self.buckets[2],
            self.buckets[3],
            self.buckets[4],
            self.buckets[5],
            self.buckets[6],
        );
        let mut trigger_primary = HeaplessString::new();
        let _ = write!(
            trigger_primary,
            "netstats: mcs_yield_cause schema=v2 reserve={} no_successor={} passive={} recovery={} operator={} other={}",
            self.triggers[PiMcsYieldTrigger::ReserveGuard.index()],
            self.triggers[PiMcsYieldTrigger::NoProductiveSuccessor.index()],
            self.triggers[PiMcsYieldTrigger::PassiveAdmission.index()],
            self.triggers[PiMcsYieldTrigger::RecoveryFence.index()],
            self.triggers[PiMcsYieldTrigger::OperatorRotation.index()],
            self.triggers[PiMcsYieldTrigger::OtherBoundary.index()],
        );
        let mut last = HeaplessString::new();
        let _ = write!(
            last,
            "netstats: mcs_yield_last schema=v1 lane={} generation={} conn={} pending=0x{:x} trigger={} hiatus_us={}",
            self.last_lane.label(),
            self.last_generation,
            self.last_connection_id,
            self.last_pending_mask,
            self.last_trigger.label(),
            last_us,
        );
        [counts, timing, buckets, trigger_primary, last]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PiMcsBudgetGuardSummary {
    total: [u64; PiMcsBudgetGuardStage::COUNT],
    pending: [u64; PiMcsBudgetGuardStage::COUNT],
    reasons: [u64; 4],
    reason_mask: u32,
}

impl PiMcsBudgetGuardSummary {
    const fn new() -> Self {
        Self {
            total: [0; PiMcsBudgetGuardStage::COUNT],
            pending: [0; PiMcsBudgetGuardStage::COUNT],
            reasons: [0; 4],
            reason_mask: 0,
        }
    }

    fn record(&mut self, stage: PiMcsBudgetGuardStage, pending_mask: u32, reason_mask: u32) {
        let index = stage.index();
        self.total[index] = self.total[index].saturating_add(1);
        if pending_mask != 0 {
            self.pending[index] = self.pending[index].saturating_add(1);
        }
        for (reason_index, reason_bit) in [1u32, 2, 4, 8].into_iter().enumerate() {
            if reason_mask & reason_bit != 0 {
                self.reasons[reason_index] = self.reasons[reason_index].saturating_add(1);
            }
        }
        self.reason_mask |= reason_mask;
    }

    fn lines(self) -> [HeaplessString<DEFAULT_LINE_CAPACITY>; 2] {
        let mut stages = HeaplessString::new();
        let _ = write!(
            stages,
            "netstats: mcs_budget_guard schema=v2 totals={},{},{},{} pending={},{},{},{}",
            self.total[PiMcsBudgetGuardStage::Activation.index()],
            self.total[PiMcsBudgetGuardStage::Attached.index()],
            self.total[PiMcsBudgetGuardStage::BootstrapOperator.index()],
            self.total[PiMcsBudgetGuardStage::BootstrapDriver.index()],
            self.pending[PiMcsBudgetGuardStage::Activation.index()],
            self.pending[PiMcsBudgetGuardStage::Attached.index()],
            self.pending[PiMcsBudgetGuardStage::BootstrapOperator.index()],
            self.pending[PiMcsBudgetGuardStage::BootstrapDriver.index()],
        );
        let mut reasons = HeaplessString::new();
        let _ = write!(
            reasons,
            "netstats: mcs_budget_reason schema=v1 cap={} clock={} reserve={} policy={} mask=0x{:x}",
            self.reasons[0],
            self.reasons[1],
            self.reasons[2],
            self.reasons[3],
            self.reason_mask,
        );
        [stages, reasons]
    }
}

impl PiMcsCommandDispatchSummary {
    const fn new() -> Self {
        Self {
            samples: 0,
            total_ms: 0,
            last_ms: 0,
            max_ms: 0,
            invalid_samples: 0,
        }
    }

    fn record(&mut self, published_ms: u64, dispatched_ms: u64) {
        let Some(age_ms) = dispatched_ms
            .checked_sub(published_ms)
            .filter(|_| published_ms != 0 && dispatched_ms != 0)
        else {
            self.invalid_samples = self.invalid_samples.saturating_add(1);
            return;
        };
        self.samples = self.samples.saturating_add(1);
        self.total_ms = self.total_ms.saturating_add(age_ms);
        self.last_ms = age_ms;
        self.max_ms = self.max_ms.max(age_ms);
    }

    fn line(self, name: &str) -> HeaplessString<DEFAULT_LINE_CAPACITY> {
        let average_ms = if self.samples == 0 {
            0
        } else {
            self.total_ms / self.samples
        };
        let mut line = HeaplessString::new();
        let _ = write!(
            line,
            "netstats: {} schema=v1 samples={} invalid={} avg_ms={} last_ms={} max_ms={}",
            name, self.samples, self.invalid_samples, average_ms, self.last_ms, self.max_ms,
        );
        line
    }
}

impl PiMcsTimingSummary {
    const fn new() -> Self {
        Self {
            previous_started_ticks: 0,
            counter_hz: 0,
            samples: 0,
            material_samples: 0,
            period_samples: 0,
            invalid_period_samples: 0,
            pending_samples: 0,
            stalled_samples: 0,
            invalid_samples: 0,
            lane_samples: [0; 2],
            exits: [0; PiMcsExit::COUNT],
            period_ticks_total: 0,
            period_ticks_max: 0,
            run_ticks_total: 0,
            run_ticks_max: 0,
            bucket_edges_ticks: PI4_LATENCY_BUCKET_EDGES,
            gap_buckets: [0; LATENCY_BUCKETS],
            run_buckets: [0; LATENCY_BUCKETS],
            last_lane: PiMcsLane::Wifi,
            last_generation: 0,
            last_connection_id: 0,
            last_progress_mask: 0,
            last_pending_before: 0,
            last_pending_after: 0,
            last_period_ticks: 0,
            last_run_ticks: 0,
            last_exit: PiMcsExit::Yield,
        }
    }

    fn record(&mut self, record: PiMcsQuantumRecord) {
        self.samples = self.samples.saturating_add(1);
        self.lane_samples[record.lane.index()] =
            self.lane_samples[record.lane.index()].saturating_add(1);
        self.exits[record.exit.index()] = self.exits[record.exit.index()].saturating_add(1);

        if record.counter_hz != PI4_COUNTER_HZ
            || record.started_ticks == 0
            || record.finished_ticks < record.started_ticks
            || (self.counter_hz != 0 && self.counter_hz != record.counter_hz)
        {
            self.invalid_samples = self.invalid_samples.saturating_add(1);
            return;
        }
        let period_ticks = if self.previous_started_ticks == 0 {
            None
        } else if record.started_ticks < self.previous_started_ticks {
            self.invalid_period_samples = self.invalid_period_samples.saturating_add(1);
            None
        } else {
            Some(record.started_ticks - self.previous_started_ticks)
        };
        self.previous_started_ticks = record.started_ticks;
        let run_ticks = record.finished_ticks - record.started_ticks;
        self.counter_hz = record.counter_hz;
        self.pending_samples = self
            .pending_samples
            .saturating_add(u64::from(record.pending_before != 0));
        self.stalled_samples = self.stalled_samples.saturating_add(u64::from(
            record.pending_before != 0 && record.progress_mask == 0 && record.pending_after != 0,
        ));
        self.last_lane = record.lane;
        self.last_generation = record.generation;
        self.last_connection_id = record.connection_id;
        self.last_progress_mask = record.progress_mask;
        self.last_pending_before = record.pending_before;
        self.last_pending_after = record.pending_after;
        self.last_period_ticks = period_ticks.unwrap_or(0);
        self.last_run_ticks = run_ticks;
        self.last_exit = record.exit;
        if record.pending_before == 0 && record.progress_mask == 0 && record.pending_after == 0 {
            return;
        }
        self.material_samples = self.material_samples.saturating_add(1);
        self.run_ticks_total = self.run_ticks_total.saturating_add(run_ticks);
        self.run_ticks_max = self.run_ticks_max.max(run_ticks);
        let run_bucket = latency_bucket_from_edges(run_ticks, self.bucket_edges_ticks);
        self.run_buckets[run_bucket] = self.run_buckets[run_bucket].saturating_add(1);
        if let Some(period_ticks) = period_ticks {
            self.period_samples = self.period_samples.saturating_add(1);
            self.period_ticks_total = self.period_ticks_total.saturating_add(period_ticks);
            self.period_ticks_max = self.period_ticks_max.max(period_ticks);
            let period_bucket = latency_bucket_from_edges(period_ticks, self.bucket_edges_ticks);
            self.gap_buckets[period_bucket] = self.gap_buckets[period_bucket].saturating_add(1);
        }
    }

    fn lines(self) -> [HeaplessString<DEFAULT_LINE_CAPACITY>; 8] {
        let period_average_ticks = if self.period_samples == 0 {
            0
        } else {
            self.period_ticks_total / self.period_samples
        };
        let run_average_ticks = if self.material_samples == 0 {
            0
        } else {
            self.run_ticks_total / self.material_samples
        };
        let period_average_us = ticks_to_us(period_average_ticks, self.counter_hz).unwrap_or(0);
        let period_max_us = ticks_to_us(self.period_ticks_max, self.counter_hz).unwrap_or(0);
        let run_average_us = ticks_to_us(run_average_ticks, self.counter_hz).unwrap_or(0);
        let run_max_us = ticks_to_us(self.run_ticks_max, self.counter_hz).unwrap_or(0);
        let period_total_us = ticks_to_us(self.period_ticks_total, self.counter_hz).unwrap_or(0);
        let run_total_us = ticks_to_us(self.run_ticks_total, self.counter_hz).unwrap_or(0);
        let last_period_us = ticks_to_us(self.last_period_ticks, self.counter_hz).unwrap_or(0);
        let last_run_us = ticks_to_us(self.last_run_ticks, self.counter_hz).unwrap_or(0);
        let mut counts = HeaplessString::new();
        let _ = write!(
            counts,
            "netstats: mcs_quantum schema=v1 hz={} samples={} material={} periods={} invalid={} invalid_period={}",
            self.counter_hz,
            self.samples,
            self.material_samples,
            self.period_samples,
            self.invalid_samples,
            self.invalid_period_samples,
        );
        let mut lanes = HeaplessString::new();
        let _ = write!(
            lanes,
            "netstats: mcs_quantum_lane schema=v1 wifi={} genet={}",
            self.lane_samples[PiMcsLane::Wifi.index()],
            self.lane_samples[PiMcsLane::Genet.index()],
        );
        let mut timing = HeaplessString::new();
        let _ = write!(
            timing,
            "netstats: mcs_quantum_timing schema=v1 period_avg_us={} period_max_us={} run_avg_us={} run_max_us={}",
            period_average_us,
            period_max_us,
            run_average_us,
            run_max_us,
        );
        let mut totals = HeaplessString::new();
        let _ = write!(
            totals,
            "netstats: mcs_quantum_total schema=v1 period_us={} run_us={}",
            period_total_us, run_total_us,
        );
        let mut gap_buckets = HeaplessString::new();
        let _ = write!(
            gap_buckets,
            "netstats: mcs_quantum_period schema=v1 bounds_us=1000,3000,6000,9000,12000,20000 buckets={},{},{},{},{},{},{}",
            self.gap_buckets[0],
            self.gap_buckets[1],
            self.gap_buckets[2],
            self.gap_buckets[3],
            self.gap_buckets[4],
            self.gap_buckets[5],
            self.gap_buckets[6],
        );
        let mut run_buckets = HeaplessString::new();
        let _ = write!(
            run_buckets,
            "netstats: mcs_quantum_run schema=v1 bounds_us=1000,3000,6000,9000,12000,20000 buckets={},{},{},{},{},{},{}",
            self.run_buckets[0],
            self.run_buckets[1],
            self.run_buckets[2],
            self.run_buckets[3],
            self.run_buckets[4],
            self.run_buckets[5],
            self.run_buckets[6],
        );
        let mut last = HeaplessString::new();
        let _ = write!(
            last,
            "netstats: mcs_quantum_last schema=v1 lane={} generation={} conn={} progress=0x{:x} pending=0x{:x}->0x{:x} period_us={} run_us={} exit={}",
            self.last_lane.label(),
            self.last_generation,
            self.last_connection_id,
            self.last_progress_mask,
            self.last_pending_before,
            self.last_pending_after,
            last_period_us,
            last_run_us,
            self.last_exit.label(),
        );
        let mut exits = HeaplessString::new();
        let _ = write!(
            exits,
            "netstats: mcs_quantum_exit schema=v2 yields={} retains={} fences={} faults={} pending={} stalled={}",
            self.exits[PiMcsExit::Yield.index()],
            self.exits[PiMcsExit::Retain.index()],
            self.exits[PiMcsExit::Fence.index()],
            self.exits[PiMcsExit::Fault.index()],
            self.pending_samples,
            self.stalled_samples,
        );
        [
            counts,
            lanes,
            totals,
            timing,
            gap_buckets,
            run_buckets,
            last,
            exits,
        ]
    }
}

fn ticks_to_us(ticks: u64, counter_hz: u64) -> Option<u64> {
    if counter_hz == 0 {
        return None;
    }
    let scaled = u128::from(ticks).saturating_mul(1_000_000);
    u64::try_from(scaled / u128::from(counter_hz)).ok()
}

fn latency_bucket_from_edges(ticks: u64, edges: [u64; LATENCY_BUCKETS - 1]) -> usize {
    edges
        .into_iter()
        .position(|edge| ticks < edge)
        .unwrap_or(LATENCY_BUCKETS - 1)
}

static TIMING: Mutex<PiMcsTimingSummary> = Mutex::new(PiMcsTimingSummary::new());
static COMMAND_DISPATCH: Mutex<PiMcsCommandDispatchSummary> =
    Mutex::new(PiMcsCommandDispatchSummary::new());
static OBSERVE_DISPATCH: Mutex<PiMcsCommandDispatchSummary> =
    Mutex::new(PiMcsCommandDispatchSummary::new());
static YIELD: Mutex<PiMcsYieldSummary> = Mutex::new(PiMcsYieldSummary::new());
static BUDGET_GUARD: Mutex<PiMcsBudgetGuardSummary> = Mutex::new(PiMcsBudgetGuardSummary::new());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PiMcsIdleCut {
    BeforeEnable,
    AfterEnable,
    TimerRejected,
}

/// Count existing predicate samples without adding a timer, wake or admission.
struct PiMcsIdleSummary {
    cuts: [u64; 3],
    clear: [u64; 2],
    fences: [u64; 16],
    last_mask: u32,
    last_cut: u8,
}

impl PiMcsIdleSummary {
    const fn new() -> Self {
        Self {
            cuts: [0; 3],
            clear: [0; 2],
            fences: [0; 16],
            last_mask: 0,
            last_cut: 0,
        }
    }

    fn record(&mut self, cut: PiMcsIdleCut, mask: u32) {
        let index = match cut {
            PiMcsIdleCut::BeforeEnable => 0,
            PiMcsIdleCut::AfterEnable => 1,
            PiMcsIdleCut::TimerRejected => 2,
        };
        self.cuts[index] = self.cuts[index].saturating_add(1);
        if index < 2 && mask == 0 {
            self.clear[index] = self.clear[index].saturating_add(1);
        }
        for (bit, count) in self.fences.iter_mut().enumerate() {
            if mask & (1 << bit) != 0 {
                *count = count.saturating_add(1);
            }
        }
        self.last_mask = mask;
        self.last_cut = index as u8;
    }

    fn lines(&self) -> [HeaplessString<DEFAULT_LINE_CAPACITY>; 3] {
        let mut lines = core::array::from_fn(|_| HeaplessString::new());
        let _ = write!(lines[0],
            "netstats: mcs_idle schema=v1 before={} after={} timer_reject={} clear={}/{} last_cut={} mask=0x{:04x}",
            self.cuts[0], self.cuts[1], self.cuts[2], self.clear[0], self.clear[1], self.last_cut, self.last_mask);
        for group in 0..2 {
            let i = group * 8;
            let _ = write!(
                lines[group + 1],
                "netstats: mcs_idle_fences schema=v2 base={} counts={},{},{},{},{},{},{},{}",
                i,
                self.fences[i],
                self.fences[i + 1],
                self.fences[i + 2],
                self.fences[i + 3],
                self.fences[i + 4],
                self.fences[i + 5],
                self.fences[i + 6],
                self.fences[i + 7]
            );
        }
        lines
    }
}

static IDLE: Mutex<PiMcsIdleSummary> = Mutex::new(PiMcsIdleSummary::new());

/// Retain the latest nonzero TCP identity across disconnect. Later UART
/// diagnostic typing must not overwrite the session's idle/Yield evidence.
struct PiMcsSessionSummary {
    generation: u64,
    connection: u64,
    idle: PiMcsIdleSummary,
    operator: [u64; 6],
    yields: u64,
    yield_us: u64,
    yield_max_us: u64,
    yield_invalid: u64,
    yield_causes: [u32; PiMcsYieldTrigger::COUNT],
    worst_yield: Option<PiMcsYieldRecord>,
}

impl PiMcsSessionSummary {
    const fn new() -> Self {
        Self {
            generation: 0,
            connection: 0,
            idle: PiMcsIdleSummary::new(),
            operator: [0; 6],
            yields: 0,
            yield_us: 0,
            yield_max_us: 0,
            yield_invalid: 0,
            yield_causes: [0; PiMcsYieldTrigger::COUNT],
            worst_yield: None,
        }
    }

    fn select(&mut self, generation: u64, connection: u64) -> bool {
        if generation == 0 || connection == 0 {
            return false;
        }
        if (generation, connection) != (self.generation, self.connection) {
            *self = Self::new();
            self.generation = generation;
            self.connection = connection;
        }
        true
    }

    fn record_idle(
        &mut self,
        generation: u64,
        connection: u64,
        cut: PiMcsIdleCut,
        mask: u32,
        operator: u8,
    ) {
        if !self.select(generation, connection) {
            return;
        }
        self.idle.record(cut, mask);
        for (bit, count) in self.operator.iter_mut().enumerate() {
            if operator & (1 << bit) != 0 {
                *count = count.saturating_add(1);
            }
        }
    }

    fn record_yield(&mut self, record: PiMcsYieldRecord) {
        if !self.select(record.generation, record.connection_id) {
            return;
        }
        let Some(us) = (record.entered_ticks != 0)
            .then_some(record.resumed_ticks)
            .and_then(|resumed| resumed.checked_sub(record.entered_ticks))
            .and_then(|ticks| ticks_to_us(ticks, record.counter_hz))
        else {
            self.yield_invalid = self.yield_invalid.saturating_add(1);
            return;
        };
        self.yields = self.yields.saturating_add(1);
        let cause = &mut self.yield_causes[record.trigger.index()];
        *cause = cause.saturating_add(1);
        self.yield_us = self.yield_us.saturating_add(us);
        if self.worst_yield.is_none() || us > self.yield_max_us {
            self.yield_max_us = us;
            self.worst_yield = Some(record);
        }
    }

    fn lines(&self) -> [HeaplessString<DEFAULT_LINE_CAPACITY>; 8] {
        let mut lines = core::array::from_fn(|_| HeaplessString::new());
        let _ = write!(lines[0], "netstats: mcs_session schema=v1 generation={} conn={} before={} after={} timer_reject={} clear={}/{}",
            self.generation, self.connection, self.idle.cuts[0], self.idle.cuts[1], self.idle.cuts[2], self.idle.clear[0], self.idle.clear[1]);
        for group in 0..4 {
            let i = group * 4;
            let _ = write!(
                lines[group + 1],
                "netstats: mcs_session_fences schema=v1 base={} counts={},{},{},{}",
                i,
                self.idle.fences[i],
                self.idle.fences[i + 1],
                self.idle.fences[i + 2],
                self.idle.fences[i + 3]
            );
        }
        let _ = write!(lines[5], "netstats: mcs_session_operator schema=v1 serial_rx={} serial_line={} local_line={} local_chunk={} usb_bytes={} usb_service={}",
            self.operator[0], self.operator[1], self.operator[2], self.operator[3], self.operator[4], self.operator[5]);
        let _ = write!(
            lines[6],
            "netstats: mcs_session_yield schema=v1 samples={} total_us={} max_us={} invalid={} causes={},{},{},{},{},{}",
            self.yields, self.yield_us, self.yield_max_us, self.yield_invalid,
            self.yield_causes[0], self.yield_causes[1], self.yield_causes[2],
            self.yield_causes[3], self.yield_causes[4], self.yield_causes[5]
        );
        match self
            .worst_yield
            .and_then(|record| record.context.map(|context| (record, context)))
        {
            Some((record, context)) => {
                let _ = write!(lines[7],
                    "netstats: mcs_session_yield_cut schema=v1 cause={} pending={:x} phase={} pub={} cmd={:x} stage={:x} drain={:x} ticks={:x}/{:x}",
                    record.trigger.label(), record.pending_mask, context.phase,
                    context.child_publication, context.accepted_commands, context.stages,
                    context.drains, record.entered_ticks, record.resumed_ticks);
            }
            None => {
                let _ = write!(
                    lines[7],
                    "netstats: mcs_session_yield_cut schema=v1 absent=yes"
                );
            }
        }
        lines
    }
}

static SESSION: Mutex<PiMcsSessionSummary> = Mutex::new(PiMcsSessionSummary::new());

pub(crate) fn record_session_idle(
    generation: u64,
    connection: u64,
    cut: PiMcsIdleCut,
    mask: u32,
    operator: u8,
) {
    SESSION
        .lock()
        .record_idle(generation, connection, cut, mask, operator);
}

pub(crate) fn session_snapshot_lines() -> [HeaplessString<DEFAULT_LINE_CAPACITY>; 8] {
    SESSION.lock().lines()
}

pub(crate) fn record_idle_fence(cut: PiMcsIdleCut, mask: u32) {
    IDLE.lock().record(cut, mask);
}

pub(crate) fn idle_snapshot_lines() -> [HeaplessString<DEFAULT_LINE_CAPACITY>; 3] {
    IDLE.lock().lines()
}

pub(crate) fn record_quantum(record: PiMcsQuantumRecord) {
    TIMING.lock().record(record);
}

pub(crate) fn record_yield(record: PiMcsYieldRecord) {
    YIELD.lock().record(record);
    if record.generation != 0 && record.connection_id != 0 {
        SESSION.lock().record_yield(record);
    }
}

pub(crate) fn record_budget_guard(
    stage: PiMcsBudgetGuardStage,
    pending_mask: u32,
    reason_mask: u32,
) {
    BUDGET_GUARD.lock().record(stage, pending_mask, reason_mask);
}

pub(crate) fn record_command_dispatch(
    published_ms: u64,
    observed_ms: Option<u64>,
    dispatched_ms: u64,
) {
    COMMAND_DISPATCH.lock().record(published_ms, dispatched_ms);
    if let Some(observed_ms) = observed_ms {
        OBSERVE_DISPATCH.lock().record(observed_ms, dispatched_ms);
    }
}

pub(crate) fn snapshot_lines() -> [HeaplessString<DEFAULT_LINE_CAPACITY>; 17] {
    let timing = TIMING.lock().lines();
    let yield_lines = YIELD.lock().lines();
    let budget_guard = BUDGET_GUARD.lock().lines();
    let dispatch = COMMAND_DISPATCH.lock().line("mcs_command_dispatch");
    let observe_dispatch = OBSERVE_DISPATCH.lock().line("mcs_observe_dispatch");
    [
        timing[0].clone(),
        timing[1].clone(),
        timing[2].clone(),
        timing[3].clone(),
        timing[4].clone(),
        timing[5].clone(),
        timing[6].clone(),
        timing[7].clone(),
        dispatch,
        observe_dispatch,
        yield_lines[0].clone(),
        yield_lines[1].clone(),
        yield_lines[2].clone(),
        yield_lines[3].clone(),
        yield_lines[4].clone(),
        budget_guard[0].clone(),
        budget_guard[1].clone(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_summary_retains_disconnect_and_resets_on_new_identity() {
        let mut summary = PiMcsSessionSummary::new();
        summary.record_idle(7, 11, PiMcsIdleCut::BeforeEnable, 1 << 3, 0b000010);
        summary.record_idle(7, 11, PiMcsIdleCut::AfterEnable, 0, 0);
        let sample = PiMcsYieldRecord {
            lane: PiMcsLane::Genet,
            entered_ticks: 100,
            resumed_ticks: 540_100,
            counter_hz: 54_000_000,
            generation: 7,
            connection_id: 11,
            pending_mask: 0,
            trigger: PiMcsYieldTrigger::NoProductiveSuccessor,
            context: Some(PiMcsYieldContext {
                accepted_commands: 55,
                stages: 55,
                drains: 54,
                phase: 3,
                child_publication: 2,
            }),
        };
        summary.record_yield(sample);
        summary.record_idle(7, 0, PiMcsIdleCut::BeforeEnable, 0xffff, 0x3f);
        summary.record_yield(PiMcsYieldRecord {
            connection_id: 0,
            ..sample
        });
        assert_eq!(summary.idle.cuts, [1, 1, 0]);
        assert_eq!(summary.idle.clear, [0, 1]);
        assert_eq!(summary.operator, [0, 1, 0, 0, 0, 0]);
        assert_eq!(
            (summary.yields, summary.yield_us, summary.yield_max_us),
            (1, 10_000, 10_000)
        );
        summary.record_yield(PiMcsYieldRecord {
            resumed_ticks: 99,
            ..sample
        });
        summary.record_yield(PiMcsYieldRecord {
            counter_hz: 0,
            ..sample
        });
        summary.record_yield(PiMcsYieldRecord {
            entered_ticks: 0,
            ..sample
        });
        assert_eq!(summary.yield_invalid, 3);
        assert_eq!(summary.yield_causes, [0, 1, 0, 0, 0, 0]);
        assert_eq!(summary.worst_yield, Some(sample));
        summary.record_yield(PiMcsYieldRecord {
            resumed_ticks: 270_100,
            context: None,
            ..sample
        });
        assert_eq!(summary.worst_yield, Some(sample));
        assert!(summary.lines()[7].contains("phase=3 pub=2 cmd=37 stage=37 drain=36"));
        assert!(summary.lines()[6].ends_with("causes=0,2,0,0,0,0"));
        summary.record_idle(8, 11, PiMcsIdleCut::BeforeEnable, 0, 0);
        assert_eq!((summary.generation, summary.connection), (8, 11));
        assert!(summary.worst_yield.is_none());
        assert_eq!(summary.yield_causes, [0; 6]);
        assert_eq!(summary.idle.cuts, [1, 0, 0]);
        assert_eq!(summary.operator, [0; 6]);
        assert_eq!(
            (summary.yields, summary.yield_us, summary.yield_invalid),
            (0, 0, 0)
        );
        summary.record_idle(8, 12, PiMcsIdleCut::TimerRejected, 1 << 15, 0);
        assert_eq!(summary.idle.cuts, [0, 0, 1]);
    }

    #[test]
    fn session_summary_saturates_and_preserves_complete_diagnostic_rows() {
        let mut summary = PiMcsSessionSummary::new();
        summary.select(u64::MAX, u64::MAX);
        summary.idle.cuts = [u64::MAX; 3];
        summary.idle.clear = [u64::MAX; 2];
        summary.idle.fences = [u64::MAX; 16];
        summary.operator = [u64::MAX; 6];
        summary.yields = u64::MAX;
        summary.yield_us = u64::MAX;
        summary.yield_max_us = u64::MAX;
        summary.yield_invalid = u64::MAX;
        summary.yield_causes = [u32::MAX; 6];
        let sample = PiMcsYieldRecord {
            lane: PiMcsLane::Wifi,
            entered_ticks: 54,
            resumed_ticks: 108,
            counter_hz: PI4_COUNTER_HZ,
            generation: u64::MAX,
            connection_id: u64::MAX,
            pending_mask: 0,
            trigger: PiMcsYieldTrigger::ReserveGuard,
            context: None,
        };
        summary.worst_yield = Some(sample);
        summary.record_yield(sample);
        assert_eq!(summary.yield_causes, [u32::MAX; 6]);
        assert_eq!(summary.yields, u64::MAX);
        assert_eq!(summary.yield_us, u64::MAX);
        assert_eq!(summary.yield_max_us, u64::MAX);
        summary.record_idle(u64::MAX, u64::MAX, PiMcsIdleCut::BeforeEnable, 0xffff, 0x3f);
        assert_eq!(summary.idle.fences, [u64::MAX; 16]);
        assert_eq!(summary.operator, [u64::MAX; 6]);
        let lines = summary.lines();
        assert!(lines[0].ends_with("clear=18446744073709551615/18446744073709551615"));
        assert!(lines[5].ends_with("usb_service=18446744073709551615"));
        assert!(lines[6].contains("invalid=18446744073709551615 causes="));
        assert!(lines[6]
            .ends_with("causes=4294967295,4294967295,4294967295,4294967295,4294967295,4294967295"));
        for line in lines {
            assert!(line.len() < DEFAULT_LINE_CAPACITY);
        }
    }

    #[test]
    fn idle_fence_summary_separates_clear_race_and_timer_rejection() {
        let mut summary = PiMcsIdleSummary::new();
        summary.record(PiMcsIdleCut::BeforeEnable, (1 << 3) | (1 << 5));
        summary.record(PiMcsIdleCut::BeforeEnable, 0);
        summary.record(PiMcsIdleCut::AfterEnable, 1 << 14);
        summary.record(PiMcsIdleCut::TimerRejected, 1 << 15);
        assert_eq!(summary.cuts, [2, 1, 1]);
        assert_eq!(summary.clear, [1, 0]);
        assert_eq!(
            summary.fences,
            [0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1]
        );
        assert_eq!(summary.last_cut, 2);
        assert_eq!(summary.last_mask, 0x8000);
        assert_eq!(
            summary.lines()[1],
            "netstats: mcs_idle_fences schema=v2 base=0 counts=0,0,0,1,0,1,0,0"
        );
        assert_eq!(
            summary.lines()[2],
            "netstats: mcs_idle_fences schema=v2 base=8 counts=0,0,0,0,0,0,1,1"
        );
        summary.cuts = [u64::MAX; 3];
        summary.clear = [u64::MAX; 2];
        summary.fences = [u64::MAX; 16];
        summary.record(PiMcsIdleCut::BeforeEnable, 0xffff);
        assert_eq!(summary.cuts, [u64::MAX; 3]);
        assert_eq!(summary.fences, [u64::MAX; 16]);
        assert_eq!(summary.lines()[2], "netstats: mcs_idle_fences schema=v2 base=8 counts=18446744073709551615,18446744073709551615,18446744073709551615,18446744073709551615,18446744073709551615,18446744073709551615,18446744073709551615,18446744073709551615");
        for line in summary.lines() {
            assert!(!line.is_empty());
            assert!(line.len() < DEFAULT_LINE_CAPACITY);
        }
    }

    fn record(
        lane: PiMcsLane,
        started_us: u64,
        finished_us: u64,
        progress_mask: u32,
    ) -> PiMcsQuantumRecord {
        PiMcsQuantumRecord {
            lane,
            started_ticks: started_us.saturating_mul(54),
            finished_ticks: finished_us.saturating_mul(54),
            counter_hz: PI4_COUNTER_HZ,
            generation: 7,
            connection_id: 9,
            pending_before: 0,
            pending_after: 0,
            progress_mask,
            exit: PiMcsExit::Yield,
        }
    }

    #[test]
    fn material_summary_separates_quantum_period_from_run_time() {
        let mut summary = PiMcsTimingSummary::new();
        summary.record(record(PiMcsLane::Wifi, 1_000, 1_100, 0));
        summary.record(record(PiMcsLane::Wifi, 11_000, 11_400, 0x2));
        summary.record(record(PiMcsLane::Genet, 31_000, 39_500, 0x4));

        assert_eq!(summary.samples, 3);
        assert_eq!(summary.material_samples, 2);
        assert_eq!(summary.period_ticks_total, 30_000 * 54);
        assert_eq!(summary.run_ticks_total, 8_900 * 54);
        assert_eq!(summary.gap_buckets[4], 1);
        assert_eq!(summary.gap_buckets[6], 1);
        assert_eq!(summary.run_buckets[0], 1);
        assert_eq!(summary.run_buckets[3], 1);
        assert_eq!(summary.last_progress_mask, 0x4);
        let lines = summary.lines();
        assert!(lines.iter().all(|line| !line.is_empty()));
        assert!(lines.iter().all(|line| line.len() < DEFAULT_LINE_CAPACITY));
        assert!(lines
            .iter()
            .any(|line| line.starts_with("netstats: mcs_quantum_period schema=v1")));
    }

    #[test]
    fn pending_quantum_without_progress_is_retained_as_stall_evidence() {
        let mut summary = PiMcsTimingSummary::new();
        let mut stalled = record(PiMcsLane::Genet, 10_000, 10_200, 0);
        stalled.pending_before = 0x3;
        stalled.pending_after = 0x2;
        summary.record(stalled);
        assert_eq!(summary.samples, 1);
        assert_eq!(summary.material_samples, 1);
        assert_eq!(summary.pending_samples, 1);
        assert_eq!(summary.stalled_samples, 1);
        assert_eq!(summary.last_pending_before, 0x3);
        assert_eq!(summary.last_pending_after, 0x2);
    }

    #[test]
    fn yield_summary_retains_exact_pending_scheduler_hiatus() {
        let mut summary = PiMcsYieldSummary::new();
        summary.record(PiMcsYieldRecord {
            lane: PiMcsLane::Wifi,
            entered_ticks: 1_000 * 54,
            resumed_ticks: 11_000 * 54,
            counter_hz: PI4_COUNTER_HZ,
            generation: 7,
            connection_id: 9,
            pending_mask: 0x40,
            trigger: PiMcsYieldTrigger::ReserveGuard,
            context: None,
        });
        assert_eq!(summary.samples, 1);
        assert_eq!(summary.pending_samples, 1);
        assert_eq!(summary.ticks_total, 10_000 * 54);
        assert_eq!(summary.buckets[4], 1);
        assert!(summary.lines()[1].contains("avg_us=10000"));
        assert!(summary.lines()[3].contains("reserve=1"));
        assert!(summary.lines()[4].contains("trigger=RESERVE_GUARD"));
    }

    #[test]
    fn invalid_frequency_and_backward_run_fail_closed() {
        assert_eq!(ticks_to_us(1, 0), None);
        let mut summary = PiMcsTimingSummary::new();
        let mut invalid = record(PiMcsLane::Genet, 20, 10, 1);
        invalid.counter_hz = 0;
        summary.record(invalid);
        assert_eq!(summary.invalid_samples, 1);
        assert_eq!(summary.material_samples, 0);
    }

    #[test]
    fn backward_period_is_invalid_and_never_enters_cadence_totals() {
        let mut summary = PiMcsTimingSummary::new();
        let mut first = record(PiMcsLane::Genet, 20_000, 20_100, 1);
        first.pending_before = 1;
        summary.record(first);
        let mut backward = record(PiMcsLane::Genet, 10_000, 10_100, 1);
        backward.pending_before = 1;
        summary.record(backward);

        assert_eq!(summary.material_samples, 2);
        assert_eq!(summary.period_samples, 0);
        assert_eq!(summary.invalid_period_samples, 1);
        assert_eq!(summary.period_ticks_total, 0);
        assert_eq!(summary.gap_buckets, [0; LATENCY_BUCKETS]);
    }

    #[test]
    fn latency_buckets_have_strict_stable_edges() {
        let edges = PI4_LATENCY_BUCKET_EDGES;
        assert_eq!(latency_bucket_from_edges(999 * 54, edges), 0);
        assert_eq!(latency_bucket_from_edges(1_000 * 54, edges), 1);
        assert_eq!(latency_bucket_from_edges(2_999 * 54, edges), 1);
        assert_eq!(latency_bucket_from_edges(3_000 * 54, edges), 2);
        assert_eq!(latency_bucket_from_edges(8_999 * 54, edges), 3);
        assert_eq!(latency_bucket_from_edges(9_000 * 54, edges), 4);
        assert_eq!(latency_bucket_from_edges(20_000 * 54, edges), 6);
    }

    #[test]
    fn command_dispatch_age_is_monotonic_and_bounded() {
        let mut summary = PiMcsCommandDispatchSummary::new();
        summary.record(100, 107);
        summary.record(200, 211);
        summary.record(0, 220);
        summary.record(230, 229);
        assert_eq!(summary.samples, 2);
        assert_eq!(summary.total_ms, 18);
        assert_eq!(summary.last_ms, 11);
        assert_eq!(summary.max_ms, 11);
        assert_eq!(summary.invalid_samples, 2);
        let line = summary.line("mcs_command_dispatch");
        assert!(line.contains("avg_ms=9"));
        assert!(line.len() < DEFAULT_LINE_CAPACITY);
    }

    #[test]
    fn saturated_diagnostic_rows_remain_complete_and_bounded() {
        let mut timing = PiMcsTimingSummary::new();
        timing.counter_hz = PI4_COUNTER_HZ;
        timing.samples = u64::MAX;
        timing.material_samples = u64::MAX;
        timing.period_samples = u64::MAX;
        timing.invalid_period_samples = u64::MAX;
        timing.pending_samples = u64::MAX;
        timing.stalled_samples = u64::MAX;
        timing.invalid_samples = u64::MAX;
        timing.lane_samples = [u64::MAX; 2];
        timing.exits = [u64::MAX; PiMcsExit::COUNT];
        timing.period_ticks_total = u64::MAX;
        timing.period_ticks_max = u64::MAX;
        timing.run_ticks_total = u64::MAX;
        timing.run_ticks_max = u64::MAX;
        timing.gap_buckets = [u64::MAX; LATENCY_BUCKETS];
        timing.run_buckets = [u64::MAX; LATENCY_BUCKETS];
        timing.last_generation = u64::MAX;
        timing.last_connection_id = u64::MAX;
        timing.last_progress_mask = u32::MAX;
        timing.last_pending_before = u32::MAX;
        timing.last_pending_after = u32::MAX;
        timing.last_period_ticks = u64::MAX;
        timing.last_run_ticks = u64::MAX;

        let timing_lines = timing.lines();
        assert!(timing_lines
            .iter()
            .all(|line| !line.is_empty() && line.len() < DEFAULT_LINE_CAPACITY));
        assert_eq!(timing_lines[7], "netstats: mcs_quantum_exit schema=v2 yields=18446744073709551615 retains=18446744073709551615 fences=18446744073709551615 faults=18446744073709551615 pending=18446744073709551615 stalled=18446744073709551615");

        let mut yields = PiMcsYieldSummary::new();
        yields.samples = u64::MAX;
        yields.invalid_samples = u64::MAX;
        yields.pending_samples = u64::MAX;
        yields.lane_samples = [u64::MAX; 2];
        yields.triggers = [u64::MAX; PiMcsYieldTrigger::COUNT];
        yields.ticks_total = u64::MAX;
        yields.ticks_max = u64::MAX;
        yields.buckets = [u64::MAX; LATENCY_BUCKETS];
        yields.counter_hz = PI4_COUNTER_HZ;
        yields.last_generation = u64::MAX;
        yields.last_connection_id = u64::MAX;
        yields.last_pending_mask = u32::MAX;
        yields.last_ticks = u64::MAX;
        let yield_lines = yields.lines();
        assert!(yield_lines
            .iter()
            .all(|line| !line.is_empty() && line.len() < DEFAULT_LINE_CAPACITY));
        assert_eq!(yield_lines[3], "netstats: mcs_yield_cause schema=v2 reserve=18446744073709551615 no_successor=18446744073709551615 passive=18446744073709551615 recovery=18446744073709551615 operator=18446744073709551615 other=18446744073709551615");

        let mut session = PiMcsSessionSummary::new();
        session.worst_yield = Some(PiMcsYieldRecord {
            lane: PiMcsLane::Genet,
            entered_ticks: u64::MAX,
            resumed_ticks: u64::MAX,
            counter_hz: PI4_COUNTER_HZ,
            generation: u64::MAX,
            connection_id: u64::MAX,
            pending_mask: u32::MAX,
            trigger: PiMcsYieldTrigger::NoProductiveSuccessor,
            context: Some(PiMcsYieldContext {
                accepted_commands: u64::MAX,
                stages: u64::MAX,
                drains: u64::MAX,
                phase: u8::MAX,
                child_publication: u8::MAX,
            }),
        });
        let row = &session.lines()[7];
        assert!(row.len() < DEFAULT_LINE_CAPACITY);
        assert!(row.ends_with("ticks=ffffffffffffffff/ffffffffffffffff"));

        let mut guards = PiMcsBudgetGuardSummary::new();
        guards.total = [u64::MAX; PiMcsBudgetGuardStage::COUNT];
        guards.pending = [u64::MAX; PiMcsBudgetGuardStage::COUNT];
        guards.reasons = [u64::MAX; 4];
        guards.reason_mask = u32::MAX;
        let guard_lines = guards.lines();
        assert!(guard_lines
            .iter()
            .all(|line| !line.is_empty() && line.len() < DEFAULT_LINE_CAPACITY));
        assert_eq!(guard_lines[0], "netstats: mcs_budget_guard schema=v2 totals=18446744073709551615,18446744073709551615,18446744073709551615,18446744073709551615 pending=18446744073709551615,18446744073709551615,18446744073709551615,18446744073709551615");
    }
}
