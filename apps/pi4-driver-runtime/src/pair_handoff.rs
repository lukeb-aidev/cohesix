// Author: Lukas Bower
// Purpose: Observe the first linked child handoff without changing driver or scheduler authority.
// Copyright 2026 Lukas Bower

use pi4_driver_abi::*;

use super::{DriverTaskCommandRecord, RuntimeNotificationRoute, RuntimeStateSlot};
use core::sync::atomic::{AtomicU32, Ordering};

static TRACE: RuntimeStateSlot<DriverRuntimePairHandoff> =
    RuntimeStateSlot::new(DriverRuntimePairHandoff::empty());
static ACTIVE_ROLE: AtomicU32 = AtomicU32::new(0);

const _: () = assert!(
    super::DRIVER_TASK_RING_COMPLETION_OFFSET
        + core::mem::size_of::<super::DriverTaskCompletionRecord>()
        == DRIVER_RUNTIME_PAIR_HANDOFF_OFFSET
);

pub(super) fn reset() {
    ACTIVE_ROLE.store(0, Ordering::Release);
    TRACE.with_mut(|record| *record = DriverRuntimePairHandoff::empty());
    publish(DriverRuntimePairHandoff::empty());
}

fn route_code(route: RuntimeNotificationRoute) -> u8 {
    match route {
        RuntimeNotificationRoute::Unavailable => 0,
        RuntimeNotificationRoute::SdioOwner => 1,
        RuntimeNotificationRoute::Cyw43Client => 2,
        _ => 3,
    }
}

fn publish(mut record: DriverRuntimePairHandoff) {
    let ring = super::RuntimeRingWindow::local();
    let base = DRIVER_RUNTIME_PAIR_HANDOFF_OFFSET;
    let commit = base + 40;
    record.committed_publication = 0;
    if !ring.write_u32(commit, 0) {
        return;
    }
    super::driver_task_shared_store_barrier();
    for (index, word) in record.words()[..10].iter().copied().enumerate() {
        if !ring.write_u32(base + index * 4, word) {
            return;
        }
    }
    super::driver_task_shared_store_barrier();
    if !ring.write_u32(commit, record.publication) {
        return;
    }
    super::driver_task_shared_store_barrier();
    super::driver_task_shared_clean_range(super::DRIVER_TASK_RING_VADDR + base, 44);
}

/// Each stage is retained once, so stalled receives/owner loops cannot turn
/// this diagnostic into a log or polling workload. Neither a failed write nor
/// a stale/missing record is allowed to influence its caller's result.
fn update(
    role: u8,
    request: Option<u32>,
    stages: u32,
    edit: impl FnOnce(&mut DriverRuntimePairHandoff),
) {
    if ACTIVE_ROLE.load(Ordering::Acquire) != u32::from(role) {
        return;
    }
    let changed = TRACE.with_mut(|record| {
        if record.role != role
            || record.stages & (PAIR_HANDOFF_TERMINAL | PAIR_HANDOFF_RECOVERY) != 0
            || request.is_some_and(|request| record.request != request)
            || record.stages & stages == stages
        {
            return None;
        }
        edit(record);
        record.stages |= stages;
        record.publication = record.publication.saturating_add(1);
        record.committed_publication = record.publication;
        record.cntvct_lo = super::runtime_timer_counter_ticks() as u32;
        Some(*record)
    });
    if let Some(record) = changed {
        publish(record);
        if record.stages & (PAIR_HANDOFF_TERMINAL | PAIR_HANDOFF_RECOVERY) != 0 {
            ACTIVE_ROLE.store(0x80, Ordering::Release);
        }
    }
}

fn arm(record: DriverRuntimePairHandoff) {
    if ACTIVE_ROLE.load(Ordering::Acquire) != 0 {
        return;
    }
    let admitted = TRACE.with_mut(|current| {
        if current.role != 0 {
            return false;
        }
        *current = record;
        true
    });
    if admitted {
        ACTIVE_ROLE.store(u32::from(record.role), Ordering::Release);
        publish(record);
    }
}

pub(super) fn owner_engine_retired(sequence: u32, route: RuntimeNotificationRoute) {
    arm(DriverRuntimePairHandoff {
        role: PAIR_HANDOFF_SDIO,
        route: route_code(route),
        publication: 1,
        parent: sequence,
        stages: PAIR_HANDOFF_ARMED,
        cntvct_lo: super::runtime_timer_counter_ticks() as u32,
        committed_publication: 1,
        ..DriverRuntimePairHandoff::empty()
    });
}

pub(super) fn owner_prewait(kind: u8) {
    update(PAIR_HANDOFF_SDIO, None, PAIR_HANDOFF_PREWAIT, |record| {
        record.route = (record.route & 0x0f) | (kind << 4);
    });
}

pub(super) fn owner_raw_receive(badge: u64, message_info: u64) {
    update(PAIR_HANDOFF_SDIO, None, PAIR_HANDOFF_RAW_WAKE, |record| {
        record.detail = badge as u32;
        record.witness = message_info as u32;
    });
}

pub(super) fn owner_ring_seen(command: DriverTaskCommandRecord) {
    if !super::runtime_delegated_continuation_command(command)
        || command.aux1 == 0
        || command.flags & DRIVER_RUNTIME_COMMAND_FLAG_ONE_WAY == 0
    {
        return;
    }
    update(PAIR_HANDOFF_SDIO, None, PAIR_HANDOFF_RING_SEEN, |record| {
        record.request = command.sequence;
        record.generation = command.aux1;
    });
}

pub(super) fn owner_stage(command: DriverTaskCommandRecord, stage: u32) {
    if command.arg0 == super::HOT_PATH_SDIO_HOST {
        update(PAIR_HANDOFF_SDIO, Some(command.sequence), stage, |_| {});
    }
}

pub(super) fn producer_precommit(
    child: DriverTaskCommandRecord,
    parent: u32,
    rejection: Option<u32>,
    atomic: bool,
) {
    arm(DriverRuntimePairHandoff {
        role: PAIR_HANDOFF_CYW43,
        route: 2 | if atomic { 4 << 4 } else { 0 },
        publication: 1,
        request: child.sequence,
        parent,
        generation: child.aux1,
        stages: PAIR_HANDOFF_ARMED
            | PAIR_HANDOFF_PRECOMMIT
            | if rejection.is_some() {
                PAIR_HANDOFF_REJECTED
            } else {
                0
            },
        detail: rejection.unwrap_or(0),
        cntvct_lo: super::runtime_timer_counter_ticks() as u32,
        committed_publication: 1,
        ..DriverRuntimePairHandoff::empty()
    });
}

pub(super) fn producer_returned(sequence: u32, badge: Option<u32>) {
    update(
        PAIR_HANDOFF_CYW43,
        Some(sequence),
        PAIR_HANDOFF_SEND_RETURNED,
        |record| {
            if let Some(badge) = badge {
                record.stages |= PAIR_HANDOFF_RAW_WAKE;
                record.witness = badge;
            }
        },
    );
}

pub(super) fn producer_stage(sequence: u32, stage: u32) {
    update(PAIR_HANDOFF_CYW43, Some(sequence), stage, |_| {});
}

pub(super) fn producer_recovery() {
    update(PAIR_HANDOFF_CYW43, None, PAIR_HANDOFF_RECOVERY, |_| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_handoff_producer_preserves_rejection_and_first_return_until_reset() {
        let _guard = super::super::test_state_guard();
        super::super::reset_test_ring();
        reset();
        let command = DriverTaskCommandRecord {
            sequence: 42,
            aux1: 9,
            ..DriverTaskCommandRecord::empty()
        };
        producer_precommit(command, 3, Some(472), false);
        let rejected = TRACE.with_ref(|record| *record);
        assert!(rejected.valid());
        assert_eq!(rejected.stages, 0x2201);
        assert_eq!(rejected.detail, 472);
        assert_eq!(rejected.route, 2);
        producer_returned(43, Some(512));
        assert_eq!(TRACE.with_ref(|record| *record), rejected);
        producer_returned(42, Some(256));
        producer_returned(42, Some(512));
        producer_stage(42, PAIR_HANDOFF_CHILD_RETURNED | PAIR_HANDOFF_TERMINAL);
        let terminal = TRACE.with_ref(|record| *record);
        producer_recovery();
        assert_eq!(TRACE.with_ref(|record| *record), terminal);
        assert_eq!(terminal.witness, 256);
        assert_eq!(terminal.stages, 0x3705);
        reset();
        assert_eq!(
            super::super::RuntimeRingWindow::local()
                .read_u32(DRIVER_RUNTIME_PAIR_HANDOFF_OFFSET + 40),
            Some(0)
        );
        producer_precommit(command, 3, None, true);
        producer_recovery();
        let recovery = TRACE.with_ref(|record| *record);
        assert_eq!(recovery.route, 0x42);
        assert_eq!(recovery.stages, 0xa01);
        assert_eq!(recovery.detail, 0);
        reset();
    }

    #[test]
    fn pair_handoff_retains_first_child_and_freezes_without_authority_changes() {
        let _guard = super::super::test_state_guard();
        super::super::reset_test_ring();
        reset();
        owner_engine_retired(2, RuntimeNotificationRoute::SdioOwner);
        owner_prewait(3);
        owner_raw_receive(256, 1);
        owner_ring_seen(DriverTaskCommandRecord {
            sequence: 2,
            flags: DRIVER_RUNTIME_COMMAND_FLAG_ONE_WAY,
            arg0: super::super::HOT_PATH_SDIO_HOST,
            arg1: super::super::ROLE_SDIO,
            ..DriverTaskCommandRecord::empty()
        });
        assert_eq!(TRACE.with_ref(|record| record.request), 0);
        assert_eq!(TRACE.with_ref(|record| record.stages), 0x7);
        let command = DriverTaskCommandRecord {
            sequence: 0x8000_0042,
            flags: DRIVER_RUNTIME_COMMAND_FLAG_ONE_WAY,
            arg0: super::super::HOT_PATH_SDIO_HOST,
            arg1: super::super::ROLE_SDIO,
            aux1: 9,
            ..DriverTaskCommandRecord::empty()
        };
        owner_ring_seen(command);
        owner_stage(command, PAIR_HANDOFF_INTAKE_BEGIN);
        owner_stage(command, PAIR_HANDOFF_SEALED);
        owner_stage(command, PAIR_HANDOFF_DISPATCH);
        owner_stage(command, PAIR_HANDOFF_ACTION_RETURNED);
        owner_stage(command, PAIR_HANDOFF_TERMINAL);
        let first = TRACE.with_ref(|record| *record);
        assert!(first.valid());
        assert_eq!(first.request, 0x8000_0042);
        assert_eq!(first.parent, 2);
        assert_eq!(first.detail, 256);
        assert_eq!(first.route, 0x31);
        assert_eq!(first.stages, 0x1ff);
        owner_raw_receive(512, 0);
        owner_ring_seen(DriverTaskCommandRecord {
            sequence: 0x8000_0043,
            ..command
        });
        owner_stage(command, PAIR_HANDOFF_REJECTED);
        assert_eq!(TRACE.with_ref(|record| *record), first);
        reset();
        assert!(!TRACE.with_ref(|record| record.valid()));
    }
}
