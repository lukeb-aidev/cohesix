// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Retain bounded QEMU root-control activation records for post-run performance diagnosis.
// Author: Lukas Bower

use core::fmt::Write;

use heapless::{Deque, String as HeaplessString, Vec as HeaplessVec};
use spin::Mutex;

use crate::serial::DEFAULT_LINE_CAPACITY;

/// Three summary records plus this ring exactly fit NineDoor's 64-line stream.
const ACTIVATION_RECORD_CAPACITY: usize = 61;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RootControlPhase {
    Operator,
    Runtime,
    Network,
    Response,
}

impl RootControlPhase {
    const fn label(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Runtime => "runtime",
            Self::Network => "network",
            Self::Response => "response",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActivationExitReason {
    NoWork,
    Block,
    Quota,
    BudgetGuard,
    Yield,
    Timeout,
    Fault,
}

impl ActivationExitReason {
    const COUNT: usize = 7;

    const fn index(self) -> usize {
        match self {
            Self::NoWork => 0,
            Self::Block => 1,
            Self::Quota => 2,
            Self::BudgetGuard => 3,
            Self::Yield => 4,
            Self::Timeout => 5,
            Self::Fault => 6,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::NoWork => "NO_WORK",
            Self::Block => "BLOCK",
            Self::Quota => "QUOTA",
            Self::BudgetGuard => "BUDGET_GUARD",
            Self::Yield => "YIELD",
            Self::Timeout => "TIMEOUT",
            Self::Fault => "FAULT",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RootControlActivationRecord {
    pub timestamp_ticks: u64,
    pub counter_hz: u64,
    pub gap_ticks: u64,
    pub run_ticks: u64,
    pub sequence: u64,
    pub generation: u64,
    pub phase: RootControlPhase,
    pub queue_depth: u32,
    pub work_available: u32,
    pub work_completed: u32,
    pub work_remaining: u32,
    pub service_units: u16,
    pub exit_reason: ActivationExitReason,
}

impl RootControlActivationRecord {
    const fn material(self) -> bool {
        self.queue_depth != 0
            || self.work_remaining != 0
            || !matches!(self.exit_reason, ActivationExitReason::NoWork)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FlightSummary {
    activations: u64,
    material_activations: u64,
    dropped_records: u64,
    counter_hz: u64,
    gap_samples: u64,
    gap_ticks_total: u64,
    gap_ticks_min: u64,
    gap_ticks_max: u64,
    run_ticks_total: u64,
    run_ticks_max: u64,
    service_units_total: u64,
    service_units_max: u16,
    queue_high_water: u32,
    work_available_total: u64,
    work_completed_total: u64,
    work_remaining_last: u32,
    exits: [u64; ActivationExitReason::COUNT],
}

impl FlightSummary {
    const fn new() -> Self {
        Self {
            activations: 0,
            material_activations: 0,
            dropped_records: 0,
            counter_hz: 0,
            gap_samples: 0,
            gap_ticks_total: 0,
            gap_ticks_min: 0,
            gap_ticks_max: 0,
            run_ticks_total: 0,
            run_ticks_max: 0,
            service_units_total: 0,
            service_units_max: 0,
            queue_high_water: 0,
            work_available_total: 0,
            work_completed_total: 0,
            work_remaining_last: 0,
            exits: [0; ActivationExitReason::COUNT],
        }
    }

    fn observe(&mut self, record: RootControlActivationRecord) {
        self.activations = self.activations.saturating_add(1);
        self.counter_hz = record.counter_hz;
        if record.sequence > 1 && record.gap_ticks != 0 {
            self.gap_samples = self.gap_samples.saturating_add(1);
            self.gap_ticks_total = self.gap_ticks_total.saturating_add(record.gap_ticks);
            self.gap_ticks_min = if self.gap_ticks_min == 0 {
                record.gap_ticks
            } else {
                self.gap_ticks_min.min(record.gap_ticks)
            };
            self.gap_ticks_max = self.gap_ticks_max.max(record.gap_ticks);
        }
        self.run_ticks_total = self.run_ticks_total.saturating_add(record.run_ticks);
        self.run_ticks_max = self.run_ticks_max.max(record.run_ticks);
        self.service_units_total = self
            .service_units_total
            .saturating_add(u64::from(record.service_units));
        self.service_units_max = self.service_units_max.max(record.service_units);
        self.queue_high_water = self.queue_high_water.max(record.queue_depth);
        if record.material() {
            self.work_available_total = self
                .work_available_total
                .saturating_add(u64::from(record.work_available));
            self.work_completed_total = self
                .work_completed_total
                .saturating_add(u64::from(record.work_completed));
        }
        self.work_remaining_last = record.work_remaining;
        self.exits[record.exit_reason.index()] =
            self.exits[record.exit_reason.index()].saturating_add(1);
        if record.material() {
            self.material_activations = self.material_activations.saturating_add(1);
        }
    }
}

struct FlightRing {
    records: Deque<RootControlActivationRecord, ACTIVATION_RECORD_CAPACITY>,
    summary: FlightSummary,
}

impl FlightRing {
    const fn new() -> Self {
        Self {
            records: Deque::new(),
            summary: FlightSummary::new(),
        }
    }

    fn record(&mut self, record: RootControlActivationRecord) {
        self.summary.observe(record);
        if !record.material() {
            return;
        }
        if self.records.is_full() {
            let _ = self.records.pop_front();
            self.summary.dropped_records = self.summary.dropped_records.saturating_add(1);
        }
        let _ = self.records.push_back(record);
    }

    fn snapshot_lines_into<const LIMIT: usize>(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, LIMIT>,
    ) {
        output.clear();
        let gap_average = if self.summary.gap_samples == 0 {
            0
        } else {
            self.summary.gap_ticks_total / self.summary.gap_samples
        };
        let run_average = if self.summary.activations == 0 {
            0
        } else {
            self.summary.run_ticks_total / self.summary.activations
        };
        let unit_average = if self.summary.activations == 0 {
            0
        } else {
            self.summary.service_units_total / self.summary.activations
        };
        let mut summary = HeaplessString::new();
        let _ = write!(
            summary,
            "QEMU_FLIGHT_SUMMARY schema=v1 c=root-control ev=quantum hz={} a={} active={} retained={} dropped={} log_contention_drops={} uart_lock_deferrals={} q_hwm={}",
            self.summary.counter_hz,
            self.summary.activations,
            self.summary.material_activations,
            self.records.len(),
            self.summary.dropped_records,
            crate::log_buffer::contention_dropped_writes(),
            crate::serial::qemu_uart_tx_lock_deferrals(),
            self.summary.queue_high_water,
        );
        let _ = output.push(summary);

        let mut timing_summary = HeaplessString::new();
        let _ = write!(
            timing_summary,
            "QEMU_FLIGHT_TIMING schema=v1 gap_min={} gap_max={} gap_avg={} run_max={} run_avg={} units_max={} units_avg={}",
            self.summary.gap_ticks_min,
            self.summary.gap_ticks_max,
            gap_average,
            self.summary.run_ticks_max,
            run_average,
            self.summary.service_units_max,
            unit_average,
        );
        let _ = output.push(timing_summary);

        let exits = self.summary.exits;
        let mut exit_summary = HeaplessString::new();
        let _ = write!(
            exit_summary,
            "QEMU_FLIGHT_EXITS schema=v1 NO_WORK={} BLOCK={} QUOTA={} BUDGET_GUARD={} YIELD={} TIMEOUT={} FAULT={} avail={} done={} rem={}",
            exits[ActivationExitReason::NoWork.index()],
            exits[ActivationExitReason::Block.index()],
            exits[ActivationExitReason::Quota.index()],
            exits[ActivationExitReason::BudgetGuard.index()],
            exits[ActivationExitReason::Yield.index()],
            exits[ActivationExitReason::Timeout.index()],
            exits[ActivationExitReason::Fault.index()],
            self.summary.work_available_total,
            self.summary.work_completed_total,
            self.summary.work_remaining_last,
        );
        let _ = output.push(exit_summary);

        for record in &self.records {
            if output.is_full() {
                break;
            }
            let mut line = HeaplessString::new();
            let _ = write!(
                line,
                "QEMU_FLIGHT schema=v1 ts={} c=root-control ev=quantum phase={} seq={} gen={} gap={} q={} avail={} done={} rem={} units={} run={} exit={}",
                record.timestamp_ticks,
                record.phase.label(),
                record.sequence,
                record.generation,
                record.gap_ticks,
                record.queue_depth,
                record.work_available,
                record.work_completed,
                record.work_remaining,
                record.service_units,
                record.run_ticks,
                record.exit_reason.label(),
            );
            let _ = output.push(line);
        }
    }
}

static FLIGHT_RING: Mutex<FlightRing> = Mutex::new(FlightRing::new());

pub(crate) fn record_root_control_activation(record: RootControlActivationRecord) {
    FLIGHT_RING.lock().record(record);
}

pub(crate) fn snapshot_lines_into<const LIMIT: usize>(
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, LIMIT>,
) {
    FLIGHT_RING.lock().snapshot_lines_into(output);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(sequence: u64, work: u32) -> RootControlActivationRecord {
        RootControlActivationRecord {
            timestamp_ticks: sequence.saturating_mul(240_000),
            counter_hz: 24_000_000,
            gap_ticks: if sequence == 1 { 0 } else { 240_000 },
            run_ticks: 2_400,
            sequence,
            generation: 7,
            phase: RootControlPhase::Operator,
            queue_depth: work,
            work_available: work,
            work_completed: work,
            work_remaining: 0,
            service_units: 15,
            exit_reason: ActivationExitReason::NoWork,
        }
    }

    #[test]
    fn fixed_ring_retains_material_tail_and_accounts_overwrite() {
        let mut ring = FlightRing::new();
        ring.record(record(1, 0));
        assert!(
            ring.records.is_empty(),
            "idle activations stay aggregate-only"
        );
        for sequence in 2..=(ACTIVATION_RECORD_CAPACITY as u64 + 3) {
            ring.record(record(sequence, 1));
        }
        assert_eq!(ring.records.len(), ACTIVATION_RECORD_CAPACITY);
        assert_eq!(ring.summary.dropped_records, 2);
        assert_eq!(
            ring.summary.activations,
            ACTIVATION_RECORD_CAPACITY as u64 + 3
        );

        let mut lines: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, 64> = HeaplessVec::new();
        ring.snapshot_lines_into(&mut lines);
        assert_eq!(lines.len(), 64);
        assert!(lines[0].starts_with("QEMU_FLIGHT_SUMMARY schema=v1"));
        assert!(lines[1].starts_with("QEMU_FLIGHT_TIMING schema=v1"));
        assert!(lines[2].contains("BUDGET_GUARD=0"));
        assert!(lines[3].contains("seq=4 "));
        assert!(lines[63].contains("seq=64 "));
    }

    #[test]
    fn transient_idle_flags_do_not_displace_queued_work() {
        let mut ring = FlightRing::new();
        let mut idle = record(1, 0);
        idle.work_available = 2;
        idle.work_completed = 2;
        ring.record(idle);

        assert!(ring.records.is_empty());
        assert_eq!(ring.summary.material_activations, 0);
        assert_eq!(ring.summary.work_available_total, 0);
        assert_eq!(ring.summary.work_completed_total, 0);
    }

    #[test]
    fn exit_reason_labels_match_post_run_decoder_contract() {
        assert_eq!(ActivationExitReason::NoWork.label(), "NO_WORK");
        assert_eq!(ActivationExitReason::Block.label(), "BLOCK");
        assert_eq!(ActivationExitReason::Quota.label(), "QUOTA");
        assert_eq!(ActivationExitReason::BudgetGuard.label(), "BUDGET_GUARD");
        assert_eq!(ActivationExitReason::Yield.label(), "YIELD");
        assert_eq!(ActivationExitReason::Timeout.label(), "TIMEOUT");
        assert_eq!(ActivationExitReason::Fault.label(), "FAULT");
    }
}
