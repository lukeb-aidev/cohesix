// Author: Lukas Bower
// Purpose: Virtio MMIO network device driver for the root task, including TX/RX queue management and invariants.
//! Virtio MMIO network device driver used by the root task.
//!
//! Virtio MMIO network device driver used by the root task on the ARM `virt`
//! platform. RX descriptor handling and smoltcp integration are instrumented to
//! aid debugging end-to-end TCP console flows.
#![cfg(all(feature = "kernel", feature = "net-console"))]
#![allow(unsafe_code)]

#[cfg(target_arch = "aarch64")]
use core::arch::asm;
use core::fmt::{self, Write as FmtWrite};
#[cfg(feature = "net-backend-virtio")]
use core::mem::MaybeUninit;
use core::ops::Range;
use core::ptr::read_unaligned;
use core::ptr::{read_volatile, write_volatile, NonNull};
use core::sync::atomic::{
    compiler_fence, AtomicBool, AtomicU32, AtomicU64, Ordering as AtomicOrdering,
};
use core::{cell::Cell, sync::atomic::fence};

use heapless::{String as HeaplessString, Vec as HeaplessVec};
use log::{debug, error, info, trace, warn};
use sel4_sys::{seL4_ARM_Page_Uncached, seL4_Error, seL4_NotEnoughMemory, seL4_PageBits};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;
#[cfg(test)]
use spin::Mutex;

use crate::bootstrap::bootinfo_snapshot::BootInfoState;
use crate::debug::watched_write_bytes;
use crate::guards;
use crate::hal::cache::{cache_clean, cache_invalidate};
use crate::hal::dma::{self, PinnedDmaRange};
use crate::hal::{HalError, Hardware};
use crate::net::{
    NetDevice, NetDeviceCounters, NetDriverError, NetStage, CONSOLE_TCP_PORT, NET_DIAG, NET_STAGE,
};
use crate::net_consts::MAX_FRAME_LEN;
use crate::sel4::{seL4_CapInitThreadVSpace, DeviceFrame, RamFrame, BOOTINFO_WINDOW_GUARD};

const FORENSICS: bool = true;
const FORENSICS_PUBLISH_LOG_LIMIT: u32 = 64;
const NET_VIRTIO_TX_V2: bool = cfg!(feature = "net-virtio-tx-v2");
const VIRTIO_GUARD_QUEUE: bool = cfg!(feature = "virtio_guard_queue");
const VIRTIO_DMA_TRACE: bool = cfg!(feature = "net-diag") || cfg!(feature = "trace-heavy-init");
const DMA_NONCOHERENT: bool = cfg!(target_arch = "aarch64");
const VIRTIO_TX_CLEAR_DESC_ON_FREE: bool = false;

const VIRTIO_MMIO_BASE: usize = 0x0a00_0000;
const VIRTIO_MMIO_STRIDE: usize = 0x200;
const VIRTIO_MMIO_SLOTS: usize = 8;
const TX_WRAP_TRIPWIRE_LIMIT: u32 = 4;

const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976;
const VIRTIO_MMIO_VERSION_LEGACY: u32 = 1;
const VIRTIO_MMIO_VERSION_MODERN: u32 = 2;
const VIRTIO_DEVICE_ID_NET: u32 = 1;

const DEVICE_FRAME_BITS: usize = 12;

const STATUS_ACKNOWLEDGE: u32 = 1 << 0;
const STATUS_DRIVER: u32 = 1 << 1;
const STATUS_DRIVER_OK: u32 = 1 << 2;
const STATUS_FEATURES_OK: u32 = 1 << 3;
const STATUS_DEVICE_NEEDS_RESET: u32 = 1 << 6;
const STATUS_FAILED: u32 = 1 << 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VirtioMmioMode {
    Modern,
    Legacy,
}

const VIRTQ_DESC_F_NEXT: u16 = 1 << 0;
const VIRTQ_DESC_F_WRITE: u16 = 1 << 1;

const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const VIRTIO_NET_F_MRG_RXBUF: u64 = 1 << 15;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;
const SUPPORTED_NET_FEATURES: u64 = VIRTIO_NET_F_MAC | VIRTIO_NET_F_MRG_RXBUF;

const PAGE_BYTES: usize = 1 << seL4_PageBits;

const RX_QUEUE_INDEX: u32 = 0;
const TX_QUEUE_INDEX: u32 = 1;

const RX_QUEUE_SIZE: usize = 16;
const TX_QUEUE_SIZE: usize = 16;
const MAX_QUEUE_SIZE: usize = if RX_QUEUE_SIZE > TX_QUEUE_SIZE {
    RX_QUEUE_SIZE
} else {
    TX_QUEUE_SIZE
};
const TX_NOTIFY_BATCH_PACKETS: u16 = 4;
const TX_NOTIFY_BATCH_BYTES: usize = 4096;
const TX_RECLAIM_IRQ_BUDGET: u16 = 8;
const TX_RECLAIM_POLL_BUDGET: u16 = 2;
#[cfg(debug_assertions)]
const TX_RECLAIM_STALL_POLL_LIMIT: u16 = 32;
const TX_STATS_LOG_MS: u64 = 1_000;
const TX_WARN_COOLDOWN_MS: u64 = 2_000;
const TX_CANARY_VALUE: u32 = 0xC0DE_DDCC;
#[cfg(debug_assertions)]
const TRACK_TX_HEAD: Option<u16> = Some(4);
#[cfg(not(debug_assertions))]
const TRACK_TX_HEAD: Option<u16> = None;
const VIRTIO_NET_HEADER_LEN_BASIC: usize = core::mem::size_of::<VirtioNetHdr>();
const VIRTIO_NET_HEADER_LEN_MRG: usize = core::mem::size_of::<VirtioNetHdrMrgRxbuf>();
const FRAME_BUFFER_LEN: usize = MAX_FRAME_LEN + VIRTIO_NET_HEADER_LEN_MRG;
static LOG_TCP_DEST_PORT: AtomicBool = AtomicBool::new(true);
static RX_NOTIFY_LOGGED: AtomicBool = AtomicBool::new(false);
static TX_NOTIFY_LOGGED: AtomicBool = AtomicBool::new(false);
static RX_PUBLISH_FENCE_LOGGED: AtomicBool = AtomicBool::new(false);
static TX_PUBLISH_FENCE_LOGGED: AtomicBool = AtomicBool::new(false);
static RX_ARM_START_LOGGED: AtomicBool = AtomicBool::new(false);
static RX_ARM_END_LOGGED: AtomicBool = AtomicBool::new(false);
static DMA_CLEAN_LOGGED: AtomicBool = AtomicBool::new(false);
static DMA_INVALIDATE_LOGGED: AtomicBool = AtomicBool::new(false);
static RX_CACHE_POLICY_LOGGED: AtomicBool = AtomicBool::new(false);
static DMA_SKIP_LOGGED: AtomicBool = AtomicBool::new(false);
static DMA_ERROR_LOGGED: AtomicBool = AtomicBool::new(false);
static DMA_QMEM_LOGGED: AtomicBool = AtomicBool::new(false);
static USED_RING_INVALIDATE_LOGGED: AtomicBool = AtomicBool::new(false);
static USED_LEN_ZERO_VISIBILITY_LOGGED: AtomicBool = AtomicBool::new(false);
static VQ_LAYOUT_LOGGED: AtomicBool = AtomicBool::new(false);
static DMA_FORCE_LOGGED: AtomicBool = AtomicBool::new(false);
static VQ_ADDRESS_LOGGED: [AtomicBool; VIRTIO_MMIO_SLOTS] =
    [const { AtomicBool::new(false) }; VIRTIO_MMIO_SLOTS];
static RING_SLOT_CANARY_LOGGED: [AtomicBool; VIRTIO_MMIO_SLOTS] =
    [const { AtomicBool::new(false) }; VIRTIO_MMIO_SLOTS];
static FORENSICS_FROZEN: AtomicBool = AtomicBool::new(false);
static FORENSICS_DUMPED: AtomicBool = AtomicBool::new(false);
static TX_WRAP_DMA_LOGGED: AtomicBool = AtomicBool::new(false);
static TX_WRAP_TRIPWIRE: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "net-backend-virtio")]
static mut VIRTIO_NET_STORAGE: MaybeUninit<VirtioNet> = MaybeUninit::uninit();
#[cfg(debug_assertions)]
static VIRTIO_NET_SIZE_LOGGED: AtomicBool = AtomicBool::new(false);

fn check_bootinfo_canary(mark: &'static str) -> Result<(), DriverError> {
    if let Some(state) = BootInfoState::get() {
        if let Err(err) = state.verify("virtio-net", mark) {
            error!(
                target: "virtio-net",
                "[bootinfo:virtio] canary divergence mark={mark} err={err:?}"
            );
            return Err(DriverError::QueueInvariant("bootinfo canary diverged"));
        }
    }
    Ok(())
}

fn bootinfo_protected_ranges() -> HeaplessVec<Range<usize>, 4> {
    let mut ranges = HeaplessVec::<Range<usize>, 4>::new();
    if let Some(state) = BootInfoState::get() {
        let _ = ranges.push(state.snapshot_region());
    }
    if let Some((ptr, len)) = BOOTINFO_WINDOW_GUARD.watched_region() {
        let start = ptr as usize;
        let end = start.saturating_add(len);
        let _ = ranges.push(start..end);
    }
    let text = guards::text_bounds();
    if text.start < text.end {
        let _ = ranges.push(text);
    }
    ranges
}

fn assert_no_overlap(paddr: usize, len: usize, label: &'static str) {
    let end = paddr.saturating_add(len);
    for protected in bootinfo_protected_ranges() {
        if end <= protected.start || paddr >= protected.end {
            continue;
        }
        panic!(
            "[virtio-net] DMA range overlaps protected region: label={} paddr=[0x{paddr:016x}..0x{end:016x}) protected=[0x{prot_start:016x}..0x{prot_end:016x})",
            label,
            prot_start = protected.start,
            prot_end = protected.end,
        );
    }
}

fn log_dma_programming(label: &str, vaddr: usize, paddr: usize, len: usize) {
    let align_ok = paddr & (PAGE_BYTES - 1) == 0;
    info!(
        target: "virtio-net",
        "[virtio-net][dma] {label}: vaddr=0x{vaddr:016x} paddr=0x{paddr:016x} len=0x{len:08x} page_aligned={align_ok}",
        label = label,
        vaddr = vaddr,
        paddr = paddr,
        len = len,
    );
}

fn assert_dma_region(label: &'static str, vaddr: usize, paddr: usize, len: usize) {
    let aligned = paddr & (PAGE_BYTES - 1) == 0;
    if !aligned && len > PAGE_BYTES {
        panic!(
            "[virtio-net] DMA region not page aligned: label={} paddr=0x{paddr:016x} len=0x{len:x}",
            label
        );
    } else if !aligned {
        warn!(
            target: "virtio-net",
            "[virtio-net] DMA region misaligned (len within single page): label={} paddr=0x{paddr:016x} len=0x{len:x}",
            label
        );
    }
    assert!(
        paddr != vaddr,
        "[virtio-net] DMA region paddr mirrors vaddr (likely virtual): label={} vaddr=0x{vaddr:016x} paddr=0x{paddr:016x}",
        label
    );
    assert_no_overlap(paddr, len, label);
}

#[inline]
fn virtq_publish_barrier() {
    compiler_fence(AtomicOrdering::Release);
    #[cfg(target_arch = "aarch64")]
    unsafe {
        asm!("dmb oshst", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    fence(AtomicOrdering::Release);
}

#[inline]
fn virtq_notify_barrier() {
    compiler_fence(AtomicOrdering::Release);
    #[cfg(target_arch = "aarch64")]
    unsafe {
        asm!("dmb oshst", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    fence(AtomicOrdering::Release);
}

#[inline]
fn virtq_used_load_barrier() {
    compiler_fence(AtomicOrdering::Acquire);
    #[cfg(target_arch = "aarch64")]
    unsafe {
        asm!("dmb oshld", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    fence(AtomicOrdering::Acquire);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TxHeadState {
    Free,
    Prepared { gen: u32 },
    Published { slot: u16, gen: u32 },
    InFlight { slot: u16, gen: u32 },
    Completed { gen: u32 },
}

impl Default for TxHeadState {
    fn default() -> Self {
        TxHeadState::Free
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TxHeadError {
    OutOfRange,
    InvalidState,
    FreeListFull,
    SlotBusy,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TxHeadEntry {
    state: TxHeadState,
    last_len: u32,
    last_addr: u64,
}

#[derive(Clone, Debug)]
/// Manages virtio TX descriptor heads and prevents reuse while in-flight.
/// A TX head must not be re-posted until reclaimed from the used ring.
struct TxHeadManager {
    free_mask: u32,
    entries: [TxHeadEntry; TX_QUEUE_SIZE],
    published_slots: [Option<(u16, u32)>; TX_QUEUE_SIZE],
    in_avail: [bool; TX_QUEUE_SIZE],
    advertised: [bool; TX_QUEUE_SIZE],
    publish_present: [bool; TX_QUEUE_SIZE],
    publish_slot: [u16; TX_QUEUE_SIZE],
    publish_gen: [u32; TX_QUEUE_SIZE],
    publish_avail_idx: [u16; TX_QUEUE_SIZE],
    next_gen: u32,
    size: u16,
    allocation_violation_logged: bool,
    dup_alloc_refused: u64,
    dup_publish_blocked: u64,
    invalid_used_id: u64,
    invalid_used_state: u64,
    zero_len_publish_blocked: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TxSlotState {
    Free { gen: u32 },
    Reserved { gen: u32 },
    InFlight { gen: u32 },
}

impl Default for TxSlotState {
    fn default() -> Self {
        TxSlotState::Free { gen: 0 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TxSlotError {
    OutOfRange,
    NotReserved,
    NotInFlight,
}

#[derive(Clone, Debug)]
struct TxSlotTracker {
    states: [TxSlotState; TX_QUEUE_SIZE],
    free_count: u16,
    in_flight: u16,
    next_alloc: u16,
    last_alloc: Option<u16>,
    next_gen: u32,
    size: u16,
}

impl TxSlotTracker {
    fn new(size: u16) -> Self {
        let mut tracker = Self {
            states: [TxSlotState::default(); TX_QUEUE_SIZE],
            free_count: size,
            in_flight: 0,
            next_alloc: 0,
            last_alloc: None,
            next_gen: 1,
            size,
        };
        // Ensure entries outside the configured queue size stay marked free with a generation of 0.
        for idx in usize::from(size)..TX_QUEUE_SIZE {
            tracker.states[idx] = TxSlotState::Free { gen: 0 };
        }
        tracker
    }

    fn state(&self, id: u16) -> Option<TxSlotState> {
        self.states.get(id as usize).copied()
    }

    fn reserve_next(&mut self) -> Option<(u16, bool, u32)> {
        if self.free_count == 0 || self.size == 0 {
            return None;
        }
        let size = usize::from(self.size);
        for _ in 0..size {
            let slot = self.next_alloc;
            self.next_alloc = self.next_alloc.wrapping_add(1) % self.size;
            match self.states.get_mut(slot as usize) {
                Some(state @ TxSlotState::Free { .. }) => {
                    let gen = self.next_gen;
                    self.next_gen = self.next_gen.wrapping_add(1);
                    let wrap = self.last_alloc.map(|last| slot < last).unwrap_or(false);
                    *state = TxSlotState::Reserved { gen };
                    self.last_alloc = Some(slot);
                    self.free_count = self.free_count.saturating_sub(1);
                    return Some((slot, wrap, gen));
                }
                _ => continue,
            }
        }
        None
    }

    fn cancel(&mut self, id: u16) -> Result<(), TxSlotError> {
        if id >= self.size {
            return Err(TxSlotError::OutOfRange);
        }
        match self.states.get_mut(id as usize) {
            Some(state @ TxSlotState::Reserved { .. }) => {
                *state = TxSlotState::Free { gen: self.next_gen };
                self.next_gen = self.next_gen.wrapping_add(1);
                self.free_count = self.free_count.saturating_add(1);
                Ok(())
            }
            Some(TxSlotState::InFlight { .. }) => Err(TxSlotError::NotReserved),
            _ => Err(TxSlotError::NotReserved),
        }
    }

    fn mark_in_flight(&mut self, id: u16) -> Result<u32, TxSlotError> {
        if id >= self.size {
            return Err(TxSlotError::OutOfRange);
        }
        match self.states.get_mut(id as usize) {
            Some(state) => match state {
                TxSlotState::Reserved { gen } => {
                    let gen_val = *gen;
                    *state = TxSlotState::InFlight { gen: gen_val };
                    self.in_flight = self.in_flight.saturating_add(1);
                    Ok(gen_val)
                }
                TxSlotState::InFlight { .. } => Err(TxSlotError::NotReserved),
                _ => Err(TxSlotError::NotReserved),
            },
            None => Err(TxSlotError::NotReserved),
        }
    }

    fn complete(&mut self, id: u16) -> Result<u32, TxSlotError> {
        if id >= self.size {
            return Err(TxSlotError::OutOfRange);
        }
        match self.states.get_mut(id as usize) {
            Some(state) => match state {
                TxSlotState::InFlight { gen } => {
                    let gen_val = *gen;
                    *state = TxSlotState::Free { gen: gen_val };
                    self.in_flight = self.in_flight.saturating_sub(1);
                    self.free_count = self.free_count.saturating_add(1);
                    Ok(gen_val)
                }
                TxSlotState::Reserved { .. } => Err(TxSlotError::NotInFlight),
                _ => Err(TxSlotError::NotInFlight),
            },
            None => Err(TxSlotError::NotInFlight),
        }
    }

    fn free_count(&self) -> u16 {
        self.free_count
    }

    fn in_flight(&self) -> u16 {
        self.in_flight
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TxReclaimResult {
    Reclaimed,
    InvalidId,
    HeadNotInFlight(Option<TxHeadState>),
    SlotStateInvalid(Option<TxSlotState>),
    SlotMismatch,
    PublishRecordMismatch,
    HeadTransitionFailed,
    SlotCompletionFailed(TxSlotError),
}

fn reclaim_used_entry_common(
    head_mgr: &mut TxHeadManager,
    slots: Option<&mut TxSlotTracker>,
    id: u16,
    ring_slot: u16,
) -> TxReclaimResult {
    if id >= head_mgr.size {
        return TxReclaimResult::InvalidId;
    }
    let entry = match head_mgr.entry(id) {
        Some(entry) => entry,
        None => return TxReclaimResult::HeadNotInFlight(None),
    };
    let (expected_slot, expected_gen) = match entry.state {
        TxHeadState::InFlight { slot, gen } => (slot, gen),
        state => return TxReclaimResult::HeadNotInFlight(Some(state)),
    };
    let slot_state = slots.as_ref().and_then(|tracker| tracker.state(id));
    if let Some(state) = slot_state {
        if !matches!(state, TxSlotState::InFlight { .. }) {
            return TxReclaimResult::SlotStateInvalid(Some(state));
        }
    }
    if expected_slot != ring_slot {
        return TxReclaimResult::SlotMismatch;
    }
    if head_mgr.published_for_slot(ring_slot) != Some((id, expected_gen)) {
        return TxReclaimResult::PublishRecordMismatch;
    }
    if head_mgr
        .take_publish_record(id, ring_slot, expected_gen)
        .is_err()
    {
        return TxReclaimResult::PublishRecordMismatch;
    }
    if head_mgr
        .mark_completed(id, Some(expected_gen))
        .and_then(|_| head_mgr.reclaim_head(id))
        .is_err()
    {
        return TxReclaimResult::HeadTransitionFailed;
    }
    if let Some(mut tracker) = slots {
        if let Err(err) = tracker.complete(id) {
            return TxReclaimResult::SlotCompletionFailed(err);
        }
    }
    TxReclaimResult::Reclaimed
}

fn record_zero_len_used(
    counter: &mut u64,
    log_ms: &mut u64,
    id: u16,
    ring_slot: u16,
    used_idx: u16,
    avail_idx: u16,
    last_used: u16,
    head_state: Option<TxHeadState>,
    slot_state: Option<TxSlotState>,
) {
    *counter = counter.saturating_add(1);
    let now_ms = crate::hal::timebase().now_ms();
    if *log_ms == 0 || now_ms.saturating_sub(*log_ms) >= 1_000 {
        *log_ms = now_ms;
        warn!(
            target: "net-console",
            "[virtio-net][tx] used.len zero: head={} ring_slot={} used_idx={} last_used={} avail_idx={} head_state={:?} slot_state={:?}",
            id,
            ring_slot,
            used_idx,
            last_used,
            avail_idx,
            head_state,
            slot_state,
        );
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TxPublishRecord {
    slot: u16,
    gen: u32,
    avail_idx: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TxReservation {
    head_id: u16,
    head_gen: u32,
    slot_gen: Option<u32>,
}

#[derive(Default)]
struct VirtioNetTxStats {
    enqueue_ok: AtomicU64,
    enqueue_would_block: AtomicU64,
    kick_count: AtomicU64,
    reclaim_calls_irq: AtomicU64,
    reclaim_calls_poll: AtomicU64,
    used_reaped: AtomicU64,
    inflight_highwater: AtomicU32,
    ring_full_events: AtomicU64,
    irq_count: AtomicU64,
}

impl VirtioNetTxStats {
    fn record_enqueue_ok(&self, inflight: u16) {
        self.enqueue_ok.fetch_add(1, AtomicOrdering::Relaxed);
        self.update_highwater(inflight);
    }

    fn record_would_block(&self, inflight: u16, free: u16, qsize: u16) {
        self.enqueue_would_block
            .fetch_add(1, AtomicOrdering::Relaxed);
        if inflight >= qsize || free == 0 {
            self.ring_full_events.fetch_add(1, AtomicOrdering::Relaxed);
        }
    }

    fn record_kick(&self) {
        self.kick_count.fetch_add(1, AtomicOrdering::Relaxed);
    }

    fn record_irq_reclaim(&self) {
        self.reclaim_calls_irq.fetch_add(1, AtomicOrdering::Relaxed);
    }

    fn record_poll_reclaim(&self) {
        self.reclaim_calls_poll
            .fetch_add(1, AtomicOrdering::Relaxed);
    }

    fn record_used_reaped(&self) {
        self.used_reaped.fetch_add(1, AtomicOrdering::Relaxed);
    }

    fn record_irq(&self) {
        self.irq_count.fetch_add(1, AtomicOrdering::Relaxed);
    }

    fn update_highwater(&self, inflight: u16) {
        let mut current = self.inflight_highwater.load(AtomicOrdering::Relaxed);
        while inflight as u32 > current {
            match self.inflight_highwater.compare_exchange(
                current,
                inflight as u32,
                AtomicOrdering::AcqRel,
                AtomicOrdering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }
}

#[derive(Default, Clone, Copy)]
struct TxDiagState {
    last_avail_idx: u16,
    last_used_idx: u16,
    last_inflight: u16,
    last_irq_count: u64,
    last_would_block: u64,
    last_kick_count: u64,
    last_enqueue_ok: u64,
    last_warn_ms: u64,
}

#[derive(Default, Clone, Copy)]
struct TxStatsSnapshot {
    enqueue_ok: u64,
    enqueue_would_block: u64,
    kick_count: u64,
    reclaim_calls_irq: u64,
    reclaim_calls_poll: u64,
    used_reaped: u64,
    inflight_highwater: u32,
    ring_full_events: u64,
    irq_count: u64,
}

#[derive(Clone, Copy)]
enum TxReclaimSource {
    Irq,
    Poll,
}

impl TxHeadManager {
    fn trace_transition(&self, id: u16, from: TxHeadState, to: TxHeadState, label: &'static str) {
        if let Some(track) = TRACK_TX_HEAD {
            if track == id {
                debug!(
                    target: "net-console",
                    "[virtio-net][tx-head-trace] head={} {}: {:?} -> {:?}",
                    id,
                    label,
                    from,
                    to
                );
            }
        }
    }

    fn new(size: u16) -> Self {
        // Analysis: Reusing a TX head before the device returns ownership lets QEMU read a later
        // generation's zeroed descriptors, tripping the "zero sized buffers" abort. The free_mask
        // acts as the allocator's source of truth so we never hand out a head twice without a used
        // entry, and the publish guards below ensure we will not place a zero-length descriptor
        // into the avail ring even if bookkeeping is corrupted.
        let free_mask = if size == 0 {
            0
        } else {
            1u32.checked_shl(size as u32).unwrap_or(0).wrapping_sub(1)
        };
        Self {
            free_mask,
            entries: [TxHeadEntry::default(); TX_QUEUE_SIZE],
            published_slots: [None; TX_QUEUE_SIZE],
            in_avail: [false; TX_QUEUE_SIZE],
            advertised: [false; TX_QUEUE_SIZE],
            publish_present: [false; TX_QUEUE_SIZE],
            publish_slot: [0; TX_QUEUE_SIZE],
            publish_gen: [0; TX_QUEUE_SIZE],
            publish_avail_idx: [0; TX_QUEUE_SIZE],
            next_gen: 1,
            size,
            allocation_violation_logged: false,
            dup_alloc_refused: 0,
            dup_publish_blocked: 0,
            invalid_used_id: 0,
            invalid_used_state: 0,
            zero_len_publish_blocked: 0,
        }
    }

    fn active_mask(&self) -> u32 {
        if self.size == 0 {
            0
        } else {
            1u32.checked_shl(self.size as u32)
                .unwrap_or(0)
                .wrapping_sub(1)
        }
    }

    fn state(&self, id: u16) -> Option<TxHeadState> {
        self.entries.get(id as usize).map(|entry| entry.state)
    }

    fn entry(&self, id: u16) -> Option<&TxHeadEntry> {
        self.entries.get(id as usize)
    }

    fn entry_mut(&mut self, id: u16) -> Option<&mut TxHeadEntry> {
        self.entries.get_mut(id as usize)
    }

    fn is_advertised(&self, id: u16) -> bool {
        self.advertised.get(id as usize).copied().unwrap_or(false)
    }

    fn mark_advertised(&mut self, id: u16) -> Result<(), TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        let entry = self.entry(id).copied().ok_or(TxHeadError::OutOfRange)?;
        if self.is_advertised(id) {
            debug_assert!(
                false,
                "tx head already advertised: id={} state={:?}",
                id, entry.state
            );
            return Err(TxHeadError::InvalidState);
        }
        if !matches!(
            entry.state,
            TxHeadState::Published { .. } | TxHeadState::InFlight { .. }
        ) {
            debug_assert!(
                false,
                "tx head not published during advertise mark: id={} state={:?}",
                id, entry.state
            );
            return Err(TxHeadError::InvalidState);
        }
        if let Some(flag) = self.advertised.get_mut(id as usize) {
            *flag = true;
        }
        Ok(())
    }

    fn clear_advertised(&mut self, id: u16) -> Result<(), TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        if !self.is_advertised(id) {
            debug_assert!(false, "tx head not advertised: id={}", id);
            return Err(TxHeadError::InvalidState);
        }
        if let Some(flag) = self.advertised.get_mut(id as usize) {
            *flag = false;
        }
        Ok(())
    }

    fn free_mask_for(&self, id: u16) -> Result<u32, TxHeadError> {
        let mask = 1u32.checked_shl(id as u32).ok_or(TxHeadError::OutOfRange)?;
        if mask == 0 {
            return Err(TxHeadError::OutOfRange);
        }
        Ok(mask)
    }

    fn mark_free_entry(&mut self, id: u16) -> Result<(), TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        let mask = self.free_mask_for(id)?;
        if (self.free_mask & mask) != 0 {
            if mark_forensics_frozen() {
                error!(
                    target: "net-console",
                    "[virtio-net][tx-free] duplicate free detected id={} state={:?} in_avail={} free_mask=0x{mask:08x}",
                    id,
                    self.state(id),
                    self.in_avail(id),
                    mask = self.free_mask,
                );
            }
            return Err(TxHeadError::InvalidState);
        }
        self.free_mask |= mask;
        Ok(())
    }

    fn record_dup_alloc(&mut self) {
        self.dup_alloc_refused = self.dup_alloc_refused.saturating_add(1);
    }

    fn record_dup_publish(&mut self) {
        self.dup_publish_blocked = self.dup_publish_blocked.saturating_add(1);
    }

    fn record_invalid_used_id(&mut self) {
        self.invalid_used_id = self.invalid_used_id.saturating_add(1);
    }

    fn record_invalid_used_state(&mut self) {
        self.invalid_used_state = self.invalid_used_state.saturating_add(1);
    }

    fn record_zero_len_publish(&mut self) {
        self.zero_len_publish_blocked = self.zero_len_publish_blocked.saturating_add(1);
    }

    fn prepare_publish(
        &mut self,
        id: u16,
        slot: u16,
        len: u32,
        addr: u64,
    ) -> Result<(), TxHeadError> {
        if len == 0 || addr == 0 {
            self.record_zero_len_publish();
            return Err(TxHeadError::InvalidState);
        }
        if id >= self.size || slot >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        if self.is_advertised(id) || self.in_avail(id) || self.publish_present(id) {
            self.record_dup_publish();
            return Err(TxHeadError::InvalidState);
        }
        if self
            .published_slots
            .get(slot as usize)
            .copied()
            .flatten()
            .is_some()
        {
            return Err(TxHeadError::SlotBusy);
        }
        let entry = self.entry_mut(id).ok_or(TxHeadError::OutOfRange)?;
        match entry.state {
            TxHeadState::Prepared { .. } => {
                entry.last_len = len;
                entry.last_addr = addr;
                Ok(())
            }
            _ => {
                self.record_dup_publish();
                Err(TxHeadError::InvalidState)
            }
        }
    }

    fn alloc_specific(&mut self, id: u16) -> Option<u16> {
        if id >= self.size {
            return None;
        }
        let mask = self.free_mask_for(id).ok()?;
        if (self.free_mask & mask) == 0 {
            self.record_dup_alloc();
            return None;
        }
        let state = self.state(id);
        if !matches!(state, Some(TxHeadState::Free)) || self.is_advertised(id) || self.in_avail(id)
        {
            if mark_forensics_frozen() && !self.allocation_violation_logged {
                self.allocation_violation_logged = true;
                warn!(
                    target: "net-console",
                    "[virtio-net][tx-guard] allocation refused: id={} state={:?} advertised={} in_avail={} free_mask=0x{mask:08x}",
                    id,
                    state,
                    self.is_advertised(id),
                    self.in_avail(id),
                    mask = self.free_mask,
                );
            }
            self.record_dup_alloc();
            return None;
        }
        self.free_mask &= !mask;
        let gen = self.next_gen;
        self.next_gen = self.next_gen.wrapping_add(1);
        let next_state = TxHeadState::Prepared { gen };
        if let Some(prev_state) = self.entry_mut(id).map(|entry| {
            let prev_state = entry.state;
            entry.state = next_state;
            entry.last_len = 0;
            entry.last_addr = 0;
            prev_state
        }) {
            self.trace_transition(id, prev_state, next_state, "alloc");
        }
        if let Some(flag) = self.publish_present.get_mut(id as usize) {
            *flag = false;
        }
        Some(id)
    }

    fn alloc_head(&mut self) -> Option<u16> {
        let active = self.active_mask();
        let free = self.free_mask & active;
        if free == 0 {
            return None;
        }
        let id = free.trailing_zeros() as u16;
        self.alloc_specific(id)
    }

    fn submit_ready(&self, id: u16, slot: u16) -> Result<(), TxHeadError> {
        if id >= self.size || slot >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        match self.state(id) {
            Some(
                TxHeadState::Published { slot: s, .. } | TxHeadState::InFlight { slot: s, .. },
            ) if s == slot => {}
            _ => return Err(TxHeadError::InvalidState),
        }
        if !self.in_avail(id) {
            return Err(TxHeadError::InvalidState);
        }
        if self.publish_present(id) {
            return Err(TxHeadError::InvalidState);
        }
        Ok(())
    }

    fn promote_published_to_inflight(&mut self, id: u16) -> Result<(u16, u32), TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        let in_avail = self.in_avail.get(id as usize).copied().unwrap_or(false);
        if !in_avail {
            return Err(TxHeadError::InvalidState);
        }
        let (slot, gen) = match self.state(id) {
            Some(TxHeadState::Published { slot, gen }) => (slot, gen),
            Some(TxHeadState::InFlight { .. }) => return Err(TxHeadError::InvalidState),
            _ => return Err(TxHeadError::InvalidState),
        };
        let entry = self.entry_mut(id).ok_or(TxHeadError::OutOfRange)?;
        entry.state = TxHeadState::InFlight { slot, gen };
        Ok((slot, gen))
    }

    fn mark_published(
        &mut self,
        id: u16,
        slot: u16,
        len: u32,
        addr: u64,
    ) -> Result<u32, TxHeadError> {
        debug_assert_ne!(len, 0, "tx head publish length must be non-zero");
        debug_assert_ne!(addr, 0, "tx head publish address must be non-zero");
        if len == 0 || addr == 0 {
            self.record_zero_len_publish();
            return Err(TxHeadError::InvalidState);
        }
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        if slot >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        if self.is_advertised(id) {
            self.record_dup_publish();
            return Err(TxHeadError::InvalidState);
        }
        if self
            .published_slots
            .get(slot as usize)
            .copied()
            .flatten()
            .is_some()
        {
            return Err(TxHeadError::SlotBusy);
        }
        if self.in_avail.get(id as usize).copied().unwrap_or(false) {
            return Err(TxHeadError::InvalidState);
        }
        let prev_state = self.state(id).ok_or(TxHeadError::OutOfRange)?;
        let gen = match prev_state {
            TxHeadState::Prepared { gen } => gen,
            _ => {
                self.record_dup_publish();
                return Err(TxHeadError::InvalidState);
            }
        };
        let next_state = TxHeadState::Published { slot, gen };
        if let Some(entry) = self.entry_mut(id) {
            debug_assert_eq!(
                entry.last_len, len,
                "last_len must be staged before publish"
            );
            debug_assert_eq!(
                entry.last_addr, addr,
                "last_addr must be staged before publish"
            );
            entry.state = next_state;
            entry.last_len = len;
            entry.last_addr = addr;
        }
        self.trace_transition(id, prev_state, next_state, "publish");
        self.published_slots[slot as usize] = Some((id, gen));
        if let Some(flag) = self.in_avail.get_mut(id as usize) {
            *flag = true;
        }
        Ok(gen)
    }

    fn mark_in_flight(&mut self, id: u16) -> Result<(u16, u32), TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        if !self.is_advertised(id) {
            debug_assert!(
                false,
                "tx head missing advertised flag on inflight transition: id={}",
                id
            );
            self.record_dup_publish();
            return Err(TxHeadError::InvalidState);
        }
        let in_avail = self.in_avail.get(id as usize).copied().unwrap_or(false);
        let state = self.state(id).ok_or(TxHeadError::OutOfRange)?;
        let (slot, gen) = match state {
            TxHeadState::InFlight { .. } => return Err(TxHeadError::InvalidState),
            TxHeadState::Published { slot, gen } => (slot, gen),
            _ => return Err(TxHeadError::InvalidState),
        };
        if self.published_for_slot(slot) != Some((id, gen)) {
            return Err(TxHeadError::InvalidState);
        }
        if !in_avail {
            debug_assert!(
                false,
                "tx head not marked in avail on inflight transition: id={} slot={} gen={}",
                id, slot, gen
            );
            return Err(TxHeadError::InvalidState);
        }
        let next_state = TxHeadState::InFlight { slot, gen };
        let prev_state = self
            .entry_mut(id)
            .map(|entry| {
                let prev_state = entry.state;
                entry.state = next_state;
                prev_state
            })
            .ok_or(TxHeadError::OutOfRange)?;
        self.trace_transition(id, prev_state, next_state, "inflight");
        Ok((slot, gen))
    }

    fn note_avail_publish(
        &mut self,
        id: u16,
        slot: u16,
        avail_idx: u16,
    ) -> Result<(), TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        let entry = self.entry(id).copied().ok_or(TxHeadError::OutOfRange)?;
        let gen = match entry.state {
            TxHeadState::Published { slot: s, gen } if s == slot => gen,
            _ => return Err(TxHeadError::InvalidState),
        };
        if !self.in_avail.get(id as usize).copied().unwrap_or(false) {
            debug_assert!(
                false,
                "tx head not marked in avail during publish: id={} slot={}",
                id, slot
            );
            return Err(TxHeadError::InvalidState);
        }
        self.mark_advertised(id)?;
        let record = self
            .publish_present
            .get_mut(id as usize)
            .ok_or(TxHeadError::OutOfRange)?;
        if *record {
            debug_assert!(
                false,
                "tx head already recorded as published: id={} slot={} state={:?}",
                id, slot, entry.state
            );
            return Err(TxHeadError::InvalidState);
        }
        *record = true;
        if let Some(record_slot) = self.publish_slot.get_mut(id as usize) {
            *record_slot = slot;
        }
        if let Some(record_gen) = self.publish_gen.get_mut(id as usize) {
            *record_gen = gen;
        }
        if let Some(record_avail) = self.publish_avail_idx.get_mut(id as usize) {
            *record_avail = avail_idx;
        }
        Ok(())
    }

    fn take_publish_record(
        &mut self,
        id: u16,
        expected_slot: u16,
        expected_gen: u32,
    ) -> Result<TxPublishRecord, TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        let record_present = self
            .publish_present
            .get_mut(id as usize)
            .ok_or(TxHeadError::OutOfRange)?;
        if !*record_present {
            return Err(TxHeadError::InvalidState);
        }
        let slot = *self
            .publish_slot
            .get(id as usize)
            .ok_or(TxHeadError::OutOfRange)?;
        let gen = *self
            .publish_gen
            .get(id as usize)
            .ok_or(TxHeadError::OutOfRange)?;
        let avail_idx = *self
            .publish_avail_idx
            .get(id as usize)
            .ok_or(TxHeadError::OutOfRange)?;
        *record_present = false;
        if slot != expected_slot || gen != expected_gen {
            debug_assert!(
                false,
                "tx publish record mismatch: id={} expected_slot={} expected_gen={} recorded_slot={} recorded_gen={}",
                id,
                expected_slot,
                expected_gen,
                slot,
                gen,
            );
            return Err(TxHeadError::InvalidState);
        }
        Ok(TxPublishRecord {
            slot,
            gen,
            avail_idx,
        })
    }

    fn mark_completed(
        &mut self,
        id: u16,
        expected_gen: Option<u32>,
    ) -> Result<(u16, u32), TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        let (slot, gen) = {
            let entry = self.entry_mut(id).ok_or(TxHeadError::OutOfRange)?;
            match entry.state {
                TxHeadState::InFlight { slot, gen } => (slot, gen),
                _ => return Err(TxHeadError::InvalidState),
            }
        };
        if let Some(expected) = expected_gen {
            if expected != gen {
                return Err(TxHeadError::InvalidState);
            }
        }
        if !self.is_advertised(id) {
            debug_assert!(
                false,
                "tx head completion missing advertised mark: id={} slot={} gen={}",
                id, slot, gen
            );
            return Err(TxHeadError::InvalidState);
        }
        if self.published_for_slot(slot) != Some((id, gen)) {
            return Err(TxHeadError::InvalidState);
        }
        if !self.in_avail.get(id as usize).copied().unwrap_or(false) {
            debug_assert!(
                false,
                "tx head completion without avail presence: id={} slot={} gen={}",
                id, slot, gen
            );
            return Err(TxHeadError::InvalidState);
        }
        let next_state = TxHeadState::Completed { gen };
        let prev_state = self
            .entry_mut(id)
            .map(|entry| {
                let prev_state = entry.state;
                entry.state = next_state;
                prev_state
            })
            .ok_or(TxHeadError::OutOfRange)?;
        self.trace_transition(id, prev_state, next_state, "complete");
        if let Some(slot_entry) = self.published_slots.get_mut(slot as usize) {
            if matches!(slot_entry, Some((head, g)) if *head == id && *g == gen) {
                *slot_entry = None;
            }
        }
        if let Some(flag) = self.in_avail.get_mut(id as usize) {
            *flag = false;
        }
        if let Some(present) = self.publish_present.get_mut(id as usize) {
            *present = false;
        }
        Ok((slot, gen))
    }

    fn reclaim_head(&mut self, id: u16) -> Result<(), TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        if !self.is_advertised(id) {
            debug_assert!(false, "tx head reclaim without advertise mark: id={}", id);
            return Err(TxHeadError::InvalidState);
        }
        if self.in_avail.get(id as usize).copied().unwrap_or(false) {
            let state = self.state(id);
            debug_assert!(
                false,
                "tx head reclaim while still marked in avail: id={} state={:?}",
                id, state
            );
            return Err(TxHeadError::InvalidState);
        }
        let prev_state = self
            .entry_mut(id)
            .map(|entry| {
                let prev_state = entry.state;
                (
                    prev_state,
                    matches!(entry.state, TxHeadState::Completed { .. }),
                )
            })
            .ok_or(TxHeadError::OutOfRange)?;
        if !prev_state.1 {
            return Err(TxHeadError::InvalidState);
        }
        let recorded_state = prev_state.0;
        let next_state = TxHeadState::Free;
        if let Some(entry) = self.entry_mut(id) {
            entry.state = next_state;
        }
        self.trace_transition(id, recorded_state, next_state, "reclaim");
        self.clear_advertised(id)?;
        if let Some(present) = self.publish_present.get_mut(id as usize) {
            *present = false;
        }
        self.mark_free_entry(id)
    }

    fn cancel_publish(&mut self, id: u16) -> Result<(), TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        if self.is_advertised(id) || self.publish_present(id) {
            return Err(TxHeadError::InvalidState);
        }
        let slot = {
            let entry = self.entry_mut(id).ok_or(TxHeadError::OutOfRange)?;
            match entry.state {
                TxHeadState::Published { slot, .. } => slot,
                _ => return Err(TxHeadError::InvalidState),
            }
        };
        let next = self.next_gen;
        self.next_gen = self.next_gen.wrapping_add(1);
        self.clear_slot(slot);
        let next_state = TxHeadState::Prepared { gen: next };
        let prev_state = self
            .entry_mut(id)
            .map(|entry| {
                let prev_state = entry.state;
                entry.state = next_state;
                prev_state
            })
            .ok_or(TxHeadError::OutOfRange)?;
        self.trace_transition(id, prev_state, next_state, "cancel");
        if let Some(flag) = self.in_avail.get_mut(id as usize) {
            *flag = false;
        }
        if let Some(present) = self.publish_present.get_mut(id as usize) {
            *present = false;
        }
        Ok(())
    }

    fn release_unused(&mut self, id: u16) -> Result<(), TxHeadError> {
        if id >= self.size {
            return Err(TxHeadError::OutOfRange);
        }
        if self.in_avail.get(id as usize).copied().unwrap_or(false) {
            let state = self.state(id);
            debug_assert!(
                false,
                "tx head release while still marked in avail: id={} state={:?}",
                id, state
            );
            return Err(TxHeadError::InvalidState);
        }
        let (prev_state, was_prepared) = self
            .entry_mut(id)
            .map(|entry| {
                let prev_state = entry.state;
                (
                    prev_state,
                    matches!(entry.state, TxHeadState::Prepared { .. }),
                )
            })
            .ok_or(TxHeadError::OutOfRange)?;
        if !was_prepared {
            debug_assert!(
                false,
                "tx head release violation: id={} state={:?}",
                id, prev_state
            );
            return Err(TxHeadError::InvalidState);
        }
        let next_state = TxHeadState::Free;
        if let Some(state) = self.entry_mut(id) {
            state.state = next_state;
            state.last_len = 0;
            state.last_addr = 0;
        }
        self.trace_transition(id, prev_state, next_state, "release");
        if let Some(present) = self.publish_present.get_mut(id as usize) {
            *present = false;
        }
        self.mark_free_entry(id)
    }

    fn is_in_flight(&self, id: u16) -> bool {
        matches!(self.state(id), Some(TxHeadState::InFlight { .. }))
    }

    fn free_len(&self) -> u16 {
        (self.free_mask & self.active_mask()).count_ones() as u16
    }

    fn in_flight_count(&self) -> u16 {
        self.entries
            .iter()
            .take(self.size as usize)
            .filter(|entry| {
                matches!(
                    entry.state,
                    TxHeadState::Published { .. } | TxHeadState::InFlight { .. }
                )
            })
            .count() as u16
    }

    fn posted_entries(&self) -> impl Iterator<Item = (usize, &TxHeadEntry)> {
        self.entries
            .iter()
            .enumerate()
            .take(self.size as usize)
            .filter(|(_, entry)| {
                matches!(
                    entry.state,
                    TxHeadState::Published { .. } | TxHeadState::InFlight { .. }
                )
            })
    }

    fn next_gen(&self) -> u32 {
        self.next_gen
    }

    fn published_for_slot(&self, slot: u16) -> Option<(u16, u32)> {
        self.published_slots.get(slot as usize).copied().flatten()
    }

    fn clear_slot(&mut self, slot: u16) {
        if let Some(slot_entry) = self.published_slots.get_mut(slot as usize) {
            *slot_entry = None;
        }
    }

    fn generation(&self, id: u16) -> Option<u32> {
        self.entries
            .get(id as usize)
            .and_then(|entry| match entry.state {
                TxHeadState::Prepared { gen }
                | TxHeadState::Published { gen, .. }
                | TxHeadState::InFlight { gen, .. }
                | TxHeadState::Completed { gen } => Some(gen),
                TxHeadState::Free => None,
            })
    }

    fn in_avail(&self, id: u16) -> bool {
        self.in_avail.get(id as usize).copied().unwrap_or(false)
    }

    fn publish_present(&self, id: u16) -> bool {
        self.publish_present
            .get(id as usize)
            .copied()
            .unwrap_or(false)
    }

    fn posted_count(&self) -> u16 {
        self.published_slots
            .iter()
            .take(self.size as usize)
            .filter(|entry| entry.is_some())
            .count() as u16
    }

    fn audit(&self) -> Result<(u16, u16, u16, u16, u16), TxHeadError> {
        let active_mask = self.active_mask();
        if self.free_mask & !active_mask != 0 {
            return Err(TxHeadError::InvalidState);
        }
        let mut free = 0u16;
        let mut prepared = 0u16;
        let mut in_flight = 0u16;
        let mut completed = 0u16;
        for idx in 0..self.size {
            let mask = self.free_mask_for(idx)?;
            let state = self.state(idx).ok_or(TxHeadError::OutOfRange)?;
            match state {
                TxHeadState::Free => {
                    free = free.saturating_add(1);
                    if (self.free_mask & mask) == 0 || self.in_avail(idx) || self.is_advertised(idx)
                    {
                        return Err(TxHeadError::InvalidState);
                    }
                }
                TxHeadState::Prepared { .. } => {
                    prepared = prepared.saturating_add(1);
                    if (self.free_mask & mask) != 0 {
                        return Err(TxHeadError::InvalidState);
                    }
                }
                TxHeadState::Published { .. } | TxHeadState::InFlight { .. } => {
                    in_flight = in_flight.saturating_add(1);
                    if (self.free_mask & mask) != 0 || !self.in_avail(idx) {
                        return Err(TxHeadError::InvalidState);
                    }
                }
                TxHeadState::Completed { .. } => {
                    completed = completed.saturating_add(1);
                    if (self.free_mask & mask) != 0 {
                        return Err(TxHeadError::InvalidState);
                    }
                }
            }
        }
        Ok((free, prepared, in_flight, completed, self.posted_count()))
    }

    fn counters(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.dup_alloc_refused,
            self.dup_publish_blocked,
            self.invalid_used_id,
            self.invalid_used_state,
            self.zero_len_publish_blocked,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TxAnomalyReason {
    SmoltcpRequestedZeroLen,
    ClosureWroteZero,
    DescLenZero,
    DescAddrZero,
    MultiDescriptor,
    UsedRingLenZeroBurst,
    FreeListCorrupt,
    RingIndexUnexpected,
    DmaReadbackMismatch,
}

#[derive(Clone, Copy, Debug)]
struct DescSpec {
    addr: u64,
    len: u32,
    flags: u16,
    next: Option<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QueueKind {
    Rx,
    Tx,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ForensicFaultReason {
    InvalidIndex,
    ZeroAddress,
    ZeroLength,
    InvalidNext,
    LoopDetected,
    UsedIdOutOfRange,
    UsedDescriptorZero,
    UsedLenZeroRepeat,
}

#[derive(Clone, Copy, Debug)]
struct ForensicFault {
    queue_name: &'static str,
    qsize: u16,
    head: u16,
    idx: u16,
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
    reason: ForensicFaultReason,
}

fn forensics_frozen() -> bool {
    FORENSICS && FORENSICS_FROZEN.load(AtomicOrdering::Acquire)
}

fn mark_forensics_frozen() -> bool {
    FORENSICS && !FORENSICS_FROZEN.swap(true, AtomicOrdering::AcqRel)
}

fn mark_forensics_dumped() -> bool {
    FORENSICS && !FORENSICS_DUMPED.swap(true, AtomicOrdering::AcqRel)
}

fn validate_chain_pre_publish(
    queue_name: &'static str,
    qsize: u16,
    desc: *mut VirtqDesc,
    head: u16,
) -> Result<(), ForensicFault> {
    if qsize == 0 {
        return Err(ForensicFault {
            queue_name,
            qsize,
            head,
            idx: head,
            addr: 0,
            len: 0,
            flags: 0,
            next: 0,
            reason: ForensicFaultReason::InvalidIndex,
        });
    }

    let mut idx = head;
    for _ in 0..qsize {
        if idx >= qsize {
            return Err(ForensicFault {
                queue_name,
                qsize,
                head,
                idx,
                addr: 0,
                len: 0,
                flags: 0,
                next: 0,
                reason: ForensicFaultReason::InvalidIndex,
            });
        }
        let desc_ptr = unsafe { desc.add(idx as usize) };
        let entry = unsafe { read_volatile(desc_ptr) };
        if entry.addr == 0 {
            return Err(ForensicFault {
                queue_name,
                qsize,
                head,
                idx,
                addr: entry.addr,
                len: entry.len,
                flags: entry.flags,
                next: entry.next,
                reason: ForensicFaultReason::ZeroAddress,
            });
        }
        if entry.len == 0 {
            return Err(ForensicFault {
                queue_name,
                qsize,
                head,
                idx,
                addr: entry.addr,
                len: entry.len,
                flags: entry.flags,
                next: entry.next,
                reason: ForensicFaultReason::ZeroLength,
            });
        }
        let has_next = (entry.flags & VIRTQ_DESC_F_NEXT) != 0;
        if has_next {
            if entry.next >= qsize {
                return Err(ForensicFault {
                    queue_name,
                    qsize,
                    head,
                    idx,
                    addr: entry.addr,
                    len: entry.len,
                    flags: entry.flags,
                    next: entry.next,
                    reason: ForensicFaultReason::InvalidNext,
                });
            }
            idx = entry.next;
        } else {
            return Ok(());
        }
    }

    Err(ForensicFault {
        queue_name,
        qsize,
        head,
        idx,
        addr: 0,
        len: 0,
        flags: 0,
        next: 0,
        reason: ForensicFaultReason::LoopDetected,
    })
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct VirtioNetHdr {
    flags: u8,
    gso_type: u8,
    hdr_len: u16,
    gso_size: u16,
    csum_start: u16,
    csum_offset: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct VirtioNetHdrMrgRxbuf {
    hdr: VirtioNetHdr,
    num_buffers: u16,
}

#[derive(Clone, Copy, Debug, Default)]
struct TxHeaderInspect {
    flags: u8,
    gso_type: u8,
    hdr_len: u16,
    csum_start: u16,
    csum_offset: u16,
}

/// Errors surfaced by the virtio network driver.
#[derive(Debug)]
pub enum DriverError {
    /// seL4 system call failure.
    Sel4(seL4_Error),
    /// HAL reported an error unrelated to seL4 syscalls.
    Hal(HalError),
    /// No virtio-net device was found on the MMIO bus.
    NoDevice,
    /// RX or TX queues were unavailable or zero sized.
    NoQueue,
    /// Ran out of DMA buffers when provisioning virtio rings.
    BufferExhausted,
    /// Driver stopped early due to a staged bring-up selection.
    Staged(NetStage),
    /// A virtqueue failed safety or consistency checks.
    QueueInvariant(&'static str),
    /// The virtio MMIO header contained an unexpected magic value.
    InvalidMagic(u32),
    /// The virtio MMIO header advertised an unsupported version.
    UnsupportedVersion(u32),
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sel4(err) => write!(f, "seL4 error {err:?}"),
            Self::Hal(err) => write!(f, "hal error: {err}"),
            Self::NoDevice => f.write_str("virtio-net device not found"),
            Self::NoQueue => f.write_str("virtio-net queues unavailable"),
            Self::BufferExhausted => f.write_str("virtio-net DMA buffer exhausted"),
            Self::Staged(stage) => write!(f, "virtio-net staged stop: {}", stage.as_str()),
            Self::QueueInvariant(reason) => write!(f, "virtqueue invariant failed: {reason}"),
            Self::InvalidMagic(magic) => {
                write!(f, "virtio-mmio magic value unexpected: 0x{magic:08x}")
            }
            Self::UnsupportedVersion(version) => {
                if *version == VIRTIO_MMIO_VERSION_LEGACY {
                    write!(
                        f,
                        "legacy virtio-mmio v1 requires --features virtio-mmio-legacy or QEMU -global virtio-mmio.force-legacy=false"
                    )
                } else {
                    write!(
                        f,
                        "virtio-mmio version 0x{version:08x} unsupported; prefer v2 or enable virtio-mmio-legacy for v1"
                    )
                }
            }
        }
    }
}

impl From<HalError> for DriverError {
    fn from(err: HalError) -> Self {
        match err {
            HalError::Sel4(code) => Self::Sel4(code),
            _ => Self::Hal(err),
        }
    }
}

impl NetDriverError for DriverError {
    fn is_absent(&self) -> bool {
        matches!(self, Self::NoDevice)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum TxPublishError {
    InvalidDescriptor,
    Queue(DmaError),
}

/// Virtio-net MMIO implementation providing a smoltcp PHY device.
pub struct VirtioNet {
    regs: VirtioRegs,
    mmio_mode: VirtioMmioMode,
    stage: NetStage,
    rx_queue: VirtQueue,
    tx_queue: VirtQueue,
    rx_buffers: HeaplessVec<RamFrame, RX_QUEUE_SIZE>,
    tx_buffers: HeaplessVec<RamFrame, TX_QUEUE_SIZE>,
    tx_v2_last_used: u16,
    tx_head_mgr: TxHeadManager,
    tx_slots: TxSlotTracker,
    tx_stats: VirtioNetTxStats,
    tx_diag: TxDiagState,
    tx_slot_divergence_logged: bool,
    dma_cacheable: bool,
    tx_last_used_seen: u16,
    tx_progress_log_gate: u32,
    negotiated_features: u64,
    tx_header_len: usize,
    rx_header_len: usize,
    rx_payload_capacity: usize,
    rx_frame_capacity: usize,
    rx_buffer_capacity: usize,
    tx_drops: u32,
    tx_packets: u64,
    tx_used_count: u64,
    tx_attempt_seq: u64,
    tx_attempt_log_gate: u64,
    rx_packets: u64,
    rx_publish_calls: u64,
    tx_publish_calls: u64,
    used_poll_calls: u64,
    mac: EthernetAddress,
    rx_poll_count: u64,
    rx_used_count: u64,
    last_used_idx_debug: u16,
    last_snapshot_rx_used: u16,
    last_snapshot_tx_used: u16,
    last_progress_ms: u64,
    last_snapshot_ms: u64,
    stalled_snapshot_logged: bool,
    tx_post_logged: bool,
    device_faulted: bool,
    bad_status_seen: bool,
    bad_status_logged: bool,
    descriptor_corrupt_logged: bool,
    tx_desc_clear_violation_logged: bool,
    tx_state_violation_logged: bool,
    tx_anomaly_logged: bool,
    tx_descriptor_dumped: bool,
    tx_used_window_dumped: bool,
    tx_dma_log_once: bool,
    tx_publish_verify_count: u32,
    tx_publish_guard_logged: bool,
    rx_zero_len_logged: bool,
    tx_zero_len_logged: bool,
    tx_zero_len_publish_logged: bool,
    rx_header_zero_logged: bool,
    rx_payload_zero_logged: bool,
    tx_used_recent: HeaplessVec<(u16, u32), TX_QUEUE_SIZE>,
    tx_wrap_logged: bool,
    tx_stats_log_ms: u64,
    rx_underflow_logged_ids: HeaplessVec<u16, RX_QUEUE_SIZE>,
    last_error: Option<&'static str>,
    rx_requeue_logged_ids: HeaplessVec<u16, RX_QUEUE_SIZE>,
    rx_publish_log_count: u32,
    tx_publish_log_count: u32,
    tx_dup_publish_log_ms: u64,
    tx_used_gen_mismatch_logged: bool,
    tx_invalid_publish_logged: bool,
    tx_publish_readback_logged: bool,
    tx_reclaim_state_violation_logged: bool,
    tx_reclaim_stall_logged: bool,
    #[cfg(debug_assertions)]
    tx_avail_duplicate_logged: bool,
    #[cfg(debug_assertions)]
    tx_publish_frozen: bool,
    tx_double_submit: u64,
    tx_dup_publish_blocked: u64,
    tx_bad_id_mapping: u64,
    tx_dup_used_ignored: u64,
    tx_invalid_used_state: u64,
    tx_used_zero_len_seen: u64,
    tx_alloc_blocked_inflight: u64,
    tx_invalid_publish: u64,
    tx_drop_zero_len: u64,
    tx_zero_len_attempt: u64,
    tx_zero_desc_guard: u64,
    tx_zero_desc_warn_ms: u64,
    tx_invariant_violations: u64,
    dropped_zero_len_tx: u64,
    tx_submit: u64,
    tx_complete: u64,
    publish_blocked_bad_slot_state: u64,
    publish_blocked_bad_head_state: u64,
    publish_blocked_zero_len: u64,
    token_double_consume: u64,
    tx_pending_since_kick: u16,
    tx_bytes_since_kick: usize,
    tx_canary_front: u32,
    tx_canary_back: u32,
    tx_canary_fault_logged: bool,
    tx_zero_len_log_ms: u64,
    tx_v2_log_ms: u64,
    tx_audit_log_ms: u64,
    tx_invalid_used_log_ms: u64,
    tx_used_zero_len_log_ms: u64,
    #[cfg(debug_assertions)]
    tx_reclaim_stall_polls: u16,
    #[cfg(debug_assertions)]
    tx_reclaim_stall_latched: bool,
    tx_audit_violation_logged: bool,
    forensic_dump_captured: bool,
}

/// Virtio-net driver stored in static memory to avoid large stack frames during init.
#[cfg(feature = "net-backend-virtio")]
pub struct VirtioNetStatic {
    driver: &'static mut VirtioNet,
}

impl VirtioNet {
    /// Create a new driver instance by probing the virtio MMIO slots.
    pub fn new<H>(hal: &mut H) -> Result<Self, DriverError>
    where
        H: Hardware<Error = HalError>,
    {
        Self::new_with_stage(hal, NET_STAGE)
    }

    /// Create a new driver instance for the requested bring-up stage.
    pub fn new_with_stage<H>(hal: &mut H, stage: NetStage) -> Result<Self, DriverError>
    where
        H: Hardware<Error = HalError>,
    {
        #[cfg(debug_assertions)]
        if !VIRTIO_NET_SIZE_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            info!(
                "[net-console] virtio-net sizes: VirtioNet={} TxHeadManager={} TxHeadEntry={}",
                core::mem::size_of::<Self>(),
                core::mem::size_of::<TxHeadManager>(),
                core::mem::size_of::<TxHeadEntry>(),
            );
        }
        info!("[net-console] init: probing virtio-mmio bus");
        info!(
            "[net-console] expecting virtio-net on virtio-mmio base=0x{base:08x}, slots=0-{max_slot}, stride=0x{stride:03x}",
            base = VIRTIO_MMIO_BASE,
            max_slot = VIRTIO_MMIO_SLOTS - 1,
            stride = VIRTIO_MMIO_STRIDE,
        );
        let mut regs = VirtioRegs::probe(hal)?;
        let mmio_mode = regs.mode;
        info!(
            "[net-console] virtio-mmio device located: base=0x{base:08x}",
            base = regs.base().as_ptr() as usize
        );
        match regs.mode {
            VirtioMmioMode::Modern => {
                info!("[net-console] modern virtio-mmio v2 detected; continuing")
            }
            VirtioMmioMode::Legacy => {
                info!("[net-console] legacy virtio-mmio v1 enabled via feature flag")
            }
        }

        info!("[net-console] resetting virtio-net status register");
        regs.reset_status();
        regs.set_status(STATUS_ACKNOWLEDGE);
        info!(
            target: "net-console",
            "[net-console] status set to ACKNOWLEDGE: 0x{:02x}",
            regs.read32(Registers::Status)
        );
        check_bootinfo_canary("virtio.status.ack")?;
        regs.set_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        info!(
            target: "net-console",
            "[net-console] status set to DRIVER: 0x{:02x}",
            regs.read32(Registers::Status)
        );

        if matches!(mmio_mode, VirtioMmioMode::Legacy) {
            // Legacy virtio-mmio devices require the guest page size to be provided
            // before queue PFNs are written. Without this, the device disregards the
            // queue configuration and leaves RX/TX rings idle. Advertise the seL4
            // 4KiB page size up-front so the PFN calculations below are interpreted
            // correctly by the device.
            let guest_page_size = 1u32 << seL4_PageBits;
            regs.set_guest_page_size(guest_page_size);
            info!(
                target: "net-console",
                "[net-console] guest page size set: {} bytes",
                guest_page_size
            );
        }

        info!("[net-console] querying queue sizes");
        regs.select_queue(RX_QUEUE_INDEX);
        let rx_max = regs.queue_num_max();
        regs.select_queue(TX_QUEUE_INDEX);
        let tx_max = regs.queue_num_max();
        let rx_size = core::cmp::min(rx_max as usize, RX_QUEUE_SIZE);
        let tx_size = core::cmp::min(tx_max as usize, TX_QUEUE_SIZE);
        if rx_size == 0 || tx_size == 0 {
            regs.set_status(STATUS_FAILED);
            warn!(
                "[net-console] virtio-net queues unavailable: rx_max={} tx_max={}",
                rx_max, tx_max
            );
            return Err(DriverError::NoQueue);
        }

        info!(
            "[net-console] queue sizes: rx_max={} rx_size={} tx_max={} tx_size={}",
            rx_max, rx_size, tx_max, tx_size
        );

        if (rx_size as u32) > rx_max {
            error!(
                target: "net-console",
                "[net-console] RX queue_size={} > rx_max={} — this is a bug",
                rx_size,
                rx_max
            );
            return Err(DriverError::NoQueue);
        }

        if (tx_size as u32) > tx_max {
            error!(
                target: "net-console",
                "[net-console] TX queue_size={} > tx_max={} — this is a bug",
                tx_size,
                tx_max
            );
            return Err(DriverError::NoQueue);
        }

        info!("[net-console] reading host feature bits");
        let host_features = regs.host_features();
        let supported_features = match mmio_mode {
            VirtioMmioMode::Modern => SUPPORTED_NET_FEATURES | VIRTIO_F_VERSION_1,
            VirtioMmioMode::Legacy => SUPPORTED_NET_FEATURES,
        };
        let negotiated_features = host_features & supported_features;
        info!(
            target: "virtio-net",
            "virtio-net features: {:#x}",
            negotiated_features
        );
        let merge_rxbuf = negotiated_features & VIRTIO_NET_F_MRG_RXBUF != 0;
        let net_header_len = if merge_rxbuf {
            VIRTIO_NET_HEADER_LEN_MRG
        } else {
            VIRTIO_NET_HEADER_LEN_BASIC
        };
        info!(
            "[net-console] features: host=0x{host:016x} negotiated=0x{guest:016x}",
            host = host_features,
            guest = negotiated_features
        );
        log::info!(
            target: "virtio-net",
            "features: negotiated=0x{:x}, queue_sizes RX={} TX={}",
            negotiated_features,
            rx_size,
            tx_size,
        );
        info!(
            target: "net-console",
            "[net-console] virtio-net header size={} (virtio-net hdr provided for legacy and modern devices)",
            net_header_len
        );
        regs.set_guest_features(negotiated_features);
        info!(
            target: "net-console",
            "[net-console] guest features set: status=0x{:02x}",
            regs.read32(Registers::Status)
        );
        let mut status = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
        regs.set_status(status);
        let status_after_features = regs.read32(Registers::Status);
        info!(
            target: "net-console",
            "[net-console] status set to FEATURES_OK: 0x{status_after_features:02x}",
        );
        check_bootinfo_canary("virtio.status.features_ok")?;
        if status_after_features & STATUS_FEATURES_OK == 0 {
            regs.set_status(STATUS_FAILED);
            error!(
                target: "net-console",
                "[virtio-net] device rejected FEATURES_OK: status=0x{status_after_features:02x}"
            );
            return Err(DriverError::NoQueue);
        }

        if stage == NetStage::ProbeOnly {
            return Err(DriverError::Staged(stage));
        }

        info!("[net-console] allocating virtqueue backing memory");
        let queue_map_attr = seL4_ARM_Page_Uncached;
        // Keep virtqueue rings fully visible to the device by mapping the backing pages uncached.

        let queue_mem_rx = hal.alloc_dma_frame_attr(queue_map_attr).map_err(|err| {
            regs.set_status(STATUS_FAILED);
            DriverError::from(err)
        })?;

        let queue_mem_tx = {
            let mut attempt = 0;
            loop {
                let frame = hal.alloc_dma_frame_attr(queue_map_attr).map_err(|err| {
                    regs.set_status(STATUS_FAILED);
                    DriverError::from(err)
                })?;
                if frame.paddr() != queue_mem_rx.paddr() {
                    break frame;
                }
                attempt += 1;
                warn!(
                    target: "net-console",
                    "[virtio-net] RX/TX queue PFN collision on attempt {attempt}; allocating a new TX frame"
                );
                if attempt >= 3 {
                    regs.set_status(STATUS_FAILED);
                    error!(
                        target: "net-console",
                        "[virtio-net] failed to allocate distinct TX queue backing after {attempt} attempts"
                    );
                    return Err(DriverError::NoQueue);
                }
            }
        };
        let guard_vaddr = if VIRTIO_GUARD_QUEUE {
            Some(hal.reserve_dma_guard_page().map_err(|err| {
                regs.set_status(STATUS_FAILED);
                DriverError::from(err)
            })?)
        } else {
            None
        };
        let dma_cacheable = match queue_map_attr {
            seL4_ARM_Page_Uncached => false,
            other => {
                error!(
                    target: "net-console",
                    "[virtio-net] unsupported queue map attribute: attr=0x{other:08x}; virtio-net disabled",
                );
                regs.set_status(STATUS_FAILED);
                return Err(DriverError::NoQueue);
            }
        };
        if !DMA_QMEM_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            let rx_vaddr = queue_mem_rx.ptr().as_ptr() as usize;
            let tx_vaddr = queue_mem_tx.ptr().as_ptr() as usize;
            let rx_len = queue_mem_rx.as_slice().len();
            let tx_len = queue_mem_tx.as_slice().len();
            let rx_paddr = queue_mem_rx.paddr();
            let tx_paddr = queue_mem_tx.paddr();
            let map_attr_raw: usize = unsafe { core::mem::transmute(queue_map_attr) };
            info!(
                target: "virtio-net",
                "[virtio-net][dma] qmem mapping cacheable={} map_attr=0x{map_attr_raw:08x} rx_vaddr=0x{rx_vaddr:016x}..0x{rx_vend:016x} rx_paddr=0x{rx_paddr:016x}..0x{rx_pend:016x} tx_vaddr=0x{tx_vaddr:016x}..0x{tx_vend:016x} tx_paddr=0x{tx_paddr:016x}..0x{tx_pend:016x}",
                dma_cacheable,
                rx_vaddr = rx_vaddr,
                rx_vend = rx_vaddr.saturating_add(rx_len),
                rx_paddr = rx_paddr,
                rx_pend = rx_paddr.saturating_add(rx_len),
                tx_vaddr = tx_vaddr,
                tx_vend = tx_vaddr.saturating_add(tx_len),
                tx_paddr = tx_paddr,
                tx_pend = tx_paddr.saturating_add(tx_len),
            );
        }
        if let Some(guard_vaddr) = guard_vaddr {
            let rx_vaddr = queue_mem_rx.ptr().as_ptr() as usize;
            let tx_vaddr = queue_mem_tx.ptr().as_ptr() as usize;
            let page_bytes = 1usize << seL4_PageBits;
            let base = core::cmp::min(rx_vaddr, tx_vaddr);
            let end = core::cmp::max(rx_vaddr, tx_vaddr).saturating_add(page_bytes);
            let len = end.saturating_sub(base);
            info!(
                target: "virtio-net",
                "virtio.guard_queue=1 base=0x{base:016x} len={len} guard=0x{guard:016x}",
                base = base,
                guard = guard_vaddr,
            );
        }

        info!(
            "[net-console] provisioning RX descriptors ({} entries)",
            rx_size
        );
        let rx_queue = VirtQueue::new(
            &mut regs,
            queue_mem_rx,
            RX_QUEUE_INDEX,
            rx_size,
            mmio_mode,
            dma_cacheable,
            VIRTIO_GUARD_QUEUE && matches!(mmio_mode, VirtioMmioMode::Modern),
        )
        .map_err(|err| {
            regs.set_status(STATUS_FAILED);
            err
        })?;
        info!(
            "[net-console] provisioning TX descriptors ({} entries)",
            tx_size
        );
        let tx_queue = VirtQueue::new(
            &mut regs,
            queue_mem_tx,
            TX_QUEUE_INDEX,
            tx_size,
            mmio_mode,
            dma_cacheable,
            VIRTIO_GUARD_QUEUE && matches!(mmio_mode, VirtioMmioMode::Modern),
        )
        .map_err(|err| {
            regs.set_status(STATUS_FAILED);
            err
        })?;

        if stage == NetStage::QueueInitOnly {
            return Err(DriverError::Staged(stage));
        }

        let mut rx_buffers = HeaplessVec::<RamFrame, RX_QUEUE_SIZE>::new();
        for _ in 0..rx_size {
            let frame = hal.alloc_dma_frame_attr(queue_map_attr).map_err(|err| {
                regs.set_status(STATUS_FAILED);
                DriverError::from(err)
            })?;
            rx_buffers.push(frame).map_err(|_| {
                regs.set_status(STATUS_FAILED);
                DriverError::BufferExhausted
            })?;
        }

        let mut tx_buffers = HeaplessVec::<RamFrame, TX_QUEUE_SIZE>::new();
        for _ in 0..tx_size {
            let frame = hal.alloc_dma_frame_attr(queue_map_attr).map_err(|err| {
                regs.set_status(STATUS_FAILED);
                DriverError::from(err)
            })?;
            tx_buffers.push(frame).map_err(|_| {
                regs.set_status(STATUS_FAILED);
                DriverError::BufferExhausted
            })?;
        }

        let rx_buffer_capacity = rx_buffers
            .first()
            .map(|frame| FRAME_BUFFER_LEN.min(frame.as_slice().len()))
            .unwrap_or(0);
        let rx_payload_capacity = rx_buffer_capacity.saturating_sub(net_header_len);
        let rx_frame_capacity = net_header_len.saturating_add(rx_payload_capacity);
        if net_header_len == 0 || rx_payload_capacity == 0 || rx_frame_capacity == 0 {
            regs.set_status(STATUS_FAILED);
            error!(
                target: "net-console",
                "[virtio-net] invalid RX capacity: header_len={} payload_capacity={} frame_capacity={} buffer_capacity={}",
                net_header_len,
                rx_payload_capacity,
                rx_frame_capacity,
                rx_buffer_capacity,
            );
            return Err(DriverError::QueueInvariant(
                "virtio-net rx capacity must be non-zero",
            ));
        }

        let fallback_mac = EthernetAddress::from_bytes(&[0x02, 0, 0, 0, 0, 1]);
        let mac = if negotiated_features & VIRTIO_NET_F_MAC != 0 {
            let reported = regs.read_mac().unwrap_or(fallback_mac);
            info!("[net-console] device-reported MAC: {reported}");
            reported
        } else {
            fallback_mac
        };

        info!(
            "[net-console] virtio-net ready: rx_buffers={} tx_buffers={} mac={}",
            rx_buffers.len(),
            tx_buffers.len(),
            mac
        );

        let now_ms = crate::hal::timebase().now_ms();

        let mut driver = Self {
            regs,
            mmio_mode,
            stage,
            rx_queue,
            tx_queue,
            rx_buffers,
            tx_buffers,
            tx_v2_last_used: 0,
            tx_head_mgr: TxHeadManager::new(tx_size as u16),
            tx_slots: TxSlotTracker::new(tx_size as u16),
            tx_stats: VirtioNetTxStats::default(),
            tx_diag: TxDiagState::default(),
            tx_slot_divergence_logged: false,
            tx_last_used_seen: 0,
            tx_progress_log_gate: 0,
            negotiated_features,
            tx_header_len: net_header_len,
            dma_cacheable,
            rx_header_len: net_header_len,
            rx_payload_capacity,
            rx_frame_capacity,
            rx_buffer_capacity,
            tx_drops: 0,
            tx_packets: 0,
            tx_used_count: 0,
            tx_attempt_seq: 0,
            tx_attempt_log_gate: 0,
            rx_packets: 0,
            rx_publish_calls: 0,
            tx_publish_calls: 0,
            used_poll_calls: 0,
            mac,
            rx_poll_count: 0,
            rx_used_count: 0,
            last_used_idx_debug: 0,
            last_snapshot_rx_used: 0,
            last_snapshot_tx_used: 0,
            last_progress_ms: now_ms,
            last_snapshot_ms: now_ms,
            stalled_snapshot_logged: false,
            tx_post_logged: false,
            device_faulted: false,
            bad_status_seen: false,
            bad_status_logged: false,
            descriptor_corrupt_logged: false,
            tx_desc_clear_violation_logged: false,
            tx_state_violation_logged: false,
            tx_anomaly_logged: false,
            tx_descriptor_dumped: false,
            tx_used_window_dumped: false,
            tx_dma_log_once: false,
            tx_publish_verify_count: 0,
            tx_publish_guard_logged: false,
            rx_zero_len_logged: false,
            tx_zero_len_logged: false,
            tx_zero_len_publish_logged: false,
            rx_header_zero_logged: false,
            rx_payload_zero_logged: false,
            tx_used_recent: HeaplessVec::new(),
            tx_wrap_logged: false,
            tx_stats_log_ms: now_ms,
            rx_underflow_logged_ids: HeaplessVec::new(),
            last_error: None,
            rx_requeue_logged_ids: HeaplessVec::new(),
            rx_publish_log_count: 0,
            tx_publish_log_count: 0,
            tx_dup_publish_log_ms: 0,
            tx_used_gen_mismatch_logged: false,
            tx_invalid_publish_logged: false,
            tx_publish_readback_logged: false,
            tx_reclaim_state_violation_logged: false,
            tx_reclaim_stall_logged: false,
            #[cfg(debug_assertions)]
            tx_avail_duplicate_logged: false,
            #[cfg(debug_assertions)]
            tx_publish_frozen: false,
            tx_double_submit: 0,
            tx_dup_publish_blocked: 0,
            tx_bad_id_mapping: 0,
            tx_dup_used_ignored: 0,
            tx_invalid_used_state: 0,
            tx_used_zero_len_seen: 0,
            tx_alloc_blocked_inflight: 0,
            tx_invalid_publish: 0,
            tx_drop_zero_len: 0,
            tx_zero_len_attempt: 0,
            tx_zero_desc_guard: 0,
            tx_zero_desc_warn_ms: 0,
            tx_invariant_violations: 0,
            dropped_zero_len_tx: 0,
            tx_submit: 0,
            tx_complete: 0,
            publish_blocked_bad_slot_state: 0,
            publish_blocked_bad_head_state: 0,
            publish_blocked_zero_len: 0,
            token_double_consume: 0,
            tx_pending_since_kick: 0,
            tx_bytes_since_kick: 0,
            tx_canary_front: TX_CANARY_VALUE,
            tx_canary_back: TX_CANARY_VALUE,
            tx_canary_fault_logged: false,
            tx_zero_len_log_ms: now_ms,
            tx_v2_log_ms: now_ms,
            tx_audit_log_ms: now_ms,
            tx_invalid_used_log_ms: 0,
            tx_used_zero_len_log_ms: 0,
            #[cfg(debug_assertions)]
            tx_reclaim_stall_polls: 0,
            #[cfg(debug_assertions)]
            tx_reclaim_stall_latched: false,
            tx_audit_violation_logged: false,
            forensic_dump_captured: false,
        };
        driver.initialise_queues();

        let queue0_pfn = driver.rx_queue.pfn;
        let queue1_pfn = driver.tx_queue.pfn;
        let status_reg_value = driver.regs.status();
        log::info!(
            target: "net-console",
            "[virtio-net] post-setup: queue0_pfn=0x{:x}, queue1_pfn=0x{:x}, status=0x{:02x}",
            queue0_pfn,
            queue1_pfn,
            status_reg_value,
        );
        if queue0_pfn == queue1_pfn {
            warn!(
                target: "net-console",
                "[virtio-net] warning: RX/TX queues share PFN 0x{:x}; DMA frames should be distinct",
                queue0_pfn
            );
        }

        info!(target: "virtio-net", "[virtio-net] DRIVER_OK about to set");
        status |= STATUS_DRIVER_OK;
        driver.regs.set_status(status);
        info!(
            "[net-console] driver status set to DRIVER_OK (status=0x{:02x})",
            driver.regs.read32(Registers::Status)
        );
        Ok(driver)
    }

    /// Return the negotiated Ethernet address for the device.
    #[must_use]
    pub fn mac(&self) -> EthernetAddress {
        self.mac
    }

    /// Fetch the number of frames dropped due to TX descriptor exhaustion.
    #[must_use]
    pub fn tx_drop_count(&self) -> u32 {
        self.tx_drops
    }

    pub fn debug_snapshot(&mut self) {
        let now_ms = crate::hal::timebase().now_ms();
        let stalled = now_ms.saturating_sub(self.last_progress_ms) >= 1_000;
        if !stalled {
            return;
        }

        if self.stalled_snapshot_logged && now_ms.saturating_sub(self.last_snapshot_ms) < 1_000 {
            return;
        }

        let isr = self.regs.isr_status();
        let status = self.regs.status();
        let (rx_used_idx, rx_avail_idx) = self.rx_queue.indices();
        let (tx_used_idx, tx_avail_idx) = self.tx_queue.indices();

        if self.stalled_snapshot_logged
            && rx_used_idx == self.last_snapshot_rx_used
            && tx_used_idx == self.last_snapshot_tx_used
        {
            return;
        }

        self.last_snapshot_rx_used = rx_used_idx;
        self.last_snapshot_tx_used = tx_used_idx;
        self.last_snapshot_ms = now_ms;
        self.stalled_snapshot_logged = true;

        self.rx_queue.debug_dump("rx");
        self.tx_queue.debug_dump("tx");

        log::info!(
            target: "net-console",
            "[virtio-net] debug_snapshot: stalled_ms={} status=0x{:02x} isr=0x{:02x} tx_avail_idx={} tx_used_idx={} rx_avail_idx={} rx_used_idx={} last_error={} rx_used_count={} rx_poll_count={} used_poll_calls={} tx_publish_calls={} rx_publish_calls={} tx_dup_publish_blocked={} tx_bad_id_mapping={} tx_dup_used_ignored={} tx_invalid_used_state={} tx_alloc_blocked_inflight={} dropped_zero_len_tx={}",
            now_ms.saturating_sub(self.last_progress_ms),
            status,
            isr,
            tx_avail_idx,
            tx_used_idx,
            rx_avail_idx,
            rx_used_idx,
            self.last_error.unwrap_or("none"),
            self.rx_used_count,
            self.rx_poll_count,
            self.used_poll_calls,
            self.tx_publish_calls,
            self.rx_publish_calls,
            self.tx_dup_publish_blocked,
            self.tx_bad_id_mapping,
            self.tx_dup_used_ignored,
            self.tx_invalid_used_state,
            self.tx_alloc_blocked_inflight,
            self.dropped_zero_len_tx,
        );
    }

    fn log_zero_len_enqueue(
        &mut self,
        queue_label: &'static str,
        head_id: u16,
        descs: &[DescSpec],
        header_len: Option<usize>,
        payload_len: Option<usize>,
        frame_capacity: Option<usize>,
        used_len: Option<usize>,
    ) {
        let already_logged = match queue_label {
            "RX" => &mut self.rx_zero_len_logged,
            _ => &mut self.tx_zero_len_logged,
        };
        if *already_logged {
            return;
        }
        *already_logged = true;

        let queue = match queue_label {
            "RX" => &self.rx_queue,
            _ => &self.tx_queue,
        };
        let (used_idx, avail_idx) = queue.indices();
        let pending = avail_idx.wrapping_sub(used_idx);
        let free_entries = queue.size.saturating_sub(pending);

        error!(
            target: "net-console",
            "[virtio-net] blocked zero-len descriptor enqueue: queue={} head_id={} used_idx={} avail_idx={} last_used={} pending={} free_entries={} header_len={:?} payload_len={:?} frame_capacity={:?} used_len={:?}",
            queue_label,
            head_id,
            used_idx,
            avail_idx,
            queue.last_used,
            pending,
            free_entries,
            header_len,
            payload_len,
            frame_capacity,
            used_len,
        );
        for (idx, spec) in descs.iter().enumerate() {
            error!(
                target: "net-console",
                "[virtio-net] zero-len chain desc[{idx}]: addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next:?}",
                len = spec.len,
                addr = spec.addr,
                flags = spec.flags,
                next = spec.next,
            );
        }
    }

    fn log_zero_len_publish(
        &mut self,
        head_id: u16,
        publish_slot: u16,
        desc_len: u32,
        total_len: u32,
    ) {
        if self.tx_zero_len_publish_logged {
            return;
        }
        self.tx_zero_len_publish_logged = true;
        let header_len = self.rx_header_len as u32;
        let payload_len = total_len.saturating_sub(header_len);
        let (used_idx, avail_idx) = self.tx_queue.indices_no_sync();
        let reason = if desc_len == 0 {
            "desc_len_zero"
        } else {
            "total_len_zero"
        };
        error!(
            target: "net-console",
            "[virtio-net][tx-guard] zero-len publish blocked: head={} slot={} header_len={} payload_len={} total_len={} desc_len={} avail_idx={} used_idx={} reason={}",
            head_id,
            publish_slot,
            header_len,
            payload_len,
            total_len,
            desc_len,
            avail_idx,
            used_idx,
            reason,
        );
    }

    fn validate_chain_nonzero(
        &mut self,
        queue_label: &'static str,
        head_id: u16,
        descs: &[DescSpec],
        header_len: Option<usize>,
        payload_len: Option<usize>,
        frame_capacity: Option<usize>,
        used_len: Option<usize>,
    ) -> Result<(), ()> {
        let zero_header = header_len.map_or(false, |len| len == 0);
        let zero_payload = payload_len.map_or(false, |len| len == 0);
        let zero_capacity = frame_capacity.map_or(false, |len| len == 0);
        let zero_desc = descs.iter().any(|desc| desc.len == 0);

        if zero_header || zero_payload || zero_capacity || zero_desc {
            self.log_zero_len_enqueue(
                queue_label,
                head_id,
                descs,
                header_len,
                payload_len,
                frame_capacity,
                used_len,
            );
            self.device_faulted = true;
            self.last_error = Some("zero_len_desc");
            return Err(());
        }

        Ok(())
    }

    fn describe_fault_reason(reason: ForensicFaultReason) -> &'static str {
        match reason {
            ForensicFaultReason::InvalidIndex => "invalid_index",
            ForensicFaultReason::ZeroAddress => "zero_addr",
            ForensicFaultReason::ZeroLength => "zero_len",
            ForensicFaultReason::InvalidNext => "invalid_next",
            ForensicFaultReason::LoopDetected => "loop_detected",
            ForensicFaultReason::UsedIdOutOfRange => "used_id_oob",
            ForensicFaultReason::UsedDescriptorZero => "used_desc_zero",
            ForensicFaultReason::UsedLenZeroRepeat => "used_len_zero_repeat",
        }
    }

    fn log_forensic_fault(&self, fault: &ForensicFault) {
        error!(
            target: "net-console",
            "[virtio-net][forensics] fault queue={} head={} idx={} qsize={} addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next} reason={reason}",
            fault.queue_name,
            fault.head,
            fault.idx,
            fault.qsize,
            addr = fault.addr,
            len = fault.len,
            flags = fault.flags,
            next = fault.next,
            reason = Self::describe_fault_reason(fault.reason),
        );
    }

    fn handle_forensic_fault(&mut self, fault: ForensicFault) -> Result<(), ()> {
        self.log_forensic_fault(&fault);
        self.freeze_and_capture("forensic_fault");
        self.device_faulted = true;
        self.last_error.get_or_insert("forensic_fault");
        Err(())
    }

    fn freeze_and_capture(&mut self, reason: &'static str) {
        if mark_forensics_frozen() {
            warn!(
                target: "net-console",
                "[virtio-net][forensics] freezing queue activity (reason={reason})"
            );
        }
        self.device_faulted = true;
        self.last_error.get_or_insert(reason);
        self.capture_forensics_once(reason);
    }

    fn capture_forensics_once(&mut self, reason: &'static str) {
        if !FORENSICS || self.forensic_dump_captured || !mark_forensics_dumped() {
            return;
        }
        self.forensic_dump_captured = true;
        let status = self.regs.status();
        let isr = self.regs.isr_status();
        error!(
            target: "net-console",
            "[virtio-net][forensics] capturing dump: reason={} status=0x{status:02x} isr=0x{isr:02x}",
            reason,
        );
        self.rx_queue.debug_dump("rx");
        self.tx_queue.debug_dump("tx");
        self.dump_rx_window();
        self.dump_tx_avail_window();
        self.dump_tx_used_window();
        self.dump_tx_states();
        self.dump_descriptor_table("rx", &self.rx_queue);
        self.dump_descriptor_table("tx", &self.tx_queue);
        debug!(
            target: "net-console",
            "[virtio-net][forensics] capture complete (reason={reason})"
        );
    }

    fn dump_descriptor_table(&self, label: &str, queue: &VirtQueue) {
        for idx in 0..queue.size {
            let desc = queue.read_descriptor(idx);
            info!(
                target: "net-console",
                "[virtio-net][forensics] {label} desc[{idx}]: addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next}",
                addr = desc.addr,
                len = desc.len,
                flags = desc.flags,
                next = desc.next,
            );
        }
    }

    fn dump_tx_avail_window(&self) {
        let qsize = usize::from(self.tx_queue.size);
        if qsize == 0 {
            return;
        }
        let avail = self.tx_queue.avail.as_ptr();
        let avail_idx = unsafe { read_volatile(&(*avail).idx) };
        let used_idx = self.tx_queue.last_used;
        let window = core::cmp::min(qsize, 16);
        let start = avail_idx.wrapping_sub(window as u16);
        info!(
            target: "net-console",
            "[virtio-net][forensics] tx avail window: avail.idx={} used.idx={} size={}",
            avail_idx,
            used_idx,
            qsize,
        );
        for offset in 0..window {
            let slot_idx = start.wrapping_add(offset as u16);
            let ring_slot = (slot_idx as usize) % qsize;
            let ring_ptr = unsafe { (*avail).ring.as_ptr().add(ring_slot) as *const u16 };
            let head = unsafe { read_volatile(ring_ptr) };
            let desc = self.tx_queue.read_descriptor(head);
            info!(
                target: "net-console",
                "[virtio-net][forensics] tx avail[{ring_slot}] -> head={head} desc{head}=0x{addr:016x}/{len}/0x{flags:04x}/next={next}",
                addr = desc.addr,
                len = desc.len,
                flags = desc.flags,
                next = desc.next,
            );
        }
    }

    fn dump_tx_used_window(&self) {
        let qsize = usize::from(self.tx_queue.size);
        if qsize == 0 {
            return;
        }
        let used = self.tx_queue.used.as_ptr();
        if self.tx_queue.invalidate_used_header_for_cpu().is_err() {
            warn!(
                target: "net-console",
                "[virtio-net][forensics] used header invalidate failed; aborting dump"
            );
            return;
        }
        let used_idx = unsafe { read_volatile(&(*used).idx) };
        let last_used = self.tx_queue.last_used;
        let window = core::cmp::min(qsize, 16);
        let start = last_used.wrapping_sub(window as u16 / 2);
        info!(
            target: "net-console",
            "[virtio-net][forensics] tx used window: last_used={} used.idx={} size={} window={}",
            last_used,
            used_idx,
            qsize,
            window,
        );
        for offset in 0..window {
            let slot_idx = start.wrapping_add(offset as u16);
            let ring_slot = (slot_idx as usize) % qsize;
            let ring_ptr = unsafe { (*used).ring.as_ptr().add(ring_slot) as *const VirtqUsedElem };
            if self
                .tx_queue
                .invalidate_used_elem_for_cpu(ring_slot)
                .is_err()
            {
                warn!(
                    target: "net-console",
                    "[virtio-net][forensics] used elem invalidate failed slot={ring_slot}; aborting dump"
                );
                return;
            }
            let elem = unsafe { read_volatile(ring_ptr) };
            info!(
                target: "net-console",
                "[virtio-net][forensics] tx used[{ring_slot}] -> id={} len={}",
                elem.id,
                elem.len,
            );
        }
    }

    fn dump_tx_states(&self) {
        let free = self.tx_head_mgr.free_len() as usize;
        let mut posted =
            HeaplessVec::<(usize, u32, u64, u32, u16, TxHeadState), TX_QUEUE_SIZE>::new();
        for (idx, entry) in self.tx_head_mgr.posted_entries() {
            let (slot, gen) = match entry.state {
                TxHeadState::Published { slot, gen } | TxHeadState::InFlight { slot, gen } => {
                    (slot, gen)
                }
                _ => (0, 0),
            };
            let _ = posted.push((idx, entry.last_len, entry.last_addr, gen, slot, entry.state));
        }
        info!(
            target: "net-console",
            "[virtio-net][forensics] tx states: free={} posted={} in_flight={} tx_free_len={} tx_gen={} last_used={} used.idx={} avail.idx={}",
            free,
            posted.len(),
            self.tx_head_mgr.in_flight_count(),
            self.tx_head_mgr.free_len(),
            self.tx_head_mgr.next_gen(),
            self.tx_queue.last_used,
            self.tx_queue.indices().0,
            self.tx_queue.indices().1,
        );
        info!(
            target: "net-console",
            "[virtio-net][forensics] tx counters: dup_publish_blocked={} dup_used_ignored={} invalid_used_state={} alloc_blocked_inflight={} dropped_zero_len_tx={}",
            self.tx_dup_publish_blocked,
            self.tx_dup_used_ignored,
            self.tx_invalid_used_state,
            self.tx_alloc_blocked_inflight,
            self.dropped_zero_len_tx,
        );
        for (idx, len, addr, gen, slot, state) in posted {
            info!(
                target: "net-console",
                "[virtio-net][forensics] tx posted id={id} len={len} addr=0x{addr:016x} gen={gen} slot={slot} state={state:?}",
                id = idx,
                len = len,
                addr = addr,
                gen = gen,
                slot = slot,
                state = state,
            );
        }
    }

    fn dump_rx_window(&self) {
        let qsize = usize::from(self.rx_queue.size);
        if qsize == 0 {
            return;
        }
        let avail = self.rx_queue.avail.as_ptr();
        let avail_idx = unsafe { read_volatile(&(*avail).idx) };
        let used_idx = self.rx_queue.last_used;
        let window = core::cmp::min(qsize, 16);
        let start = avail_idx.wrapping_sub(window as u16 / 2);
        info!(
            target: "net-console",
            "[virtio-net][forensics] rx window: avail.idx={} used.idx={} size={}",
            avail_idx,
            used_idx,
            qsize,
        );
        for offset in 0..window {
            let slot_idx = start.wrapping_add(offset as u16);
            let ring_slot = (slot_idx as usize) % qsize;
            let ring_ptr = unsafe { (*avail).ring.as_ptr().add(ring_slot) as *const u16 };
            let head = unsafe { read_volatile(ring_ptr) };
            let desc = self.rx_queue.read_descriptor(head);
            info!(
                target: "net-console",
                "[virtio-net][forensics] rx avail[{ring_slot}] -> head={head} desc{head}=0x{addr:016x}/{len}/0x{flags:04x}/next={next}",
                addr = desc.addr,
                len = desc.len,
                flags = desc.flags,
                next = desc.next,
            );
        }
    }

    fn should_verify_tx_publish(&mut self) -> bool {
        let current = self.tx_publish_verify_count;
        self.tx_publish_verify_count = self.tx_publish_verify_count.wrapping_add(1);
        current < 32 || (current & 0xff) == 0
    }

    fn tx_state_violation(
        &mut self,
        reason: &'static str,
        head_id: u16,
        slot: Option<u16>,
    ) -> Result<(), ()> {
        if self.tx_state_violation_logged {
            return Err(());
        }
        self.tx_state_violation_logged = true;
        let (used_idx, avail_idx) = self.tx_queue.indices();
        let pending = avail_idx.wrapping_sub(used_idx);
        let free_entries = self.tx_queue.size.saturating_sub(pending);
        error!(
            target: "net-console",
            "[virtio-net][forensics] tx_state_violation reason={} head={} slot={:?} avail.idx={} used.idx={} last_used={} in_flight={} tx_free={} tx_gen={} pending={} free_entries={} device_faulted={} frozen={}",
            reason,
            head_id,
            slot,
            avail_idx,
            used_idx,
            self.tx_queue.last_used,
            self.tx_head_mgr.in_flight_count(),
            self.tx_head_mgr.free_len(),
            self.tx_head_mgr.next_gen(),
            pending,
            free_entries,
            self.device_faulted,
            forensics_frozen(),
        );
        self.log_tx_avail_window(avail_idx, core::cmp::min(self.tx_queue.size, 8));
        self.freeze_and_capture("tx_state_violation");
        Err(())
    }

    fn record_tx_used_entry(&mut self, id: u16, len: u32) {
        if self.tx_used_recent.len() == self.tx_used_recent.capacity() {
            let _ = self.tx_used_recent.remove(0);
        }
        let _ = self.tx_used_recent.push((id, len));
    }

    fn dump_tx_recent_entries(&self) {
        for (idx, (id, len)) in self.tx_used_recent.iter().enumerate() {
            error!(
                target: "net-console",
                "[virtio-net][tx-anomaly] recent_used[{idx}]: id={} len={}",
                id,
                len,
            );
        }
        for (id, _) in &self.tx_used_recent {
            let desc = self.tx_queue.read_descriptor(*id);
            error!(
                target: "net-console",
                "[virtio-net][tx-anomaly] desc[{id}]: addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next}",
                addr = desc.addr,
                len = desc.len,
                flags = desc.flags,
                next = desc.next,
            );
        }
    }

    fn dump_tx_descriptor_table_once(&mut self) {
        if self.tx_descriptor_dumped {
            return;
        }
        self.tx_descriptor_dumped = true;
        self.dump_descriptor_table("tx", &self.tx_queue);
    }

    fn dump_tx_used_window_once(&mut self) {
        if self.tx_used_window_dumped {
            return;
        }
        self.tx_used_window_dumped = true;
        self.dump_tx_used_window();
    }

    fn log_tx_avail_window(&self, center: u16, window: u16) {
        if window == 0 {
            return;
        }
        let avail = self.tx_queue.avail.as_ptr();
        let qsize = usize::from(self.tx_queue.size);
        if qsize == 0 {
            return;
        }
        let span = core::cmp::min(window, self.tx_queue.size);
        let start = center.wrapping_sub(span / 2);
        for offset in 0..span {
            let idx = start.wrapping_add(offset);
            let ring_slot = (idx as usize) % qsize;
            let ring_ptr = unsafe { (*avail).ring.as_ptr().add(ring_slot) as *const u16 };
            let head = unsafe { read_volatile(ring_ptr) };
            let desc = self.tx_queue.read_descriptor(head);
            error!(
                target: "net-console",
                "[virtio-net][forensics] tx avail[{ring_slot}] idx={idx} head={head} desc=0x{addr:016x}/{len} flags=0x{flags:04x} next={next}",
                addr = desc.addr,
                len = desc.len,
                flags = desc.flags,
                next = desc.next,
            );
        }
    }

    fn tx_anomaly(&mut self, reason: TxAnomalyReason, snapshot: &str) {
        if self.tx_anomaly_logged {
            self.freeze_and_capture("tx_anomaly_repeat");
            return;
        }
        self.tx_anomaly_logged = true;
        let (used_idx, avail_idx) = self.tx_queue.indices();
        error!(
            target: "net-console",
            "[virtio-net][tx-anomaly] reason={:?} snapshot={} avail.idx={} used.idx={} last_used={} tx_free={} in_flight={} tx_gen={}",
            reason,
            snapshot,
            avail_idx,
            used_idx,
            self.tx_queue.last_used,
            self.tx_head_mgr.free_len(),
            self.tx_head_mgr.in_flight_count(),
            self.tx_head_mgr.next_gen(),
        );
        self.dump_tx_descriptor_table_once();
        self.dump_tx_used_window_once();
        self.dump_tx_states();
        self.dump_tx_recent_entries();
        self.freeze_and_capture("tx_anomaly");
    }

    fn assert_tx_desc_len_nonzero(
        &mut self,
        head_id: u16,
        context: &'static str,
    ) -> Result<(), ()> {
        let desc = self.tx_queue.read_descriptor(head_id);
        if desc.len == 0 {
            self.device_faulted = true;
            self.last_error.get_or_insert(context);
            self.freeze_and_capture(context);
            return Err(());
        }
        Ok(())
    }

    fn note_zero_desc_guard(&mut self, head_id: u16, publish_slot: u16, desc: &VirtqDesc) {
        self.tx_zero_desc_guard = self.tx_zero_desc_guard.saturating_add(1);
        let now_ms = crate::hal::timebase().now_ms();
        if self.tx_zero_desc_warn_ms == 0
            || now_ms.saturating_sub(self.tx_zero_desc_warn_ms) >= 1_000
        {
            self.tx_zero_desc_warn_ms = now_ms;
            let guard_hits = self.tx_zero_desc_guard;
            let (used_idx, avail_idx) = self.tx_queue.indices_no_sync();
            let free = self.tx_free_count();
            let inflight = self.tx_inflight_count();
            warn!(
                target: "net-console",
                "[virtio-net][tx-guard] zero descriptor refused: head={head} slot={slot} addr=0x{addr:016x} len={len} used_idx={used_idx} avail_idx={avail_idx} guard_hits={guard_hits} free={free} inflight={inflight}",
                head = head_id,
                slot = publish_slot,
                addr = desc.addr,
                len = desc.len,
                used_idx = used_idx,
                avail_idx = avail_idx,
                guard_hits = guard_hits,
                free = free,
                inflight = inflight,
            );
        }
    }

    fn publish_tx_avail(
        &mut self,
        head_id: u16,
        publish_slot: u16,
    ) -> Result<(u16, u16, u16), TxPublishError> {
        if NET_VIRTIO_TX_V2 {
            match self.tx_slots.state(head_id) {
                Some(TxSlotState::Reserved { .. }) => {}
                _ => {
                    self.publish_blocked_bad_slot_state =
                        self.publish_blocked_bad_slot_state.saturating_add(1);
                    self.cancel_tx_slot(head_id, "tx_publish_slot_state");
                    return Err(TxPublishError::InvalidDescriptor);
                }
            }
        }
        match self.tx_head_mgr.state(head_id) {
            Some(TxHeadState::Prepared { .. }) => {}
            other => {
                self.publish_blocked_bad_head_state =
                    self.publish_blocked_bad_head_state.saturating_add(1);
                self.note_publish_blocked(head_id, other);
                let _ = self.tx_head_mgr.release_unused(head_id);
                self.cancel_tx_slot(head_id, "tx_publish_head_state");
                return Err(TxPublishError::InvalidDescriptor);
            }
        }
        let (used_idx, avail_idx) = self.tx_queue.indices_no_sync();
        let desc = self.tx_queue.read_descriptor(head_id);
        let entry = self.tx_head_mgr.entry(head_id).copied().unwrap_or_default();
        let total_len = entry.last_len;
        if desc.len == 0 || total_len == 0 || entry.last_addr == 0 {
            self.tx_head_mgr.record_zero_len_publish();
            self.dropped_zero_len_tx = self.dropped_zero_len_tx.saturating_add(1);
            self.publish_blocked_zero_len = self.publish_blocked_zero_len.saturating_add(1);
            self.log_zero_len_publish(head_id, publish_slot, desc.len, total_len);
            self.cancel_tx_slot(head_id, "tx_publish_zero_len");
            return Err(TxPublishError::InvalidDescriptor);
        }
        if total_len < VIRTIO_NET_HEADER_LEN_BASIC as u32 {
            self.dropped_zero_len_tx = self.dropped_zero_len_tx.saturating_add(1);
            warn!(
                target: "net-console",
                "[virtio-net][tx-guard] descriptor shorter than header: head={} slot={} len={} required={}",
                head_id,
                publish_slot,
                total_len,
                VIRTIO_NET_HEADER_LEN_BASIC
            );
            self.last_error.get_or_insert("tx_desc_len_short");
            return Err(TxPublishError::InvalidDescriptor);
        }
        if desc.addr == 0 {
            self.dropped_zero_len_tx = self.dropped_zero_len_tx.saturating_add(1);
            self.publish_blocked_zero_len = self.publish_blocked_zero_len.saturating_add(1);
            error!(
                target: "net-console",
                "[virtio-net][tx-guard] zero address publish blocked: head={} slot={} avail_idx={} used_idx={} free={} in_flight={}",
                head_id,
                publish_slot,
                avail_idx,
                used_idx,
                self.tx_head_mgr.free_len(),
                self.tx_head_mgr.in_flight_count(),
            );
            self.last_error.get_or_insert("tx_desc_addr_zero");
            return Err(TxPublishError::InvalidDescriptor);
        }
        if desc.len != total_len || desc.addr != entry.last_addr {
            self.device_faulted = true;
            let _ = self.tx_state_violation("tx_desc_len_mismatch", head_id, Some(publish_slot));
            self.freeze_tx_publishes("tx_desc_len_mismatch");
            self.last_error.get_or_insert("tx_desc_len_mismatch");
            return Err(TxPublishError::InvalidDescriptor);
        }
        if self
            .tx_head_mgr
            .mark_published(head_id, publish_slot, desc.len, desc.addr)
            .is_err()
        {
            self.tx_dup_publish_blocked = self.tx_dup_publish_blocked.saturating_add(1);
            self.cancel_tx_slot(head_id, "tx_publish_mark_failed");
            return Err(TxPublishError::InvalidDescriptor);
        }
        debug_assert!(
            desc.len != 0 && desc.addr != 0,
            "tx publish descriptor must be non-zero: head={} len={} addr=0x{:016x}",
            head_id,
            desc.len,
            desc.addr
        );
        debug_assert!(total_len != 0, "tx publish total_len must be non-zero");
        if validate_tx_publish_descriptor(
            head_id,
            publish_slot,
            &desc,
            total_len,
            avail_idx,
            used_idx,
            self.tx_head_mgr.in_flight_count(),
            self.tx_head_mgr.free_len(),
            &mut self.tx_invalid_publish_logged,
        )
        .is_err()
        {
            self.tx_invalid_publish = self.tx_invalid_publish.saturating_add(1);
            self.last_error.get_or_insert("tx_invalid_descriptor");
            return Err(TxPublishError::InvalidDescriptor);
        }
        debug_assert!(
            desc.len != 0,
            "tx publish descriptor len zero before avail push"
        );
        self.tx_queue
            .push_avail(head_id)
            .map_err(TxPublishError::Queue)
            .and_then(|(slot, new_idx, old_idx)| {
                if self
                    .tx_head_mgr
                    .note_avail_publish(head_id, slot, new_idx)
                    .is_err()
                {
                    self.tx_dup_publish_blocked = self.tx_dup_publish_blocked.saturating_add(1);
                    let _ = self.tx_head_mgr.cancel_publish(head_id);
                    self.cancel_tx_slot(head_id, "tx_publish_avail_record_failed");
                    return Err(TxPublishError::InvalidDescriptor);
                }
                if self.tx_head_mgr.mark_in_flight(head_id).is_err() {
                    self.tx_dup_publish_blocked = self.tx_dup_publish_blocked.saturating_add(1);
                    let _ = self.tx_head_mgr.cancel_publish(head_id);
                    self.cancel_tx_slot(head_id, "tx_publish_inflight_failed");
                    return Err(TxPublishError::InvalidDescriptor);
                }
                if NET_VIRTIO_TX_V2 {
                    if self.tx_slots.mark_in_flight(head_id).is_err() {
                        self.publish_blocked_bad_slot_state =
                            self.publish_blocked_bad_slot_state.saturating_add(1);
                        let _ = self.tx_head_mgr.cancel_publish(head_id);
                        self.cancel_tx_slot(head_id, "tx_publish_slot_inflight_failed");
                        return Err(TxPublishError::InvalidDescriptor);
                    }
                }
                Ok((slot, new_idx, old_idx))
            })
    }

    fn clear_tx_desc_chain(&mut self, head: u16) {
        let state = self.tx_head_mgr.state(head);
        debug_assert!(
            !matches!(
                state,
                Some(TxHeadState::Published { .. }) | Some(TxHeadState::InFlight { .. })
            ),
            "tx descriptor clear attempted while active: head={} state={state:?}",
            head
        );
        match self.tx_head_mgr.state(head) {
            Some(TxHeadState::Published { .. }) | Some(TxHeadState::InFlight { .. }) => {
                let _ = self.tx_state_violation("tx_clear_active", head, None);
                return;
            }
            Some(TxHeadState::Prepared { .. }) | Some(TxHeadState::Completed { .. }) => {
                let _ = self.tx_state_violation("tx_clear_nonfree", head, None);
                return;
            }
            _ => {}
        }
        let qsize = usize::from(self.tx_queue.size);
        if qsize == 0 {
            return;
        }
        if !VIRTIO_TX_CLEAR_DESC_ON_FREE {
            return;
        }
        let mut idx = head;
        for _depth in 0..qsize {
            let desc_ptr = unsafe { self.tx_queue.desc.as_ptr().add(idx as usize) };
            let desc = unsafe { read_volatile(desc_ptr) };
            unsafe {
                write_volatile(
                    desc_ptr,
                    VirtqDesc {
                        addr: 0,
                        len: 0,
                        flags: 0,
                        next: 0,
                    },
                );
            }
            if desc.flags & VIRTQ_DESC_F_NEXT == 0 {
                break;
            }
            if desc.next >= self.tx_queue.size || desc.next == idx {
                break;
            }
            idx = desc.next;
        }
    }

    /// Prevent TX head reuse while the device may still follow the descriptor chain.
    ///
    /// Root cause: the previous implementation keyed reuse solely off the free
    /// vector and len/addr fields. A head could be re-published while still
    /// device-owned (duplicate avail entries observed in forensics), letting the
    /// device chase into later generations where the descriptor table had been
    /// zeroed (desc entries showed {addr=0,len=0}). That duplicated publish
    /// eventually surfaced as QEMU aborting with "virtio: zero sized buffers are
    /// not allowed" while used.id repeated for the same head. The state machine
    /// below makes the generation+slot transition explicit so a Published head
    /// cannot be allocated or cleared until reclaim confirms device ownership
    /// has returned.
    fn guard_tx_post_state(&mut self, head_id: u16, slot: u16, desc: &DescSpec) -> Result<(), ()> {
        if desc.len == 0 || desc.addr == 0 {
            self.tx_state_violation("tx_post_zero", head_id, Some(slot))?;
            return Err(());
        }
        if self.tx_head_mgr.published_for_slot(slot).is_some() {
            self.tx_dup_publish_blocked = self.tx_dup_publish_blocked.saturating_add(1);
            #[cfg(debug_assertions)]
            self.freeze_tx_publishes("tx_slot_occupied");
            self.tx_state_violation("tx_slot_occupied", head_id, Some(slot))?;
            return Err(());
        }
        match self
            .tx_head_mgr
            .prepare_publish(head_id, slot, desc.len, desc.addr)
        {
            Ok(_) => Ok(()),
            Err(TxHeadError::SlotBusy) => {
                self.tx_dup_publish_blocked = self.tx_dup_publish_blocked.saturating_add(1);
                #[cfg(debug_assertions)]
                self.freeze_tx_publishes("tx_slot_busy");
                self.tx_state_violation("tx_slot_busy", head_id, Some(slot))?;
                Err(())
            }
            Err(_) => {
                self.tx_dup_publish_blocked = self.tx_dup_publish_blocked.saturating_add(1);
                #[cfg(debug_assertions)]
                self.freeze_tx_publishes("tx_duplicate_publish");
                self.tx_state_violation("tx_double_post", head_id, Some(slot))?;
                Err(())
            }
        }
    }

    fn tx_pre_publish_tripwire(
        &mut self,
        head_id: u16,
        slot: u16,
        expected: &DescSpec,
        header_len: usize,
        payload_len: usize,
    ) -> Result<(), ()> {
        let desc = self.tx_queue.read_descriptor(head_id);
        let next_expected = expected.next.unwrap_or(0);
        let next_flag_expected = expected.next.is_some();
        let next_flag_observed = (desc.flags & VIRTQ_DESC_F_NEXT) != 0;
        let len_ok =
            desc.len == expected.len && desc.len >= header_len.saturating_add(payload_len) as u32;
        let flags_ok = (desc.flags & !VIRTQ_DESC_F_NEXT) == (expected.flags & !VIRTQ_DESC_F_NEXT);
        let next_ok = if next_flag_expected {
            next_flag_observed && desc.next == next_expected
        } else {
            !next_flag_observed && desc.next == next_expected
        };
        if desc.addr == 0
            || desc.len == 0
            || !len_ok
            || desc.addr != expected.addr
            || !next_ok
            || !flags_ok
        {
            error!(
                target: "net-console",
                "[virtio-net][tx-tripwire] publish aborted head={head} slot={slot} addr=0x{addr:016x} len={len} expected_addr=0x{expected_addr:016x} expected_len={expected_len} header_len={header_len} payload_len={payload_len} next_ok={next_ok} flags=0x{flags:04x} next={next} expected_next={expected_next}",
                head = head_id,
                slot = slot,
                addr = desc.addr,
                len = desc.len,
                expected_addr = expected.addr,
                expected_len = expected.len,
                header_len = header_len,
                payload_len = payload_len,
                next_ok = next_ok,
                flags = desc.flags,
                next = desc.next,
                expected_next = next_expected,
            );
            self.tx_state_violation("tx_tripwire", head_id, Some(slot))?;
            self.device_faulted = true;
            self.last_error.get_or_insert("tx_tripwire");
            return Err(());
        }
        Ok(())
    }

    fn tx_wrap_tripwire(&mut self, old_idx: u16, avail_idx: u16, slot: u16, head_id: u16) {
        let qsize = usize::from(self.tx_queue.size);
        if qsize == 0 {
            return;
        }
        let wrap_boundary = (old_idx as usize % qsize) > (avail_idx as usize % qsize);
        let at_slot_zero = slot == 0;
        if !wrap_boundary && !at_slot_zero {
            return;
        }
        let count = TX_WRAP_TRIPWIRE.fetch_add(1, AtomicOrdering::AcqRel);
        if count >= TX_WRAP_TRIPWIRE_LIMIT {
            return;
        }
        let avail_slot_val = self.tx_queue.read_avail_slot(slot as usize);
        let desc = self.tx_queue.read_descriptor(head_id);
        let next_candidate = if (desc.flags & VIRTQ_DESC_F_NEXT) != 0 {
            desc.next
        } else {
            head_id.wrapping_add(1)
        };
        let bounded_next = next_candidate % self.tx_queue.size;
        let next_desc = self.tx_queue.read_descriptor(bounded_next);
        let avail_idx_readback = self.tx_queue.read_avail_idx();
        info!(
            target: "virtio-net",
            "[virtio-net][tx-wrap-tripwire] old_idx={} avail_idx={} slot={} head={} wrap={} ring_slot_head={} desc_len={desc_len} desc_addr=0x{desc_addr:016x} desc_flags=0x{desc_flags:04x} desc_next={desc_next} next_desc_len={next_desc_len} next_desc_addr=0x{next_desc_addr:016x} avail_idx_readback={avail_idx_readback}",
            old_idx,
            avail_idx,
            slot,
            head_id,
            wrap_boundary,
            avail_slot_val,
            desc_len = desc.len,
            desc_addr = desc.addr,
            desc_flags = desc.flags,
            desc_next = desc.next,
            next_desc_len = next_desc.len,
            next_desc_addr = next_desc.addr,
            avail_idx_readback = avail_idx_readback,
        );
        #[cfg(debug_assertions)]
        self.tx_assert_invariants("wrap_tripwire");
    }

    fn validate_tx_publish_guard(
        &mut self,
        head_id: u16,
        descs: &[DescSpec],
        header_len: usize,
        avail_idx: u16,
    ) -> Result<(), ()> {
        let required_header = core::cmp::max(header_len as u32, VIRTIO_NET_HEADER_LEN_BASIC as u32);
        let mut total_len: u32 = 0;
        let mut zero_len = false;
        for desc in descs {
            if desc.len == 0 {
                zero_len = true;
            }
            total_len = total_len.saturating_add(desc.len);
        }
        let qsize = usize::from(self.tx_queue.size);
        let slot = if qsize == 0 {
            0
        } else {
            (avail_idx as usize % qsize) as u16
        };
        let (used_idx, _) = self.tx_queue.indices_no_sync();
        let head_state = self.tx_head_mgr.state(head_id);
        let in_flight = self.tx_head_mgr.is_in_flight(head_id);
        let first_len = descs.first().map(|d| d.len).unwrap_or(0);
        let first_addr = descs.first().map(|d| d.addr).unwrap_or(0);
        let payload_len = first_len.saturating_sub(required_header);
        let chain_len = descs.len();
        let sum_mismatch = first_len != total_len;
        let guard_failed = zero_len || total_len < required_header || in_flight || sum_mismatch;
        if guard_failed {
            if !self.tx_publish_guard_logged {
                self.tx_publish_guard_logged = true;
                let next_idx = avail_idx.wrapping_add(1);
                error!(
                    target: "net-console",
                    "[virtio-net][tx-guard] blocked publish: head={head_id} state={head_state:?} slot={slot} avail_idx={avail_idx}->{next_idx} used_idx={used_idx} chain_len={chain_len} first_addr=0x{first_addr:016x} first_len={first_len} payload_len={payload_len} required_header={required_header} total_len={total_len} in_flight={in_flight} zero_len_desc={zero_len} sum_mismatch={sum_mismatch}",
                    head_id = head_id,
                    head_state = head_state,
                    slot = slot,
                    avail_idx = avail_idx,
                    next_idx = next_idx,
                    used_idx = used_idx,
                    chain_len = chain_len,
                    first_addr = first_addr,
                    first_len = first_len,
                    payload_len = payload_len,
                    required_header = required_header,
                    total_len = total_len,
                    in_flight = in_flight,
                    zero_len = zero_len,
                    sum_mismatch = sum_mismatch,
                );
            }
            self.tx_drops = self.tx_drops.saturating_add(1);
            return Err(());
        }
        Ok(())
    }

    fn tx_publish_preflight(
        &mut self,
        head_id: u16,
        slot: u16,
        desc: &DescSpec,
        payload_len: usize,
    ) -> Result<(), ()> {
        if desc.addr == 0 || desc.len == 0 || payload_len == 0 {
            error!(
                target: "net-console",
                "[virtio-net][tx-preflight] invalid descriptor before publish: head={} slot={} addr=0x{addr:016x} len={len} payload_len={payload_len}",
                head_id,
                slot,
                addr = desc.addr,
                len = desc.len,
                payload_len = payload_len,
            );
            let _ = self.tx_state_violation("tx_preflight", head_id, Some(slot));
            return Err(());
        }
        Ok(())
    }

    fn reclaim_posted_head(
        &mut self,
        id: u16,
        ring_slot: u16,
        used_len: u32,
        used_idx: u16,
    ) -> Result<TxReclaimResult, ()> {
        let head_state = self.tx_head_mgr.state(id);
        let slot_state = if NET_VIRTIO_TX_V2 {
            self.tx_slots.state(id)
        } else {
            None
        };
        if used_len == 0 {
            record_zero_len_used(
                &mut self.tx_used_zero_len_seen,
                &mut self.tx_used_zero_len_log_ms,
                id,
                ring_slot,
                used_idx,
                self.tx_queue.indices_no_sync().1,
                self.tx_queue.last_used,
                head_state,
                slot_state,
            );
        }
        let slot_tracker = if NET_VIRTIO_TX_V2 {
            Some(&mut self.tx_slots)
        } else {
            None
        };
        match reclaim_used_entry_common(&mut self.tx_head_mgr, slot_tracker, id, ring_slot) {
            TxReclaimResult::Reclaimed => Ok(TxReclaimResult::Reclaimed),
            TxReclaimResult::InvalidId => {
                self.last_error.get_or_insert("tx_used_id_oob");
                self.device_faulted = true;
                self.tx_invalid_used_state = self.tx_invalid_used_state.saturating_add(1);
                self.tx_head_mgr.record_invalid_used_id();
                self.log_invalid_used_state(id, None, ring_slot, used_idx);
                Ok(TxReclaimResult::InvalidId)
            }
            TxReclaimResult::HeadNotInFlight(state) => {
                self.tx_dup_used_ignored = self.tx_dup_used_ignored.saturating_add(1);
                self.tx_invalid_used_state = self.tx_invalid_used_state.saturating_add(1);
                self.tx_head_mgr.record_invalid_used_state();
                self.log_invalid_used_state(id, state, ring_slot, used_idx);
                Ok(TxReclaimResult::HeadNotInFlight(state))
            }
            TxReclaimResult::SlotStateInvalid(slot_state) => {
                self.tx_invalid_used_state = self.tx_invalid_used_state.saturating_add(1);
                self.tx_head_mgr.record_invalid_used_state();
                warn!(
                    target: "net-console",
                    "[virtio-net][tx] used entry saw invalid slot state: head={} slot_state={:?} ring_slot={} used_idx={}",
                    id,
                    slot_state,
                    ring_slot,
                    used_idx,
                );
                Ok(TxReclaimResult::SlotStateInvalid(slot_state))
            }
            TxReclaimResult::SlotMismatch => {
                self.tx_invalid_used_state = self.tx_invalid_used_state.saturating_add(1);
                self.tx_head_mgr.record_invalid_used_state();
                self.log_invalid_used_state(id, head_state, ring_slot, used_idx);
                Ok(TxReclaimResult::SlotMismatch)
            }
            TxReclaimResult::PublishRecordMismatch => {
                self.tx_invalid_used_state = self.tx_invalid_used_state.saturating_add(1);
                self.tx_head_mgr.record_invalid_used_state();
                self.log_invalid_used_state(id, head_state, ring_slot, used_idx);
                Ok(TxReclaimResult::PublishRecordMismatch)
            }
            TxReclaimResult::HeadTransitionFailed => {
                if !self.tx_reclaim_state_violation_logged {
                    self.tx_reclaim_state_violation_logged = true;
                    warn!(
                        target: "net-console",
                        "[virtio-net][tx] reclaim invariant violated: head={} state={:?}",
                        id,
                        self.tx_head_mgr.state(id),
                    );
                }
                self.tx_invalid_used_state = self.tx_invalid_used_state.saturating_add(1);
                self.log_invalid_used_state(id, self.tx_head_mgr.state(id), ring_slot, used_idx);
                Ok(TxReclaimResult::HeadTransitionFailed)
            }
            TxReclaimResult::SlotCompletionFailed(err) => {
                self.tx_invalid_used_state = self.tx_invalid_used_state.saturating_add(1);
                warn!(
                    target: "net-console",
                    "[virtio-net][tx-slot] completion failed: id={} err={:?} ring_slot={} used_idx={}",
                    id,
                    err,
                    ring_slot,
                    used_idx,
                );
                self.log_invalid_used_state(id, self.tx_head_mgr.state(id), ring_slot, used_idx);
                Ok(TxReclaimResult::SlotCompletionFailed(err))
            }
        }
    }

    fn guard_tx_publish_readback(
        &mut self,
        slot: u16,
        head_id: u16,
        expected: &DescSpec,
    ) -> Result<(), ()> {
        let observed_head = self.tx_queue.read_avail_slot(slot as usize);
        let desc = self.tx_queue.read_descriptor(head_id);
        if observed_head != head_id
            || desc.addr != expected.addr
            || desc.len != expected.len
            || desc.len == 0
        {
            if !self.tx_publish_readback_logged {
                self.tx_publish_readback_logged = true;
                warn!(
                    target: "net-console",
                    "[virtio-net][tx-readback] publish mismatch: slot={} expected_head={} observed_head={} desc_addr=0x{desc_addr:016x} expected_addr=0x{expected_addr:016x} desc_len={desc_len} expected_len={expected_len}",
                    slot,
                    head_id,
                    observed_head,
                    desc_addr = desc.addr,
                    expected_addr = expected.addr,
                    desc_len = desc.len,
                    expected_len = expected.len,
                );
            }
            self.freeze_tx_publishes("tx_publish_readback_mismatch");
            self.last_error
                .get_or_insert("tx_publish_readback_mismatch");
            self.device_faulted = true;
            return Err(());
        }
        Ok(())
    }

    fn verify_tx_publish(
        &mut self,
        slot: u16,
        head_id: u16,
        expected: &DescSpec,
    ) -> Result<(), ()> {
        if !self.should_verify_tx_publish() {
            return Ok(());
        }
        let qsize = usize::from(self.tx_queue.size);
        if qsize == 0 {
            return Ok(());
        }
        let avail = self.tx_queue.avail.as_ptr();
        let ring_slot = (slot as usize) % qsize;
        let ring_ptr = unsafe { (*avail).ring.as_ptr().add(ring_slot) as *const u16 };
        let observed_head = unsafe { read_volatile(ring_ptr) };
        let desc = self.tx_queue.read_descriptor(head_id);
        if observed_head != head_id
            || desc.addr == 0
            || desc.len == 0
            || desc.addr != expected.addr
            || desc.len != expected.len
        {
            error!(
                target: "net-console",
                "[virtio-net][forensics] tx publish verify failed: slot={} expected_head={} observed_head={} desc_len={} expected_len={} desc_addr=0x{addr:016x} expected_addr=0x{expected_addr:016x} last_used={} used.idx={} avail.idx={}",
                ring_slot,
                head_id,
                observed_head,
                desc.len,
                expected.len,
                self.tx_queue.last_used,
                self.tx_queue.indices().0,
                self.tx_queue.indices().1,
                addr = desc.addr,
                expected_addr = expected.addr,
            );
            return self.tx_state_violation("tx_publish_mismatch", head_id, Some(slot));
        }
        Ok(())
    }

    fn log_publish_transaction(
        &mut self,
        queue_label: &'static str,
        queue_kind: QueueKind,
        old_idx: u16,
        new_idx: u16,
        slot: u16,
        head: u16,
        force: bool,
    ) {
        if forensics_frozen() {
            return;
        }
        let counter = match queue_label {
            "RX" => &mut self.rx_publish_log_count,
            _ => &mut self.tx_publish_log_count,
        };
        let should_log = force || (*counter as u32) < FORENSICS_PUBLISH_LOG_LIMIT;
        if should_log {
            *counter = counter.saturating_add(1);
            let head_desc = match queue_kind {
                QueueKind::Rx => self.rx_queue.read_descriptor(head),
                QueueKind::Tx => self.tx_queue.read_descriptor(head),
            };
            info!(
                target: "net-console",
                "[virtio-net][forensics] publish {queue_label} avail={old_idx}->{new_idx} slot={slot} head={head} desc{{addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next}}}",
                addr = head_desc.addr,
                len = head_desc.len,
                flags = head_desc.flags,
                next = head_desc.next,
            );
        }
    }

    fn log_pre_publish_if_suspicious(
        &mut self,
        queue_label: &'static str,
        queue_kind: QueueKind,
        head_id: u16,
        descs: &[DescSpec],
    ) -> Result<(), ()> {
        let queue = match queue_kind {
            QueueKind::Rx => &self.rx_queue,
            QueueKind::Tx => &self.tx_queue,
        };

        let mut reasons = HeaplessString::<128>::new();
        let mut suspicious = false;
        for desc in descs {
            if desc.len == 0 {
                suspicious = true;
                let _ = write!(reasons, "len0;");
            }
            if desc.addr == 0 {
                suspicious = true;
                let _ = write!(reasons, "addr0;");
            }
            if let Some(next) = desc.next {
                if next >= queue.size {
                    suspicious = true;
                    let _ = write!(reasons, "next_oob({next});");
                }
                if desc.flags & VIRTQ_DESC_F_NEXT == 0 {
                    suspicious = true;
                    let _ = write!(reasons, "flags_missing_next;");
                }
            } else if desc.flags & VIRTQ_DESC_F_NEXT != 0 {
                suspicious = true;
                let _ = write!(reasons, "flags_unexpected_next;");
            }
        }

        if suspicious {
            error!(
                target: "net-console",
                "[virtio-net] suspicious descriptor publish blocked: queue={} head_id={} queue_size={} reasons={}",
                queue_label,
                head_id,
                queue.size,
                reasons.as_str(),
            );
            for (idx, desc) in descs.iter().enumerate() {
                error!(
                    target: "net-console",
                    "[virtio-net] pre-publish desc[{idx}]: addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next:?}",
                    addr = desc.addr,
                    len = desc.len,
                    flags = desc.flags,
                    next = desc.next,
                );
            }
            self.device_faulted = true;
            self.last_error.get_or_insert("suspicious_desc");
            return Err(());
        }

        Ok(())
    }

    fn verify_descriptor_write(
        &mut self,
        queue_label: &'static str,
        queue_kind: QueueKind,
        head_id: u16,
        expected_chain: &[DescSpec],
    ) -> Result<(), ()> {
        let mut mismatch = false;
        let mut actual_chain: HeaplessVec<VirtqDesc, MAX_QUEUE_SIZE> = HeaplessVec::new();

        let queue = match queue_kind {
            QueueKind::Rx => &self.rx_queue,
            QueueKind::Tx => &self.tx_queue,
        };

        for (idx, expected) in expected_chain.iter().enumerate() {
            let desc_index = head_id.wrapping_add(idx as u16);
            let actual = queue.read_descriptor(desc_index);
            let _ = actual_chain.push(actual);

            let expected_next_flag = expected.next.is_some();
            let expected_next_value = expected.next.unwrap_or(0);
            let actual_next_flag = (actual.flags & VIRTQ_DESC_F_NEXT) != 0;

            let next_mismatch = if expected_next_flag {
                !actual_next_flag || actual.next != expected_next_value
            } else {
                actual_next_flag || actual.next != expected_next_value
            };

            if actual.addr != expected.addr
                || actual.len != expected.len
                || actual.len == 0
                || actual.flags != expected.flags
                || next_mismatch
            {
                mismatch = true;
            }
        }

        if mismatch {
            if !self.descriptor_corrupt_logged {
                self.descriptor_corrupt_logged = true;

                let (used_idx, avail_idx) = queue.indices();
                let pending = avail_idx.wrapping_sub(used_idx);
                let free_entries = queue.size.saturating_sub(pending);

                error!(
                    target: "net-console",
                    "[virtio-net] descriptor write mismatch: queue={} head_id={} avail.idx={} used.idx={} last_used={} pending={}
                free_entries={}",
                    queue_label,
                    head_id,
                    avail_idx,
                    used_idx,
                    queue.last_used,
                    pending,
                    free_entries,
                );

                for (idx, expected) in expected_chain.iter().enumerate() {
                    error!(
                        target: "net-console",
                        "[virtio-net] descriptor expected[{idx}]: addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next:?}",
                        addr = expected.addr,
                        len = expected.len,
                        flags = expected.flags,
                        next = expected.next,
                    );
                }

                for (idx, actual) in actual_chain.iter().enumerate() {
                    error!(
                        target: "net-console",
                        "[virtio-net] descriptor actual[{idx}]: addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next}",
                        addr = actual.addr,
                        len = actual.len,
                        flags = actual.flags,
                        next = actual.next,
                    );
                }
            }

            self.device_faulted = true;
            self.last_error = Some("descriptor_write_corrupt");
            self.freeze_and_capture("descriptor_visibility_mismatch");
            return Err(());
        }

        Ok(())
    }

    fn enqueue_rx_chain_checked(
        &mut self,
        head_id: u16,
        descs: &[DescSpec],
        header_len: usize,
        payload_len: usize,
        frame_capacity: usize,
        used_len: Option<usize>,
        notify: bool,
    ) -> Result<(), ()> {
        self.check_device_health();
        if self.device_faulted || forensics_frozen() {
            return Err(());
        }
        assert!(
            descs.len() <= usize::from(self.rx_queue.size),
            "virtqueue rx chain too long: len={} qsize={} head_id={} base_vaddr=0x{base:016x}",
            descs.len(),
            self.rx_queue.size,
            head_id,
            base = self.rx_queue.base_vaddr,
        );

        self.validate_chain_nonzero(
            "RX",
            head_id,
            descs,
            Some(header_len),
            Some(payload_len),
            Some(frame_capacity),
            used_len,
        )?;
        self.rx_publish_calls = self.rx_publish_calls.wrapping_add(1);

        let mut resolved_descs: HeaplessVec<DescSpec, RX_QUEUE_SIZE> = HeaplessVec::new();
        for (idx, spec) in descs.iter().enumerate() {
            let desc_index = head_id.wrapping_add(idx as u16);
            let next = if idx + 1 < descs.len() {
                Some(head_id.wrapping_add((idx + 1) as u16))
            } else {
                spec.next
            };
            if self
                .rx_queue
                .setup_descriptor(desc_index, spec.addr, spec.len, spec.flags, next)
                .is_err()
            {
                self.device_faulted = true;
                self.last_error.get_or_insert("rx_desc_cache_clean_failed");
                self.freeze_and_capture("rx_desc_cache_clean_failed");
                return Err(());
            }
            let resolved_flags = match next {
                Some(_) => spec.flags | VIRTQ_DESC_F_NEXT,
                None => spec.flags,
            };
            let _ = resolved_descs.push(DescSpec {
                addr: spec.addr,
                len: spec.len,
                flags: resolved_flags,
                next,
            });
        }

        let header_len = self.rx_header_len;
        let total_len = resolved_descs
            .get(0)
            .map(|desc| desc.len as usize)
            .unwrap_or(0);
        let payload_len = total_len.saturating_sub(header_len);
        if total_len < header_len {
            self.tx_anomaly(TxAnomalyReason::DescLenZero, "tx_total_lt_header");
            return Err(());
        }
        if header_len == 0 {
            self.tx_anomaly(TxAnomalyReason::DescLenZero, "tx_header_len_zero");
            return Err(());
        }
        if payload_len == 0 {
            self.tx_anomaly(TxAnomalyReason::DescLenZero, "tx_payload_len_zero");
        }
        let _header_fields = self.inspect_tx_header(head_id, header_len);
        let _payload_overlaps = resolved_descs.get(0).map_or(false, |desc| {
            let header_end = desc.addr.saturating_add(header_len as u64);
            let payload_addr = desc.addr.saturating_add(header_len as u64);
            payload_len > 0 && payload_addr < header_end
        });

        virtq_publish_barrier();
        self.verify_descriptor_write("RX", QueueKind::Rx, head_id, &resolved_descs)?;
        if self.rx_queue.sync_descriptor_table_for_device().is_err() {
            self.freeze_and_capture("rx_desc_sync_failed");
            return Err(());
        }
        self.log_pre_publish_if_suspicious("RX", QueueKind::Rx, head_id, &resolved_descs)?;
        virtq_publish_barrier();
        if let Err(fault) = validate_chain_pre_publish(
            "RX",
            self.rx_queue.size,
            self.rx_queue.desc.as_ptr(),
            head_id,
        ) {
            return self.handle_forensic_fault(fault);
        }

        let (slot, avail_idx, old_idx) = match self.rx_queue.push_avail(head_id) {
            Ok(result) => result,
            Err(_) => {
                self.device_faulted = true;
                self.last_error.get_or_insert("rx_avail_write_failed");
                return Err(());
            }
        };
        if self.rx_queue.sync_avail_ring_for_device().is_err() {
            self.freeze_and_capture("rx_avail_sync_failed");
            return Err(());
        }
        self.log_publish_transaction(
            "RX",
            QueueKind::Rx,
            old_idx,
            avail_idx,
            slot,
            head_id,
            false,
        );
        NET_DIAG.record_rx_desc_posted();
        if !RX_PUBLISH_FENCE_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            debug!(
                target: "virtio-net",
                "[virtio-net] enqueue publish: fences applied (rx) head={} slot={} avail_idx={}",
                head_id,
                slot,
                avail_idx,
            );
        }
        if notify && !self.device_faulted {
            if self
                .rx_queue
                .notify(&mut self.regs, RX_QUEUE_INDEX)
                .is_err()
            {
                self.freeze_and_capture("rx_notify_failed");
                return Err(());
            }
        }
        Ok(())
    }

    fn enqueue_tx_chain_checked(
        &mut self,
        head_id: u16,
        descs: &[DescSpec],
        used_len: Option<usize>,
        notify: bool,
    ) -> Result<(), ()> {
        let log_breadcrumb = self.tx_publish_log_count < 4 || self.tx_anomaly_logged;
        if log_breadcrumb {
            debug!(
                target: "virtio-net",
                "[virtio-net][tx-bc] begin head={} descs={} used_len={:?}",
                head_id,
                descs.len(),
                used_len,
            );
        }
        if self.tx_publish_blocked() {
            return Err(());
        }
        self.check_device_health();
        if self.device_faulted || forensics_frozen() {
            return Err(());
        }
        let (used_idx_before, avail_idx_before) = self.tx_queue.indices_no_sync();
        let qsize = usize::from(self.tx_queue.size);
        assert!(
            descs.len() <= usize::from(self.tx_queue.size),
            "virtqueue tx chain too long: len={} qsize={} head_id={} base_vaddr=0x{base:016x}",
            descs.len(),
            self.tx_queue.size,
            head_id,
            base = self.tx_queue.base_vaddr,
        );

        if descs.iter().any(|desc| desc.len == 0) {
            self.tx_anomaly(TxAnomalyReason::DescLenZero, "enqueue_tx_chain_zero_len");
            self.device_faulted = true;
            return Err(());
        }
        if descs.iter().any(|desc| desc.addr == 0) {
            self.tx_anomaly(TxAnomalyReason::DescAddrZero, "enqueue_tx_chain_zero_addr");
            self.device_faulted = true;
            return Err(());
        }
        if descs.len() != 1 {
            let slot = if qsize == 0 {
                0
            } else {
                (avail_idx_before as usize % qsize) as u16
            };
            error!(
                target: "net-console",
                "[virtio-net][tx-guard] multi-descriptor tx chain blocked head={head_id} chain_len={chain_len} slot={slot} avail_idx_before={avail_idx_before} used_idx_before={used_idx_before} first_addr=0x{addr:016x} first_len={len} header_len={header_len} payload_len={payload_len}",
                head_id = head_id,
                chain_len = descs.len(),
                slot = slot,
                avail_idx_before = avail_idx_before,
                used_idx_before = used_idx_before,
                addr = descs.get(0).map(|d| d.addr).unwrap_or(0),
                len = descs.get(0).map(|d| d.len).unwrap_or(0),
                header_len = self.tx_header_len,
                payload_len = descs
                    .get(0)
                    .map(|d| d.len.saturating_sub(self.tx_header_len as u32))
                    .unwrap_or(0),
            );
            self.tx_anomaly(TxAnomalyReason::MultiDescriptor, "tx_multi_desc_blocked");
            self.tx_drops = self.tx_drops.saturating_add(1);
            return Err(());
        }
        self.validate_chain_nonzero("TX", head_id, descs, None, None, None, used_len)?;
        if self.ensure_tx_head_prepared(head_id).is_err() {
            self.tx_drops = self.tx_drops.saturating_add(1);
            self.tx_double_submit = self.tx_double_submit.wrapping_add(1);
            return Err(());
        }
        self.tx_publish_calls = self.tx_publish_calls.wrapping_add(1);

        let mut resolved_descs: HeaplessVec<DescSpec, TX_QUEUE_SIZE> = HeaplessVec::new();
        for (idx, spec) in descs.iter().enumerate() {
            let desc_index = head_id.wrapping_add(idx as u16);
            let next = if idx + 1 < descs.len() {
                Some(head_id.wrapping_add((idx + 1) as u16))
            } else {
                spec.next
            };
            if self
                .tx_queue
                .setup_descriptor(desc_index, spec.addr, spec.len, spec.flags, next)
                .is_err()
            {
                self.device_faulted = true;
                self.last_error.get_or_insert("tx_desc_cache_clean_failed");
                self.freeze_and_capture("tx_desc_cache_clean_failed");
                return Err(());
            }
            let resolved_flags = match next {
                Some(_) => spec.flags | VIRTQ_DESC_F_NEXT,
                None => spec.flags,
            };
            let _ = resolved_descs.push(DescSpec {
                addr: spec.addr,
                len: spec.len,
                flags: resolved_flags,
                next,
            });
        }

        let header_len = self.tx_header_len;
        let total_len = resolved_descs
            .get(0)
            .map(|desc| desc.len as usize)
            .unwrap_or(0);
        let payload_len = total_len.saturating_sub(header_len);
        if total_len < header_len {
            self.tx_anomaly(TxAnomalyReason::DescLenZero, "tx_total_lt_header");
            return Err(());
        }
        if header_len == 0 {
            self.tx_anomaly(TxAnomalyReason::DescLenZero, "tx_header_len_zero");
            return Err(());
        }
        if payload_len == 0 {
            self.tx_anomaly(TxAnomalyReason::DescLenZero, "tx_payload_len_zero");
        }
        let header_fields = self.inspect_tx_header(head_id, header_len);
        let payload_overlaps = resolved_descs.get(0).map_or(false, |desc| {
            let header_end = desc.addr.saturating_add(header_len as u64);
            let payload_addr = desc.addr.saturating_add(header_len as u64);
            payload_len > 0 && payload_addr < header_end
        });

        virtq_publish_barrier();
        self.verify_descriptor_write("TX", QueueKind::Tx, head_id, &resolved_descs)?;
        for (offset, _) in resolved_descs.iter().enumerate() {
            let desc_index = head_id.wrapping_add(offset as u16);
            if self
                .tx_queue
                .clean_desc_entry_for_device(desc_index)
                .is_err()
            {
                self.freeze_and_capture("tx_desc_sync_failed");
                return Err(());
            }
        }
        self.log_tx_descriptor_readback(head_id, &resolved_descs);
        self.log_pre_publish_if_suspicious("TX", QueueKind::Tx, head_id, &resolved_descs)?;
        virtq_publish_barrier();
        if let Err(fault) = validate_chain_pre_publish(
            "TX",
            self.tx_queue.size,
            self.tx_queue.desc.as_ptr(),
            head_id,
        ) {
            return self.handle_forensic_fault(fault);
        }

        let buffer_range = self
            .clean_tx_buffer_for_device(
                head_id,
                resolved_descs[0].len as usize,
                self.tx_anomaly_logged,
            )
            .ok_or(())?;
        // Ensure cache maintenance and descriptor writes are visible before handing ownership to the device.
        dma_barrier();

        let (_, avail_idx_before) = self.tx_queue.indices_no_sync();
        if self
            .validate_tx_publish_guard(head_id, &resolved_descs, header_len, avail_idx_before)
            .is_err()
        {
            if matches!(
                self.tx_head_mgr.state(head_id),
                Some(TxHeadState::Prepared { .. })
            ) {
                self.release_tx_head(head_id, "tx_guard_reject");
            }
            return Err(());
        }
        if !matches!(
            self.tx_head_mgr.state(head_id),
            Some(TxHeadState::Prepared { .. })
        ) {
            self.tx_dup_publish_blocked = self.tx_dup_publish_blocked.saturating_add(1);
            #[cfg(debug_assertions)]
            self.freeze_tx_publishes("tx_publish_state_violation");
            return Err(());
        }
        let qsize = usize::from(self.tx_queue.size);
        if qsize == 0 {
            self.tx_state_violation("tx_qsize_zero", head_id, None)?;
            return Err(());
        }
        let publish_slot = (avail_idx_before as usize % qsize) as u16;
        if self
            .tx_publish_preflight(head_id, publish_slot, &resolved_descs[0], payload_len)
            .is_err()
        {
            return Err(());
        }
        trace!(
            target: "net-console",
            "[virtio-net][tx-submit-path=A] head={} slot={} avail_idx_before={}",
            head_id,
            publish_slot,
            avail_idx_before
        );
        #[cfg(debug_assertions)]
        self.debug_trace_tx_publish_state(head_id, publish_slot, "pre_guard_tx_post_state");
        if self
            .guard_tx_post_state(head_id, publish_slot, &resolved_descs[0])
            .is_err()
        {
            return Err(());
        }
        #[cfg(debug_assertions)]
        self.debug_trace_tx_publish_state(head_id, publish_slot, "post_guard_tx_post_state");
        self.tx_pre_publish_tripwire(
            head_id,
            publish_slot,
            &resolved_descs[0],
            header_len,
            payload_len,
        )?;
        #[cfg(debug_assertions)]
        self.debug_trace_tx_publish_state(head_id, publish_slot, "post_pre_publish_tripwire");
        #[cfg(debug_assertions)]
        self.debug_assert_tx_publish_ready(head_id, publish_slot);
        if self
            .assert_tx_desc_len_nonzero(head_id, "tx_desc_len_zero_before_avail")
            .is_err()
        {
            return Err(());
        }
        #[cfg(debug_assertions)]
        self.debug_trace_tx_publish_state(head_id, publish_slot, "pre_push_avail");
        virtq_publish_barrier();
        let (slot, avail_idx, old_idx) = match self.publish_tx_avail(head_id, publish_slot) {
            Ok(result) => result,
            Err(TxPublishError::InvalidDescriptor) => {
                let _ = self.tx_head_mgr.cancel_publish(head_id);
                self.tx_drops = self.tx_drops.saturating_add(1);
                return Err(());
            }
            Err(TxPublishError::Queue(_)) => {
                self.device_faulted = true;
                self.last_error.get_or_insert("tx_avail_write_failed");
                self.tx_state_violation("tx_avail_write_failed", head_id, Some(publish_slot))?;
                return Err(());
            }
        };
        if slot != publish_slot {
            self.tx_state_violation("tx_slot_mismatch", head_id, Some(slot))?;
            return Err(());
        }
        self.tx_wrap_tripwire(old_idx, avail_idx, slot, head_id);
        self.guard_tx_publish_readback(slot, head_id, &resolved_descs[0])?;
        let wrap_boundary = (old_idx as usize % qsize) > (avail_idx as usize % qsize);
        if wrap_boundary && !self.tx_wrap_logged {
            self.tx_wrap_logged = true;
            info!(
                target: "virtio-net",
                "[virtio-net][tx-wrap] avail_idx {old_idx}->{avail_idx} slot={slot} head={head_id} free={free} in_flight={in_flight} wrap_detected={wrap_detected} qsize={qsize} desc_len={desc_len} desc_addr=0x{desc_addr:016x} avail_head={avail_head}",
                free = self.tx_head_mgr.free_len(),
                in_flight = self.tx_head_mgr.in_flight_count(),
                wrap_detected = wrap_boundary,
                qsize = qsize,
                desc_len = resolved_descs.get(0).map(|d| d.len).unwrap_or_default(),
                desc_addr = resolved_descs.get(0).map(|d| d.addr).unwrap_or(0),
                avail_head = self.tx_queue.read_avail_slot(slot as usize),
            );
        }
        self.verify_tx_publish(slot, head_id, &resolved_descs[0])?;
        self.debug_check_tx_avail_uniqueness(self.tx_queue.last_used, avail_idx);
        self.debug_check_tx_outstanding_window(self.tx_queue.last_used, avail_idx);
        if log_breadcrumb {
            debug!(
                target: "virtio-net",
                "[virtio-net][tx-bc] avail synced head={} slot={} idx {}->{}",
                head_id,
                slot,
                old_idx,
                avail_idx,
            );
        }
        self.log_tx_dma_ranges(
            head_id,
            total_len,
            header_len,
            buffer_range,
            &resolved_descs,
            self.tx_anomaly_logged,
        );
        dma_barrier();
        if slot == 0 && !TX_WRAP_DMA_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            info!(
                target: "virtio-net",
                "[virtio-net][dma] tx wrap head={} len={} desc_bytes={} avail_bytes={} buffer=0x{buf_start:016x}..0x{buf_end:016x}",
                head_id,
                resolved_descs[0].len,
                self.tx_queue.layout.desc_len,
                self.tx_queue.layout.avail_len,
                buf_start = buffer_range.0,
                buf_end = buffer_range.1,
            );
        }
        self.log_publish_transaction(
            "TX",
            QueueKind::Tx,
            old_idx,
            avail_idx,
            slot,
            head_id,
            false,
        );
        self.log_tx_chain_publish(
            head_id,
            slot,
            old_idx,
            avail_idx,
            header_len,
            payload_len,
            payload_overlaps,
            header_fields,
            &resolved_descs,
        );
        if VIRTIO_DMA_TRACE {
            let desc_snapshot = self.tx_queue.read_descriptor(head_id);
            let avail_slot_val = self.tx_queue.read_avail_slot(slot as usize);
            let avail_idx_now = self.tx_queue.read_avail_idx();
            debug!(
                target: "virtio-net",
                "[virtio-net][tx-trace] pre-kick head={head_id} slot={slot} desc=0x{addr:016x}/{len} flags=0x{flags:04x} next={next} avail_slot={avail_slot} avail_idx_now={avail_idx_now} old_idx={old_idx}",
                head_id = head_id,
                slot = slot,
                addr = desc_snapshot.addr,
                len = desc_snapshot.len,
                flags = desc_snapshot.flags,
                next = desc_snapshot.next,
                avail_slot = avail_slot_val,
                avail_idx_now = avail_idx_now,
                old_idx = old_idx,
            );
            debug_assert!(
                desc_snapshot.len != 0 && desc_snapshot.addr != 0,
                "tx-trace: descriptor must be initialised before kick"
            );
        }
        if !TX_PUBLISH_FENCE_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            debug!(
                target: "virtio-net",
                "[virtio-net] enqueue publish: fences applied (tx) head={} slot={} avail_idx={}",
                head_id,
                slot,
                avail_idx,
            );
        }
        if log_breadcrumb {
            debug!(
                target: "virtio-net",
                "[virtio-net][tx-bc] pre-kick head={} notify={}",
                head_id,
                notify,
            );
        }
        if notify && !self.device_faulted {
            if self
                .tx_queue
                .notify(&mut self.regs, TX_QUEUE_INDEX)
                .is_err()
            {
                self.freeze_and_capture("tx_notify_failed");
                return Err(());
            }
            if log_breadcrumb {
                debug!(
                    target: "virtio-net",
                    "[virtio-net][tx-bc] post-kick head={} slot={}",
                    head_id,
                    slot,
                );
            }
        }
        Ok(())
    }

    fn initialise_queues(&mut self) {
        let header_len = self.rx_header_len;
        let payload_capacity = self.rx_payload_capacity;
        let frame_capacity = self.rx_frame_capacity;

        if self
            .validate_chain_nonzero(
                "RX",
                0,
                &[],
                Some(header_len),
                Some(payload_capacity),
                Some(frame_capacity),
                None,
            )
            .is_err()
        {
            return;
        }

        let rx_len = self.rx_buffers.len();
        if !RX_ARM_START_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            info!(
                target: "virtio-net",
                "[virtio-net][rx-arm] start buffers={} hdr_len={} payload_cap={} frame_cap={}",
                rx_len,
                header_len,
                payload_capacity,
                frame_capacity,
            );
        }
        for slot in 0..rx_len {
            let head_idx = slot as u16;
            let (desc, buffer_capacity, payload_len) = {
                let buffer = &mut self.rx_buffers[slot];
                let buffer_capacity = self
                    .rx_buffer_capacity
                    .min(buffer.as_slice().len())
                    .min(FRAME_BUFFER_LEN);
                let payload_len = self
                    .rx_payload_capacity
                    .min(buffer_capacity.saturating_sub(header_len));
                let total_len = header_len
                    .saturating_add(payload_len)
                    .min(buffer_capacity)
                    .min(buffer.as_slice().len());
                let total_len_u32 =
                    u32::try_from(total_len).expect("rx buffer length must fit in u32");
                let vaddr = buffer.ptr().as_ptr() as usize;
                let paddr = buffer.paddr();
                log_dma_programming("virtq.rx.buffer", vaddr, paddr, total_len);
                assert_dma_region("virtq.rx.buffer", vaddr, paddr, total_len);
                let desc = [DescSpec {
                    addr: buffer.paddr() as u64,
                    len: total_len_u32,
                    flags: VIRTQ_DESC_F_WRITE,
                    next: None,
                }];

                if let Err(err) = Self::sync_rx_slot_for_device(
                    buffer,
                    header_len,
                    payload_len,
                    self.dma_cacheable,
                ) {
                    warn!(
                        target: "net-console",
                        "[virtio-net] rx cache clean failed slot={} err={err:?}; freezing queue",
                        slot
                    );
                    self.freeze_and_capture("rx_cache_clean_failed");
                    return;
                }

                (desc, buffer_capacity, payload_len)
            };

            let notify = slot + 1 == rx_len;
            if self
                .enqueue_rx_chain_checked(
                    head_idx,
                    &desc,
                    header_len,
                    payload_len,
                    buffer_capacity,
                    None,
                    notify,
                )
                .is_err()
            {
                return;
            }

            if slot < 2 {
                let head_desc = self.rx_queue.read_descriptor(head_idx);
                info!(
                    target: "net-console",
                    "[virtio-net] rx[{slot}] desc={head_desc_idx} addr=0x{head_addr:016x} len={head_len} flags=0x{head_flags:04x} next={head_next} header_len={header_len} payload_len={payload_len}",
                    head_desc_idx = head_idx,
                    head_addr = head_desc.addr,
                    head_len = head_desc.len,
                    head_flags = head_desc.flags,
                    head_next = head_desc.next,
                    header_len = header_len,
                    payload_len = payload_len,
                );
            }
        }
        let (used_idx, avail_idx) = self.rx_queue.indices_no_sync();
        if !RX_ARM_END_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            info!(
                target: "virtio-net",
                "[virtio-net][rx-arm] end buffers={} avail.idx={} used.idx={}",
                rx_len,
                avail_idx,
                used_idx,
            );
        }
        info!(target: "virtio-net", "[virtio-net][rx-arm] complete");
        let first_paddr = self.rx_buffers.first().map(|buf| buf.paddr()).unwrap_or(0);
        let last_paddr = self.rx_buffers.last().map(|buf| buf.paddr()).unwrap_or(0);
        log::debug!(
            target: "virtio-net",
            "[RX] posted buffers={} used_idx={} avail_idx={}",
            self.rx_buffers.len(),
            used_idx,
            avail_idx,
        );
        if avail_idx as usize != self.rx_buffers.len() {
            warn!(
                target: "net-console",
                "[virtio-net] RX avail.idx {} does not match posted buffers {}",
                avail_idx,
                self.rx_buffers.len()
            );
        }
        if cfg!(debug_assertions) {
            debug_assert_eq!(used_idx, 0, "RX used_idx must start at zero");
            debug_assert_eq!(
                avail_idx as usize,
                self.rx_buffers.len(),
                "RX avail_idx should equal posted buffer count"
            );
        }
        info!(
            "[virtio-net] RX queue armed: size={} buffers={} last_used={}",
            self.rx_queue.size,
            self.rx_buffers.len(),
            self.rx_queue.last_used,
        );
        info!(
            target: "net-console",
            "[virtio-net] RX queue initialised: size={} buffers={} avail.idx={} used.idx={} first_paddr=0x{first:08x} last_paddr=0x{last:08x}",
            self.rx_queue.size,
            self.rx_buffers.len(),
            avail_idx,
            used_idx,
            first = first_paddr,
            last = last_paddr,
        );

        // TX ownership audit (virtio-net):
        // - Call chain: smoltcp `Device::transmit` → `prepare_tx_token`/`VirtioTxToken::consume`
        //   → `submit_tx_v2` → `tx_queue.setup_descriptor` + buffer clean → `publish_tx_avail`
        //   (writes avail slot/index and optional kick) → `tx_reclaim_used`/`reclaim_posted_head`
        //   (IRQ or poll) → `TxSlotTracker::complete` + `TxHeadManager::reclaim_head`.
        // - TX buffers come from `tx_buffers`: one pinned DMA frame per queue slot (size==queue).
        // - Free tracking pairs the round-robin `TxSlotTracker` (Free/Reserved/InFlight) with
        //   `TxHeadManager` so a slot cannot be reallocated until a used entry returns ownership.
        // - Packet → descriptor mapping is 1:1: the slot id is the descriptor index and buffer.
        //   Generations are tracked in both managers to detect reuse without reclaim.
        // - Used ring reaping happens in `tx_reclaim_used` (IRQ and poll), which calls
        //   `reclaim_posted_head` to correlate used.id with the slot and clear the reservation.
        // - Descriptors stay populated until reclaim; clearing only happens after ownership
        //   returns to avoid exposing {addr=0,len=0} to the device.
        let free_entries = self.tx_free_count();
        let tx_size = self.tx_queue.size;
        let tx_buffer_count = self.tx_buffers.len();
        log::info!(
            target: "net-console",
            "[virtio-net] TX queue initialised: size={} buffers={} free_entries={}",
            tx_size,
            tx_buffer_count,
            free_entries,
        );
    }

    fn sync_rx_slot_for_device(
        buffer: &RamFrame,
        header_len: usize,
        payload_len: usize,
        cacheable: bool,
    ) -> Result<(), DmaError> {
        NET_DIAG.record_rx_cache_clean();
        if !cacheable && !RX_CACHE_POLICY_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            log::info!(
                target: "virtio-net",
                "[virtio-net][dma] rx buffers mapped non-cacheable; cache clean should be redundant"
            );
        }
        let header_ptr = buffer.ptr().as_ptr();
        let payload_ptr = unsafe { header_ptr.add(header_len) };
        let payload_len = core::cmp::min(
            payload_len,
            buffer.as_slice().len().saturating_sub(header_len),
        );
        dma_clean(header_ptr, header_len, cacheable, "clean rx buffer header")?;
        dma_clean(
            payload_ptr,
            payload_len,
            cacheable,
            "clean rx buffer payload",
        )?;
        Ok(())
    }

    fn sync_rx_slot_for_cpu(
        buffer: &RamFrame,
        header_len: usize,
        written_len: usize,
        cacheable: bool,
    ) -> Result<(), DmaError> {
        NET_DIAG.record_rx_cache_invalidate();
        let header_len = core::cmp::min(header_len, written_len);
        let header_ptr = buffer.ptr().as_ptr();
        dma_invalidate(
            header_ptr,
            header_len,
            cacheable,
            "invalidate rx buffer header",
        )?;
        let payload_len = written_len
            .saturating_sub(header_len)
            .min(buffer.as_slice().len().saturating_sub(header_len));
        if payload_len > 0 {
            let payload_ptr = unsafe { header_ptr.add(header_len) };
            dma_invalidate(
                payload_ptr,
                payload_len,
                cacheable,
                "invalidate rx buffer payload",
            )?;
        }
        Ok(())
    }

    fn poll_interrupts(&mut self) {
        self.verify_tx_canary("irq");
        let (status, isr_ack) = self.regs.acknowledge_interrupts();
        log::debug!(
            target: "virtio-net",
            "ISR status=0x{:02x}, ISRACK=0x{:02x}",
            status,
            isr_ack,
        );
        if status != 0 {
            NET_DIAG.record_rx_irq();
            self.tx_stats.record_irq();
        }
        self.check_device_health();
        if self.device_faulted {
            return;
        }
        if NET_VIRTIO_TX_V2 {
            self.tx_reclaim_used(TX_RECLAIM_IRQ_BUDGET, TxReclaimSource::Irq);
            self.log_tx_stats_snapshot();
            return;
        }
        self.reclaim_tx();
        self.log_tx_stats_snapshot();
    }

    fn check_device_health(&mut self) {
        let status = self.regs.status();
        if (status & (STATUS_DEVICE_NEEDS_RESET | STATUS_FAILED)) != 0 {
            if !self.bad_status_logged {
                warn!(
                    target: "net-console",
                    "[virtio-net] entered bad status (0x{status:02x}); continuing until forensic log captured"
                );
                info!(
                    target: "net-console",
                    "[virtio-net][forensics] dma_noncoherent_active={} dma_cacheable={} arch_aarch64={}",
                    DMA_NONCOHERENT && self.dma_cacheable,
                    self.dma_cacheable,
                    DMA_NONCOHERENT,
                );
                self.bad_status_logged = true;
                self.freeze_and_capture("device_bad_status");
            } else if !self.device_faulted {
                self.device_faulted = true;
                self.last_error.get_or_insert("device_faulted");
                self.freeze_and_capture("device_bad_status_repeat");
            }

            self.bad_status_seen = true;
        }
    }

    fn note_progress(&mut self) {
        self.last_progress_ms = crate::hal::timebase().now_ms();
        self.stalled_snapshot_logged = false;
    }

    fn reclaim_tx(&mut self) {
        if forensics_frozen() {
            return;
        }
        self.verify_tx_canary("tx_reclaim_v1");
        if NET_VIRTIO_TX_V2 {
            self.tx_reclaim_used(TX_RECLAIM_POLL_BUDGET, TxReclaimSource::Poll);
            return;
        }
        self.used_poll_calls = self.used_poll_calls.wrapping_add(1);
        let (used_idx, avail_idx) = self.tx_queue.indices();
        let should_log =
            used_idx != self.tx_last_used_seen || (self.tx_progress_log_gate & 0x3f) == 0;
        if should_log {
            info!(
                target: "net-console",
                "[virtio-net] tx poll: avail.idx={} used.idx={} last_used={} in_flight={} tx_free={} tx_gen={}",
                avail_idx,
                used_idx,
                self.tx_queue.last_used,
                self.tx_head_mgr.in_flight_count(),
                self.tx_head_mgr.free_len(),
                self.tx_head_mgr.next_gen(),
            );
            self.tx_last_used_seen = used_idx;
        }
        self.tx_progress_log_gate = self.tx_progress_log_gate.wrapping_add(1);
        // used.idx can advance while last_used stays put if the used element is not yet visible
        // (or the device leaves used.len at zero); last_used only advances after a decoded entry.
        // This path reads tx_queue.used.idx via cache invalidate + volatile reads and validates
        // used.elem.id against TX head state before reclaiming.
        // used.len is advisory for TX; accept zero-length after the visibility retry so we do
        // not stall reclaim when the device advances used.idx but leaves len as zero.
        let mut progressed = false;
        loop {
            match self.tx_queue.pop_used("TX", true) {
                Ok(Some((id, len, ring_slot))) => {
                    progressed = true;
                    self.record_tx_used_entry(id, len);
                    // used.len is ignored for TX; ownership returns solely via used.id and tracked state.
                    let reclaim_result =
                        match self.reclaim_posted_head(id, ring_slot, len, self.tx_queue.last_used)
                        {
                            Ok(result) => result,
                            Err(()) => break,
                        };
                    if matches!(reclaim_result, TxReclaimResult::Reclaimed) {
                        self.clear_tx_desc_chain(id);
                        NET_DIAG.record_tx_completion();
                    }
                    self.tx_used_count = self.tx_used_count.wrapping_add(1);
                    NET_DIAG.record_tx_used_seen();
                    self.note_progress();
                    if !matches!(reclaim_result, TxReclaimResult::Reclaimed) {
                        // Duplicate completions are ignored after state validation.
                        continue;
                    }
                }
                Ok(None) => break,
                Err(fault) => {
                    let _ = self.handle_forensic_fault(fault);
                    break;
                }
            }
        }
        let (used_idx_after, avail_idx_after) = self.tx_queue.indices();
        if !progressed && used_idx_after != self.tx_queue.last_used {
            self.log_tx_reclaim_stall(
                used_idx_after,
                avail_idx_after,
                self.tx_queue.last_used,
            );
        }
        #[cfg(debug_assertions)]
        {
            if !progressed && used_idx_after != self.tx_queue.last_used {
                self.tx_reclaim_stall_polls = self.tx_reclaim_stall_polls.saturating_add(1);
                if self.tx_reclaim_stall_polls >= TX_RECLAIM_STALL_POLL_LIMIT
                    && !self.tx_reclaim_stall_latched
                {
                    self.tx_reclaim_stall_latched = true;
                    self.dump_tx_used_window_once();
                }
            } else {
                self.tx_reclaim_stall_polls = 0;
            }
        }
    }

    fn tx_reclaim_used(&mut self, budget: u16, source: TxReclaimSource) {
        if budget == 0 || forensics_frozen() {
            return;
        }
        self.verify_tx_canary("tx_reclaim");
        self.used_poll_calls = self.used_poll_calls.wrapping_add(1);
        let used = self.tx_queue.used.as_ptr();
        if self.tx_queue.invalidate_used_header_for_cpu().is_err() {
            self.freeze_and_capture("tx_v2_used_header_invalidate_failed");
            return;
        }
        let used_idx = unsafe { read_volatile(&(*used).idx) };
        virtq_used_load_barrier();
        let qsize = usize::from(self.tx_queue.size);

        assert!(qsize != 0, "virtqueue size must be non-zero");

        let mut processed: u16 = 0;
        while self.tx_v2_last_used != used_idx && processed < budget {
            self.tx_queue.last_used = self.tx_v2_last_used;
            let ring_slot = (self.tx_v2_last_used as usize) % qsize;
            if self
                .tx_queue
                .invalidate_used_elem_for_cpu(ring_slot)
                .is_err()
            {
                self.freeze_and_capture("tx_v2_used_elem_invalidate_failed");
                return;
            }
            virtq_used_load_barrier();
            dma_load_barrier();
            let elem_ptr = unsafe { (*used).ring.as_ptr().add(ring_slot) as *const VirtqUsedElem };
            let mut elem = unsafe { read_volatile(elem_ptr) };
            let mut elem_len = u32::from_le(elem.len);
            if elem_len == 0 {
                if self
                    .tx_queue
                    .invalidate_used_elem_for_cpu(ring_slot)
                    .is_err()
                {
                    self.freeze_and_capture("tx_v2_used_elem_retry_invalidate_failed");
                    return;
                }
                virtq_used_load_barrier();
                dma_load_barrier();
                let retry = unsafe { read_volatile(elem_ptr) };
                let retry_len = u32::from_le(retry.len);
                if retry_len == 0 {
                    let retry_id = u32::from_le(retry.id);
                    if !USED_LEN_ZERO_VISIBILITY_LOGGED.swap(true, AtomicOrdering::AcqRel) {
                        warn!(
                            target: "net-console",
                            "[virtio-net] tx used len zero after re-read: head={} idx={} ring_slot={}",
                            retry_id,
                            self.tx_v2_last_used,
                            ring_slot,
                        );
                    }
                    return;
                }
                elem = retry;
                elem_len = retry_len;
            }
            let id = u32::from_le(elem.id) as u16;
            let next_used = self.tx_v2_last_used.wrapping_add(1);
            log::debug!(
                target: "virtio-net",
                "[virtio-net][tx-complete] used_idx={}→{} id={} len={}",
                self.tx_v2_last_used,
                next_used,
                id,
                elem_len
            );
            // used.len is advisory; the head lifecycle is guarded by state, not the device-provided length.
            let reclaim_result = match self.reclaim_posted_head(
                id,
                ring_slot as u16,
                elem_len,
                self.tx_v2_last_used,
            ) {
                Ok(result) => result,
                Err(()) => break,
            };
            if matches!(reclaim_result, TxReclaimResult::Reclaimed) {
                self.clear_tx_desc_chain(id);
                self.tx_complete = self.tx_complete.wrapping_add(1);
                NET_DIAG.record_tx_completion();
                NET_DIAG.record_tx_used_seen();
                self.tx_stats.record_used_reaped();
            } else {
                NET_DIAG.record_tx_used_seen();
            }
            self.tx_used_count = self.tx_used_count.wrapping_add(1);
            self.tx_v2_last_used = next_used;
            self.note_progress();
            processed = processed.saturating_add(1);
            if !matches!(reclaim_result, TxReclaimResult::Reclaimed) {
                if matches!(reclaim_result, TxReclaimResult::InvalidId) {
                    break;
                }
                continue;
            }
        }
        self.tx_queue.last_used = self.tx_v2_last_used;

        let avail_idx_now = self.tx_queue.indices_no_sync().1;
        self.debug_check_tx_outstanding_window(self.tx_queue.last_used, avail_idx_now);

        self.audit_tx_accounting("reclaim_tx_v2");
        self.log_tx_v2_invariants();
        #[cfg(debug_assertions)]
        self.tx_assert_invariants("reclaim");
        match source {
            TxReclaimSource::Irq => self.tx_stats.record_irq_reclaim(),
            TxReclaimSource::Poll => self.tx_stats.record_poll_reclaim(),
        }
    }

    fn audit_tx_accounting(&mut self, context: &'static str) {
        match self.tx_head_mgr.audit() {
            Ok((free, prepared, in_flight, completed, posted)) => {
                let qsize = self.tx_queue.size;
                let total = free
                    .saturating_add(prepared)
                    .saturating_add(in_flight)
                    .saturating_add(completed);
                if total != qsize || posted != in_flight {
                    if !self.tx_audit_violation_logged {
                        self.tx_audit_violation_logged = true;
                        warn!(
                            target: "net-console",
                            "[virtio-net][tx-audit] invariant mismatch context={} total={} free={} prepared={} in_flight={} completed={} posted={} qsize={}",
                            context,
                            total,
                            free,
                            prepared,
                            in_flight,
                            completed,
                            posted,
                            qsize,
                        );
                    }
                }
                let now_ms = crate::hal::timebase().now_ms();
                if now_ms.saturating_sub(self.tx_audit_log_ms) >= 1_000 {
                    self.tx_audit_log_ms = now_ms;
                    info!(
                        target: "net-console",
                        "[virtio-net][tx-audit] context={} free={} prepared={} in_flight={} completed={} posted={} free_mask=0x{mask:08x} qsize={qsize}",
                        context,
                        free,
                        prepared,
                        in_flight,
                        completed,
                        posted,
                        mask = self.tx_head_mgr.free_mask & self.tx_head_mgr.active_mask(),
                        qsize = qsize,
                    );
                }
            }
            Err(_) => {
                if !self.tx_audit_violation_logged {
                    self.tx_audit_violation_logged = true;
                    warn!(
                        target: "net-console",
                        "[virtio-net][tx-audit] accounting failed context={} free_mask=0x{mask:08x}",
                        context,
                        mask = self.tx_head_mgr.free_mask,
                    );
                }
            }
        }
    }

    fn log_tx_v2_invariants(&mut self) {
        let now_ms = crate::hal::timebase().now_ms();
        if now_ms.saturating_sub(self.tx_v2_log_ms) < 1_000 {
            return;
        }
        self.tx_v2_log_ms = now_ms;
        let in_flight = self.tx_head_mgr.in_flight_count();
        let free = self.tx_head_mgr.free_len();
        let qsize = self.tx_queue.size;
        if in_flight + free != qsize {
            warn!(
                target: "net-console",
                "[virtio-net][tx-v2] invariant break: free={} in_flight={} qsize={}",
                free,
                in_flight,
                qsize,
            );
        }
        let (dup_alloc, dup_publish, invalid_id, invalid_state, zero_len_publish) =
            self.tx_head_mgr.counters();
        info!(
            target: "net-console",
            "[virtio-net][tx-v2] stats free={} in_flight={} submit={} complete={} zero_len={} double_submit={} dup_alloc_refused={} dup_publish_blocked={} invalid_used_id={} invalid_used_state={} zero_len_publish_blocked={} zero_guard={} invariant_violations={}",
            free,
            in_flight,
            self.tx_submit,
            self.tx_complete,
            self.tx_zero_len_attempt,
            self.tx_double_submit,
            dup_alloc,
            dup_publish,
            invalid_id,
            invalid_state,
            zero_len_publish,
            self.tx_zero_desc_guard,
            self.tx_invariant_violations,
        );
    }

    #[cfg(debug_assertions)]
    fn tx_publish_blocked(&self) -> bool {
        self.tx_publish_frozen
    }

    #[cfg(not(debug_assertions))]
    fn tx_publish_blocked(&self) -> bool {
        false
    }

    #[cfg(debug_assertions)]
    fn freeze_tx_publishes(&mut self, reason: &'static str) {
        if self.tx_publish_frozen {
            return;
        }
        self.tx_publish_frozen = true;
        if !self.tx_avail_duplicate_logged {
            self.tx_avail_duplicate_logged = true;
            error!(
                target: "net-console",
                "[virtio-net][tx] publish frozen: reason={} avail.idx={} used.idx={} last_used={}",
                reason,
                self.tx_queue.indices_no_sync().1,
                self.tx_queue.indices_no_sync().0,
                self.tx_queue.last_used,
            );
        }
        self.freeze_and_capture(reason);
    }

    #[cfg(not(debug_assertions))]
    fn freeze_tx_publishes(&mut self, _reason: &'static str) {}

    #[cfg(debug_assertions)]
    fn debug_check_tx_avail_uniqueness(&mut self, used_idx: u16, avail_idx: u16) {
        if self.tx_publish_blocked() {
            return;
        }
        let pending = avail_idx.wrapping_sub(used_idx);
        let window = core::cmp::min(pending, self.tx_queue.size);
        let avail = self.tx_queue.avail.as_ptr();
        let qsize = usize::from(self.tx_queue.size);
        let mut seen: HeaplessVec<u16, TX_QUEUE_SIZE> = HeaplessVec::new();
        for offset in 0..usize::from(window) {
            let slot_idx = avail_idx.wrapping_sub(window).wrapping_add(offset as u16);
            let ring_slot = (slot_idx as usize) % qsize;
            let ring_ptr = unsafe { (*avail).ring.as_ptr().add(ring_slot) as *const u16 };
            let head = unsafe { read_volatile(ring_ptr) };
            if seen.iter().any(|&entry| entry == head) {
                if !self.tx_avail_duplicate_logged {
                    self.tx_avail_duplicate_logged = true;
                    error!(
                        target: "net-console",
                        "[virtio-net][tx] duplicate head detected in avail ring: head={} used.idx={} avail.idx={} window={} ring_slot={}",
                        head,
                        used_idx,
                        avail_idx,
                        window,
                        ring_slot,
                    );
                }
                self.freeze_tx_publishes("tx_avail_duplicate");
                return;
            }
            let _ = seen.push(head);
        }
    }

    #[cfg(not(debug_assertions))]
    fn debug_check_tx_avail_uniqueness(&mut self, _used_idx: u16, _avail_idx: u16) {}

    #[cfg(debug_assertions)]
    fn debug_check_tx_outstanding_window(&mut self, used_idx: u16, avail_idx: u16) {
        if self.tx_queue.size == 0 {
            return;
        }
        let pending = avail_idx.wrapping_sub(used_idx);
        let window = core::cmp::min(pending, self.tx_queue.size);
        let avail = self.tx_queue.avail.as_ptr();
        let qsize = usize::from(self.tx_queue.size);
        let mut seen: [u8; TX_QUEUE_SIZE] = [0; TX_QUEUE_SIZE];
        for offset in 0..usize::from(window) {
            let slot_idx = used_idx.wrapping_add(offset as u16);
            let ring_slot = (slot_idx as usize) % qsize;
            let ring_ptr = unsafe { (*avail).ring.as_ptr().add(ring_slot) as *const u16 };
            let head = unsafe { read_volatile(ring_ptr) };
            if (head as usize) < seen.len() {
                seen[head as usize] = seen[head as usize].saturating_add(1);
                if seen[head as usize] > 1 {
                    error!(
                        target: "net-console",
                        "[virtio-net][tx] duplicate head in outstanding window: head={} ring_slot={} used_idx={} avail_idx={} pending={}",
                        head,
                        ring_slot,
                        used_idx,
                        avail_idx,
                        window,
                    );
                    self.freeze_tx_publishes("tx_outstanding_duplicate");
                    return;
                }
            } else {
                self.tx_head_mgr.record_invalid_used_id();
                error!(
                    target: "net-console",
                    "[virtio-net][tx] outstanding head out of range: head={} ring_slot={} used_idx={} avail_idx={}",
                    head,
                    ring_slot,
                    used_idx,
                    avail_idx,
                );
                self.freeze_tx_publishes("tx_outstanding_id_oob");
                return;
            }
        }
        let posted = self.tx_head_mgr.posted_count();
        if window != posted {
            error!(
                target: "net-console",
                "[virtio-net][tx] outstanding window mismatch: pending={} posted={} used_idx={} avail_idx={}",
                window,
                posted,
                used_idx,
                avail_idx,
            );
            self.freeze_tx_publishes("tx_outstanding_mismatch");
        }
    }

    #[cfg(not(debug_assertions))]
    fn debug_check_tx_outstanding_window(&mut self, _used_idx: u16, _avail_idx: u16) {}

    fn pop_rx(&mut self) -> Option<(u16, usize)> {
        if forensics_frozen() {
            return None;
        }
        self.used_poll_calls = self.used_poll_calls.wrapping_add(1);
        match self.rx_queue.pop_used("RX", false) {
            Ok(Some((id, len, _slot))) => {
                let len = len as usize;
                let header_len = self.rx_header_len;
                if len < header_len {
                    if !self
                        .rx_underflow_logged_ids
                        .iter()
                        .any(|&logged| logged == id)
                    {
                        let _ = self.rx_underflow_logged_ids.push(id);
                        error!(
                            target: "net-console",
                            "[virtio-net] RX used entry shorter than header: id={} len={} header_len={} last_used={}",
                            id,
                            len,
                            header_len,
                            self.rx_queue.last_used,
                        );
                    }
                    self.rx_used_count = self.rx_used_count.wrapping_add(1);
                    NET_DIAG.record_rx_used_seen(crate::hal::timebase().now_ms());
                    self.note_progress();
                    self.requeue_rx(id, Some(len));
                    return None;
                }
                if let Some(buffer) = self.rx_buffers.get_mut(id as usize) {
                    if let Err(err) =
                        Self::sync_rx_slot_for_cpu(buffer, header_len, len, self.dma_cacheable)
                    {
                        warn!(
                            target: "net-console",
                            "[virtio-net] rx cache invalidate failed id={} err={err:?}; freezing queue",
                            id
                        );
                        self.freeze_and_capture("rx_cache_invalidate_failed");
                        return None;
                    }
                    debug_assert!(
                        NET_DIAG.snapshot().rx_cache_invalidate > 0,
                        "rx cache invalidate must run before consuming RX buffers"
                    );
                }
                self.last_used_idx_debug = self.rx_queue.last_used;
                let (used_idx, avail_idx) = self.rx_queue.indices();
                log::debug!(
                    target: "virtio-net",
                    "[RX] consumed used_idx={} avail_idx={}",
                    used_idx,
                    avail_idx,
                );
                self.rx_used_count = self.rx_used_count.wrapping_add(1);
                NET_DIAG.record_rx_used_seen(crate::hal::timebase().now_ms());
                self.note_progress();
                Some((id, len))
            }
            Ok(None) => None,
            Err(fault) => {
                let _ = self.handle_forensic_fault(fault);
                None
            }
        }
    }

    fn requeue_rx(&mut self, id: u16, used_len: Option<usize>) {
        self.check_device_health();
        if self.device_faulted {
            return;
        }
        if forensics_frozen() {
            return;
        }
        let slot = id as usize;
        let header_len = self.rx_header_len;
        if header_len == 0 {
            if !self.rx_header_zero_logged {
                self.rx_header_zero_logged = true;
                error!(
                    target: "net-console",
                    "[virtio-net] rx_header_len negotiated as zero; halting requeue id={id}"
                );
            }
            self.device_faulted = true;
            self.last_error.get_or_insert("rx_header_len_zero");
            return;
        }
        if let Some(buffer) = self.rx_buffers.get_mut(slot) {
            let buffer_capacity = self
                .rx_buffer_capacity
                .min(buffer.as_slice().len())
                .min(FRAME_BUFFER_LEN);
            let payload_len = self
                .rx_payload_capacity
                .min(buffer_capacity.saturating_sub(header_len));
            let frame_capacity = header_len.saturating_add(payload_len);

            debug_assert!(header_len > 0);
            debug_assert!(payload_len > 0);
            debug_assert_eq!(frame_capacity, self.rx_frame_capacity);

            if payload_len == 0 {
                if !self.rx_payload_zero_logged {
                    self.rx_payload_zero_logged = true;
                    error!(
                        target: "net-console",
                        "[virtio-net] rx_payload_len resolved to zero; halting requeue id={id} buffer_capacity={buffer_capacity} header_len={header_len}"
                    );
                }
                self.device_faulted = true;
                self.last_error.get_or_insert("rx_payload_len_zero");
                return;
            }

            let total_len = frame_capacity.min(buffer.as_slice().len());
            let total_len_u32 =
                u32::try_from(total_len).expect("requeue buffer length must fit in u32");

            let vaddr = buffer.ptr().as_ptr() as usize;
            let paddr = buffer.paddr();
            log_dma_programming("virtq.rx.buffer.requeue", vaddr, paddr, total_len);
            assert_dma_region("virtq.rx.buffer.requeue", vaddr, paddr, total_len);
            let desc = [DescSpec {
                addr: buffer.paddr() as u64,
                len: total_len_u32,
                flags: VIRTQ_DESC_F_WRITE,
                next: None,
            }];

            if let Err(err) =
                Self::sync_rx_slot_for_device(buffer, header_len, payload_len, self.dma_cacheable)
            {
                warn!(
                    target: "net-console",
                    "[virtio-net] rx requeue cache clean failed id={} err={err:?}",
                    id
                );
                self.freeze_and_capture("rx_requeue_cache_clean_failed");
                return;
            }
            debug_assert!(
                NET_DIAG.snapshot().rx_cache_clean > 0,
                "rx cache clean must run before posting descriptors"
            );

            if !self
                .rx_requeue_logged_ids
                .iter()
                .any(|&logged| logged == id)
            {
                let _ = self.rx_requeue_logged_ids.push(id);
                debug!(
                    target: "net-console",
                    "[virtio-net] rx_requeue id={id} hdr_len={header_len} payload_len={payload_len} frame_len={frame_capacity} buffer_capacity={buffer_capacity}",
                );
            }

            if self
                .enqueue_rx_chain_checked(
                    id,
                    &desc,
                    header_len,
                    payload_len,
                    frame_capacity,
                    used_len,
                    true,
                )
                .is_err()
            {
                return;
            }

            let (used_idx, avail_idx) = self.rx_queue.indices();
            log::debug!(
                target: "virtio-net",
                "[RX] posted buffers={} used_idx={} avail_idx={}",
                1,
                used_idx,
                avail_idx,
            );
        } else {
            warn!(
                target: "net-console",
                "[virtio-net] requeue_rx: id={} slot={} missing buffer entry",
                id,
                slot
            );
        }
    }

    fn prepare_tx_token(&mut self) -> VirtioTxToken {
        let driver_ptr = self as *mut _;
        self.verify_tx_canary("tx_prepare");
        if NET_VIRTIO_TX_V2 {
            if self.tx_publish_blocked() {
                return VirtioTxToken::new(driver_ptr, None);
            }
            if self.tx_free_count() == 0 {
                self.tx_reclaim_used(TX_RECLAIM_POLL_BUDGET, TxReclaimSource::Poll);
            }
            if let Some(reservation) = self.reserve_tx_slot() {
                return VirtioTxToken::new(driver_ptr, Some(reservation));
            }
            let inflight = self.tx_inflight_count();
            let free = self.tx_free_count();
            self.tx_stats
                .record_would_block(inflight, free, self.tx_queue.size);
            if inflight > 0 {
                self.tx_alloc_blocked_inflight = self.tx_alloc_blocked_inflight.saturating_add(1);
            }
            return VirtioTxToken::new(driver_ptr, None);
        }
        if self.tx_publish_blocked() {
            return VirtioTxToken::new(driver_ptr, None);
        }
        if let Some(reservation) = self.reserve_tx_slot() {
            return VirtioTxToken::new(driver_ptr, Some(reservation));
        }
        let inflight = self.tx_inflight_count();
        let free = self.tx_free_count();
        self.tx_stats
            .record_would_block(inflight, free, self.tx_queue.size);
        if inflight > 0 {
            self.tx_alloc_blocked_inflight = self.tx_alloc_blocked_inflight.saturating_add(1);
        }
        VirtioTxToken::new(driver_ptr, None)
    }

    fn next_tx_attempt_seq(&mut self) -> u64 {
        let seq = self.tx_attempt_seq;
        self.tx_attempt_seq = self.tx_attempt_seq.wrapping_add(1);
        seq
    }

    fn log_tx_attempt(
        &mut self,
        seq: u64,
        requested_len: usize,
        payload_len: usize,
        written_len: usize,
    ) {
        let should_log = seq < 32
            || written_len == 0
            || payload_len == 0
            || payload_len != requested_len
            || (self.tx_attempt_log_gate & 0x3f) == 0;
        self.tx_attempt_log_gate = self.tx_attempt_log_gate.wrapping_add(1);
        if should_log {
            info!(
                target: "net-console",
                "[virtio-net][tx-attempt] seq={} requested={} payload_len={} written={}",
                seq,
                requested_len,
                payload_len,
                written_len,
            );
        }
        if requested_len == 0 {
            self.tx_anomaly(TxAnomalyReason::SmoltcpRequestedZeroLen, "smoltcp_len_zero");
        }
        if written_len == 0 {
            self.tx_anomaly(TxAnomalyReason::ClosureWroteZero, "closure_wrote_zero");
        }
    }

    fn drop_duplicate_publish(&mut self, id: u16, state: TxHeadState) {
        self.note_publish_blocked(id, Some(state));
        self.tx_drops = self.tx_drops.saturating_add(1);
        self.tx_double_submit = self.tx_double_submit.wrapping_add(1);
    }

    fn note_publish_blocked(&mut self, id: u16, state: Option<TxHeadState>) {
        self.tx_dup_publish_blocked = self.tx_dup_publish_blocked.saturating_add(1);
        let now_ms = crate::hal::timebase().now_ms();
        let (used_idx, avail_idx) = self.tx_queue.indices_no_sync();
        if self.tx_dup_publish_log_ms == 0
            || now_ms.saturating_sub(self.tx_dup_publish_log_ms) >= 1_000
        {
            self.tx_dup_publish_log_ms = now_ms;
            warn!(
                target: "net-console",
                "[virtio-net][tx] publish gate blocked: head={} state={:?} avail_idx={} used_idx={} last_used={} in_flight={}",
                id,
                state,
                avail_idx,
                used_idx,
                self.tx_queue.last_used,
                self.tx_head_mgr.in_flight_count(),
            );
        }
    }

    fn ensure_tx_head_prepared(&mut self, id: u16) -> Result<(), ()> {
        match self.tx_head_mgr.state(id) {
            Some(TxHeadState::Prepared { .. }) => Ok(()),
            Some(state) => {
                self.note_publish_blocked(id, Some(state));
                Err(())
            }
            None => {
                self.last_error.get_or_insert("tx_state_missing");
                self.note_publish_blocked(id, None);
                Err(())
            }
        }
    }

    fn validate_tx_reservation(
        &mut self,
        reservation: TxReservation,
        context: &'static str,
    ) -> Result<u16, ()> {
        let head_id = reservation.head_id;
        let head_state = self.tx_head_mgr.state(head_id);
        let head_ok = matches!(
            head_state,
            Some(TxHeadState::Prepared { gen }) if gen == reservation.head_gen
        );
        let mut slot_ok = true;
        let slot_state = if NET_VIRTIO_TX_V2 {
            slot_ok = match (self.tx_slots.state(head_id), reservation.slot_gen) {
                (Some(TxSlotState::Reserved { gen }), Some(expected)) if gen == expected => true,
                _ => false,
            };
            self.tx_slots.state(head_id)
        } else {
            None
        };
        if head_ok && slot_ok {
            return Ok(head_id);
        }
        self.tx_bad_id_mapping = self.tx_bad_id_mapping.saturating_add(1);
        self.tx_dup_publish_blocked = self.tx_dup_publish_blocked.saturating_add(1);
        if !head_ok {
            self.publish_blocked_bad_head_state =
                self.publish_blocked_bad_head_state.saturating_add(1);
        }
        if NET_VIRTIO_TX_V2 && !slot_ok {
            self.publish_blocked_bad_slot_state =
                self.publish_blocked_bad_slot_state.saturating_add(1);
        }
        warn!(
            target: "net-console",
            "[virtio-net][tx-reservation] invalid reservation: head={} head_gen={} slot_gen={:?} head_state={:?} slot_state={:?} context={}",
            head_id,
            reservation.head_gen,
            reservation.slot_gen,
            head_state,
            slot_state,
            context,
        );
        self.last_error.get_or_insert("tx_reservation_invalid");
        Err(())
    }

    fn log_invalid_used_state(
        &mut self,
        head: u16,
        state: Option<TxHeadState>,
        ring_slot: u16,
        used_idx: u16,
    ) {
        let now_ms = crate::hal::timebase().now_ms();
        if self.tx_invalid_used_log_ms == 0
            || now_ms.saturating_sub(self.tx_invalid_used_log_ms) >= 1_000
        {
            self.tx_invalid_used_log_ms = now_ms;
            let avail_idx = self.tx_queue.indices_no_sync().1;
            warn!(
                target: "net-console",
                "[virtio-net][tx] invalid used state: head={} state={:?} ring_slot={} used_idx={} last_used={} avail_idx={} in_flight={}",
                head,
                state,
                ring_slot,
                used_idx,
                self.tx_queue.last_used,
                avail_idx,
                self.tx_head_mgr.in_flight_count(),
            );
        }
    }

    fn log_tx_reclaim_stall(&mut self, used_idx: u16, avail_idx: u16, last_used: u16) {
        if self.tx_reclaim_stall_logged {
            return;
        }
        self.tx_reclaim_stall_logged = true;
        let in_flight = self.tx_head_mgr.in_flight_count();
        let tx_free = self.tx_head_mgr.free_len();
        let tx_gen = self.tx_head_mgr.next_gen();
        let qsize = usize::from(self.tx_queue.size);
        let ring_slot = if qsize == 0 {
            0
        } else {
            (last_used as usize) % qsize
        };
        let used = self.tx_queue.used.as_ptr();
        let mut elem_id = 0u32;
        let mut elem_len = 0u32;
        let mut retry = false;
        if self.tx_queue.invalidate_used_elem_for_cpu(ring_slot).is_ok() {
            virtq_used_load_barrier();
            dma_load_barrier();
            let elem_ptr = unsafe { (*used).ring.as_ptr().add(ring_slot) as *const VirtqUsedElem };
            let elem = unsafe { read_volatile(elem_ptr) };
            elem_id = u32::from_le(elem.id);
            elem_len = u32::from_le(elem.len);
            if elem_len == 0 && self.tx_queue.invalidate_used_elem_for_cpu(ring_slot).is_ok() {
                retry = true;
                virtq_used_load_barrier();
                dma_load_barrier();
                let retry_elem = unsafe { read_volatile(elem_ptr) };
                elem_id = u32::from_le(retry_elem.id);
                elem_len = u32::from_le(retry_elem.len);
            }
        }
        warn!(
            target: "net-console",
            "[virtio-net][tx] reclaim stalled: avail.idx={} used.idx={} last_used={} in_flight={} tx_free={} tx_gen={} head={} used.id={} used.len={} retry={}",
            avail_idx,
            used_idx,
            last_used,
            in_flight,
            tx_free,
            tx_gen,
            elem_id as u16,
            elem_id,
            elem_len,
            retry,
        );
    }

    fn release_tx_head(&mut self, id: u16, label: &'static str) {
        if self.tx_head_mgr.release_unused(id).is_err() {
            self.tx_drops = self.tx_drops.saturating_add(1);
            self.last_error.get_or_insert(label);
        }
        self.cancel_tx_slot(id, label);
    }

    fn drop_zero_len_tx(&mut self, id: u16, payload_len: usize, written_len: usize) {
        self.tx_drop_zero_len = self.tx_drop_zero_len.saturating_add(1);
        let now_ms = crate::hal::timebase().now_ms();
        if now_ms.saturating_sub(self.tx_zero_len_log_ms) >= 1_000 {
            self.tx_zero_len_log_ms = now_ms;
            debug!(
                target: "net-console",
                "[virtio-net][tx-drop] dropping zero-length payload head={} payload_len={} written={} drops={}",
                id,
                payload_len,
                written_len,
                self.tx_drop_zero_len,
            );
        }
        self.tx_drops = self.tx_drops.saturating_add(1);
        self.release_tx_head(id, "tx_v2_zero_len_drop");
    }

    fn compute_written_len(payload_len: usize, before: &[u8], after: &[u8]) -> usize {
        let limit = core::cmp::min(payload_len, core::cmp::min(before.len(), after.len()));
        let mut written = 0usize;
        for idx in 0..limit {
            if before[idx] != after[idx] {
                written = idx + 1;
            }
        }
        written
    }

    fn tx_total_len(header_len: usize, written_len: usize) -> Option<usize> {
        header_len
            .checked_add(written_len)
            .filter(|&len| len > 0 && written_len > 0)
    }

    fn verify_tx_canary(&mut self, context: &'static str) {
        if self.tx_canary_front == TX_CANARY_VALUE && self.tx_canary_back == TX_CANARY_VALUE {
            return;
        }
        if self.tx_canary_fault_logged {
            return;
        }
        self.tx_canary_fault_logged = true;
        error!(
            target: "net-console",
            "[virtio-net][tx-canary] violation context={} front=0x{:08x} back=0x{:08x}",
            context,
            self.tx_canary_front,
            self.tx_canary_back
        );
    }

    fn tx_slot_counts(&mut self) -> (u16, u16) {
        if !NET_VIRTIO_TX_V2 {
            return (
                self.tx_head_mgr.free_len(),
                self.tx_head_mgr.in_flight_count(),
            );
        }
        let free_slots = self.tx_slots.free_count();
        let inflight_slots = self.tx_slots.in_flight();
        let mgr_free = self.tx_head_mgr.free_len();
        let mgr_inflight = self.tx_head_mgr.in_flight_count();
        if free_slots != mgr_free || inflight_slots != mgr_inflight {
            if !self.tx_slot_divergence_logged {
                self.tx_slot_divergence_logged = true;
                warn!(
                    target: "net-console",
                    "[virtio-net][tx-slot] divergence: tracker_free={} mgr_free={} tracker_inflight={} mgr_inflight={} qsize={}",
                    free_slots,
                    mgr_free,
                    inflight_slots,
                    mgr_inflight,
                    self.tx_queue.size,
                );
            }
            #[cfg(debug_assertions)]
            self.freeze_tx_publishes("tx_slot_divergence");
        }
        (free_slots, inflight_slots)
    }

    fn tx_free_count(&mut self) -> u16 {
        let (free, _) = self.tx_slot_counts();
        free
    }

    fn tx_inflight_count(&mut self) -> u16 {
        let (_, inflight) = self.tx_slot_counts();
        inflight
    }

    fn reserve_tx_slot(&mut self) -> Option<TxReservation> {
        if !NET_VIRTIO_TX_V2 {
            return self.tx_head_mgr.alloc_head().and_then(|id| {
                let head_gen = self.tx_head_mgr.generation(id)?;
                Some(TxReservation {
                    head_id: id,
                    head_gen,
                    slot_gen: None,
                })
            });
        }
        let (slot, wrap, slot_gen) = self.tx_slots.reserve_next()?;
        match self.tx_head_mgr.alloc_specific(slot) {
            Some(id) => {
                if wrap {
                    let avail_idx = self.tx_queue.indices_no_sync().1;
                    let (free, inflight) = self.tx_slot_counts();
                    info!(
                        target: "virtio-net",
                        "[virtio-net][tx-wrap-reserve] avail_idx={} slot={} free={} inflight={} qsize={}",
                        avail_idx,
                        slot,
                        free,
                        inflight,
                        self.tx_queue.size,
                    );
                }
                let head_gen = self.tx_head_mgr.generation(id)?;
                Some(TxReservation {
                    head_id: id,
                    head_gen,
                    slot_gen: Some(slot_gen),
                })
            }
            None => {
                let _ = self.tx_slots.cancel(slot);
                None
            }
        }
    }

    fn cancel_tx_slot(&mut self, id: u16, context: &'static str) {
        if !NET_VIRTIO_TX_V2 {
            return;
        }
        if let Err(err) = self.tx_slots.cancel(id) {
            if !self.tx_slot_divergence_logged {
                self.tx_slot_divergence_logged = true;
                warn!(
                    target: "net-console",
                    "[virtio-net][tx-slot] cancel failed: id={} err={:?} context={} state_inflight={} free={}",
                    id,
                    err,
                    context,
                    self.tx_slots.in_flight(),
                    self.tx_slots.free_count(),
                );
            }
        }
    }

    #[allow(dead_code)]
    fn mark_slot_inflight(&mut self, id: u16, context: &'static str) {
        if !NET_VIRTIO_TX_V2 {
            return;
        }
        if let Err(err) = self.tx_slots.mark_in_flight(id) {
            if !self.tx_slot_divergence_logged {
                self.tx_slot_divergence_logged = true;
                warn!(
                    target: "net-console",
                    "[virtio-net][tx-slot] inflight mark failed: id={} err={:?} context={} free={} inflight={}",
                    id,
                    err,
                    context,
                    self.tx_slots.free_count(),
                    self.tx_slots.in_flight(),
                );
            }
        }
    }

    #[allow(dead_code)]
    fn complete_tx_slot(&mut self, id: u16, context: &'static str) {
        if !NET_VIRTIO_TX_V2 {
            return;
        }
        if let Err(err) = self.tx_slots.complete(id) {
            if !self.tx_slot_divergence_logged {
                self.tx_slot_divergence_logged = true;
                warn!(
                    target: "net-console",
                    "[virtio-net][tx-slot] complete failed: id={} err={:?} context={} free={} inflight={}",
                    id,
                    err,
                    context,
                    self.tx_slots.free_count(),
                    self.tx_slots.in_flight(),
                );
            }
        }
    }

    fn snapshot_tx_stats(&self) -> TxStatsSnapshot {
        TxStatsSnapshot {
            enqueue_ok: self.tx_stats.enqueue_ok.load(AtomicOrdering::Relaxed),
            enqueue_would_block: self
                .tx_stats
                .enqueue_would_block
                .load(AtomicOrdering::Relaxed),
            kick_count: self.tx_stats.kick_count.load(AtomicOrdering::Relaxed),
            reclaim_calls_irq: self
                .tx_stats
                .reclaim_calls_irq
                .load(AtomicOrdering::Relaxed),
            reclaim_calls_poll: self
                .tx_stats
                .reclaim_calls_poll
                .load(AtomicOrdering::Relaxed),
            used_reaped: self.tx_stats.used_reaped.load(AtomicOrdering::Relaxed),
            inflight_highwater: self
                .tx_stats
                .inflight_highwater
                .load(AtomicOrdering::Relaxed),
            ring_full_events: self.tx_stats.ring_full_events.load(AtomicOrdering::Relaxed),
            irq_count: self.tx_stats.irq_count.load(AtomicOrdering::Relaxed),
        }
    }

    fn check_tx_contradictions(
        &mut self,
        now_ms: u64,
        avail_idx: u16,
        used_idx: u16,
        snapshot: &TxStatsSnapshot,
        inflight: u16,
    ) {
        if now_ms.saturating_sub(self.tx_diag.last_warn_ms) < TX_WARN_COOLDOWN_MS {
            self.tx_diag.last_avail_idx = avail_idx;
            self.tx_diag.last_used_idx = used_idx;
            self.tx_diag.last_inflight = inflight;
            self.tx_diag.last_irq_count = snapshot.irq_count;
            self.tx_diag.last_would_block = snapshot.enqueue_would_block;
            self.tx_diag.last_kick_count = snapshot.kick_count;
            self.tx_diag.last_enqueue_ok = snapshot.enqueue_ok;
            return;
        }
        let would_block_delta = snapshot
            .enqueue_would_block
            .saturating_sub(self.tx_diag.last_would_block);
        let irq_delta = snapshot
            .irq_count
            .saturating_sub(self.tx_diag.last_irq_count);
        if would_block_delta > 0 && used_idx == self.tx_diag.last_used_idx && irq_delta > 0 {
            warn!(
                target: "virtio-net",
                "[virtio-net][tx-warn] IRQs rising but used.idx stalled: avail_idx={} used_idx={} inflight={} kicks={} would_block={} irq_delta={}",
                avail_idx,
                used_idx,
                inflight,
                snapshot.kick_count,
                snapshot.enqueue_would_block,
                irq_delta
            );
            self.tx_diag.last_warn_ms = now_ms;
        }
        if used_idx != self.tx_diag.last_used_idx && inflight >= self.tx_diag.last_inflight {
            warn!(
                target: "virtio-net",
                "[virtio-net][tx-warn] used advanced without inflight drop: used_idx_prev={} used_idx_now={} inflight_prev={} inflight_now={}",
                self.tx_diag.last_used_idx,
                used_idx,
                self.tx_diag.last_inflight,
                inflight
            );
            self.tx_diag.last_warn_ms = now_ms;
        }
        let kick_delta = snapshot
            .kick_count
            .saturating_sub(self.tx_diag.last_kick_count);
        let enqueue_delta = snapshot
            .enqueue_ok
            .saturating_sub(self.tx_diag.last_enqueue_ok);
        if enqueue_delta > 0 && kick_delta >= enqueue_delta {
            warn!(
                target: "virtio-net",
                "[virtio-net][tx-warn] excessive kicks: kicks_delta={} enqueue_delta={} avail_idx={} used_idx={}",
                kick_delta,
                enqueue_delta,
                avail_idx,
                used_idx
            );
            self.tx_diag.last_warn_ms = now_ms;
        }
        self.tx_diag.last_avail_idx = avail_idx;
        self.tx_diag.last_used_idx = used_idx;
        self.tx_diag.last_inflight = inflight;
        self.tx_diag.last_irq_count = snapshot.irq_count;
        self.tx_diag.last_would_block = snapshot.enqueue_would_block;
        self.tx_diag.last_kick_count = snapshot.kick_count;
        self.tx_diag.last_enqueue_ok = snapshot.enqueue_ok;
    }

    fn log_tx_stats_snapshot(&mut self) {
        let now_ms = crate::hal::timebase().now_ms();
        if now_ms.saturating_sub(self.tx_stats_log_ms) < TX_STATS_LOG_MS {
            return;
        }
        self.tx_stats_log_ms = now_ms;
        let snapshot = self.snapshot_tx_stats();
        let inflight = self.tx_inflight_count();
        let free = self.tx_free_count();
        let (used_idx, avail_idx) = self.tx_queue.indices_no_sync();
        info!(
            target: "virtio-net",
            "[virtio-net][tx-stats] inflight={} free={} highwater={} enqueue_ok={} would_block={} kicks={} used_reaped={} reclaim_irq={} reclaim_poll={} ring_full={} irq={} avail_idx={} used_idx={}",
            inflight,
            free,
            snapshot.inflight_highwater,
            snapshot.enqueue_ok,
            snapshot.enqueue_would_block,
            snapshot.kick_count,
            snapshot.used_reaped,
            snapshot.reclaim_calls_irq,
            snapshot.reclaim_calls_poll,
            snapshot.ring_full_events,
            snapshot.irq_count,
            avail_idx,
            used_idx
        );
        self.check_tx_contradictions(now_ms, avail_idx, used_idx, &snapshot, inflight);
    }

    fn should_kick_after_publish(&mut self, packet_bytes: usize) -> bool {
        let prev_pending = self.tx_pending_since_kick;
        let prev_bytes = self.tx_bytes_since_kick;
        let new_pending = prev_pending.saturating_add(1);
        let new_bytes = prev_bytes.saturating_add(packet_bytes);
        self.tx_pending_since_kick = new_pending;
        self.tx_bytes_since_kick = new_bytes;
        prev_pending == 0
            || new_pending >= TX_NOTIFY_BATCH_PACKETS
            || new_bytes >= TX_NOTIFY_BATCH_BYTES
    }

    fn note_tx_kick(&mut self) {
        self.tx_pending_since_kick = 0;
        self.tx_bytes_since_kick = 0;
        self.tx_stats.record_kick();
    }

    fn submit_tx(&mut self, id: u16, len: usize) {
        if NET_VIRTIO_TX_V2 {
            self.submit_tx_v2(id, len);
            return;
        }
        self.verify_tx_canary("tx_submit_v1");
        if self.tx_publish_blocked() {
            self.release_tx_head(id, "tx_publish_blocked");
            return;
        }
        self.check_device_health();
        if self.device_faulted {
            self.release_tx_head(id, "tx_device_faulted");
            return;
        }
        if forensics_frozen() {
            self.release_tx_head(id, "tx_forensics_frozen");
            return;
        }
        if let Some((length, addr)) = self
            .tx_buffers
            .get(id as usize)
            .map(|buffer| (len.min(buffer.as_slice().len()), buffer.paddr()))
        {
            if let Some(buffer) = self.tx_buffers.get(id as usize) {
                let vaddr = buffer.ptr().as_ptr() as usize;
                log_dma_programming("virtq.tx.buffer", vaddr, addr, length);
                assert_dma_region("virtq.tx.buffer", vaddr, addr, length);
            }
            let desc = [DescSpec {
                addr: addr as u64,
                len: length as u32,
                flags: 0,
                next: None,
            }];

            if self
                .enqueue_tx_chain_checked(id, &desc, Some(length), true)
                .is_err()
            {
                self.tx_drops = self.tx_drops.saturating_add(1);
                self.release_tx_head(id, "tx_enqueue_failed");
                return;
            }
            NET_DIAG.record_tx_submit();
            if let Some(buffer) = self.tx_buffers.get_mut(id as usize) {
                let slice = buffer.as_mut_slice();
                for byte in &mut slice[length..] {
                    *byte = 0;
                }
            }
            if !self.tx_post_logged {
                info!(
                    target: "net-console",
                    "[virtio-net] TX descriptor posted: id={} len={}",
                    id,
                    length
                );
                self.tx_post_logged = true;
            }
        }
    }

    fn submit_tx_v2(&mut self, id: u16, len: usize) {
        self.verify_tx_canary("tx_submit");
        self.check_device_health();
        if self.device_faulted {
            self.release_tx_head(id, "tx_v2_device_faulted");
            return;
        }
        if forensics_frozen() {
            self.release_tx_head(id, "tx_v2_forensics_frozen");
            return;
        }
        if self.tx_publish_blocked() {
            self.release_tx_head(id, "tx_v2_publish_blocked");
            return;
        }
        if len == 0 {
            self.tx_zero_len_attempt = self.tx_zero_len_attempt.wrapping_add(1);
            debug_assert!(len != 0, "tx-v2 zero-length submit");
            self.release_tx_head(id, "tx_v2_zero_len");
            return;
        }
        if id >= self.tx_queue.size {
            self.last_error.get_or_insert("tx_v2_id_oob");
            self.release_tx_head(id, "tx_v2_id_oob");
            return;
        }
        let Some(buffer) = self.tx_buffers.get_mut(id as usize) else {
            self.last_error.get_or_insert("tx_v2_buffer_missing");
            self.release_tx_head(id, "tx_v2_buffer_missing");
            return;
        };
        let buffer_len = buffer.as_slice().len();
        let capped_len = len.min(buffer_len);
        let addr = buffer.paddr() as u64;
        debug_assert_ne!(addr, 0, "tx-v2 descriptor address must be non-zero");
        debug_assert!(capped_len > 0, "tx-v2 descriptor length must be non-zero");
        if capped_len == 0 {
            self.tx_zero_len_attempt = self.tx_zero_len_attempt.wrapping_add(1);
            debug_assert!(capped_len != 0, "tx-v2 zero-length buffer");
            self.release_tx_head(id, "tx_v2_capped_zero");
            return;
        }
        let vaddr = buffer.ptr().as_ptr() as usize;
        log_dma_programming("virtq.tx.buffer", vaddr, addr as usize, capped_len);
        assert_dma_region("virtq.tx.buffer", vaddr, addr as usize, capped_len);

        if cfg!(debug_assertions) {
            let start = addr;
            let end = start.saturating_add(buffer_len as u64);
            for (idx, other) in self.tx_buffers.iter().enumerate() {
                if idx == id as usize {
                    continue;
                }
                let other_start = other.paddr() as u64;
                let other_end = other_start.saturating_add(other.as_slice().len() as u64);
                assert!(
                    end <= other_start || start >= other_end,
                    "tx-v2 buffer overlap id={id} other={idx}"
                );
            }
        }

        if self.ensure_tx_head_prepared(id).is_err() {
            self.tx_double_submit = self.tx_double_submit.wrapping_add(1);
            self.tx_drops = self.tx_drops.saturating_add(1);
            return;
        }
        if !matches!(
            self.tx_head_mgr.state(id),
            Some(TxHeadState::Prepared { .. })
        ) && !self.tx_desc_clear_violation_logged
        {
            // Clear-on-prepare only: descriptors stay intact after completion so late device reads
            // never observe a zeroed entry. If we ever clear outside the prepare phase, log it.
            self.tx_desc_clear_violation_logged = true;
            warn!(
                target: "net-console",
                "[virtio-net][tx] descriptor clear while state={:?} head={} gen={}",
                self.tx_head_mgr.state(id),
                id,
                self.tx_head_mgr.generation(id).unwrap_or(0),
            );
        }
        if self
            .tx_queue
            .setup_descriptor(id, addr, capped_len as u32, 0, None)
            .is_err()
        {
            self.device_faulted = true;
            self.last_error
                .get_or_insert("tx_v2_desc_cache_clean_failed");
            self.freeze_and_capture("tx_v2_desc_cache_clean_failed");
            self.release_tx_head(id, "tx_v2_desc_cache_clean_failed");
            return;
        }
        if self.tx_queue.clean_desc_entry_for_device(id).is_err() {
            self.freeze_and_capture("tx_v2_desc_sync_failed");
            self.release_tx_head(id, "tx_v2_desc_sync_failed");
            return;
        }

        let (_, avail_idx_before) = self.tx_queue.indices_no_sync();
        let qsize = usize::from(self.tx_queue.size);
        if qsize == 0 {
            self.release_tx_head(id, "tx_v2_qsize_zero");
            return;
        }
        let publish_slot = (avail_idx_before as usize % qsize) as u16;

        // Root cause (forensics): after wrap the avail ring reused a slot whose descriptor entry
        // had been zeroed, letting the device observe {addr=0,len=0} and wedge the queue. The
        // guard below validates the descriptor against the slot-owned DMA buffer before the avail
        // write so a stale entry is rejected and released instead of being exposed to the device.
        let desc = self.tx_queue.read_descriptor(id);
        let total_len = desc.len as usize;
        debug_assert_ne!(total_len, 0, "tx-v2 descriptor length must be non-zero");
        debug_assert_ne!(desc.addr, 0, "tx-v2 descriptor address must be non-zero");
        if desc.addr == 0 || desc.len == 0 {
            self.note_zero_desc_guard(id, publish_slot, &desc);
            self.release_tx_head(id, "tx_v2_desc_guard_zero");
            return;
        }
        if let Some(buffer) = self.tx_buffers.get(id as usize) {
            if desc.addr != buffer.paddr() as u64 {
                self.tx_invariant_violations = self.tx_invariant_violations.saturating_add(1);
                warn!(
                    target: "net-console",
                    "[virtio-net][tx-guard] descriptor addr mismatch: head={head} slot={slot} desc_addr=0x{addr:016x} expected=0x{expected:016x} len={len}",
                    head = id,
                    slot = publish_slot,
                    addr = desc.addr,
                    expected = buffer.paddr(),
                    len = desc.len,
                );
                #[cfg(debug_assertions)]
                self.freeze_tx_publishes("tx_desc_addr_mismatch");
                self.release_tx_head(id, "tx_v2_desc_addr_mismatch");
                return;
            }
        }
        if total_len != capped_len {
            self.device_faulted = true;
            self.last_error.get_or_insert("tx_v2_desc_guard");
            self.freeze_and_capture("tx_v2_desc_guard");
            self.release_tx_head(id, "tx_v2_desc_guard");
            return;
        }

        if self
            .clean_tx_buffer_for_device(id, capped_len, self.tx_anomaly_logged)
            .is_none()
        {
            self.release_tx_head(id, "tx_v2_cache_clean_failed");
            return;
        }
        if !matches!(
            self.tx_head_mgr.state(id),
            Some(TxHeadState::Prepared { .. })
        ) {
            self.tx_dup_publish_blocked = self.tx_dup_publish_blocked.saturating_add(1);
            #[cfg(debug_assertions)]
            self.freeze_tx_publishes("tx_v2_publish_state_violation");
            return;
        }
        self.tx_publish_calls = self.tx_publish_calls.wrapping_add(1);
        if self
            .validate_tx_publish_guard(
                id,
                &[DescSpec {
                    addr: desc.addr,
                    len: desc.len,
                    flags: desc.flags,
                    next: None,
                }],
                self.tx_header_len,
                avail_idx_before,
            )
            .is_err()
        {
            self.release_tx_head(id, "tx_v2_guard_reject");
            return;
        }
        let payload_len = total_len.saturating_sub(self.tx_header_len);
        if self
            .tx_publish_preflight(
                id,
                publish_slot,
                &DescSpec {
                    addr: desc.addr,
                    len: desc.len,
                    flags: desc.flags,
                    next: None,
                },
                payload_len,
            )
            .is_err()
        {
            self.tx_drops = self.tx_drops.saturating_add(1);
            self.release_tx_head(id, "tx_v2_preflight_reject");
            return;
        }
        trace!(
            target: "net-console",
            "[virtio-net][tx-submit-path=B] head={} slot={} avail_idx_before={}",
            id,
            publish_slot,
            avail_idx_before
        );
        if self
            .guard_tx_post_state(
                id,
                publish_slot,
                &DescSpec {
                    addr: desc.addr,
                    len: desc.len,
                    flags: desc.flags,
                    next: None,
                },
            )
            .is_err()
        {
            return;
        }
        if self
            .tx_pre_publish_tripwire(
                id,
                publish_slot,
                &DescSpec {
                    addr: desc.addr,
                    len: desc.len,
                    flags: desc.flags,
                    next: None,
                },
                self.tx_header_len,
                payload_len,
            )
            .is_err()
        {
            return;
        }
        // Ensure descriptor writes and cache maintenance complete before exposing the head to the
        // device via the avail ring.
        virtq_publish_barrier();
        #[cfg(debug_assertions)]
        self.debug_assert_tx_publish_ready(id, publish_slot);
        if self
            .assert_tx_desc_len_nonzero(id, "tx_v2_desc_len_zero_before_avail")
            .is_err()
        {
            return;
        }
        virtq_publish_barrier();
        match self.publish_tx_avail(id, publish_slot) {
            Ok((slot, avail_idx, old_idx)) => {
                if slot != publish_slot {
                    let _ = self.tx_head_mgr.cancel_publish(id);
                    self.tx_state_violation("tx_v2_slot_mismatch", id, Some(slot))
                        .ok();
                    return;
                }
                self.tx_wrap_tripwire(old_idx, avail_idx, slot, id);
                let inflight_now = self.tx_inflight_count();
                self.tx_stats.record_enqueue_ok(inflight_now);
                let free_now = self.tx_free_count();
                let used_idx_now = self.tx_queue.indices_no_sync().0;
                if self
                    .guard_tx_publish_readback(
                        slot,
                        id,
                        &DescSpec {
                            addr: desc.addr,
                            len: desc.len,
                            flags: desc.flags,
                            next: None,
                        },
                    )
                    .is_err()
                {
                    self.release_tx_head(id, "tx_v2_publish_readback_mismatch");
                    return;
                }
                let desc_snapshot = self.tx_queue.read_descriptor(id);
                debug!(
                    target: "virtio-net",
                    "[virtio-net][tx-publish-forensics] head={} old_avail={} slot={} desc=0x{addr:016x}/{len} flags=0x{flags:04x} next={next}",
                    id,
                    old_idx,
                    slot,
                    addr = desc_snapshot.addr,
                    len = desc_snapshot.len,
                    flags = desc_snapshot.flags,
                    next = desc_snapshot.next
                );
                self.debug_check_tx_avail_uniqueness(self.tx_queue.last_used, avail_idx);
                self.debug_check_tx_outstanding_window(self.tx_queue.last_used, avail_idx);
                log::debug!(
                    target: "virtio-net",
                    "[virtio-net][tx-submit] head={head} paddr=0x{addr:016x} len={len} avail_idx={old_idx}→{avail_idx} slot={slot} used_idx={used_idx_now} free={free_now} inflight={inflight_now}",
                    head = id,
                    addr = desc.addr,
                    len = desc.len,
                    old_idx = old_idx,
                    avail_idx = avail_idx,
                    slot = slot,
                    used_idx_now = used_idx_now,
                    free_now = free_now,
                    inflight_now = inflight_now
                );
                #[cfg(debug_assertions)]
                self.debug_assert_tx_chain_ready(id);
                self.tx_submit = self.tx_submit.wrapping_add(1);
                #[cfg(debug_assertions)]
                self.tx_assert_invariants("publish");
                NET_DIAG.record_tx_submit();
                let qsize = usize::from(self.tx_queue.size);
                let wrap_boundary =
                    qsize != 0 && (old_idx as usize % qsize) > (avail_idx as usize % qsize);
                if wrap_boundary && !self.tx_wrap_logged {
                    self.tx_wrap_logged = true;
                    info!(
                        target: "virtio-net",
                        "[virtio-net][tx-wrap] v2 avail_idx {}->{} slot={} head={} wrap_detected={} qsize={} desc_len={desc_len} desc_addr=0x{desc_addr:016x} avail_head={avail_head}",
                        old_idx,
                        avail_idx,
                        slot,
                        id,
                        wrap_boundary,
                        qsize,
                        desc_len = desc.len,
                        desc_addr = desc.addr,
                        avail_head = self.tx_queue.read_avail_slot(slot as usize),
                    );
                }
                if VIRTIO_DMA_TRACE {
                    let desc_snapshot = self.tx_queue.read_descriptor(id);
                    let avail_slot_val = self.tx_queue.read_avail_slot(slot as usize);
                    let avail_idx_now = self.tx_queue.read_avail_idx();
                    debug!(
                        target: "virtio-net",
                        "[virtio-net][tx-trace] v2 pre-kick head={head_id} slot={slot} desc=0x{addr:016x}/{len} flags=0x{flags:04x} next={next} avail_slot={avail_slot} avail_idx_now={avail_idx_now} old_idx={old_idx}",
                        head_id = id,
                        slot = slot,
                        addr = desc_snapshot.addr,
                        len = desc_snapshot.len,
                        flags = desc_snapshot.flags,
                        next = desc_snapshot.next,
                        avail_slot = avail_slot_val,
                        avail_idx_now = avail_idx_now,
                        old_idx = old_idx,
                    );
                    debug_assert!(
                        desc_snapshot.len != 0 && desc_snapshot.addr != 0,
                        "tx-trace: v2 descriptor must be initialised before kick"
                    );
                }
                if self.should_kick_after_publish(total_len) {
                    if self
                        .tx_queue
                        .notify(&mut self.regs, TX_QUEUE_INDEX)
                        .is_err()
                    {
                        self.freeze_and_capture("tx_v2_notify_failed");
                        self.release_tx_head(id, "tx_v2_notify_failed");
                        return;
                    }
                    self.note_tx_kick();
                }
            }
            Err(TxPublishError::InvalidDescriptor) => {
                self.tx_drops = self.tx_drops.saturating_add(1);
                let _ = self.tx_head_mgr.cancel_publish(id);
                self.cancel_tx_slot(id, "tx_publish_invalid_descriptor");
                return;
            }
            Err(TxPublishError::Queue(_)) => {
                self.device_faulted = true;
                self.last_error.get_or_insert("tx_v2_avail_write_failed");
                self.release_tx_head(id, "tx_v2_avail_write_failed");
            }
        }
    }

    fn clean_tx_buffer_for_device(
        &mut self,
        head_id: u16,
        len: usize,
        force_log: bool,
    ) -> Option<(usize, usize)> {
        let (ptr, capped_len, start) = {
            let buffer = match self.tx_buffers.get(head_id as usize) {
                Some(buffer) => buffer,
                None => return None,
            };
            let capped_len = len.min(buffer.as_slice().len());
            let buffer_ptr = buffer.ptr();
            let start = buffer_ptr.as_ptr() as usize;
            (buffer_ptr.as_ptr(), capped_len, start)
        };

        if dma_clean(ptr, capped_len, self.dma_cacheable, "clean tx buffer").is_err() {
            warn!(
                target: "net-console",
                "[virtio-net] tx cache clean failed head={} len={}",
                head_id,
                capped_len,
            );
            self.freeze_and_capture("tx_cache_clean_failed");
            return None;
        }

        let payload_start = start.saturating_add(self.tx_header_len);
        let payload_end =
            payload_start.saturating_add(capped_len.saturating_sub(self.tx_header_len));
        if force_log || !self.tx_dma_log_once {
            self.tx_dma_log_once = true;
            info!(
                target: "virtio-net",
                "[virtio-net][dma] tx buffer clean head={} header=0x{start:016x}..0x{hdr_end:016x} payload=0x{payload_start:016x}..0x{payload_end:016x}",
                head_id,
                hdr_end = start.saturating_add(self.tx_header_len),
                payload_start = payload_start,
                payload_end = payload_end,
            );
        }
        Some((start, start.saturating_add(capped_len)))
    }

    fn log_tx_dma_ranges(
        &mut self,
        head_id: u16,
        total_len: usize,
        header_len: usize,
        buffer_range: (usize, usize),
        descs: &[DescSpec],
        force: bool,
    ) {
        if !force && self.tx_dma_log_once {
            return;
        }
        self.tx_dma_log_once = true;
        let payload_start = buffer_range.0.saturating_add(header_len);
        let payload_end = buffer_range.1;
        let desc_start = self.tx_queue.base_paddr + self.tx_queue.layout.desc_offset;
        let avail_start = self.tx_queue.base_paddr + self.tx_queue.layout.avail_offset;
        info!(
            target: "virtio-net",
            "[virtio-net][dma] tx ranges head={} total_len={} header_len={} buffer=0x{buf_start:016x}..0x{buf_end:016x} payload=0x{payload_start:016x}..0x{payload_end:016x} desc=0x{desc_start:016x}/{desc_len} avail=0x{avail_start:016x}/{avail_len}",
            head_id,
            total_len,
            header_len,
            buf_start = buffer_range.0,
            buf_end = buffer_range.1,
            payload_start = payload_start,
            payload_end = payload_end,
            desc_len = self.tx_queue.layout.desc_len,
            avail_len = self.tx_queue.layout.avail_len,
        );
        for (idx, desc) in descs.iter().enumerate() {
            info!(
                target: "virtio-net",
                "[virtio-net][dma] desc[{idx}] addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next:?}",
                addr = desc.addr,
                len = desc.len,
                flags = desc.flags,
                next = desc.next,
            );
        }
    }

    fn log_tx_descriptor_readback(&mut self, head_id: u16, expected_chain: &[DescSpec]) {
        if !(cfg!(debug_assertions) || self.tx_anomaly_logged) {
            return;
        }
        let mut mismatch = false;
        for (idx, expected) in expected_chain.iter().enumerate() {
            let desc_index = head_id.wrapping_add(idx as u16);
            let actual = self.tx_queue.read_descriptor(desc_index);
            if actual.addr != expected.addr
                || actual.len != expected.len
                || actual.flags != expected.flags
            {
                mismatch = true;
            }
            debug!(
                target: "virtio-net",
                "[virtio-net][dma-readback] head={} desc[{}] expected=0x{exp_addr:016x}/{exp_len}/0x{exp_flags:04x}/{exp_next:?} actual=0x{act_addr:016x}/{act_len}/0x{act_flags:04x}/{act_next}",
                head_id,
                desc_index,
                exp_addr = expected.addr,
                exp_len = expected.len,
                exp_flags = expected.flags,
                exp_next = expected.next,
                act_addr = actual.addr,
                act_len = actual.len,
                act_flags = actual.flags,
                act_next = actual.next,
            );
        }
        if mismatch {
            self.tx_anomaly(
                TxAnomalyReason::DmaReadbackMismatch,
                "tx_desc_readback_mismatch",
            );
        }
    }

    #[cfg(debug_assertions)]
    fn debug_trace_tx_publish_state(&self, head_id: u16, slot: u16, stage: &str) {
        debug!(
            target: "virtio-net",
            "[virtio-net][tx-publish-trace] head={} slot={} stage={} state={:?} in_avail={}",
            head_id,
            slot,
            stage,
            self.tx_head_mgr.state(head_id),
            self.tx_head_mgr.in_avail(head_id)
        );
    }

    #[cfg(debug_assertions)]
    fn debug_assert_tx_publish_ready(&self, head_id: u16, slot: u16) {
        let state = self.tx_head_mgr.state(head_id);
        debug_assert!(
            matches!(state, Some(TxHeadState::Published { slot: s, .. } | TxHeadState::InFlight { slot: s, .. }) if s == slot),
            "tx publish state mismatch: head={} slot={} state={state:?}",
            head_id,
            slot,
        );
        debug_assert!(
            self.tx_head_mgr.in_avail(head_id),
            "tx head not marked in avail before publish: head={} slot={}",
            head_id,
            slot
        );
        let desc = self.tx_queue.read_descriptor(head_id);
        debug_assert_ne!(
            desc.addr, 0,
            "tx publish descriptor addr zero: head={} slot={}",
            head_id, slot
        );
        debug_assert_ne!(
            desc.len, 0,
            "tx publish descriptor len zero: head={} slot={}",
            head_id, slot
        );
    }

    #[cfg(debug_assertions)]
    fn debug_assert_tx_chain_ready(&self, head_id: u16) {
        let mut idx = head_id;
        for depth in 0..TX_QUEUE_SIZE {
            let desc = self.tx_queue.read_descriptor(idx);
            debug_assert!(
                desc.addr != 0,
                "tx chain descriptor addr cleared unexpectedly: head={} idx={} depth={}",
                head_id,
                idx,
                depth
            );
            debug_assert!(
                desc.len != 0,
                "tx chain descriptor len cleared unexpectedly: head={} idx={} depth={}",
                head_id,
                idx,
                depth
            );
            if desc.flags & VIRTQ_DESC_F_NEXT == 0 {
                break;
            }
            debug_assert!(
                desc.next < self.tx_queue.size,
                "tx chain next pointer out of range: head={} idx={} depth={} next={}",
                head_id,
                idx,
                depth,
                desc.next
            );
            idx = desc.next;
        }
    }

    #[cfg(debug_assertions)]
    fn tx_assert_invariants(&mut self, context: &'static str) {
        let (used_idx, avail_idx) = self.tx_queue.indices_no_sync();
        let pending = avail_idx.wrapping_sub(used_idx);
        let window = core::cmp::min(pending, self.tx_queue.size);
        let qsize = usize::from(self.tx_queue.size);
        let mut seen: [bool; TX_QUEUE_SIZE] = [false; TX_QUEUE_SIZE];
        for offset in 0..usize::from(window) {
            let ring_slot = (used_idx.wrapping_add(offset as u16) as usize) % qsize;
            let head = self.tx_queue.read_avail_slot(ring_slot);
            debug_assert!(
                (head as usize) < TX_QUEUE_SIZE && head < self.tx_queue.size,
                "tx invariant: head out of range context={} head={} ring_slot={} used_idx={} avail_idx={}",
                context,
                head,
                ring_slot,
                used_idx,
                avail_idx
            );
            if (head as usize) < seen.len() {
                debug_assert!(
                    !seen[head as usize],
                    "tx invariant: duplicate head context={} head={} ring_slot={} used_idx={} avail_idx={}",
                    context,
                    head,
                    ring_slot,
                    used_idx,
                    avail_idx
                );
                seen[head as usize] = true;
            }
            let desc = self.tx_queue.read_descriptor(head);
            debug_assert!(
                desc.len != 0 && desc.addr != 0,
                "tx invariant: zero descriptor context={context} head={head} ring_slot={ring_slot} desc_len={desc_len} desc_addr=0x{addr:016x} used_idx={used_idx} avail_idx={avail_idx}",
                context = context,
                head = head,
                ring_slot = ring_slot,
                desc_len = desc.len,
                addr = desc.addr,
                used_idx = used_idx,
                avail_idx = avail_idx
            );
            if let Some(buffer) = self.tx_buffers.get(head as usize) {
                debug_assert!(
                    desc.addr == buffer.paddr() as u64,
                    "tx invariant: descriptor addr mismatch context={context} head={head} ring_slot={ring_slot} desc_addr=0x{addr:016x} expected=0x{expected:016x} used_idx={used_idx} avail_idx={avail_idx}",
                    context = context,
                    head = head,
                    ring_slot = ring_slot,
                    addr = desc.addr,
                    expected = buffer.paddr(),
                    used_idx = used_idx,
                    avail_idx = avail_idx
                );
            }
        }
        if NET_VIRTIO_TX_V2 {
            let tracker_sum = self
                .tx_slots
                .free_count()
                .saturating_add(self.tx_slots.in_flight());
            let mgr_sum = self
                .tx_head_mgr
                .free_len()
                .saturating_add(self.tx_head_mgr.in_flight_count());
            debug_assert!(
                tracker_sum == self.tx_queue.size,
                "tx invariant: tracker accounting mismatch context={} free={} inflight={} qsize={}",
                context,
                self.tx_slots.free_count(),
                self.tx_slots.in_flight(),
                self.tx_queue.size
            );
            debug_assert!(
                mgr_sum == self.tx_queue.size,
                "tx invariant: head manager accounting mismatch context={} free={} inflight={} qsize={}",
                context,
                self.tx_head_mgr.free_len(),
                self.tx_head_mgr.in_flight_count(),
                self.tx_queue.size
            );
            debug_assert!(
                self.tx_slots.free_count() == self.tx_head_mgr.free_len()
                    && self.tx_slots.in_flight() == self.tx_head_mgr.in_flight_count(),
                "tx invariant: tracker/head divergence context={} tracker_free={} tracker_inflight={} mgr_free={} mgr_inflight={}",
                context,
                self.tx_slots.free_count(),
                self.tx_slots.in_flight(),
                self.tx_head_mgr.free_len(),
                self.tx_head_mgr.in_flight_count(),
            );
        }
    }

    #[cfg(not(debug_assertions))]
    fn tx_assert_invariants(&mut self, _context: &'static str) {}

    fn inspect_tx_header(&self, head_id: u16, header_len: usize) -> Option<TxHeaderInspect> {
        self.tx_buffers.get(head_id as usize).and_then(|buffer| {
            let slice = buffer.as_slice();
            if slice.len() < header_len || header_len < VIRTIO_NET_HEADER_LEN_BASIC {
                return None;
            }
            let hdr = if header_len >= VIRTIO_NET_HEADER_LEN_MRG {
                let hdr = unsafe { read_unaligned(slice.as_ptr() as *const VirtioNetHdrMrgRxbuf) };
                hdr.hdr
            } else {
                unsafe { read_unaligned(slice.as_ptr() as *const VirtioNetHdr) }
            };
            Some(TxHeaderInspect {
                flags: hdr.flags,
                gso_type: hdr.gso_type,
                hdr_len: hdr.hdr_len,
                csum_start: hdr.csum_start,
                csum_offset: hdr.csum_offset,
            })
        })
    }

    fn log_tx_chain_publish(
        &mut self,
        head_id: u16,
        slot: u16,
        old_idx: u16,
        avail_idx: u16,
        header_len: usize,
        payload_len: usize,
        payload_overlaps: bool,
        header_fields: Option<TxHeaderInspect>,
        descs: &[DescSpec],
    ) {
        if self.tx_publish_log_count >= FORENSICS_PUBLISH_LOG_LIMIT && !self.tx_anomaly_logged {
            return;
        }
        self.tx_publish_log_count = self.tx_publish_log_count.wrapping_add(1);
        let total_len = descs.get(0).map(|d| d.len).unwrap_or(0);
        info!(
            target: "virtio-net",
            "[virtio-net][tx-publish] head={} slot={} idx {}->{} total_len={} header_len={} payload_len={} overlap={}",
            head_id,
            slot,
            old_idx,
            avail_idx,
            total_len,
            header_len,
            payload_len,
            payload_overlaps,
        );
        if let Some(fields) = header_fields {
            info!(
                target: "virtio-net",
                "[virtio-net][tx-publish] header flags=0x{:02x} gso_type=0x{:02x} hdr_len={} csum_start={} csum_offset={}",
                fields.flags,
                fields.gso_type,
                fields.hdr_len,
                fields.csum_start,
                fields.csum_offset,
            );
        }
        for (idx, desc) in descs.iter().enumerate() {
            info!(
                target: "virtio-net",
                "[virtio-net][tx-publish] desc[{idx}] addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next:?}",
                addr = desc.addr,
                len = desc.len,
                flags = desc.flags,
                next = desc.next,
            );
        }
    }
}

fn format_ipv4(bytes: &[u8]) -> HeaplessString<16> {
    let mut out = HeaplessString::new();
    if bytes.len() >= 4 {
        let _ = FmtWrite::write_fmt(
            &mut out,
            format_args!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3]),
        );
    }
    out
}

fn tcp_flag_string(flags: u8) -> HeaplessString<8> {
    let mut out = HeaplessString::new();
    let push_flag = |buffer: &mut HeaplessString<8>, chr| {
        let _ = buffer.push(chr);
    };
    if flags & 0x02 != 0 {
        push_flag(&mut out, 'S');
    }
    if flags & 0x10 != 0 {
        push_flag(&mut out, 'A');
    }
    if flags & 0x01 != 0 {
        push_flag(&mut out, 'F');
    }
    if flags & 0x04 != 0 {
        push_flag(&mut out, 'R');
    }
    if flags & 0x08 != 0 {
        push_flag(&mut out, 'P');
    }
    if flags & 0x20 != 0 {
        push_flag(&mut out, 'U');
    }
    if flags & 0x40 != 0 {
        push_flag(&mut out, 'E');
    }
    if flags & 0x80 != 0 {
        push_flag(&mut out, 'C');
    }
    out
}

fn log_tcp_trace(direction: &str, frame: &[u8]) {
    const IPV4_ETHERTYPE: u16 = 0x0800;
    if frame.len() < 34 {
        return;
    }

    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    if ethertype != IPV4_ETHERTYPE {
        return;
    }

    let ip_start = 14;
    let ihl_words = usize::from(frame[ip_start] & 0x0f);
    if ihl_words < 5 {
        log::debug!("[net-trace] {direction} malformed ipv4 header len={ihl_words}");
        return;
    }
    let ip_header_len = ihl_words * 4;
    let total_len = u16::from_be_bytes([frame[ip_start + 2], frame[ip_start + 3]]) as usize;
    if frame.len() < ip_start + ip_header_len || total_len < ip_header_len {
        log::debug!(
            "[net-trace] {direction} malformed ipv4 total_len={total} header_len={ihl}",
            total = total_len,
            ihl = ip_header_len
        );
        return;
    }

    let protocol = frame[ip_start + 9];
    if protocol != 0x06 {
        return;
    }

    let tcp_offset = ip_start + ip_header_len;
    if frame.len() < tcp_offset + 20 {
        log::debug!("[net-trace] {direction} malformed tcp header (truncated)");
        return;
    }

    let src_port = u16::from_be_bytes([frame[tcp_offset], frame[tcp_offset + 1]]);
    let dst_port = u16::from_be_bytes([frame[tcp_offset + 2], frame[tcp_offset + 3]]);
    if src_port != CONSOLE_TCP_PORT && dst_port != CONSOLE_TCP_PORT {
        return;
    }

    let data_offset_words = usize::from(frame[tcp_offset + 12] >> 4);
    let tcp_header_len = data_offset_words * 4;
    if tcp_header_len < 20 || frame.len() < tcp_offset + tcp_header_len {
        log::debug!("[net-trace] {direction} malformed tcp data offset={data_offset_words}");
        return;
    }

    let payload_len = total_len
        .saturating_sub(ip_header_len + tcp_header_len)
        .min(frame.len().saturating_sub(tcp_offset + tcp_header_len));
    #[cfg(feature = "net-trace-31337")]
    let seq = u32::from_be_bytes([
        frame[tcp_offset + 4],
        frame[tcp_offset + 5],
        frame[tcp_offset + 6],
        frame[tcp_offset + 7],
    ]);
    #[cfg(feature = "net-trace-31337")]
    let ack = u32::from_be_bytes([
        frame[tcp_offset + 8],
        frame[tcp_offset + 9],
        frame[tcp_offset + 10],
        frame[tcp_offset + 11],
    ]);
    #[cfg(feature = "net-trace-31337")]
    let flags = frame[tcp_offset + 13];
    let src_ip = format_ipv4(&frame[ip_start + 12..ip_start + 16]);
    let dst_ip = format_ipv4(&frame[ip_start + 16..ip_start + 20]);

    log::info!(
        target: "net-trace",
        "[net-trace] {direction} tcp {}:{} -> {}:{} len={payload_len}",
        src_ip.as_str(),
        src_port,
        dst_ip.as_str(),
        dst_port,
    );

    #[cfg(feature = "net-trace-31337")]
    {
        let flag_str = tcp_flag_string(flags);
        log::debug!(
            target: "net-trace",
            "[net-trace] {direction} tcp flags={flags} seq={seq} ack={ack}",
            flags = flag_str.as_str(),
        );
    }
}

fn log_first_tcp_dest_port(frame: &[u8]) {
    const IPV4_ETHERTYPE: u16 = 0x0800;
    const TCP_PROTOCOL: u8 = 0x06;

    if frame.len() < 34 {
        return;
    }

    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    if ethertype != IPV4_ETHERTYPE {
        return;
    }

    let ip_start = 14;
    let ihl_words = usize::from(frame[ip_start] & 0x0f);
    let ip_header_len = ihl_words.saturating_mul(4);
    if ihl_words < 5 || frame.len() < ip_start + ip_header_len || ip_header_len < 20 {
        return;
    }

    if frame[ip_start + 9] != TCP_PROTOCOL {
        return;
    }

    let tcp_offset = ip_start + ip_header_len;
    if frame.len() < tcp_offset + 4 {
        return;
    }

    let dest_port = u16::from_be_bytes([frame[tcp_offset + 2], frame[tcp_offset + 3]]);
    log::info!(
        target: "net-console",
        "[virtio-net] debug: RX TCP dest_port={}",
        dest_port
    );
}

fn log_tcp_dest_port_once(frame: &[u8]) {
    if LOG_TCP_DEST_PORT.swap(false, AtomicOrdering::AcqRel) {
        log_first_tcp_dest_port(frame);
    }
}

impl Device for VirtioNet {
    type RxToken<'a>
        = VirtioRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = VirtioTxToken
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if self.stage == NetStage::TxOnly {
            return None;
        }
        self.rx_poll_count = self.rx_poll_count.wrapping_add(1);

        if self.rx_poll_count % 1024 == 0 {
            log::debug!(
                target: "net-console",
                "[virtio-net] receive() polled {} times (rx_used_count={})",
                self.rx_poll_count,
                self.rx_used_count,
            );
        }

        self.poll_interrupts();

        if self.device_faulted {
            return None;
        }

        let (used_idx, avail_idx) = self.rx_queue.indices();
        if used_idx != self.last_used_idx_debug {
            log::info!(
                target: "net-console",
                "[virtio-net] rx observed used.idx advance: prev={} now={} avail.idx={} rx_used_count={} rx_poll_count={} isr=0x{:02x} status=0x{:02x}",
                self.last_used_idx_debug,
                used_idx,
                avail_idx,
                self.rx_used_count,
                self.rx_poll_count,
                self.regs.isr_status(),
                self.regs.status()
            );
            self.last_used_idx_debug = used_idx;
        }
        log::debug!(
            target: "net-console",
            "[virtio-net] rx poll: avail.idx={} used.idx={} last_used={}",
            avail_idx,
            used_idx,
            self.rx_queue.last_used,
        );
        self.rx_queue.debug_descriptors("rx", 2);

        if let Some((id, len)) = self.pop_rx() {
            self.rx_used_count = self.rx_used_count.wrapping_add(1);
            self.note_progress();
            log::info!(
                target: "net-console",
                "[virtio-net] RX: used descriptor received: id={} len={} (rx_used_count={})",
                id,
                len,
                self.rx_used_count,
            );
            let (used_after, avail_after) = self.rx_queue.indices();
            log::debug!(
                target: "net-console",
                "[virtio-net] RX ring post-drain: avail.idx={} used.idx={} last_used={}",
                avail_after,
                used_after,
                self.rx_queue.last_used,
            );
            let driver_ptr = self as *mut _;
            let rx = VirtioRxToken {
                driver: driver_ptr,
                id,
                len,
            };
            let tx = self.prepare_tx_token();
            if !tx.has_id() {
                // Smoltcp expects a writable TX buffer when an RxToken is returned.
                self.requeue_rx(id, Some(len));
                return None;
            }
            NET_DIAG.record_rx_frame_to_stack();
            Some((rx, tx))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.stage == NetStage::RxOnly {
            return None;
        }
        self.poll_interrupts();
        if self.device_faulted {
            return None;
        }
        if self.tx_publish_blocked() {
            return None;
        }
        let token = self.prepare_tx_token();
        if token.has_id() {
            Some(token)
        } else {
            None
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = MAX_FRAME_LEN;
        caps.max_burst_size = Some(1);
        caps.medium = Medium::Ethernet;
        caps
    }
}

impl NetDevice for VirtioNet {
    type Error = DriverError;

    fn create<H>(hal: &mut H) -> Result<Self, Self::Error>
    where
        H: Hardware<Error = HalError>,
        Self: Sized,
    {
        Self::new(hal)
    }

    fn create_with_stage<H>(hal: &mut H, stage: NetStage) -> Result<Self, Self::Error>
    where
        H: Hardware<Error = HalError>,
        Self: Sized,
    {
        Self::new_with_stage(hal, stage)
    }

    fn mac(&self) -> EthernetAddress {
        self.mac
    }

    fn tx_drop_count(&self) -> u32 {
        self.tx_drops
    }

    fn debug_scan_tx_avail_duplicates(&mut self) {
        let (used_idx, avail_idx) = self.tx_queue.indices_no_sync();
        self.debug_check_tx_avail_uniqueness(used_idx, avail_idx);
    }

    fn counters(&self) -> NetDeviceCounters {
        let tx_free = self.tx_head_mgr.free_len() as u64;
        let tx_in_flight = self.tx_head_mgr.in_flight_count() as u64;
        let (tx_submit, tx_complete, tx_double_submit, tx_zero_len_attempt) = if NET_VIRTIO_TX_V2 {
            (
                self.tx_submit,
                self.tx_complete,
                self.tx_double_submit,
                self.tx_zero_len_attempt,
            )
        } else {
            (0, 0, 0, 0)
        };
        NetDeviceCounters {
            rx_packets: self.rx_packets,
            tx_packets: self.tx_packets,
            rx_used_advances: self.rx_used_count,
            tx_used_advances: self.tx_used_count,
            tx_submit,
            tx_complete,
            tx_free,
            tx_in_flight,
            tx_double_submit,
            tx_zero_len_attempt,
            dropped_zero_len_tx: self.dropped_zero_len_tx,
            tx_dup_publish_blocked: self.tx_dup_publish_blocked as u64,
            tx_dup_used_ignored: self.tx_dup_used_ignored as u64,
            tx_invalid_used_state: self.tx_invalid_used_state as u64,
            tx_alloc_blocked_inflight: self.tx_alloc_blocked_inflight as u64,
        }
    }

    fn name() -> &'static str
    where
        Self: Sized,
    {
        "virtio-net"
    }

    fn debug_snapshot(&mut self) {
        VirtioNet::debug_snapshot(self);
    }
}

#[cfg(feature = "net-backend-virtio")]
impl NetDevice for VirtioNetStatic {
    type Error = DriverError;

    fn create<H>(hal: &mut H) -> Result<Self, Self::Error>
    where
        H: Hardware<Error = HalError>,
        Self: Sized,
    {
        Self::create_with_stage(hal, NET_STAGE)
    }

    fn create_with_stage<H>(hal: &mut H, stage: NetStage) -> Result<Self, Self::Error>
    where
        H: Hardware<Error = HalError>,
        Self: Sized,
    {
        let driver = unsafe { VIRTIO_NET_STORAGE.write(VirtioNet::new_with_stage(hal, stage)?) };
        Ok(Self { driver })
    }

    fn mac(&self) -> EthernetAddress {
        self.driver.mac()
    }

    fn tx_drop_count(&self) -> u32 {
        self.driver.tx_drop_count()
    }

    fn debug_scan_tx_avail_duplicates(&mut self) {
        self.driver.debug_scan_tx_avail_duplicates();
    }

    fn counters(&self) -> NetDeviceCounters {
        self.driver.counters()
    }

    fn name() -> &'static str
    where
        Self: Sized,
    {
        VirtioNet::name()
    }

    fn debug_snapshot(&mut self) {
        self.driver.debug_snapshot();
    }

    fn buffer_bounds(&self) -> Option<Range<usize>> {
        let rx_start = self.driver.rx_queue.base_vaddr;
        let rx_end = rx_start.saturating_add(self.driver.rx_queue.base_len);
        let tx_start = self.driver.tx_queue.base_vaddr;
        let tx_end = tx_start.saturating_add(self.driver.tx_queue.base_len);
        let start = rx_start.min(tx_start);
        let end = rx_end.max(tx_end);
        Some(start..end)
    }
}

#[cfg(feature = "net-backend-virtio")]
impl Device for VirtioNetStatic {
    type RxToken<'a>
        = VirtioRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = VirtioTxToken
    where
        Self: 'a;

    fn receive(&mut self, timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        self.driver.receive(timestamp)
    }

    fn transmit(&mut self, timestamp: Instant) -> Option<Self::TxToken<'_>> {
        self.driver.transmit(timestamp)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        self.driver.capabilities()
    }
}

/// Receive token that hands out buffers backed by virtio RX descriptors.
pub struct VirtioRxToken {
    driver: *mut VirtioNet,
    id: u16,
    len: usize,
}

impl RxToken for VirtioRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        NET_DIAG.record_smoltcp_rx();
        let driver = unsafe { &mut *self.driver };
        let slot = self.id as usize;
        let buffer = driver
            .rx_buffers
            .get_mut(slot)
            .expect("rx descriptor out of range");
        let header_len = driver.rx_header_len;
        let available = core::cmp::min(self.len, buffer.as_mut_slice().len());
        if available < header_len {
            warn!(
                "[virtio-net] RX: frame too small for virtio-net header (len={})",
                available
            );
            driver.requeue_rx(self.id, Some(self.len));
            return f(&[]);
        }

        let payload_len = available - header_len;
        let mut_slice = &mut buffer.as_mut_slice()[..available];
        let payload = &mut mut_slice[header_len..header_len + payload_len];
        let preview_len = core::cmp::min(payload.len(), 16);
        log_tcp_dest_port_once(payload);
        log::debug!(
            target: "net-console",
            "[virtio] RX packet len={} first_bytes={:02x?}",
            payload_len,
            &payload[..preview_len],
        );
        log_tcp_trace("RX", payload);
        driver.rx_packets = driver.rx_packets.saturating_add(1);
        let result = f(payload);
        driver.requeue_rx(self.id, Some(self.len));
        result
    }
}

/// Transmit token that queues frames onto the virtio TX ring.
pub struct VirtioTxToken {
    driver: *mut VirtioNet,
    reservation: Cell<Option<TxReservation>>,
}

impl VirtioTxToken {
    fn new(driver: *mut VirtioNet, reservation: Option<TxReservation>) -> Self {
        Self {
            driver,
            reservation: Cell::new(reservation),
        }
    }

    fn take_reservation(&self) -> Option<TxReservation> {
        self.reservation.take()
    }

    fn has_id(&self) -> bool {
        self.reservation.get().is_some()
    }
}

impl TxToken for VirtioTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let driver = unsafe { &mut *self.driver };
        let reservation = match self.take_reservation() {
            Some(reservation) => reservation,
            None => {
                driver.token_double_consume = driver.token_double_consume.saturating_add(1);
                debug_assert!(
                    false,
                    "VirtioTxToken consumed more than once without reservation"
                );
                driver.tx_drops = driver.tx_drops.saturating_add(1);
                return f(&mut []);
            }
        };
        let id = match driver.validate_tx_reservation(reservation, "tx_token_consume") {
            Ok(id) => id,
            Err(()) => {
                driver.tx_drops = driver.tx_drops.saturating_add(1);
                return f(&mut []);
            }
        };
        let attempt_seq = driver.next_tx_attempt_seq();
        if len == 0 {
            if NET_VIRTIO_TX_V2 {
                driver.tx_zero_len_attempt = driver.tx_zero_len_attempt.wrapping_add(1);
                driver.release_tx_head(id, "tx_v2_zero_len_release");
            }
            driver.tx_drops = driver.tx_drops.saturating_add(1);
            driver.log_tx_attempt(attempt_seq, len, 0, 0);
            return f(&mut []);
        }
        NET_DIAG.record_smoltcp_tx();
        {
            let buffer = driver
                .tx_buffers
                .get_mut(id as usize)
                .expect("tx descriptor out of range");
            let header_len = driver.tx_header_len;
            let max_len = buffer.as_mut_slice().len();
            if max_len <= header_len {
                driver.tx_drops = driver.tx_drops.saturating_add(1);
                if NET_VIRTIO_TX_V2 {
                    driver.release_tx_head(id, "tx_v2_buffer_small");
                }
                return f(&mut []);
            }

            let payload_len = len
                .min(MAX_FRAME_LEN)
                .min(max_len.saturating_sub(header_len));
            let result;
            let written_len;
            let total_capacity = payload_len.saturating_add(header_len);
            {
                let mut_slice = &mut buffer.as_mut_slice()[..total_capacity];
                let (header, payload) =
                    mut_slice.split_at_mut(core::cmp::min(header_len, total_capacity));
                header.fill(0);
                let copy_len = core::cmp::min(payload_len, MAX_FRAME_LEN);
                let mut payload_before = [0u8; MAX_FRAME_LEN];
                let mut payload_after = [0u8; MAX_FRAME_LEN];
                payload_before[..copy_len].copy_from_slice(&payload[..copy_len]);
                result = f(&mut payload[..payload_len]);
                let after_len = core::cmp::min(payload_len, MAX_FRAME_LEN);
                payload_after[..after_len].copy_from_slice(&payload[..after_len]);
                written_len = VirtioNet::compute_written_len(
                    payload_len,
                    &payload_before[..copy_len],
                    &payload_after[..after_len],
                );
                log_tcp_trace("TX", &payload[..payload_len]);
            }
            driver.log_tx_attempt(attempt_seq, len, payload_len, written_len);
            let Some(total_len) = VirtioNet::tx_total_len(header_len, written_len) else {
                driver.drop_zero_len_tx(id, payload_len, written_len);
                return result;
            };
            debug_assert!(total_len > 0, "tx total_len must be non-zero before submit");
            driver.submit_tx(id, total_len);
            driver.tx_packets = driver.tx_packets.saturating_add(1);
            result
        }
    }

    fn set_meta(&mut self, _meta: smoltcp::phy::PacketMeta) {}
}

struct VirtioRegs {
    mmio: DeviceFrame,
    mode: VirtioMmioMode,
}

impl VirtioRegs {
    fn probe<H>(hal: &mut H) -> Result<Self, DriverError>
    where
        H: Hardware<Error = HalError>,
    {
        for slot in 0..VIRTIO_MMIO_SLOTS {
            let base = VIRTIO_MMIO_BASE + slot * VIRTIO_MMIO_STRIDE;
            info!(
                "[net-console] probing virtio-mmio slot={} paddr=0x{base:08x}",
                slot,
                base = base
            );
            if hal.device_coverage(base, DEVICE_FRAME_BITS).is_none() {
                continue;
            }
            let frame = match hal.map_device(base) {
                Ok(frame) => frame,
                Err(HalError::Sel4(err)) if err == seL4_NotEnoughMemory => {
                    log::trace!(
                        "virtio-mmio: slot {slot} @ 0x{base:08x} unavailable (no device coverage)",
                    );
                    continue;
                }
                Err(err) => return Err(DriverError::from(err)),
            };
            let regs = VirtioRegs {
                mmio: frame,
                mode: VirtioMmioMode::Modern,
            };
            let identifiers = regs.read_identifiers();
            identifiers.log();
            let version = identifiers.version;
            let device_id = identifiers.device_id;
            let vendor_id = regs.read32(Registers::VendorId);
            match identifiers.evaluate(vendor_id) {
                Ok(mode) => {
                    info!(
                        target: "net-console",
                        "[virtio-net] found device: slot={} mmio=0x{base:08x} device_id=0x{device_id:04x} vendor=0x{vendor_id:04x} version=0x{version:08x}",
                        slot,
                        base = base,
                        device_id = device_id,
                        vendor_id = vendor_id,
                        version = version,
                    );
                    return Ok(VirtioRegs {
                        mmio: regs.mmio,
                        mode,
                    });
                }
                Err(DriverError::NoDevice) => {
                    warn!(
                        target: "net-console",
                        "[virtio-net] virtio-mmio slot={} hosts device_id=0x{device_id:04x}; expecting virtio-net (1)",
                        slot,
                        device_id = device_id,
                    );
                    continue;
                }
                Err(err) => {
                    error!(
                        "[net-console] virtio-mmio id rejected for slot {}: {}",
                        slot, err
                    );
                    return Err(err);
                }
            }
        }
        error!("[net-console] no virtio-net device found on virtio-mmio bus; TCP console disabled");
        Err(DriverError::NoDevice)
    }

    fn base(&self) -> NonNull<u8> {
        self.mmio.ptr()
    }

    fn read32(&self, offset: Registers) -> u32 {
        unsafe { read_volatile(self.base().as_ptr().add(offset as usize) as *const u32) }
    }

    fn write32(&mut self, offset: Registers, value: u32) {
        unsafe { write_volatile(self.base().as_ptr().add(offset as usize) as *mut u32, value) };
    }

    fn write16(&mut self, offset: Registers, value: u16) {
        unsafe { write_volatile(self.base().as_ptr().add(offset as usize) as *mut u16, value) };
    }

    fn reset_status(&mut self) {
        self.write32(Registers::Status, 0);
    }

    fn set_status(&mut self, status: u32) {
        self.write32(Registers::Status, status);
    }

    fn select_queue(&mut self, index: u32) {
        self.write32(Registers::QueueSel, index);
    }

    fn queue_num_max(&self) -> u32 {
        self.read32(Registers::QueueNumMax)
    }

    fn set_queue_size(&mut self, size: u16) {
        self.write16(Registers::QueueNum, size);
    }

    fn set_queue_align(&mut self, align: u32) {
        self.write32(Registers::QueueAlign, align);
    }

    fn set_queue_pfn(&mut self, pfn: u32) {
        self.write32(Registers::QueuePfn, pfn);
    }

    fn queue_ready(&mut self, ready: u32) {
        self.write32(Registers::QueueReady, ready);
    }

    fn status(&self) -> u32 {
        self.read32(Registers::Status)
    }

    fn notify(&mut self, queue: u32) {
        self.write32(Registers::QueueNotify, queue);
    }

    fn set_guest_features(&mut self, features: u64) {
        let lo = features as u32;
        let hi = (features >> 32) as u32;

        self.write32(Registers::GuestFeaturesSel, 0);
        self.write32(Registers::GuestFeatures, lo);

        if matches!(self.mode, VirtioMmioMode::Modern) {
            self.write32(Registers::GuestFeaturesSel, 1);
            self.write32(Registers::GuestFeatures, hi);
        }
    }

    fn set_guest_page_size(&mut self, page_size: u32) {
        self.write32(Registers::GuestPageSize, page_size);
    }

    fn host_features(&mut self) -> u64 {
        self.write32(Registers::HostFeaturesSel, 0);
        let lo = self.read32(Registers::HostFeatures) as u64;
        if matches!(self.mode, VirtioMmioMode::Modern) {
            self.write32(Registers::HostFeaturesSel, 1);
            let hi = self.read32(Registers::HostFeatures) as u64;
            (hi << 32) | lo
        } else {
            lo
        }
    }

    fn set_queue_desc_addr(&mut self, paddr: usize) {
        self.write32(Registers::QueueDescLow, paddr as u32);
        self.write32(Registers::QueueDescHigh, (paddr >> 32) as u32);
    }

    fn set_queue_driver_addr(&mut self, paddr: usize) {
        self.write32(Registers::QueueDriverLow, paddr as u32);
        self.write32(Registers::QueueDriverHigh, (paddr >> 32) as u32);
    }

    fn set_queue_device_addr(&mut self, paddr: usize) {
        self.write32(Registers::QueueDeviceLow, paddr as u32);
        self.write32(Registers::QueueDeviceHigh, (paddr >> 32) as u32);
    }

    fn acknowledge_interrupts(&mut self) -> (u32, u32) {
        let status = self.read32(Registers::InterruptStatus);
        let mut ack = 0;
        if status != 0 {
            self.write32(Registers::InterruptAck, status);
            ack = status;
        }
        (status, ack)
    }

    fn isr_status(&self) -> u32 {
        self.read32(Registers::InterruptStatus)
    }

    fn read_mac(&self) -> Option<EthernetAddress> {
        let mut bytes = [0u8; 6];
        let base = Registers::Config as usize;
        for (idx, byte) in bytes.iter_mut().enumerate() {
            *byte = unsafe { read_volatile(self.base().as_ptr().add(base + idx) as *const u8) };
        }
        if bytes.iter().all(|&b| b == 0) {
            None
        } else {
            Some(EthernetAddress::from_bytes(&bytes))
        }
    }

    fn read_identifiers(&self) -> VirtioMmioId {
        VirtioMmioId {
            magic: self.read32(Registers::MagicValue),
            version: self.read32(Registers::Version),
            device_id: self.read32(Registers::DeviceId),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VirtioMmioId {
    magic: u32,
    version: u32,
    device_id: u32,
}

impl VirtioMmioId {
    fn log(&self) {
        info!(
            target: "net-console",
            "virtio-mmio id: magic=0x{magic:08x}, version=0x{version:08x}, device_id=0x{device_id:08x}",
            magic = self.magic,
            version = self.version,
            device_id = self.device_id,
        );
    }

    fn evaluate(&self, vendor_id: u32) -> Result<VirtioMmioMode, DriverError> {
        if self.magic != VIRTIO_MMIO_MAGIC {
            error!(
                target: "net-console",
                "virtio-mmio header magic mismatch: expected=0x{expected:08x} actual=0x{actual:08x}",
                expected = VIRTIO_MMIO_MAGIC,
                actual = self.magic
            );
            return Err(DriverError::InvalidMagic(self.magic));
        }

        if self.device_id != VIRTIO_DEVICE_ID_NET {
            warn!(
                target: "net-console",
                "virtio-mmio device_id=0x{device:04x} vendor=0x{vendor:04x} is not virtio-net",
                device = self.device_id,
                vendor = vendor_id,
            );
            return Err(DriverError::NoDevice);
        }

        match self.version {
            VIRTIO_MMIO_VERSION_MODERN => {
                info!(
                    target: "net-console",
                    "modern virtio-mmio v2 detected (vendor=0x{vendor:04x})",
                    vendor = vendor_id
                );
                Ok(VirtioMmioMode::Modern)
            }
            VIRTIO_MMIO_VERSION_LEGACY => {
                let message = "legacy virtio-mmio (v1) detected";
                #[cfg(feature = "virtio-mmio-legacy")]
                {
                    warn!(target: "net-console", "{message}; feature virtio-mmio-legacy enabled");
                    Ok(VirtioMmioMode::Legacy)
                }
                #[cfg(not(feature = "virtio-mmio-legacy"))]
                {
                    error!(
                        target: "net-console",
                        "{message}; rebuild with --features virtio-mmio-legacy or boot QEMU with -global virtio-mmio.force-legacy=false"
                    );
                    Err(DriverError::UnsupportedVersion(self.version))
                }
            }
            other => {
                error!(
                    target: "net-console",
                    "virtio-mmio version unsupported: 0x{other:08x} (expected 0x{modern:08x} or legacy v1)",
                    modern = VIRTIO_MMIO_VERSION_MODERN
                );
                Err(DriverError::UnsupportedVersion(other))
            }
        }
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    use core::fmt::Write as FmtWrite;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use heapless::{String as HeaplessString, Vec as HeaplessVec};

    fn make_fake_mmio(magic: u32, version: u32, device_id: u32) -> (NonNull<u8>, Box<[u32; 4]>) {
        let mut backing = Box::new([0u32; 4]);
        backing[0] = magic;
        backing[1] = version;
        backing[2] = device_id;
        let base = NonNull::new(backing.as_mut_ptr() as *mut u8).expect("fake mmio base");
        (base, backing)
    }

    fn read_identifiers_from_base(base: NonNull<u8>) -> VirtioMmioId {
        unsafe {
            VirtioMmioId {
                magic: read_volatile(
                    base.as_ptr().add(Registers::MagicValue as usize) as *const u32
                ),
                version: read_volatile(base.as_ptr().add(Registers::Version as usize) as *const u32),
                device_id: read_volatile(
                    base.as_ptr().add(Registers::DeviceId as usize) as *const u32
                ),
            }
        }
    }

    #[test]
    fn detects_invalid_magic() {
        let (base, _backing) =
            make_fake_mmio(0x0, VIRTIO_MMIO_VERSION_LEGACY, VIRTIO_DEVICE_ID_NET);
        let identifiers = read_identifiers_from_base(base);
        assert_eq!(identifiers.magic, 0x0);
        assert!(matches!(
            identifiers.evaluate(0x1),
            Err(DriverError::InvalidMagic(0x0))
        ));
    }

    #[test]
    fn accepts_modern_v2() {
        let (base, _backing) = make_fake_mmio(
            VIRTIO_MMIO_MAGIC,
            VIRTIO_MMIO_VERSION_MODERN,
            VIRTIO_DEVICE_ID_NET,
        );
        let identifiers = read_identifiers_from_base(base);
        assert_eq!(
            identifiers.evaluate(0x1).expect("modern v2 accepted"),
            VirtioMmioMode::Modern
        );
    }

    #[cfg(not(feature = "virtio-mmio-legacy"))]
    #[test]
    fn rejects_legacy_without_feature() {
        let (base, _backing) = make_fake_mmio(
            VIRTIO_MMIO_MAGIC,
            VIRTIO_MMIO_VERSION_LEGACY,
            VIRTIO_DEVICE_ID_NET,
        );
        let identifiers = read_identifiers_from_base(base);
        assert!(matches!(
            identifiers.evaluate(0x1),
            Err(DriverError::UnsupportedVersion(VIRTIO_MMIO_VERSION_LEGACY))
        ));
    }

    #[cfg(feature = "virtio-mmio-legacy")]
    #[test]
    fn accepts_legacy_with_feature() {
        let (base, _backing) = make_fake_mmio(
            VIRTIO_MMIO_MAGIC,
            VIRTIO_MMIO_VERSION_LEGACY,
            VIRTIO_DEVICE_ID_NET,
        );
        let identifiers = read_identifiers_from_base(base);
        assert_eq!(
            identifiers.evaluate(0x1).expect("legacy gated accept"),
            VirtioMmioMode::Legacy
        );
    }

    #[test]
    fn rejects_unknown_versions() {
        let (base, _backing) = make_fake_mmio(VIRTIO_MMIO_MAGIC, 3, VIRTIO_DEVICE_ID_NET);
        let identifiers = read_identifiers_from_base(base);
        assert!(matches!(
            identifiers.evaluate(0x1),
            Err(DriverError::UnsupportedVersion(3))
        ));
    }

    #[test]
    fn unsupported_error_message_is_actionable() {
        let legacy_err = DriverError::UnsupportedVersion(VIRTIO_MMIO_VERSION_LEGACY);
        let mut legacy_msg = HeaplessString::<192>::new();
        write!(&mut legacy_msg, "{legacy_err}").unwrap();
        assert!(legacy_msg.contains("virtio-mmio-legacy"));
        assert!(legacy_msg.contains("force-legacy"));

        let unknown_err = DriverError::UnsupportedVersion(7);
        let mut unknown_msg = HeaplessString::<192>::new();
        write!(&mut unknown_msg, "{unknown_err}").unwrap();
        assert!(unknown_msg.contains("0x00000007"));
        assert!(unknown_msg.contains("v2"));
    }

    #[cfg(all(
        feature = "net-backend-virtio",
        feature = "net-virtio-tx-v2",
        feature = "net-console",
        feature = "kernel"
    ))]
    #[test]
    fn tx_avail_wrap_preserves_descriptor_and_slot() {
        const QSIZE: u16 = 8;
        let layout = VirtqLayout::compute_vq_layout(QSIZE, false).expect("layout");
        let mut backing = vec![0u8; layout.total_len];
        let base_ptr = backing.as_mut_ptr();
        let desc_ptr = NonNull::new(base_ptr as *mut VirtqDesc).expect("desc ptr");
        let avail_ptr =
            NonNull::new(unsafe { base_ptr.add(layout.avail_offset) } as *mut VirtqAvail)
                .expect("avail ptr");
        let used_ptr = NonNull::new(unsafe { base_ptr.add(layout.used_offset) } as *mut VirtqUsed)
            .expect("used ptr");
        let mut queue = VirtQueue {
            _frame: unsafe { core::mem::zeroed() },
            layout,
            size: QSIZE,
            desc: desc_ptr,
            avail: avail_ptr,
            used: used_ptr,
            last_used: 0,
            pfn: 0,
            base_paddr: base_ptr as usize,
            base_vaddr: base_ptr as usize,
            base_len: layout.total_len,
            _dma: None,
            cacheable: false,
            used_zero_len_head: None,
            last_error: None,
        };
        let mut head_mgr = TxHeadManager::new(QSIZE);
        let mut in_flight: HeaplessVec<u16, TX_QUEUE_SIZE> = HeaplessVec::new();

        for publish in 0..(usize::from(QSIZE) + 4) {
            if head_mgr.free_len() == 0 {
                let completed = in_flight.remove(0);
                head_mgr.mark_completed(completed, None).unwrap();
                head_mgr.reclaim_head(completed).unwrap();
            }
            let head = head_mgr.alloc_head().expect("head alloc");
            let (_, old_avail) = queue.indices_no_sync();
            let slot = (old_avail as usize % usize::from(QSIZE)) as u16;
            let addr = 0x2000u64 + publish as u64 * 0x10 + u64::from(head);
            queue
                .setup_descriptor(head, addr, 64, 0, None)
                .expect("desc setup");
            head_mgr
                .mark_published(head, slot, 64, addr)
                .expect("mark published");
            head_mgr
                .promote_published_to_inflight(head)
                .expect("promote inflight");
            virtq_publish_barrier();
            let (ring_slot, new_idx, observed_old) = queue.push_avail(head).unwrap();
            assert_eq!(observed_old, old_avail);
            assert_eq!(ring_slot, slot);
            let ring_ptr = unsafe {
                (*queue.avail.as_ptr())
                    .ring
                    .as_ptr()
                    .add(ring_slot as usize)
            };
            let ring_head = unsafe { read_volatile(ring_ptr) };
            assert_eq!(ring_head, head);
            let desc = queue.read_descriptor(head);
            assert_ne!(desc.addr, 0);
            assert_ne!(desc.len, 0);
            head_mgr
                .note_avail_publish(head, ring_slot, new_idx)
                .expect("record publish");
            head_mgr.mark_in_flight(head).expect("mark inflight");
            in_flight.push(head).unwrap();
        }
    }

    static USED_ELEM_PTR: Mutex<Option<*mut VirtqUsedElem>> = Mutex::new(None);
    static USED_ELEM_INVALIDATES: AtomicUsize = AtomicUsize::new(0);

    fn used_elem_visibility_hook(op: CacheOp, ptr: usize, len: usize) {
        if op != CacheOp::Invalidate || len != core::mem::size_of::<VirtqUsedElem>() {
            return;
        }
        let Some(elem_ptr) = *USED_ELEM_PTR.lock() else {
            return;
        };
        if ptr != elem_ptr as usize {
            return;
        }
        let count = USED_ELEM_INVALIDATES.fetch_add(1, Ordering::SeqCst);
        if count == 1 {
            unsafe {
                (*elem_ptr).len = 64u32.to_le();
            }
        }
    }

    #[test]
    fn used_elem_visibility_retry_reads_len() {
        const QSIZE: u16 = 4;
        let layout = VirtqLayout::compute_vq_layout(QSIZE, false).expect("layout");
        let mut backing = vec![0u8; layout.total_len];
        let base_ptr = backing.as_mut_ptr();
        let desc_ptr = NonNull::new(base_ptr as *mut VirtqDesc).expect("desc ptr");
        let avail_ptr =
            NonNull::new(unsafe { base_ptr.add(layout.avail_offset) } as *mut VirtqAvail)
                .expect("avail ptr");
        let used_ptr = NonNull::new(unsafe { base_ptr.add(layout.used_offset) } as *mut VirtqUsed)
            .expect("used ptr");
        unsafe {
            write_volatile(
                desc_ptr.as_ptr(),
                VirtqDesc {
                    addr: 0x2000,
                    len: 64,
                    flags: 0,
                    next: 0,
                },
            );
            write_volatile(&mut (*used_ptr.as_ptr()).idx, 1u16.to_le());
            let elem_ptr = (*used_ptr.as_ptr()).ring.as_ptr() as *mut VirtqUsedElem;
            write_volatile(
                elem_ptr,
                VirtqUsedElem {
                    id: 0u32.to_le(),
                    len: 0u32.to_le(),
                },
            );
            *USED_ELEM_PTR.lock() = Some(elem_ptr);
        }
        USED_ELEM_INVALIDATES.store(0, Ordering::SeqCst);
        let mut queue = VirtQueue {
            _frame: unsafe { core::mem::zeroed() },
            layout,
            size: QSIZE,
            desc: desc_ptr,
            avail: avail_ptr,
            used: used_ptr,
            last_used: 0,
            pfn: 0,
            base_paddr: base_ptr as usize,
            base_vaddr: base_ptr as usize,
            base_len: layout.total_len,
            _dma: None,
            cacheable: true,
            used_zero_len_head: None,
            last_error: None,
        };

        let mut result = None;
        with_dma_test_hook(Some(used_elem_visibility_hook), || {
            result = queue.pop_used("TX", false).expect("pop used ok");
        });

        let (id, len, ring_slot) = result.expect("used elem visible after retry");
        assert_eq!(id, 0);
        assert_eq!(len, 64);
        assert_eq!(ring_slot, 0);
        assert_eq!(queue.last_used, 1);
        assert_eq!(
            USED_ELEM_INVALIDATES.load(Ordering::SeqCst),
            2,
            "visibility retry should re-invalidate used element"
        );
        *USED_ELEM_PTR.lock() = None;
    }
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum Registers {
    MagicValue = 0x000,
    Version = 0x004,
    DeviceId = 0x008,
    VendorId = 0x00c,
    HostFeatures = 0x010,
    HostFeaturesSel = 0x014,
    GuestFeatures = 0x020,
    GuestFeaturesSel = 0x024,
    GuestPageSize = 0x028,
    QueueSel = 0x030,
    QueueNumMax = 0x034,
    QueueNum = 0x038,
    QueueAlign = 0x03c,
    QueuePfn = 0x040,
    QueueDescLow = 0x080,
    QueueDescHigh = 0x084,
    QueueDriverLow = 0x090,
    QueueDriverHigh = 0x094,
    QueueDeviceLow = 0x0a0,
    QueueDeviceHigh = 0x0a4,
    QueueReady = 0x044,
    QueueNotify = 0x050,
    InterruptStatus = 0x060,
    InterruptAck = 0x064,
    Status = 0x070,
    Config = 0x100,
}

struct VirtQueue {
    _frame: RamFrame,
    layout: VirtqLayout,
    size: u16,
    desc: NonNull<VirtqDesc>,
    avail: NonNull<VirtqAvail>,
    used: NonNull<VirtqUsed>,
    last_used: u16,
    pfn: u32,
    base_paddr: usize,
    base_vaddr: usize,
    base_len: usize,
    _dma: Option<PinnedDmaRange>,
    cacheable: bool,
    used_zero_len_head: Option<u16>,
    last_error: Option<&'static str>,
}

impl VirtQueue {
    fn assert_index_in_range(&self, index: u16, label: &'static str) {
        assert!(
            index < self.size,
            "virtqueue {label} index out of range: index={index} size={} base_vaddr=0x{base:016x} base_paddr=0x{paddr:016x}",
            self.size,
            base = self.base_vaddr,
            paddr = self.base_paddr,
        );
    }

    fn assert_offset_in_range(&self, offset: usize, len: usize, label: &'static str) {
        let end = offset.saturating_add(len);
        let ptr = self.base_vaddr.saturating_add(offset);
        assert!(
            end <= self.base_len,
            "virtqueue {label} offset out of range: offset=0x{offset:x} len=0x{len:x} total=0x{total:x} ptr=0x{ptr:016x} base_vaddr=0x{base:016x} base_paddr=0x{paddr:016x}",
            offset = offset,
            len = len,
            total = self.base_len,
            ptr = ptr,
            base = self.base_vaddr,
            paddr = self.base_paddr,
        );
    }

    fn assert_ring_slot(&self, idx: u16, slot: usize, label: &'static str) {
        let qsize = usize::from(self.size);
        assert!(
            qsize.is_power_of_two(),
            "virtqueue {label} size must be power-of-two: qsize={qsize} base_vaddr=0x{base:016x}",
            base = self.base_vaddr,
        );
        assert!(
            qsize != 0,
            "virtqueue {label} size must be non-zero base_vaddr=0x{base:016x}",
            base = self.base_vaddr,
        );
        assert!(
            slot < qsize,
            "virtqueue {label} slot out of range: idx={idx} slot={slot} qsize={qsize} base_vaddr=0x{base:016x}",
            base = self.base_vaddr,
        );
        if qsize.is_power_of_two() {
            let mask_slot = (idx as usize) & (qsize - 1);
            assert!(
                slot == mask_slot,
                "virtqueue {label} wrap mismatch: idx={idx} slot={slot} mask_slot={mask_slot} qsize={qsize} base_vaddr=0x{base:016x}",
                base = self.base_vaddr,
            );
        }
    }

    fn new(
        regs: &mut VirtioRegs,
        mut frame: RamFrame,
        index: u32,
        size: usize,
        mode: VirtioMmioMode,
        cacheable: bool,
        end_align: bool,
    ) -> Result<Self, DriverError> {
        check_bootinfo_canary("virtio.queue.addr.pre")?;
        let queue_size = size as u16;
        let frame_paddr = frame.paddr();
        let frame_ptr = frame.ptr();
        let page_bytes = 1usize << seL4_PageBits;

        let frame_capacity = {
            let frame_slice = frame.as_mut_slice();
            let capacity = frame_slice.len();

            frame_slice.fill(0);
            capacity
        };

        unsafe {
            watched_write_bytes(
                frame_ptr.as_ptr(),
                0,
                frame_capacity,
                "virtqueue.frame.zero",
            );
        }

        if frame_capacity != page_bytes {
            error!(
                target: "net-console",
                "[virtio-net] virtqueue backing not 4KiB: capacity={} expected={}",
                frame_capacity,
                page_bytes
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue backing must be exactly one page",
            ));
        }

        if frame_paddr & (page_bytes - 1) != 0 {
            error!(
                target: "net-console",
                "[virtio-net] virtqueue backing not page aligned: paddr=0x{frame_paddr:x}"
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue backing not page aligned",
            ));
        }

        const LEGACY_QUEUE_ALIGN: usize = 4;
        let layout = VirtqLayout::compute_vq_layout(queue_size, false)?;
        if end_align && matches!(mode, VirtioMmioMode::Legacy) {
            error!(
                target: "net-console",
                "[virtio-net] end-aligned virtqueue unsupported for legacy mode"
            );
            return Err(DriverError::QueueInvariant(
                "legacy virtqueue must be page-aligned",
            ));
        }
        let base_offset = if end_align {
            const DESC_ALIGN: usize = 16;
            let raw_offset = frame_capacity.saturating_sub(layout.total_len);
            raw_offset & !(DESC_ALIGN - 1)
        } else {
            0
        };
        assert!(
            base_offset + layout.total_len <= frame_capacity,
            "virtqueue backing overflow: base_offset=0x{base_offset:x} total_len=0x{total_len:x} capacity=0x{frame_capacity:x}",
            base_offset = base_offset,
            total_len = layout.total_len,
            frame_capacity = frame_capacity,
        );
        let base_paddr = frame_paddr.saturating_add(base_offset);
        let base_ptr =
            unsafe { NonNull::new_unchecked(frame_ptr.as_ptr().add(base_offset) as *mut u8) };
        let base_vaddr = base_ptr.as_ptr() as usize;

        let dma = match dma::pin(base_vaddr, base_paddr, layout.total_len, "virtq") {
            Ok(range) => Some(range),
            Err(err) => {
                warn!(
                    target: "net-console",
                    "[virtio-net][dma] pin skipped for queue {}: {:?}",
                    index,
                    err
                );
                None
            }
        };

        debug_assert!(
            layout.desc_offset + layout.desc_len <= layout.avail_offset,
            "virtqueue layout overlap: desc/avail"
        );
        debug_assert!(
            layout.avail_offset + layout.avail_len <= layout.used_offset,
            "virtqueue layout overlap: avail/used"
        );
        debug_assert!(
            layout.total_len <= frame_capacity,
            "virtqueue layout exceeds backing"
        );

        if !VQ_LAYOUT_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            info!(
                target: "net-console",
                "[virtio-net][layout] computed: qsize={} desc@+0x{desc_off:03x}/len={desc_len} avail@+0x{avail_off:03x}/len={avail_len} used@+0x{used_off:03x}/len={used_len} total={total}",
                queue_size,
                desc_off = layout.desc_offset,
                desc_len = layout.desc_len,
                avail_off = layout.avail_offset,
                avail_len = layout.avail_len,
                used_off = layout.used_offset,
                used_len = layout.used_len,
                total = layout.total_len,
            );
        }

        let desc_end = layout.desc_offset + layout.desc_len;
        let avail_end = layout.avail_offset + layout.avail_len;
        let used_end = layout.used_offset + layout.used_len;

        let desc_vaddr = base_vaddr.saturating_add(layout.desc_offset);
        let avail_vaddr = base_vaddr.saturating_add(layout.avail_offset);
        let used_vaddr = base_vaddr.saturating_add(layout.used_offset);
        log_dma_programming(
            "virtq.desc",
            desc_vaddr,
            base_paddr + layout.desc_offset,
            layout.desc_len,
        );
        log_dma_programming(
            "virtq.avail",
            avail_vaddr,
            base_paddr + layout.avail_offset,
            layout.avail_len,
        );
        log_dma_programming(
            "virtq.used",
            used_vaddr,
            base_paddr + layout.used_offset,
            layout.used_len,
        );
        assert_dma_region(
            "virtq.desc",
            desc_vaddr,
            base_paddr + layout.desc_offset,
            layout.desc_len,
        );
        assert_dma_region(
            "virtq.avail",
            avail_vaddr,
            base_paddr + layout.avail_offset,
            layout.avail_len,
        );
        assert_dma_region(
            "virtq.used",
            used_vaddr,
            base_paddr + layout.used_offset,
            layout.used_len,
        );
        let addr_slot = (index as usize) % VIRTIO_MMIO_SLOTS;
        if !VQ_ADDRESS_LOGGED[addr_slot].swap(true, AtomicOrdering::AcqRel) {
            info!(
                target: "net-console",
                "[virtio-net][layout] addr queue={} base_vaddr=0x{base_vaddr:016x} base_paddr=0x{base_paddr:016x} desc=[0x{desc:016x} len=0x{desc_len:04x}] avail=[0x{avail:016x} len=0x{avail_len:04x}] used=[0x{used:016x} len=0x{used_len:04x}] total=0x{total:04x}",
                index,
                desc = desc_vaddr,
                desc_len = layout.desc_len,
                avail = avail_vaddr,
                avail_len = layout.avail_len,
                used = used_vaddr,
                used_len = layout.used_len,
                total = layout.total_len,
            );
        }

        if layout.desc_offset != 0 {
            error!(
                target: "net-console",
                "[virtio-net][layout] descriptor offset must be zero: desc_off={}",
                layout.desc_offset,
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue descriptor offset mismatch",
            ));
        }

        if layout.avail_offset != layout.desc_len {
            error!(
                target: "net-console",
                "[virtio-net][layout] avail offset mismatch: avail_off={} desc_len={} (expected equality)",
                layout.avail_offset,
                layout.desc_len,
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue avail offset mismatch",
            ));
        }

        if layout.used_offset & (LEGACY_QUEUE_ALIGN - 1) != 0 {
            error!(
                target: "net-console",
                "[virtio-net][layout] used ring misaligned: used_offset={} align={}",
                layout.used_offset,
                LEGACY_QUEUE_ALIGN,
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue used ring alignment violation",
            ));
        }

        if desc_end > layout.avail_offset
            || avail_end > layout.used_offset
            || used_end > layout.total_len
            || layout.avail_offset < desc_end
            || layout.used_offset < avail_end
        {
            error!(
                target: "net-console",
                "[virtio-net][layout] overlap detected: desc_end={} avail_base={} avail_end={} used_base={} used_end={} frame_cap={}",
                desc_end,
                layout.avail_offset,
                avail_end,
                layout.used_offset,
                used_end,
                frame_capacity,
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue layout overlap detected",
            ));
        }

        if used_end > frame_capacity {
            error!(
                target: "net-console",
                "[virtio-net] virtqueue layout exceeds backing: avail_end={} used_end={} capacity={}",
                avail_end,
                used_end,
                frame_capacity,
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue layout exceeds backing",
            ));
        }

        if used_end != layout.total_len {
            error!(
                target: "net-console",
                "[virtio-net][layout] total length mismatch: used_end={} total_len={} frame_capacity={}",
                used_end,
                layout.total_len,
                frame_capacity,
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue total length mismatch",
            ));
        }

        let desc_ptr = base_ptr.cast::<VirtqDesc>();
        let avail_ptr = unsafe {
            NonNull::new_unchecked(base_ptr.as_ptr().add(layout.avail_offset) as *mut VirtqAvail)
        };
        let used_ptr = unsafe {
            NonNull::new_unchecked(base_ptr.as_ptr().add(layout.used_offset) as *mut VirtqUsed)
        };

        let avail_idx = u16::from_le(unsafe { read_volatile(&(*avail_ptr.as_ptr()).idx) });
        let used_idx = u16::from_le(unsafe { read_volatile(&(*used_ptr.as_ptr()).idx) });

        if avail_idx != 0 || used_idx != 0 {
            error!(
                target: "net-console",
                "[virtio-net] virtqueue not zeroed: avail.idx={} used.idx={}",
                avail_idx,
                used_idx,
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue indices must start at zero",
            ));
        }

        debug_assert_eq!(avail_idx, 0, "virtqueue avail.idx must start at zero");
        debug_assert_eq!(used_idx, 0, "virtqueue used.idx must start at zero");

        if (index as usize) < RING_SLOT_CANARY_LOGGED.len()
            && !RING_SLOT_CANARY_LOGGED[index as usize].swap(true, AtomicOrdering::AcqRel)
        {
            info!(
                target: "net-console",
                "[virtio-net] queue {index} ring canary: size={queue_size} initial_avail_idx={avail_idx} slot_rule=idx%size",
            );
        }

        info!(
            target: "net-console",
            "[virtio-net][layout] queue={index} size={queue_size} qmem_vaddr=0x{vaddr:016x} qmem_paddr=0x{paddr:016x} desc@+0x{desc_off:03x}/len={desc_len} avail@+0x{avail_off:03x}/len={avail_len} used@+0x{used_off:03x}/len={used_len} total_len={total}",
            vaddr = base_vaddr,
            paddr = base_paddr,
            desc_off = layout.desc_offset,
            desc_len = layout.desc_len,
            avail_off = layout.avail_offset,
            avail_len = layout.avail_len,
            used_off = layout.used_offset,
            used_len = layout.used_len,
            total = layout.total_len,
        );

        regs.select_queue(index);
        regs.set_queue_size(queue_size);

        let queue_pfn = (base_paddr >> seL4_PageBits) as u32;
        match mode {
            VirtioMmioMode::Modern => {
                regs.set_queue_desc_addr(base_paddr);
                regs.set_queue_driver_addr(base_paddr + layout.avail_offset);
                regs.set_queue_device_addr(base_paddr + layout.used_offset);
            }
            VirtioMmioMode::Legacy => {
                regs.set_queue_align(LEGACY_QUEUE_ALIGN as u32);
                regs.set_queue_pfn(queue_pfn);
            }
        }
        check_bootinfo_canary("virtio.queue.addr.post")?;
        regs.queue_ready(1);
        info!(
            target: "net-console",
            "[virtio-net] queue {} configured: size={} pfn=0x{:x} mode={:?}",
            index,
            queue_size,
            queue_pfn,
            mode,
        );

        Ok(Self {
            _frame: frame,
            layout,
            size: queue_size,
            desc: desc_ptr,
            avail: avail_ptr,
            used: used_ptr,
            last_used: used_idx,
            pfn: queue_pfn,
            base_paddr,
            base_vaddr,
            base_len: layout.total_len,
            _dma: dma,
            cacheable,
            used_zero_len_head: None,
            last_error: None,
        })
    }

    /// Descriptors are cleared and rewritten only during the prepare phase. Completion paths leave
    /// descriptor contents intact so late device reads cannot observe a zeroed entry from a newer
    /// generation.
    fn setup_descriptor(
        &self,
        index: u16,
        addr: u64,
        len: u32,
        flags: u16,
        next: Option<u16>,
    ) -> Result<(), DmaError> {
        self.assert_index_in_range(index, "desc");
        let desc_offset = self
            .layout
            .desc_offset
            .saturating_add(core::mem::size_of::<VirtqDesc>().saturating_mul(index as usize));
        self.assert_offset_in_range(desc_offset, core::mem::size_of::<VirtqDesc>(), "desc");
        let flags = match next {
            Some(_) => flags | VIRTQ_DESC_F_NEXT,
            None => flags,
        };
        debug_assert_ne!(len, 0, "descriptor length must be non-zero");
        debug_assert_ne!(addr, 0, "descriptor address must be non-zero");
        let desc = VirtqDesc {
            addr,
            len,
            flags,
            next: next.unwrap_or(0),
        };
        let desc_ptr = unsafe { self.desc.as_ptr().add(index as usize) };
        unsafe { write_volatile(desc_ptr, desc) };
        if FORENSICS {
            let verify = unsafe { read_volatile(desc_ptr) };
            debug_assert_eq!(verify.addr, desc.addr, "descriptor addr mismatch");
            debug_assert_eq!(verify.len, desc.len, "descriptor len mismatch");
            debug_assert_eq!(verify.flags, desc.flags, "descriptor flags mismatch");
            debug_assert_eq!(verify.next, desc.next, "descriptor next mismatch");
        }
        // AArch64 QEMU virtio is non-coherent here; barriers are insufficient; must clean/invalidate rings/descriptors.
        self.clean_desc_entry_for_device(index)
    }

    /// Publish an available descriptor after descriptors have been written and cleaned.
    /// Ordering is enforced with device-scope store barriers so the device never observes a
    /// partially initialised (zeroed) descriptor even when the queue is mapped cacheable.
    /// Per the VirtIO 1.1 spec (§2.6.9), the device may consume descriptors as soon as `avail.idx`
    /// is updated, so the writes must be visible in this exact order:
    /// 1) descriptor writes + cache maintenance (caller, per-entry clean)
    /// 2) release/device fence
    /// 3) avail.ring slot write + per-slot clean
    /// 4) release/device fence
    /// 5) avail.idx write + per-field clean
    /// 6) release/device fence
    /// 7) notify (performed by the caller)
    ///
    /// Why this fixes the wrap abort: QEMU aborted on `len=0` when a stale avail entry raced with
    /// a freshly cleared descriptor. Cleaning the touched bytes and forcing `dmb oshst` between
    /// desc → avail → idx → notify guarantees the device sees the fully populated descriptor chain
    /// before it reuses a wrapped slot.
    fn push_avail(&self, index: u16) -> Result<(u16, u16, u16), DmaError> {
        self.assert_index_in_range(index, "avail");
        let avail = self.avail.as_ptr();
        let qsize = usize::from(self.size);

        let old_avail = u16::from_le(unsafe { read_volatile(&(*avail).idx) });
        let ring_slot = (old_avail as usize) % qsize;
        self.assert_ring_slot(old_avail, ring_slot, "avail");
        unsafe {
            let idx_ptr = &(*avail).idx as *const u16 as usize;
            assert!(
                idx_ptr >= self.base_vaddr,
                "virtqueue avail.idx pointer below base: ptr=0x{idx_ptr:016x} base=0x{base:016x}",
                base = self.base_vaddr,
            );
            let idx_offset = idx_ptr - self.base_vaddr;
            self.assert_offset_in_range(idx_offset, core::mem::size_of::<u16>(), "avail.idx");
            let ring_ptr = (*avail).ring.as_ptr().add(ring_slot as usize) as *mut u16;
            let ring_addr = ring_ptr as usize;
            assert!(
                ring_addr >= self.base_vaddr,
                "virtqueue avail.ring pointer below base: ptr=0x{ring_addr:016x} base=0x{base:016x}",
                base = self.base_vaddr,
            );
            let ring_offset = ring_addr - self.base_vaddr;
            self.assert_offset_in_range(ring_offset, core::mem::size_of::<u16>(), "avail.ring");
            virtq_publish_barrier();
            write_volatile(ring_ptr, index.to_le());
            debug_assert_eq!(
                ring_slot,
                (old_avail as usize) % qsize,
                "tx publish slot mismatch: old_avail={} ring_slot={} qsize={}",
                old_avail,
                ring_slot,
                qsize
            );
            let ring_written = u16::from_le(read_volatile(ring_ptr));
            debug_assert_eq!(
                ring_written, index,
                "tx publish ring write mismatch: slot={} expected_head={} observed_head={} old_avail={}",
                ring_slot, index, ring_written, old_avail
            );
            if DMA_NONCOHERENT {
                self.clean_avail_entry_for_device(ring_slot)?;
            }
            virtq_publish_barrier();
            let new_idx = old_avail.wrapping_add(1);
            write_volatile(&mut (*avail).idx, new_idx.to_le());
            if DMA_NONCOHERENT {
                self.clean_avail_idx_for_device()?;
            }
            virtq_publish_barrier();
            Ok((ring_slot as u16, new_idx, old_avail))
        }
    }

    fn notify(&mut self, regs: &mut VirtioRegs, queue: u32) -> Result<(), DmaError> {
        // Ensure descriptor and avail writes are visible before the MMIO notify.
        dma_barrier();
        virtq_notify_barrier();
        if queue == TX_QUEUE_INDEX {
            log::debug!(target: "virtio-net", "[virtio-net][tx-notify] queue={queue}");
        }
        regs.notify(queue);
        let notify_flag = match queue {
            RX_QUEUE_INDEX => Some(&RX_NOTIFY_LOGGED),
            TX_QUEUE_INDEX => Some(&TX_NOTIFY_LOGGED),
            _ => None,
        };
        match queue {
            RX_QUEUE_INDEX => NET_DIAG.record_rx_kick(),
            TX_QUEUE_INDEX => NET_DIAG.record_tx_kick(),
            _ => {}
        }
        if let Some(flag) = notify_flag {
            if !flag.swap(true, AtomicOrdering::AcqRel) {
                let label = if queue == TX_QUEUE_INDEX { "TX" } else { "RX" };
                info!(target: "virtio-net", "[virtio-net] notify queue={queue} ({label})");
            }
        }
        Ok(())
    }

    fn sync_descriptor_table_for_device(&self) -> Result<(), DmaError> {
        dma_clean(
            self.desc.as_ptr() as *const u8,
            self.layout.desc_len,
            self.cacheable,
            "clean descriptor table",
        )
    }

    fn clean_desc_entry_for_device(&self, index: u16) -> Result<(), DmaError> {
        if !DMA_NONCOHERENT {
            return Ok(());
        }
        let desc_ptr = unsafe { self.desc.as_ptr().add(index as usize) } as *const u8;
        dma_clean(
            desc_ptr,
            core::mem::size_of::<VirtqDesc>(),
            self.cacheable,
            "clean descriptor entry",
        )
    }

    fn sync_avail_ring_for_device(&self) -> Result<(), DmaError> {
        dma_clean(
            self.avail.as_ptr() as *const u8,
            self.layout.avail_len,
            self.cacheable,
            "clean avail ring",
        )
    }

    fn clean_avail_entry_for_device(&self, ring_slot: usize) -> Result<(), DmaError> {
        if !DMA_NONCOHERENT {
            return Ok(());
        }
        let avail = self.avail.as_ptr();
        let ring_ptr = unsafe { (*avail).ring.as_ptr().add(ring_slot) as *const u8 };
        dma_clean(
            ring_ptr,
            core::mem::size_of::<u16>(),
            self.cacheable,
            "clean avail ring slot",
        )
    }

    fn clean_avail_idx_for_device(&self) -> Result<(), DmaError> {
        if !DMA_NONCOHERENT {
            return Ok(());
        }
        let avail = self.avail.as_ptr();
        let idx_ptr = unsafe { &(*avail).idx as *const u16 as *const u8 };
        dma_clean(
            idx_ptr,
            core::mem::size_of::<u16>(),
            self.cacheable,
            "clean avail idx",
        )
    }

    fn invalidate_used_header_for_cpu(&self) -> Result<(), DmaError> {
        let used_ptr = self.used.as_ptr() as *const u8;
        NET_DIAG.record_rx_cache_invalidate();
        dma_invalidate(
            used_ptr,
            core::mem::size_of::<u16>() * 2,
            self.cacheable,
            "invalidate used ring header",
        )?;
        virtq_used_load_barrier();
        Ok(())
    }

    fn invalidate_used_elem_for_cpu(&self, ring_slot: usize) -> Result<(), DmaError> {
        let elem_ptr = unsafe { (*self.used.as_ptr()).ring.as_ptr().add(ring_slot) as *const u8 };
        if !USED_RING_INVALIDATE_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            info!(
                target: "virtio-net",
                "[virtio-net][dma] invalidate used ring entry slot={} addr=0x{addr:016x}",
                ring_slot,
                addr = elem_ptr as usize,
            );
        }
        dma_invalidate(
            elem_ptr,
            core::mem::size_of::<VirtqUsedElem>(),
            self.cacheable,
            "invalidate used ring entry",
        )?;
        virtq_used_load_barrier();
        Ok(())
    }

    fn debug_descriptors(&self, label: &str, count: usize) {
        let max = core::cmp::min(count, self.size as usize);
        for idx in 0..max {
            let desc = unsafe { read_volatile(self.desc.as_ptr().add(idx)) };
            log::debug!(
                target: "net-console",
                "[virtio-net] {label} desc[{idx}]: addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next}",
                addr = desc.addr,
                len = desc.len,
                flags = desc.flags,
                next = desc.next,
            );
        }
    }

    fn read_descriptor(&self, index: u16) -> VirtqDesc {
        unsafe { read_volatile(self.desc.as_ptr().add(index as usize)) }
    }

    fn read_avail_slot(&self, slot: usize) -> u16 {
        let avail = self.avail.as_ptr();
        self.assert_offset_in_range(
            self.layout
                .avail_offset
                .saturating_add(core::mem::size_of::<u16>() * slot),
            core::mem::size_of::<u16>(),
            "avail.ring.read",
        );
        unsafe { u16::from_le(read_volatile((*avail).ring.as_ptr().add(slot))) }
    }

    fn read_avail_idx(&self) -> u16 {
        let avail = self.avail.as_ptr();
        unsafe { u16::from_le(read_volatile(&(*avail).idx)) }
    }

    fn indices(&self) -> (u16, u16) {
        let used = self.used.as_ptr();
        let avail = self.avail.as_ptr();

        if let Err(err) = self.invalidate_used_header_for_cpu() {
            if !DMA_ERROR_LOGGED.swap(true, AtomicOrdering::AcqRel) {
                warn!(
                    target: "net-console",
                    "[virtio-net] used header invalidate failed err={err:?}; freezing queue"
                );
            }
            mark_forensics_frozen();
            return (self.last_used, self.last_used);
        }
        let used_idx = u16::from_le(unsafe { read_volatile(&(*used).idx) });
        let avail_idx = u16::from_le(unsafe { read_volatile(&(*avail).idx) });

        (used_idx, avail_idx)
    }

    fn indices_no_sync(&self) -> (u16, u16) {
        let used = self.used.as_ptr();
        let avail = self.avail.as_ptr();

        let used_idx = u16::from_le(unsafe { read_volatile(&(*used).idx) });
        let avail_idx = u16::from_le(unsafe { read_volatile(&(*avail).idx) });

        (used_idx, avail_idx)
    }

    fn freeze_and_capture(&mut self, reason: &'static str) {
        if mark_forensics_frozen() {
            warn!(
                target: "net-console",
                "[virtio-net][queue] freezing queue activity (reason={reason})"
            );
        }
        self.last_error.get_or_insert(reason);
    }

    pub fn debug_dump(&self, label: &str) {
        let used = self.used.as_ptr();
        let avail = self.avail.as_ptr();

        if let Err(err) = self.invalidate_used_header_for_cpu() {
            warn!(
                target: "net-console",
                "[virtio-net] debug dump skipped cache invalidate err={err:?}"
            );
            return;
        }
        let used_idx = u16::from_le(unsafe { read_volatile(&(*used).idx) });
        let avail_idx = u16::from_le(unsafe { read_volatile(&(*avail).idx) });

        log::info!(
            target: "net-console",
            "[virtio-net] queue {}: size={} last_used={} avail.idx={} used.idx={}",
            label,
            self.size,
            self.last_used,
            avail_idx,
            used_idx,
        );
    }

    fn pop_used(
        &mut self,
        queue_label: &'static str,
        allow_zero_len: bool,
    ) -> Result<Option<(u16, u32, u16)>, ForensicFault> {
        let used = self.used.as_ptr();
        if let Err(err) = self.invalidate_used_header_for_cpu() {
            warn!(
                target: "net-console",
                "[virtio-net] used header invalidate failed queue={} err={err:?}; freezing",
                queue_label
            );
            self.last_error
                .get_or_insert("used_header_invalidate_failed");
            self.freeze_and_capture("used_header_invalidate_failed");
            return Ok(None);
        }
        let idx = u16::from_le(unsafe { read_volatile(&(*used).idx) });
        virtq_used_load_barrier();
        if self.last_used == idx {
            return Ok(None);
        }
        let distance = idx.wrapping_sub(self.last_used);
        if distance > self.size {
            error!(
                target: "net-console",
                "[virtio-net] used ring advanced beyond queue size: last_used={} idx={} size={} distance={}",
                self.last_used,
                idx,
                self.size,
                distance,
            );
            return Ok(None);
        }
        let qsize = usize::from(self.size);

        let ring_slot = (self.last_used as usize) % qsize;
        self.assert_ring_slot(self.last_used, ring_slot, "used");
        let elem_ptr = unsafe { (*used).ring.as_ptr().add(ring_slot) as *const VirtqUsedElem };
        let idx_ptr = unsafe { &(*used).idx as *const u16 as usize };
        assert!(
            idx_ptr >= self.base_vaddr,
            "virtqueue used.idx pointer below base: ptr=0x{idx_ptr:016x} base=0x{base:016x}",
            base = self.base_vaddr,
        );
        let idx_offset = idx_ptr - self.base_vaddr;
        self.assert_offset_in_range(idx_offset, core::mem::size_of::<u16>(), "used.idx");
        let ring_addr = elem_ptr as usize;
        assert!(
            ring_addr >= self.base_vaddr,
            "virtqueue used.ring pointer below base: ptr=0x{ring_addr:016x} base=0x{base:016x}",
            base = self.base_vaddr,
        );
        let ring_offset = ring_addr - self.base_vaddr;
        self.assert_offset_in_range(
            ring_offset,
            core::mem::size_of::<VirtqUsedElem>(),
            "used.ring",
        );
        if let Err(err) = self.invalidate_used_elem_for_cpu(ring_slot) {
            warn!(
                target: "net-console",
                "[virtio-net] used element invalidate failed queue={} slot={} err={err:?}; freezing",
                queue_label,
                ring_slot
            );
            self.last_error
                .get_or_insert("used_element_invalidate_failed");
            self.freeze_and_capture("used_element_invalidate_failed");
            return Ok(None);
        }
        dma_load_barrier();
        let mut elem = unsafe { read_volatile(elem_ptr) };
        let mut elem_len = u32::from_le(elem.len);
        if elem_len == 0 {
            if let Err(err) = self.invalidate_used_elem_for_cpu(ring_slot) {
                warn!(
                    target: "net-console",
                    "[virtio-net] used element retry invalidate failed queue={} slot={} err={err:?}; freezing",
                    queue_label,
                    ring_slot
                );
                self.last_error
                    .get_or_insert("used_element_invalidate_retry_failed");
                self.freeze_and_capture("used_element_invalidate_retry_failed");
                return Ok(None);
            }
            dma_load_barrier();
            let retry = unsafe { read_volatile(elem_ptr) };
            let retry_len = u32::from_le(retry.len);
            if retry_len == 0 && !allow_zero_len {
                let retry_id = u32::from_le(retry.id);
                if !USED_LEN_ZERO_VISIBILITY_LOGGED.swap(true, AtomicOrdering::AcqRel) {
                    warn!(
                        target: "net-console",
                        "[virtio-net] used len zero after re-read: queue={} head={} idx={} ring_slot={}",
                        queue_label,
                        retry_id,
                        self.last_used,
                        ring_slot,
                    );
                }
                return Ok(None);
            }
            elem = retry;
            elem_len = retry_len;
        }
        let elem_id = u32::from_le(elem.id);
        assert!(
            elem_id < u32::from(self.size),
            "virtqueue used.id out of range: id={} size={} ring_slot={} base_vaddr=0x{base:016x}",
            elem_id,
            self.size,
            ring_slot,
            base = self.base_vaddr,
        );
        if elem_len == 0 {
            let head_id = elem_id as u16;
            let desc = unsafe { read_volatile(self.desc.as_ptr().add(head_id as usize)) };
            if allow_zero_len {
                self.used_zero_len_head = None;
            } else {
                if self.used_zero_len_head == Some(head_id) {
                    return Err(ForensicFault {
                        queue_name: queue_label,
                        qsize: self.size,
                        head: head_id,
                        idx: self.last_used,
                        addr: desc.addr,
                        len: elem_len,
                        flags: desc.flags,
                        next: desc.next,
                        reason: ForensicFaultReason::UsedLenZeroRepeat,
                    });
                }
                self.used_zero_len_head = Some(head_id);
                warn!(
                    target: "net-console",
                    "[virtio-net] used len zero: queue={} head={} idx={} ring_slot={} used_len={} desc_addr=0x{addr:016x} desc_len={len} desc_flags=0x{flags:04x} desc_next={next}",
                    queue_label,
                    head_id,
                    self.last_used,
                    ring_slot,
                    elem_len,
                    addr = desc.addr,
                    len = desc.len,
                    flags = desc.flags,
                    next = desc.next,
                );
                return Ok(None);
            }
        }
        if elem_len != 0 {
            self.used_zero_len_head = None;
        }
        if elem_id >= u32::from(self.size) {
            return Err(ForensicFault {
                queue_name: queue_label,
                qsize: self.size,
                head: elem_id as u16,
                idx: self.last_used,
                addr: 0,
                len: elem_len,
                flags: 0,
                next: 0,
                reason: ForensicFaultReason::UsedIdOutOfRange,
            });
        }
        let desc_idx = elem_id as usize;
        let desc = unsafe { read_volatile(self.desc.as_ptr().add(desc_idx)) };
        if desc.addr == 0 || desc.len == 0 {
            return Err(ForensicFault {
                queue_name: queue_label,
                qsize: self.size,
                head: elem_id as u16,
                idx: self.last_used,
                addr: desc.addr,
                len: desc.len,
                flags: desc.flags,
                next: desc.next,
                reason: ForensicFaultReason::UsedDescriptorZero,
            });
        }
        debug!(
            target: "net-console",
            "[virtio-net] pop_used: last_used={} idx={} ring_slot={} id={} len={}",
            self.last_used,
            idx,
            ring_slot,
            elem_id,
            elem_len,
        );
        self.last_used = self.last_used.wrapping_add(1);
        Ok(Some((elem_id as u16, elem_len, ring_slot as u16)))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; 0],
}

#[repr(C)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[derive(Clone, Copy, Debug)]
struct VirtqLayout {
    desc_offset: usize,
    desc_len: usize,
    avail_offset: usize,
    avail_len: usize,
    used_offset: usize,
    used_len: usize,
    total_len: usize,
}

impl VirtqLayout {
    fn compute_vq_layout(queue_size: u16, event_idx: bool) -> Result<Self, DriverError> {
        const AVAIL_ALIGN: usize = 2;
        const USED_ALIGN: usize = 4;

        debug_assert!(AVAIL_ALIGN.is_power_of_two());
        debug_assert!(USED_ALIGN.is_power_of_two());

        let qsize = usize::from(queue_size);
        let desc_off = 0usize;
        let desc_bytes = 16usize.checked_mul(qsize).ok_or(DriverError::NoQueue)?;

        let avail_off = align_up(desc_off + desc_bytes, AVAIL_ALIGN);
        let avail_ring = 2usize.checked_mul(qsize).ok_or(DriverError::NoQueue)?;
        let avail_event = if event_idx { 2usize } else { 0usize };
        let avail_bytes = 4usize
            .checked_add(avail_ring)
            .and_then(|v| v.checked_add(avail_event))
            .ok_or(DriverError::NoQueue)?;

        let used_off = align_up(avail_off + avail_bytes, USED_ALIGN);
        let used_ring = 8usize.checked_mul(qsize).ok_or(DriverError::NoQueue)?;
        let used_event = if event_idx { 2usize } else { 0usize };
        let used_bytes = 4usize
            .checked_add(used_ring)
            .and_then(|v| v.checked_add(used_event))
            .ok_or(DriverError::NoQueue)?;

        let total = used_off
            .checked_add(used_bytes)
            .ok_or(DriverError::NoQueue)?;

        Ok(Self {
            desc_offset: desc_off,
            desc_len: desc_bytes,
            avail_offset: avail_off,
            avail_len: avail_bytes,
            used_offset: used_off,
            used_len: used_bytes,
            total_len: total,
        })
    }
}

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

const DMA_FORCE_CACHE_MAINTENANCE: bool = DMA_NONCOHERENT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DmaError {
    CacheOperationFailed,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CacheOp {
    Clean,
    Invalidate,
}

#[cfg(test)]
static DMA_TEST_HOOK: Mutex<Option<fn(CacheOp, usize, usize)>> = Mutex::new(None);

#[cfg(test)]
fn with_dma_test_hook<F>(hook: Option<fn(CacheOp, usize, usize)>, f: F)
where
    F: FnOnce(),
{
    {
        let mut guard = DMA_TEST_HOOK.lock();
        *guard = hook;
    }
    f();
    let mut guard = DMA_TEST_HOOK.lock();
    *guard = None;
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn dma_clean(ptr: *const u8, len: usize, cacheable: bool, reason: &str) -> Result<(), DmaError> {
    if len == 0 {
        return Ok(());
    }
    let perform_cache_op = cacheable || DMA_FORCE_CACHE_MAINTENANCE;
    #[cfg(test)]
    if let Some(hook) = *DMA_TEST_HOOK.lock() {
        hook(CacheOp::Clean, ptr as usize, len);
        return Ok(());
    }
    if !perform_cache_op {
        if !DMA_SKIP_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            info!(
                target: "virtio-net",
                "[virtio-net][dma] cache ops skipped (mapping non-cacheable)",
            );
        }
        return Ok(());
    }
    if !cacheable && !DMA_FORCE_LOGGED.swap(true, AtomicOrdering::AcqRel) {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] forcing cache maintenance for non-cacheable mapping",
        );
    }
    let log_once = !DMA_CLEAN_LOGGED.swap(true, AtomicOrdering::AcqRel);
    if VIRTIO_DMA_TRACE || log_once {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] cache op reason={reason}",
        );
    }
    if VIRTIO_DMA_TRACE || log_once {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] clean enter ptr=0x{ptr:016x} len={len}",
            ptr = ptr as usize,
            len = len,
        );
    }
    compiler_fence(AtomicOrdering::Release);
    if let Err(err) = cache_clean(seL4_CapInitThreadVSpace, ptr as usize, len) {
        if !DMA_ERROR_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            error!(
                target: "virtio-net",
                "[virtio-net][dma] clean syscall failed err={} ptr=0x{ptr:016x} len={len} reason={reason}",
                err,
                ptr = ptr as usize,
            );
        }
        mark_forensics_frozen();
        return Err(DmaError::CacheOperationFailed);
    }
    if VIRTIO_DMA_TRACE || log_once {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] clean exit ptr=0x{ptr:016x} len={len}",
            ptr = ptr as usize,
            len = len,
        );
    }
    Ok(())
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn dma_invalidate(
    ptr: *const u8,
    len: usize,
    cacheable: bool,
    reason: &str,
) -> Result<(), DmaError> {
    if len == 0 {
        return Ok(());
    }
    let perform_cache_op = cacheable || DMA_FORCE_CACHE_MAINTENANCE;
    #[cfg(test)]
    if let Some(hook) = *DMA_TEST_HOOK.lock() {
        hook(CacheOp::Invalidate, ptr as usize, len);
        return Ok(());
    }
    if !perform_cache_op {
        if !DMA_SKIP_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            info!(
                target: "virtio-net",
                "[virtio-net][dma] cache ops skipped (mapping non-cacheable)",
            );
        }
        return Ok(());
    }
    if !cacheable && !DMA_FORCE_LOGGED.swap(true, AtomicOrdering::AcqRel) {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] forcing cache maintenance for non-cacheable mapping",
        );
    }
    let log_once = !DMA_INVALIDATE_LOGGED.swap(true, AtomicOrdering::AcqRel);
    if VIRTIO_DMA_TRACE || log_once {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] cache op reason={reason}",
        );
    }
    if VIRTIO_DMA_TRACE || log_once {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] invalidate enter ptr=0x{ptr:016x} len={len}",
            ptr = ptr as usize,
            len = len,
        );
    }
    compiler_fence(AtomicOrdering::SeqCst);
    if let Err(err) = cache_invalidate(seL4_CapInitThreadVSpace, ptr as usize, len) {
        if !DMA_ERROR_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            error!(
                target: "virtio-net",
                "[virtio-net][dma] invalidate syscall failed err={} ptr=0x{ptr:016x} len={len} reason={reason}",
                err,
                ptr = ptr as usize,
            );
        }
        mark_forensics_frozen();
        return Err(DmaError::CacheOperationFailed);
    }
    if VIRTIO_DMA_TRACE || log_once {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] invalidate exit ptr=0x{ptr:016x} len={len}",
            ptr = ptr as usize,
            len = len,
        );
    }
    Ok(())
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn dma_barrier() {
    compiler_fence(AtomicOrdering::Release);
    unsafe {
        asm!("dmb oshst", options(nostack, preserves_flags));
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn dma_load_barrier() {
    compiler_fence(AtomicOrdering::Acquire);
    unsafe {
        asm!("dmb oshld", options(nostack, preserves_flags));
    }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn dma_clean(
    ptr: *const u8,
    len: usize,
    _cacheable: bool,
    _reason: &str,
) -> Result<(), DmaError> {
    #[cfg(test)]
    if let Some(hook) = *DMA_TEST_HOOK.lock() {
        hook(CacheOp::Clean, ptr as usize, len);
    }
    Ok(())
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn dma_invalidate(
    ptr: *const u8,
    len: usize,
    _cacheable: bool,
    _reason: &str,
) -> Result<(), DmaError> {
    #[cfg(test)]
    if let Some(hook) = *DMA_TEST_HOOK.lock() {
        hook(CacheOp::Invalidate, ptr as usize, len);
    }
    Ok(())
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn dma_barrier() {
    compiler_fence(AtomicOrdering::Release);
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn dma_load_barrier() {
    compiler_fence(AtomicOrdering::Acquire);
}

fn validate_tx_publish_descriptor(
    head_id: u16,
    slot: u16,
    desc: &VirtqDesc,
    total_len: u32,
    avail_idx: u16,
    used_idx: u16,
    in_flight: u16,
    tx_free: u16,
    logged: &mut bool,
) -> Result<(), ()> {
    if desc.addr == 0 || desc.len == 0 || total_len == 0 {
        if !*logged {
            *logged = true;
            error!(
                target: "net-console",
                "[virtio-net][tx-publish-guard] invalid descriptor blocked: head={} slot={} avail.idx={} used.idx={} in_flight={} tx_free={} desc_addr=0x{addr:016x} desc_len={desc_len} total_len={total_len}",
                head_id,
                slot,
                avail_idx,
                used_idx,
                in_flight,
                tx_free,
                addr = desc.addr,
                desc_len = desc.len,
                total_len = total_len,
            );
        }
        return Err(());
    }
    Ok(())
}

fn reservation_matches(
    head_mgr: &TxHeadManager,
    slots: Option<&TxSlotTracker>,
    reservation: TxReservation,
) -> bool {
    let head_ok = matches!(
        head_mgr.state(reservation.head_id),
        Some(TxHeadState::Prepared { gen }) if gen == reservation.head_gen
    );
    if !head_ok {
        return false;
    }
    if let Some(tracker) = slots {
        match (tracker.state(reservation.head_id), reservation.slot_gen) {
            (Some(TxSlotState::Reserved { gen }), Some(expected)) if gen == expected => true,
            _ => false,
        }
    } else {
        true
    }
}

#[cfg(test)]
mod tx_tests {
    use super::*;

    fn alloc_and_post(mgr: &mut TxHeadManager, id: u16, slot: u16) -> u32 {
        assert_eq!(
            mgr.alloc_head(),
            Some(id),
            "allocation order must remain stable"
        );
        let gen = mgr
            .mark_published(id, slot, 64, 0x1000 + id as u64)
            .expect("publish");
        mgr.note_avail_publish(id, slot, slot).expect("advertise");
        mgr.mark_in_flight(id).expect("in-flight");
        gen
    }

    fn reclaim_once(mgr: &mut TxHeadManager, id: u16) -> bool {
        match mgr.state(id) {
            Some(TxHeadState::InFlight { gen, .. }) => {
                mgr.mark_completed(id, Some(gen)).expect("completed");
                mgr.reclaim_head(id).expect("reclaim");
                true
            }
            Some(TxHeadState::Free | TxHeadState::Completed { .. }) => false,
            _ => panic!("invalid state for reclaim"),
        }
    }

    fn entry_for(mgr: &TxHeadManager, id: u16) -> TxHeadEntry {
        *mgr.entry(id).expect("entry present")
    }

    #[test]
    fn tx_head_reuse_prevented() {
        let mut mgr = TxHeadManager::new(TX_QUEUE_SIZE as u16);
        for idx in 0..TX_QUEUE_SIZE {
            let head = mgr.alloc_head().expect("head available");
            assert_eq!(head, idx as u16, "heads issued sequentially");
            mgr.mark_published(head, idx as u16, 64, 0x2000 + idx as u64)
                .expect("publish");
            mgr.note_avail_publish(head, idx as u16, idx as u16)
                .expect("advertise");
            mgr.mark_in_flight(head).expect("in-flight");
        }
        assert!(mgr.alloc_head().is_none(), "queue should be exhausted");
        for id in 0..TX_QUEUE_SIZE {
            let gen = mgr.generation(id as u16).expect("gen present");
            mgr.mark_completed(id as u16, Some(gen)).expect("completed");
            mgr.reclaim_head(id as u16).expect("reclaim");
        }
        assert_eq!(
            mgr.free_len(),
            TX_QUEUE_SIZE as u16,
            "all heads must return to free list after lifecycle completes"
        );
    }

    #[test]
    fn tx_allocator_never_returns_posted_head() {
        let mut mgr = TxHeadManager::new(2);
        let head = mgr.alloc_head().expect("head available");
        mgr.mark_published(head, 0, 64, 0x1000).expect("publish");
        mgr.note_avail_publish(head, 0, 0).expect("advertise");
        mgr.mark_in_flight(head).expect("in-flight");
        assert!(
            mgr.alloc_head().is_none(),
            "allocator must not hand out posted heads"
        );
        reclaim_once(&mut mgr, head);
        assert_eq!(
            mgr.alloc_head(),
            Some(head),
            "head becomes reusable only after reclaim"
        );
    }

    #[test]
    fn tx_publish_wrap_requires_reclaim_before_reuse() {
        const QSIZE: u16 = 4;
        let mut mgr = TxHeadManager::new(QSIZE);
        let mut avail_idx: u16 = 0;
        let mut in_flight: HeaplessVec<(u16, u32), 8> = HeaplessVec::new();

        for seq in 0..(QSIZE as usize + 2) {
            if mgr.free_len() == 0 {
                let (head, gen) = in_flight.remove(0);
                mgr.mark_completed(head, Some(gen)).expect("completed");
                mgr.reclaim_head(head).expect("reclaim");
            }
            let head = mgr.alloc_head().expect("head available");
            let slot = avail_idx % QSIZE;
            let len = 64 + seq as u32;
            let addr = 0x3000 + seq as u64;
            let gen = mgr.mark_published(head, slot, len, addr).expect("publish");
            mgr.note_avail_publish(head, slot, avail_idx)
                .expect("advertise");
            mgr.mark_in_flight(head).expect("in-flight");

            assert!(
                !in_flight.iter().any(|(h, _)| *h == head),
                "head {} published twice without reclaim",
                head
            );
            assert!(len > 0);
            assert_ne!(addr, 0);

            in_flight.push((head, gen)).expect("track in-flight");
            avail_idx = avail_idx.wrapping_add(1);
        }

        for (head, gen) in in_flight {
            mgr.mark_completed(head, Some(gen)).expect("completed");
            mgr.reclaim_head(head).expect("reclaim");
        }
        assert_eq!(mgr.free_len(), QSIZE);
    }

    #[test]
    fn tx_allocator_reuse_without_reclaim_guarded() {
        let mut mgr = TxHeadManager::new(2);
        let head = mgr.alloc_head().expect("head available");
        mgr.mark_published(head, 0, 64, 0x4000).expect("publish");
        mgr.note_avail_publish(head, 0, 0).expect("advertise");
        mgr.mark_in_flight(head).expect("in-flight");
        let mask = mgr.free_mask_for(head).expect("mask");
        mgr.free_mask |= mask;
        assert!(
            mgr.alloc_head().is_none(),
            "allocator must refuse reuse while head remains in-flight"
        );
    }

    #[test]
    fn tx_mark_inflight_accepts_published_state() {
        let mut mgr = TxHeadManager::new(2);
        let head = mgr.alloc_head().expect("head available");
        let gen = mgr.mark_published(head, 0, 64, 0x1000).expect("publish");
        assert!(
            mgr.in_avail(head),
            "publish should mark head as tracked in avail ring"
        );
        mgr.note_avail_publish(head, 0, 0)
            .expect("advertise before inflight");
        assert_eq!(
            mgr.mark_in_flight(head).expect("in-flight after publish"),
            (0, gen),
            "transition to in-flight should succeed once published"
        );
        assert!(
            mgr.mark_in_flight(head).is_err(),
            "second in-flight transition must be rejected"
        );
    }

    #[test]
    fn tx_promote_to_inflight_before_avail_record() {
        let mut mgr = TxHeadManager::new(2);
        let head = mgr.alloc_head().expect("head available");
        let gen = mgr.mark_published(head, 0, 64, 0x1000).expect("publish");
        assert!(
            mgr.in_avail(head),
            "publish must track head presence before promotion"
        );
        assert!(
            !mgr.is_advertised(head),
            "advertise is deferred to avail record"
        );
        let (slot, promoted_gen) = mgr
            .promote_published_to_inflight(head)
            .expect("promotion to inflight should succeed pre-avail");
        assert_eq!(slot, 0, "slot recorded during promotion");
        assert_eq!(
            promoted_gen, gen,
            "generation must not change during promotion"
        );
        assert!(
            matches!(mgr.state(head), Some(TxHeadState::InFlight { slot: s, gen: g }) if s == 0 && g == gen),
            "state must flip to InFlight before avail write"
        );
        assert!(
            !mgr.publish_present(head),
            "publish record stays clear until avail write"
        );
        assert!(
            !mgr.is_advertised(head),
            "advertise flag remains unset pre-avail"
        );
    }

    #[test]
    fn tx_cannot_publish_same_head_twice() {
        let mut mgr = TxHeadManager::new(2);
        let head = mgr.alloc_head().expect("head available");
        alloc_and_post(&mut mgr, head, 0);
        assert!(
            mgr.mark_published(head, 1, 128, 0x2000).is_err(),
            "duplicate publish must be rejected"
        );
        assert_eq!(
            mgr.free_len(),
            1,
            "posted head must stay reserved until a used entry arrives"
        );
        assert!(matches!(
            mgr.state(head),
            Some(TxHeadState::InFlight { .. })
        ));
    }

    #[test]
    fn tx_duplicate_used_id_ignored() {
        let mut mgr = TxHeadManager::new(3);
        let head = mgr.alloc_head().expect("head available");
        alloc_and_post(&mut mgr, head, 0);
        assert!(reclaim_once(&mut mgr, head), "first reclaim frees the head");
        assert!(
            !reclaim_once(&mut mgr, head),
            "duplicate used id must not free twice"
        );
        assert_eq!(mgr.free_len(), 3, "free list remains intact");
    }

    #[test]
    fn tx_reclaim_clears_after_device_returns() {
        let mut mgr = TxHeadManager::new(1);
        let head = mgr.alloc_head().expect("head available");
        alloc_and_post(&mut mgr, head, 0);
        let posted = entry_for(&mgr, head);
        assert_eq!(posted.last_len, 64, "len retained while posted");
        let gen = mgr.generation(head).expect("generation present");
        mgr.mark_completed(head, Some(gen)).expect("mark completed");
        let reclaimed = entry_for(&mgr, head);
        assert_eq!(
            reclaimed.last_len, 64,
            "reclaim transition must not clear descriptor metadata"
        );
        mgr.reclaim_head(head).expect("free");
        let freed = entry_for(&mgr, head);
        assert_eq!(
            freed.last_len, 64,
            "head metadata remains intact until the next prepare"
        );
        let _ = mgr.alloc_head().expect("head reusable after free");
        let reset = entry_for(&mgr, head);
        assert_eq!(
            reset.last_len, 0,
            "clear-on-prepare ensures descriptors reset only when reused"
        );
    }

    #[test]
    fn tx_generation_advances_per_publish() {
        let mut mgr = TxHeadManager::new(1);
        let head = mgr.alloc_head().expect("head available");
        let gen1 = alloc_and_post(&mut mgr, head, 0);
        reclaim_once(&mut mgr, head);
        let gen2 = alloc_and_post(&mut mgr, head, 1);
        assert!(
            gen2 != gen1,
            "each publish generation must advance even for the same head"
        );
    }

    #[test]
    fn tx_publish_record_prevents_republish_without_reclaim() {
        let mut mgr = TxHeadManager::new(2);
        let head = mgr.alloc_head().expect("head available");
        let gen = mgr.mark_published(head, 0, 64, 0x1000).expect("publish");
        mgr.note_avail_publish(head, 0, 0).expect("avail record");
        mgr.mark_in_flight(head).expect("in-flight");
        assert!(
            mgr.mark_published(head, 1, 64, 0x2000).is_err(),
            "head cannot be republished until reclaim"
        );
        mgr.mark_completed(head, Some(gen)).expect("complete");
        mgr.reclaim_head(head).expect("reclaim");
        let reused = mgr.alloc_head().expect("head reusable after reclaim");
        assert_eq!(reused, head, "head returns to free list after reclaim");
    }

    #[test]
    fn tx_publish_record_taken_on_reclaim() {
        let mut mgr = TxHeadManager::new(1);
        let head = mgr.alloc_head().expect("head available");
        let gen = mgr.mark_published(head, 0, 64, 0x1000).expect("publish");
        mgr.note_avail_publish(head, 0, 5).expect("record publish");
        mgr.mark_in_flight(head).expect("in-flight");
        let record = mgr
            .take_publish_record(head, 0, gen)
            .expect("record fetched");
        assert_eq!(record.avail_idx, 5, "avail idx recorded");
        mgr.mark_completed(head, Some(gen)).expect("complete");
        mgr.reclaim_head(head).expect("reclaim");
        assert_eq!(
            mgr.publish_present[head as usize], false,
            "record cleared after reclaim flag reset"
        );
    }

    #[test]
    fn tx_wrap_publish_and_reclaim_cycle() {
        let mut mgr = TxHeadManager::new(4);
        for idx in 0..6 {
            let head = mgr.alloc_head().expect("head available");
            let slot = (idx % 4) as u16;
            let gen = mgr
                .mark_published(head, slot, 64, 0x3000 + idx as u64)
                .expect("publish");
            mgr.note_avail_publish(head, slot, idx as u16)
                .expect("record publish");
            mgr.mark_in_flight(head).expect("in-flight");
            let record = mgr
                .take_publish_record(head, slot, gen)
                .expect("record retrieved");
            assert_eq!(record.slot, slot, "slot recorded correctly");
            assert_eq!(record.gen, gen, "generation recorded correctly");
            mgr.mark_completed(head, Some(gen)).expect("complete");
            mgr.reclaim_head(head).expect("reclaim");
        }
        assert_eq!(
            mgr.free_len(),
            4,
            "all heads return to free after wrap-around lifecycle"
        );
    }

    #[test]
    fn tx_desc_len_tracks_written_bytes() {
        let header_len = 12usize;
        let requested = 66usize;
        let written = 63usize;
        let total_len =
            VirtioNet::tx_total_len(header_len, written).expect("written length produces total");
        assert_eq!(
            total_len,
            header_len + written,
            "descriptor length must follow written payload bytes"
        );
        assert!(
            VirtioNet::tx_total_len(header_len, 0).is_none(),
            "zero written length must not produce a publishable descriptor"
        );
        assert_eq!(requested, 66, "requested length is not used for publishing");
    }

    #[test]
    fn tx_completion_requires_inflight() {
        let mut mgr = TxHeadManager::new(1);
        let head = mgr.alloc_head().expect("head available");
        let gen = mgr.mark_published(head, 0, 64, 0x1000).expect("publish");
        assert!(
            mgr.mark_completed(head, Some(gen)).is_err(),
            "completion must fail when head is not in-flight"
        );
        assert!(
            mgr.reclaim_head(head).is_err(),
            "head remains unavailable until a used entry is observed"
        );
        assert!(
            matches!(mgr.state(head), Some(TxHeadState::Published { .. })),
            "state remains published after failed completion"
        );
    }

    #[test]
    fn double_advertise_is_blocked() {
        let mut mgr = TxHeadManager::new(2);
        let head = mgr.alloc_head().expect("head available");
        let _gen = mgr.mark_published(head, 0, 64, 0x1000).expect("publish");
        mgr.submit_ready(head, 0).expect("first advertise allowed");
        mgr.note_avail_publish(head, 0, 1)
            .expect("record first publish");
        assert!(
            mgr.submit_ready(head, 0).is_err(),
            "second advertise must be rejected while still tracked in avail"
        );
    }

    #[test]
    fn clear_only_after_reclaim() {
        let mut mgr = TxHeadManager::new(1);
        let head = mgr.alloc_head().expect("head available");
        let gen = mgr.mark_published(head, 0, 64, 0x1000).expect("publish");
        mgr.note_avail_publish(head, 0, 0).expect("advertise noted");
        mgr.mark_in_flight(head).expect("in-flight");
        assert!(
            mgr.reclaim_head(head).is_err(),
            "reclaim must fail while advertised/in-flight"
        );
        mgr.mark_completed(head, Some(gen)).expect("complete");
        assert!(
            mgr.reclaim_head(head).is_ok(),
            "reclaim succeeds only after completion clears advertise tracking"
        );
    }

    #[test]
    fn tx_slot_tracker_invariants_hold() {
        let mut slots = TxSlotTracker::new(2);
        let (s0, _, _) = slots.reserve_next().expect("first reserve");
        let (s1, _, _) = slots.reserve_next().expect("second reserve");
        assert_ne!(s0, s1, "distinct slots allocated");
        assert!(
            slots.reserve_next().is_none(),
            "no additional slots while none are free"
        );
        assert!(matches!(
            slots.mark_in_flight(s0),
            Ok(_),
            "reserved slot transitions to inflight"
        ));
        assert!(
            slots.reserve_next().is_none(),
            "inflight slot cannot be reused until completion"
        );
        assert!(matches!(
            slots.complete(s0),
            Ok(_),
            "completion frees the slot"
        ));
        let (reuse, _, _) = slots
            .reserve_next()
            .expect("slot reusable after completion");
        assert_eq!(reuse, s0, "same slot may be reused after completion");
        assert!(matches!(
            slots.cancel(reuse),
            Ok(_),
            "cancellation permitted only from reserved"
        ));
        assert!(matches!(
            slots.cancel(s1),
            Ok(_),
            "reserved slot can be cancelled"
        ));
    }

    #[test]
    fn tx_slot_tracker_rejects_wrong_states() {
        let mut slots = TxSlotTracker::new(1);
        assert!(matches!(slots.complete(0), Err(TxSlotError::NotInFlight)));
        assert!(matches!(
            slots.mark_in_flight(0),
            Err(TxSlotError::NotReserved)
        ));
        let (slot, _, _) = slots.reserve_next().expect("reserve");
        assert!(matches!(
            slots.cancel(slot),
            Ok(()),
            "cancel allowed from reserved"
        ));
        assert!(matches!(
            slots.cancel(slot),
            Err(TxSlotError::NotReserved),
            "double cancel rejected"
        ));
    }

    #[test]
    fn tx_head_reclaim_is_only_free_path() {
        let mut mgr = TxHeadManager::new(1);
        let head = mgr.alloc_head().expect("head available");
        mgr.mark_published(head, 0, 64, 0x1111).expect("publish");
        mgr.note_avail_publish(head, 0, 0).expect("advertise");
        mgr.mark_in_flight(head).expect("inflight");
        assert!(
            mgr.alloc_specific(head).is_none(),
            "cannot reallocate in-flight"
        );
        assert!(
            mgr.release_unused(head).is_err(),
            "cannot release while active"
        );
        let gen = mgr.generation(head).expect("generation");
        assert!(
            mgr.reclaim_head(head).is_err(),
            "cannot reclaim before completion"
        );
        mgr.mark_completed(head, Some(gen)).expect("complete");
        mgr.reclaim_head(head).expect("reclaim");
        assert_eq!(
            mgr.state(head),
            Some(TxHeadState::Free),
            "only completion+reclaim returns to free"
        );
        assert_eq!(
            mgr.alloc_specific(head),
            Some(head),
            "head reusable after reclaim"
        );
    }

    #[test]
    fn tx_reclaim_requires_used_entry_for_free_with_slots() {
        let mut heads = TxHeadManager::new(1);
        let mut slots = TxSlotTracker::new(1);
        let head = heads.alloc_head().expect("head available");
        let (slot, _, _) = slots.reserve_next().expect("slot reserved");
        assert_eq!(
            slot, head,
            "slot should align with head id for single entry"
        );
        let gen = heads
            .mark_published(head, slot, 64, 0x9999)
            .expect("publish");
        heads.note_avail_publish(head, slot, 0).expect("advertise");
        heads.mark_in_flight(head).expect("inflight");
        slots.mark_in_flight(slot).expect("slot inflight");
        assert!(
            heads.release_unused(head).is_err(),
            "cannot release once published"
        );
        assert!(
            slots.cancel(slot).is_err(),
            "slot cannot be cancelled while inflight"
        );
        assert!(
            matches!(heads.state(head), Some(TxHeadState::InFlight { .. })),
            "head remains inflight after publish"
        );
        assert!(
            matches!(slots.state(slot), Some(TxSlotState::InFlight { .. })),
            "slot tracker tracks inflight state"
        );
        assert_eq!(
            reclaim_used_entry_common(&mut heads, Some(&mut slots), head, slot),
            TxReclaimResult::Reclaimed,
            "reclaim path returns ownership"
        );
        assert!(
            matches!(heads.state(head), Some(TxHeadState::Free)),
            "head returns to free only after reclaim"
        );
        assert!(
            matches!(slots.state(slot), Some(TxSlotState::Free { .. })),
            "slot tracker also frees after reclaim"
        );
        assert_eq!(heads.free_len(), 1, "all heads free after reclaim");
        assert_eq!(slots.free_count(), 1, "slot tracker free count restored");
        let reused = heads.alloc_head().expect("head reusable after reclaim");
        assert_eq!(reused, head, "reclaimed head can be reused");
        let (slot_reuse, _, _) = slots.reserve_next().expect("slot reusable");
        assert_eq!(slot_reuse, slot, "slot reused after completion");
        let _ = heads.generation(head).expect("generation still tracked");
        // Consume the publish lifecycle to ensure reclaim path remains required.
        let gen_reuse = heads
            .mark_published(head, slot_reuse, 64, 0x8888)
            .expect("publish after reclaim");
        heads
            .note_avail_publish(head, slot_reuse, 1)
            .expect("advertise after reclaim");
        heads.mark_in_flight(head).expect("inflight after reclaim");
        slots
            .mark_in_flight(slot_reuse)
            .expect("slot inflight after reclaim");
        assert_eq!(
            gen_reuse,
            gen.wrapping_add(1),
            "generation advances on reuse"
        );
        assert_eq!(
            reclaim_used_entry_common(&mut heads, Some(&mut slots), head, slot_reuse),
            TxReclaimResult::Reclaimed,
            "reclaim frees reused head"
        );
        assert!(
            matches!(heads.state(head), Some(TxHeadState::Free)),
            "head returns to free after second reclaim"
        );
        assert!(
            matches!(slots.state(slot_reuse), Some(TxSlotState::Free { .. })),
            "slot tracker returns to free after second reclaim"
        );
    }

    #[test]
    fn tx_alloc_specific_refuses_non_free() {
        let mut mgr = TxHeadManager::new(1);
        let head = mgr.alloc_specific(0).expect("alloc id 0");
        assert_eq!(head, 0);
        assert!(
            mgr.alloc_specific(0).is_none(),
            "cannot allocate the same head twice without reclaim"
        );
        let gen = mgr.mark_published(head, 0, 64, 0x2222).expect("publish");
        mgr.note_avail_publish(head, 0, 0).expect("advertise");
        mgr.mark_in_flight(head).expect("inflight");
        assert!(
            mgr.alloc_specific(head).is_none(),
            "allocator rejects published heads"
        );
        mgr.mark_completed(head, Some(gen)).expect("complete");
        mgr.reclaim_head(head).expect("reclaim");
        assert_eq!(
            mgr.alloc_specific(head),
            Some(head),
            "allocator accepts after reclaim"
        );
    }

    #[test]
    fn tx_token_single_use_take() {
        let reservation = TxReservation {
            head_id: 3,
            head_gen: 7,
            slot_gen: Some(9),
        };
        let token = VirtioTxToken::new(core::ptr::null_mut(), Some(reservation));
        assert_eq!(
            token.take_reservation(),
            Some(reservation),
            "first take yields reservation"
        );
        assert_eq!(
            token.take_reservation(),
            None,
            "second take returns none"
        );
    }

    #[test]
    fn tx_reservation_matches_current_state() {
        let mut heads = TxHeadManager::new(1);
        let mut slots = TxSlotTracker::new(1);
        let (slot, _, slot_gen) = slots.reserve_next().expect("slot reserved");
        let head = heads.alloc_specific(slot).expect("head allocated");
        let head_gen = heads.generation(head).expect("head gen");
        let reservation = TxReservation {
            head_id: head,
            head_gen,
            slot_gen: Some(slot_gen),
        };
        assert!(
            reservation_matches(&heads, Some(&slots), reservation),
            "reservation should match prepared state"
        );
    }

    #[test]
    fn tx_reservation_rejects_stale_generation() {
        let mut heads = TxHeadManager::new(1);
        let mut slots = TxSlotTracker::new(1);
        let (slot, _, slot_gen) = slots.reserve_next().expect("slot reserved");
        let head = heads.alloc_specific(slot).expect("head allocated");
        let head_gen = heads.generation(head).expect("head gen");
        let stale = TxReservation {
            head_id: head,
            head_gen,
            slot_gen: Some(slot_gen),
        };
        let gen = heads
            .mark_published(head, slot, 64, 0x1000)
            .expect("publish");
        heads.note_avail_publish(head, slot, 0).expect("advertise");
        heads.mark_in_flight(head).expect("inflight");
        slots.mark_in_flight(slot).expect("slot inflight");
        heads.mark_completed(head, Some(gen)).expect("complete");
        heads.reclaim_head(head).expect("reclaim");
        slots.complete(slot).expect("slot complete");

        let (slot_reuse, _, slot_gen_reuse) = slots.reserve_next().expect("slot reuse");
        let head_reuse = heads
            .alloc_specific(slot_reuse)
            .expect("head reuse");
        let head_gen_reuse = heads.generation(head_reuse).expect("head gen");
        let fresh = TxReservation {
            head_id: head_reuse,
            head_gen: head_gen_reuse,
            slot_gen: Some(slot_gen_reuse),
        };
        assert!(
            !reservation_matches(&heads, Some(&slots), stale),
            "stale reservation must not match new generation"
        );
        assert!(
            reservation_matches(&heads, Some(&slots), fresh),
            "fresh reservation should match current state"
        );
    }

    static OP_LOG: Mutex<HeaplessVec<(CacheOp, usize, usize), 192>> =
        Mutex::new(HeaplessVec::new());

    fn log_hook(op: CacheOp, ptr: usize, len: usize) {
        let mut log = OP_LOG.lock();
        let _ = log.push((op, ptr, len));
    }

    #[test]
    fn cache_ops_called_in_right_places() {
        let ptr = 0x1000usize as *const u8;
        OP_LOG.lock().clear();
        with_dma_test_hook(Some(log_hook), || {
            let _ = dma_clean(ptr, 64, true, "test-clean");
            let _ = dma_invalidate(ptr, 64, true, "test-invalidate");
        });
        let log = OP_LOG.lock();
        assert_eq!(log.len(), 2, "both cache ops should be recorded");
        assert_eq!(log[0].0, CacheOp::Clean);
        assert_eq!(log[1].0, CacheOp::Invalidate);
    }

    #[test]
    fn cache_ops_forced_for_uncached_wraparound_metadata() {
        OP_LOG.lock().clear();
        let publishes = usize::from(TX_QUEUE_SIZE) * 2 + 4;
        let desc_ptr = 0x2000usize as *const u8;
        let idx_ptr = 0x4000usize as *const u8;
        with_dma_test_hook(Some(log_hook), || {
            for idx in 0..publishes {
                let slot_ptr = (0x3000usize
                    + (idx % usize::from(TX_QUEUE_SIZE)) * core::mem::size_of::<u16>())
                    as *const u8;
                let _ = dma_clean(desc_ptr, 32, false, "wrap-desc");
                let _ = dma_clean(slot_ptr, core::mem::size_of::<u16>(), false, "wrap-slot");
                let _ = dma_clean(idx_ptr, core::mem::size_of::<u16>(), false, "wrap-idx");
            }
        });
        let log = OP_LOG.lock();
        let expected = publishes * 3;
        assert!(
            log.len() >= expected,
            "cache ops must run for uncached mappings across wraps (got {} expected >= {})",
            log.len(),
            expected
        );
    }

    #[test]
    fn publish_guard_rejects_zero_len_descriptor() {
        let desc = VirtqDesc {
            addr: 0x1000,
            len: 0,
            flags: 0,
            next: 0,
        };
        let mut logged = false;
        let result = validate_tx_publish_descriptor(1, 0, &desc, 0, 2, 1, 0, 4, &mut logged);
        assert!(result.is_err(), "zero-length descriptor must be blocked");
        assert!(logged, "invalid descriptor must log once");
    }

    #[test]
    fn publish_guard_rejects_zero_addr_descriptor() {
        let desc = VirtqDesc {
            addr: 0,
            len: 64,
            flags: 0,
            next: 0,
        };
        let mut logged = false;
        let result = validate_tx_publish_descriptor(2, 1, &desc, 64, 5, 3, 0, 4, &mut logged);
        assert!(result.is_err(), "zero-address descriptor must be blocked");
        assert!(logged, "invalid descriptor must log once");
    }

    #[test]
    fn publish_guard_rejects_zero_total_len() {
        let desc = VirtqDesc {
            addr: 0x1000,
            len: 64,
            flags: 0,
            next: 0,
        };
        let mut logged = false;
        let result = validate_tx_publish_descriptor(1, 0, &desc, 0, 2, 1, 0, 4, &mut logged);
        assert!(result.is_err(), "zero total length must be blocked");
        assert!(logged, "invalid descriptor must log once");
    }

    #[test]
    fn tx_zero_len_used_requires_inflight_state() {
        let mut heads = TxHeadManager::new(1);
        let mut slots = TxSlotTracker::new(1);
        let mut zero_seen = 0;
        let mut zero_log_ms = 0;
        let head = heads.alloc_head().expect("head available");
        let (slot, _, _) = slots.reserve_next().expect("slot reserved");
        record_zero_len_used(
            &mut zero_seen,
            &mut zero_log_ms,
            head,
            slot,
            0,
            0,
            0,
            heads.state(head),
            slots.state(slot),
        );
        let initial_state = heads.state(head);
        let initial_slot_state = slots.state(slot);
        assert_eq!(
            reclaim_used_entry_common(&mut heads, Some(&mut slots), head, slot),
            TxReclaimResult::HeadNotInFlight(initial_state),
            "invalid state rejects reclaim without mutation"
        );
        assert_eq!(zero_seen, 1, "zero-length used increments counter");
        assert_eq!(heads.state(head), initial_state, "head state unchanged");
        assert_eq!(
            slots.state(slot),
            initial_slot_state,
            "slot state unchanged when reclaim blocked"
        );
    }

    #[test]
    fn tx_zero_len_used_reclaims_when_states_match() {
        let mut heads = TxHeadManager::new(1);
        let mut slots = TxSlotTracker::new(1);
        let mut zero_seen = 0;
        let mut zero_log_ms = 0;
        let head = heads.alloc_head().expect("head available");
        let (slot, _, _) = slots.reserve_next().expect("slot reserved");
        let gen = heads
            .mark_published(head, slot, 64, 0x7000)
            .expect("publish");
        heads.note_avail_publish(head, slot, 0).expect("advertise");
        heads.mark_in_flight(head).expect("inflight");
        slots.mark_in_flight(slot).expect("slot inflight");
        record_zero_len_used(
            &mut zero_seen,
            &mut zero_log_ms,
            head,
            slot,
            0,
            0,
            0,
            heads.state(head),
            slots.state(slot),
        );
        assert_eq!(zero_seen, 1, "zero-length used counter increments");
        assert_eq!(
            reclaim_used_entry_common(&mut heads, Some(&mut slots), head, slot),
            TxReclaimResult::Reclaimed,
            "reclaim succeeds when states are inflight"
        );
        assert!(
            matches!(heads.state(head), Some(TxHeadState::Free)),
            "head freed after reclaim"
        );
        assert!(
            matches!(slots.state(slot), Some(TxSlotState::Free { .. })),
            "slot freed after reclaim"
        );
        assert_eq!(
            heads.generation(head),
            Some(gen),
            "generation preserved through completion"
        );
    }

    #[test]
    fn tx_invalid_used_id_does_not_mutate_state() {
        let mut heads = TxHeadManager::new(1);
        let mut slots = TxSlotTracker::new(1);
        assert_eq!(
            reclaim_used_entry_common(&mut heads, Some(&mut slots), 1, 0),
            TxReclaimResult::InvalidId,
            "invalid id is rejected"
        );
        assert_eq!(heads.free_len(), 1, "head table unchanged on invalid id");
        assert_eq!(
            slots.free_count(),
            1,
            "slot tracker unchanged on invalid id"
        );
    }
}
