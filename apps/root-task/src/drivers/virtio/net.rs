// Author: Lukas Bower
//! Virtio MMIO network device driver used by the root task.
//!
//! Virtio MMIO network device driver used by the root task on the ARM `virt`
//! platform. RX descriptor handling and smoltcp integration are instrumented to
//! aid debugging end-to-end TCP console flows.
#![cfg(all(feature = "kernel", feature = "net-console"))]
#![allow(unsafe_code)]

use core::arch::asm;
use core::fmt::{self, Write as FmtWrite};
use core::ptr::read_unaligned;
use core::ptr::{read_volatile, write_volatile, NonNull};
use core::sync::atomic::{compiler_fence, fence, AtomicBool, AtomicU32, Ordering as AtomicOrdering};

use heapless::{String as HeaplessString, Vec as HeaplessVec};
use log::{debug, error, info, warn};
use sel4_sys::{seL4_Error, seL4_NotEnoughMemory, seL4_PageBits};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;

use crate::bootstrap::bootinfo_snapshot::BootInfoState;
use crate::bootstrap::log::{uart_puthex_u64, uart_putnl, uart_puts};
use crate::hal::cache::{cache_clean, cache_invalidate};
use crate::hal::{HalError, Hardware};
use crate::net::{NetDevice, NetDeviceCounters, NetDriverError, CONSOLE_TCP_PORT};
use crate::net_consts::MAX_FRAME_LEN;
use crate::sel4::{seL4_CapInitThreadVSpace, DeviceFrame, RamFrame};

const FORENSICS: bool = true;
const FORENSICS_PUBLISH_LOG_LIMIT: u32 = 64;
const NET_VIRTIO_TX_V2: bool = cfg!(feature = "net-virtio-tx-v2");
const VIRTIO_DMA_OVERKILL: bool = false;

const VIRTIO_MMIO_BASE: usize = 0x0a00_0000;
const VIRTIO_MMIO_STRIDE: usize = 0x200;
const VIRTIO_MMIO_SLOTS: usize = 8;

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

const RX_QUEUE_INDEX: u32 = 0;
const TX_QUEUE_INDEX: u32 = 1;

const RX_QUEUE_SIZE: usize = 16;
const TX_QUEUE_SIZE: usize = 16;
const MAX_QUEUE_SIZE: usize = if RX_QUEUE_SIZE > TX_QUEUE_SIZE {
    RX_QUEUE_SIZE
} else {
    TX_QUEUE_SIZE
};
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
static DMA_SKIP_LOGGED: AtomicBool = AtomicBool::new(false);
static DMA_QMEM_LOGGED: AtomicBool = AtomicBool::new(false);
static USED_RING_INVALIDATE_LOGGED: AtomicBool = AtomicBool::new(false);
static VQ_LAYOUT_LOGGED: AtomicBool = AtomicBool::new(false);
static RING_SLOT_CANARY_LOGGED: [AtomicBool; VIRTIO_MMIO_SLOTS] =
    [const { AtomicBool::new(false) }; VIRTIO_MMIO_SLOTS];
static FORENSICS_FROZEN: AtomicBool = AtomicBool::new(false);
static FORENSICS_DUMPED: AtomicBool = AtomicBool::new(false);
static TX_WRAP_DMA_LOGGED: AtomicBool = AtomicBool::new(false);
static DMA_OVERKILL_LOGGED: AtomicBool = AtomicBool::new(false);
static BOOTINFO_ADDR_DIAG_EMITTED: AtomicBool = AtomicBool::new(false);

const BOOTINFO_ADDR_DIAG: bool = matches!(option_env!("BOOTINFO_ADDR_DIAG"), Some("1"));

const RL_TAGS: usize = 10;
static RL_COUNTERS: [AtomicU32; RL_TAGS] = [const { AtomicU32::new(0) }; RL_TAGS];

#[derive(Clone, Copy, Debug)]
enum RlTag {
    TxNotify,
    TxPublish,
    TxPoll,
    MmioReadback,
    DescRaw,
    Overkill,
    UsedZero,
    QueueRanges,
    NetDump,
    AvailScan,
}

impl RlTag {
    const fn idx(self) -> usize {
        match self {
            Self::TxNotify => 0,
            Self::TxPublish => 1,
            Self::TxPoll => 2,
            Self::MmioReadback => 3,
            Self::DescRaw => 4,
            Self::Overkill => 5,
            Self::UsedZero => 6,
            Self::QueueRanges => 7,
            Self::NetDump => 8,
            Self::AvailScan => 9,
        }
    }
}

fn rl(tag: RlTag, every: u32) -> bool {
    if every == 0 {
        return false;
    }
    let idx = tag.idx();
    let count = RL_COUNTERS[idx].fetch_add(1, AtomicOrdering::Relaxed);
    count % every == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TxState {
    Free,
    Posted { len: u32, addr: u64, gen: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TxV2State {
    Free,
    Posted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TxAnomalyReason {
    SmoltcpRequestedZeroLen,
    ClosureWroteZero,
    DescLenZero,
    DescAddrZero,
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

fn bootinfo_probe(mark: &'static str) {
    if let Some(state) = BootInfoState::get() {
        if let Err(_err) = state.verify("net.mmio", mark) {
            panic!("BOOTINFO_SNAPSHOT_CORRUPTED");
        }
        let region = state.snapshot_region();
        let (pre, post) = state.canary_values();
        info!(
            target: "net-console",
            "[bootinfo:net] mark={mark} region=[0x{start:016x}..0x{end:016x}) len=0x{len:08x} pre=0x{pre:016x} post=0x{post:016x}",
            start = region.start,
            end = region.end,
            len = region.end.saturating_sub(region.start),
        );
    }
}

fn emit_addr_diag_once(rx_queue: &VirtQueue, tx_queue: &VirtQueue) {
    if !BOOTINFO_ADDR_DIAG {
        return;
    }
    if BOOTINFO_ADDR_DIAG_EMITTED.swap(true, AtomicOrdering::AcqRel) {
        return;
    }
    if let Some(state) = BootInfoState::get() {
        let region = state.snapshot_region();
        let post_addr = state.snapshot().post_canary_addr();
        uart_puts(b"diag.bootinfo.region=0x");
        uart_puthex_u64(region.start as u64);
        uart_puts(b"..0x");
        uart_puthex_u64(region.end as u64);
        uart_putnl();
        uart_puts(b"diag.bootinfo.post_addr=0x");
        uart_puthex_u64(post_addr as u64);
        uart_putnl();
    }

    uart_ring_diag(b"diag.virtio.ring.rx_desc=0x", rx_queue, rx_queue.layout.desc_offset, rx_queue.layout.desc_len);
    uart_ring_diag(
        b"diag.virtio.ring.rx_avail=0x",
        rx_queue,
        rx_queue.layout.avail_offset,
        rx_queue.layout.avail_len,
    );
    uart_ring_diag(
        b"diag.virtio.ring.rx_used=0x",
        rx_queue,
        rx_queue.layout.used_offset,
        rx_queue.layout.used_len,
    );
    uart_ring_diag(b"diag.virtio.ring.tx_desc=0x", tx_queue, tx_queue.layout.desc_offset, tx_queue.layout.desc_len);
    uart_ring_diag(
        b"diag.virtio.ring.tx_avail=0x",
        tx_queue,
        tx_queue.layout.avail_offset,
        tx_queue.layout.avail_len,
    );
    uart_ring_diag(
        b"diag.virtio.ring.tx_used=0x",
        tx_queue,
        tx_queue.layout.used_offset,
        tx_queue.layout.used_len,
    );
}

fn uart_ring_diag(label: &[u8], queue: &VirtQueue, offset: usize, len: usize) {
    uart_puts(label);
    uart_puthex_u64(queue.base_paddr.saturating_add(offset) as u64);
    uart_puts(b" len=0x");
    uart_puthex_u64(len as u64);
    uart_putnl();
}

fn uart_ring_oob(tag: &str, a_label: &[u8], a: usize, b_label: &[u8], b: usize, c_label: &[u8], c: usize) {
    uart_puts(b"VIRTIO_RING_OOB ");
    uart_puts(tag.as_bytes());
    uart_puts(b" ");
    uart_puts(a_label);
    uart_puthex_u64(a as u64);
    uart_puts(b" ");
    uart_puts(b_label);
    uart_puthex_u64(b as u64);
    uart_puts(b" ");
    uart_puts(c_label);
    uart_puthex_u64(c as u64);
    uart_putnl();
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

/// Virtio-net MMIO implementation providing a smoltcp PHY device.
pub struct VirtioNet {
    regs: VirtioRegs,
    mmio_mode: VirtioMmioMode,
    rx_queue: VirtQueue,
    tx_queue: VirtQueue,
    rx_buffers: HeaplessVec<RamFrame, RX_QUEUE_SIZE>,
    tx_buffers: HeaplessVec<RamFrame, TX_QUEUE_SIZE>,
    tx_free: HeaplessVec<u16, TX_QUEUE_SIZE>,
    tx_v2_free: HeaplessVec<u16, TX_QUEUE_SIZE>,
    tx_v2_last_used: u16,
    tx_v2_state: [TxV2State; TX_QUEUE_SIZE],
    tx_v2_gen_counter: u32,
    tx_v2_gen: [u32; TX_QUEUE_SIZE],
    tx_v2_len: [u32; TX_QUEUE_SIZE],
    tx_v2_last_published_seq: [u32; TX_QUEUE_SIZE],
    tx_v2_last_used_seq: [u32; TX_QUEUE_SIZE],
    tx_v2_publish_seq: u32,
    tx_v2_recent_head: [u16; 16],
    tx_v2_recent_len: [u32; 16],
    tx_v2_recent_seq: [u32; 16],
    tx_v2_last_published_head: u16,
    tx_v2_last_published_len: u32,
    tx_v2_last_published_seq_latest: u32,
    tx_v2_used_zero_streak: u8,
    tx_states: [TxState; TX_QUEUE_SIZE],
    dma_cacheable: bool,
    tx_in_flight: u16,
    tx_gen: u32,
    tx_last_used_seen: u16,
    tx_progress_log_gate: u32,
    negotiated_features: u64,
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
    tx_state_violation_logged: bool,
    tx_anomaly_logged: bool,
    tx_descriptor_dumped: bool,
    tx_used_window_dumped: bool,
    tx_dma_log_once: bool,
    tx_publish_verify_count: u32,
    rx_zero_len_logged: bool,
    tx_zero_len_logged: bool,
    rx_header_zero_logged: bool,
    rx_payload_zero_logged: bool,
    tx_used_zero_streak: u8,
    tx_used_recent: HeaplessVec<(u16, u32), TX_QUEUE_SIZE>,
    tx_wrap_logged: bool,
    rx_underflow_logged_ids: HeaplessVec<u16, RX_QUEUE_SIZE>,
    last_error: Option<&'static str>,
    rx_requeue_logged_ids: HeaplessVec<u16, RX_QUEUE_SIZE>,
    rx_publish_log_count: u32,
    tx_publish_log_count: u32,
    tx_double_submit: u64,
    tx_zero_len_attempt: u64,
    tx_submit: u64,
    tx_complete: u64,
    tx_v2_log_ms: u64,
    tx_v2_used_zero_log_ms: u64,
    mmio_dumped_init: bool,
    mmio_dumped_tx_first: bool,
    forensic_dump_captured: bool,
}

impl VirtioNet {
    /// Create a new driver instance by probing the virtio MMIO slots.
    pub fn new<H>(hal: &mut H) -> Result<Self, DriverError>
    where
        H: Hardware<Error = HalError>,
    {
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
            "[net-console] virtio-mmio device located: paddr=0x{paddr:08x} vaddr=0x{vaddr:08x}",
            paddr = regs.mmio_paddr(),
            vaddr = regs.base().as_ptr() as usize
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
        bootinfo_probe("net.init.after_features_ok");
        if status_after_features & STATUS_FEATURES_OK == 0 {
            regs.set_status(STATUS_FAILED);
            error!(
                target: "net-console",
                "[virtio-net] device rejected FEATURES_OK: status=0x{status_after_features:02x}"
            );
            return Err(DriverError::NoQueue);
        }

        bootinfo_probe("net.init.before_qmem_log");
        info!("[net-console] allocating virtqueue backing memory");
        bootinfo_probe("net.init.after_qmem_log");
        bootinfo_probe("net.mmio.qmem.before");

        let queue_mem_rx = hal.alloc_dma_frame().map_err(|err| {
            regs.set_status(STATUS_FAILED);
            DriverError::from(err)
        })?;
        bootinfo_probe("net.mmio.qmem.rx.alloc");

        let queue_mem_tx = {
            let mut attempt = 0;
            loop {
                let frame = hal.alloc_dma_frame().map_err(|err| {
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
        bootinfo_probe("net.mmio.qmem.tx.alloc");
        let dma_cacheable = cfg!(feature = "cache-maintenance");
        if !DMA_QMEM_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            let rx_vaddr = queue_mem_rx.ptr().as_ptr() as usize;
            let tx_vaddr = queue_mem_tx.ptr().as_ptr() as usize;
            let rx_len = queue_mem_rx.as_slice().len();
            let tx_len = queue_mem_tx.as_slice().len();
            let rx_paddr = queue_mem_rx.paddr();
            let tx_paddr = queue_mem_tx.paddr();
            info!(
                target: "virtio-net",
                "[virtio-net][dma] qmem mapping cacheable={} map_attr=seL4_ARM_Page_Default rx_vaddr=0x{rx_vaddr:016x}..0x{rx_vend:016x} rx_paddr=0x{rx_paddr:016x}..0x{rx_pend:016x} tx_vaddr=0x{tx_vaddr:016x}..0x{tx_vend:016x} tx_paddr=0x{tx_paddr:016x}..0x{tx_pend:016x}",
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
        )
        .map_err(|err| {
            regs.set_status(STATUS_FAILED);
            err
        })?;
        bootinfo_probe("net.mmio.before");
        regs.read_queue_regs(RX_QUEUE_INDEX);
        bootinfo_probe("net.mmio.after");
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
        )
        .map_err(|err| {
            regs.set_status(STATUS_FAILED);
            err
        })?;
        regs.read_queue_regs(TX_QUEUE_INDEX);
        emit_addr_diag_once(&rx_queue, &tx_queue);

        let mut rx_buffers = HeaplessVec::<RamFrame, RX_QUEUE_SIZE>::new();
        for _ in 0..rx_size {
            let frame = hal.alloc_dma_frame().map_err(|err| {
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
            let frame = hal.alloc_dma_frame().map_err(|err| {
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

        let mut tx_free = HeaplessVec::<u16, TX_QUEUE_SIZE>::new();
        for idx in 0..tx_size {
            tx_free.push(idx as u16).map_err(|_| {
                regs.set_status(STATUS_FAILED);
                DriverError::BufferExhausted
            })?;
        }

        let mut tx_v2_free = HeaplessVec::<u16, TX_QUEUE_SIZE>::new();
        for idx in 0..tx_size {
            tx_v2_free.push(idx as u16).map_err(|_| {
                regs.set_status(STATUS_FAILED);
                DriverError::BufferExhausted
            })?;
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
            rx_queue,
            tx_queue,
            rx_buffers,
            tx_buffers,
            tx_free,
            tx_v2_free,
            tx_v2_last_used: 0,
            tx_v2_state: [TxV2State::Free; TX_QUEUE_SIZE],
            tx_v2_gen_counter: 1,
            tx_v2_gen: [0; TX_QUEUE_SIZE],
            tx_v2_len: [0; TX_QUEUE_SIZE],
            tx_v2_last_published_seq: [0; TX_QUEUE_SIZE],
            tx_v2_last_used_seq: [0; TX_QUEUE_SIZE],
            tx_v2_publish_seq: 1,
            tx_v2_recent_head: [0; 16],
            tx_v2_recent_len: [0; 16],
            tx_v2_recent_seq: [0; 16],
            tx_v2_last_published_head: 0,
            tx_v2_last_published_len: 0,
            tx_v2_last_published_seq_latest: 0,
            tx_v2_used_zero_streak: 0,
            tx_states: [TxState::Free; TX_QUEUE_SIZE],
            tx_in_flight: 0,
            tx_gen: 1,
            tx_last_used_seen: 0,
            tx_progress_log_gate: 0,
            negotiated_features,
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
            tx_state_violation_logged: false,
            tx_anomaly_logged: false,
            tx_descriptor_dumped: false,
            tx_used_window_dumped: false,
            tx_dma_log_once: false,
            tx_publish_verify_count: 0,
            rx_zero_len_logged: false,
            tx_zero_len_logged: false,
            rx_header_zero_logged: false,
            rx_payload_zero_logged: false,
            tx_used_zero_streak: 0,
            tx_used_recent: HeaplessVec::new(),
            tx_wrap_logged: false,
            rx_underflow_logged_ids: HeaplessVec::new(),
            last_error: None,
            rx_requeue_logged_ids: HeaplessVec::new(),
            rx_publish_log_count: 0,
            tx_publish_log_count: 0,
            tx_double_submit: 0,
            tx_zero_len_attempt: 0,
            tx_submit: 0,
            tx_complete: 0,
            tx_v2_log_ms: now_ms,
            tx_v2_used_zero_log_ms: now_ms,
            mmio_dumped_init: false,
            mmio_dumped_tx_first: false,
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
        driver.regs.read_queue_regs(RX_QUEUE_INDEX);
        driver.regs.read_queue_regs(TX_QUEUE_INDEX);
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
            "[virtio-net] debug_snapshot: stalled_ms={} status=0x{:02x} isr=0x{:02x} tx_avail_idx={} tx_used_idx={} rx_avail_idx={} rx_used_idx={} last_error={} rx_used_count={} rx_poll_count={}",
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
        );
    }

    pub fn debug_dump_forensics(&mut self) {
        self.log_mmio_state("netdump");
        self.check_queue_ranges_and_fingerprint();
        self.dump_tx_window_small();
        self.dump_tx_used_window_small();
        self.dump_rx_window_small();
        if NET_VIRTIO_TX_V2 {
            self.dump_tx_v2_states_compact();
        }
    }

    fn log_mmio_state_once(&mut self, reason: &'static str) {
        if reason == "init" && self.mmio_dumped_init {
            return;
        }
        if reason == "first_tx" && self.mmio_dumped_tx_first {
            return;
        }
        self.log_mmio_state(reason);
        if reason == "init" {
            self.mmio_dumped_init = true;
        }
        if reason == "first_tx" {
            self.mmio_dumped_tx_first = true;
        }
    }

    fn log_mmio_state(&mut self, reason: &'static str) {
        if reason == "notify" && !rl(RlTag::MmioReadback, 256) {
            return;
        }
        let _ = self.regs.read_queue_regs(RX_QUEUE_INDEX);
        let _ = self.regs.read_queue_regs(TX_QUEUE_INDEX);
    }

    fn notify_queue(&mut self, queue: u32, label: &'static str) {
        let sel = self.regs.queue_sel();
        let expected = if label == "TX" { TX_QUEUE_INDEX } else { RX_QUEUE_INDEX };
        if label == "TX" && queue != expected {
            warn!(
                target: "virtio-forensics",
                "[virtio-forensics] notify mismatch label={} expected_qidx={} actual_qidx={} sel={}",
                label,
                expected,
                queue,
                sel,
            );
            self.freeze_and_capture("notify_wrong_queue");
            return;
        }
        if rl(RlTag::TxNotify, 256) {
            let (rx_used_idx, rx_avail_idx) = self.rx_queue.indices();
            let (tx_used_idx, tx_avail_idx) = self.tx_queue.indices();
            info!(
                target: "virtio-forensics",
                "[virtio-forensics] notify label={} qidx={} sel={} status=0x{:02x} isr=0x{:02x} rx(desc=0x{:016x} avail=0x{:016x} used=0x{:016x} size={}) tx(desc=0x{:016x} avail=0x{:016x} used=0x{:016x} size={}) rx_idx(a/u/l)=({}/{}/{}) tx_idx(a/u/l)=({}/{}/{})",
                label,
                queue,
                sel,
                self.regs.status(),
                self.regs.isr_status(),
                self.rx_queue.base_paddr + self.rx_queue.layout.desc_offset,
                self.rx_queue.base_paddr + self.rx_queue.layout.avail_offset,
                self.rx_queue.base_paddr + self.rx_queue.layout.used_offset,
                self.rx_queue.size,
                self.tx_queue.base_paddr + self.tx_queue.layout.desc_offset,
                self.tx_queue.base_paddr + self.tx_queue.layout.avail_offset,
                self.tx_queue.base_paddr + self.tx_queue.layout.used_offset,
                self.tx_queue.size,
                rx_avail_idx,
                rx_used_idx,
                self.rx_queue.last_used,
                tx_avail_idx,
                tx_used_idx,
                self.tx_queue.last_used,
            );
        }
        if label == "TX" && rl(RlTag::MmioReadback, 256) {
            self.log_mmio_state("notify");
        }
        if label == "RX" {
            self.rx_queue.notify(&mut self.regs, queue);
        } else {
            self.tx_queue.notify(&mut self.regs, queue);
        }
    }

    fn check_queue_ranges_and_fingerprint(&mut self) {
        let rx_desc_start = self.rx_queue.base_paddr + self.rx_queue.layout.desc_offset;
        let rx_desc_end = rx_desc_start + self.rx_queue.layout.desc_len;
        let rx_avail_start = self.rx_queue.base_paddr + self.rx_queue.layout.avail_offset;
        let rx_avail_end = rx_avail_start + self.rx_queue.layout.avail_len;
        let rx_used_start = self.rx_queue.base_paddr + self.rx_queue.layout.used_offset;
        let rx_used_end = rx_used_start + self.rx_queue.layout.used_len;

        let tx_desc_start = self.tx_queue.base_paddr + self.tx_queue.layout.desc_offset;
        let tx_desc_end = tx_desc_start + self.tx_queue.layout.desc_len;
        let tx_avail_start = self.tx_queue.base_paddr + self.tx_queue.layout.avail_offset;
        let tx_avail_end = tx_avail_start + self.tx_queue.layout.avail_len;
        let tx_used_start = self.tx_queue.base_paddr + self.tx_queue.layout.used_offset;
        let tx_used_end = tx_used_start + self.tx_queue.layout.used_len;

        if rl(RlTag::QueueRanges, 1) {
            info!(
                target: "virtio-forensics",
                "[virtio-forensics] queue ranges rx desc=0x{:x}..0x{:x} avail=0x{:x}..0x{:x} used=0x{:x}..0x{:x} | tx desc=0x{:x}..0x{:x} avail=0x{:x}..0x{:x} used=0x{:x}..0x{:x}",
                rx_desc_start,
                rx_desc_end,
                rx_avail_start,
                rx_avail_end,
                rx_used_start,
                rx_used_end,
                tx_desc_start,
                tx_desc_end,
                tx_avail_start,
                tx_avail_end,
                tx_used_start,
                tx_used_end,
            );
        }

        let rx_total_start = rx_desc_start;
        let rx_total_end = rx_used_end;
        let tx_total_start = tx_desc_start;
        let tx_total_end = tx_used_end;
        if ranges_overlap(rx_total_start, rx_total_end, tx_total_start, tx_total_end) {
            self.freeze_and_capture("queue_range_overlap");
        }

        let rx_fp = desc_fingerprint_safe(&self.rx_queue);
        let tx_fp = desc_fingerprint_safe(&self.tx_queue);
        info!(
            target: "virtio-forensics",
            "[virtio-forensics] queue fingerprint rx=0x{:016x} tx=0x{:016x} rx_base=0x{:x} tx_base=0x{:x}",
            rx_fp,
            tx_fp,
            self.rx_queue.base_paddr,
            self.tx_queue.base_paddr,
        );
        if self.rx_queue.base_paddr == self.tx_queue.base_paddr && rx_fp == tx_fp {
            warn!(
                target: "virtio-forensics",
                "[virtio-forensics] queue base collision rx_base=0x{:x} tx_base=0x{:x}",
                self.rx_queue.base_paddr,
                self.tx_queue.base_paddr,
            );
        }
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
        if NET_VIRTIO_TX_V2 {
            self.dump_tx_v2_states();
        }
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
            let Some(desc) = self.tx_queue.read_descriptor_checked(head) else {
                info!(
                    target: "virtio-net",
                    "[virtio-net][forensics] tx avail[{ring_slot}] -> head={head} desc=invalid",
                );
                continue;
            };
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
        self.tx_queue.invalidate_used_header_for_cpu();
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
            self.tx_queue.invalidate_used_elem_for_cpu(ring_slot);
            let elem = unsafe { read_volatile(ring_ptr) };
            info!(
                target: "net-console",
                "[virtio-net][forensics] tx used[{ring_slot}] -> id={} len={}",
                elem.id,
                elem.len,
            );
        }
    }

    fn dump_tx_window_small(&self) {
        let qsize = usize::from(self.tx_queue.size);
        if qsize == 0 {
            return;
        }
        let avail = self.tx_queue.avail.as_ptr();
        let avail_idx = unsafe { read_volatile(&(*avail).idx) };
        let window = core::cmp::min(qsize, 8);
        let start = avail_idx.wrapping_sub(window as u16);
        for offset in 0..window {
            let slot_idx = start.wrapping_add(offset as u16);
            let ring_slot = (slot_idx as usize) % qsize;
            let ring_ptr = unsafe { (*avail).ring.as_ptr().add(ring_slot) as *const u16 };
            let head = unsafe { read_volatile(ring_ptr) };
            let Some(desc) = self.tx_queue.read_descriptor_checked(head) else {
                info!(
                    target: "virtio-forensics",
                    "[virtio-forensics] tx avail[{ring_slot}] head={head} desc=invalid",
                );
                continue;
            };
            info!(
                target: "virtio-forensics",
                "[virtio-forensics] tx avail[{ring_slot}] head={head} desc.addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next}",
                addr = desc.addr,
                len = desc.len,
                flags = desc.flags,
                next = desc.next,
            );
        }
    }

    fn dump_tx_used_window_small(&self) {
        let qsize = usize::from(self.tx_queue.size);
        if qsize == 0 {
            return;
        }
        let used = self.tx_queue.used.as_ptr();
        self.tx_queue.invalidate_used_header_for_cpu();
        let used_idx = unsafe { read_volatile(&(*used).idx) };
        let last_used = self.tx_queue.last_used;
        let window = core::cmp::min(qsize, 8);
        let start = last_used.wrapping_sub(window as u16 / 2);
        for offset in 0..window {
            let slot_idx = start.wrapping_add(offset as u16);
            let ring_slot = (slot_idx as usize) % qsize;
            let ring_ptr = unsafe { (*used).ring.as_ptr().add(ring_slot) as *const VirtqUsedElem };
            self.tx_queue.invalidate_used_elem_for_cpu(ring_slot);
            let elem = unsafe { read_volatile(ring_ptr) };
            info!(
                target: "virtio-forensics",
                "[virtio-forensics] tx used[{ring_slot}] id={} len={} used.idx={}",
                elem.id,
                elem.len,
                used_idx,
            );
        }
    }

    fn dump_tx_states(&self) {
        let mut free = 0usize;
        let mut posted = HeaplessVec::<(usize, u32, u64, u32), TX_QUEUE_SIZE>::new();
        for (idx, state) in self.tx_states.iter().enumerate() {
            match state {
                TxState::Free => free += 1,
                TxState::Posted { len, addr, gen } => {
                    let _ = posted.push((idx, *len, *addr, *gen));
                }
            }
        }
        info!(
            target: "net-console",
            "[virtio-net][forensics] tx states: free={} posted={} in_flight={} tx_free_len={} tx_gen={} last_used={} used.idx={} avail.idx={}",
            free,
            posted.len(),
            self.tx_in_flight,
            self.tx_free.len(),
            self.tx_gen,
            self.tx_queue.last_used,
            self.tx_queue.indices().0,
            self.tx_queue.indices().1,
        );
        for (idx, len, addr, gen) in posted {
            info!(
                target: "net-console",
                "[virtio-net][forensics] tx posted id={id} len={len} addr=0x{addr:016x} gen={gen} ",
                id = idx,
                len = len,
                addr = addr,
                gen = gen,
            );
        }
    }

    fn dump_tx_v2_states(&self) {
        let mut free = 0usize;
        let mut posted = 0usize;
        for idx in 0..self.tx_queue.size {
            let state = self.tx_v2_state.get(idx as usize).copied().unwrap_or(TxV2State::Free);
            match state {
                TxV2State::Free => free += 1,
                TxV2State::Posted => posted += 1,
            }
            info!(
                target: "net-console",
                "[virtio-net][forensics] tx-v2 state id={} state={:?} gen={} len={} pub_seq={} used_seq={}",
                idx,
                state,
                self.tx_v2_gen.get(idx as usize).copied().unwrap_or(0),
                self.tx_v2_len.get(idx as usize).copied().unwrap_or(0),
                self.tx_v2_last_published_seq
                    .get(idx as usize)
                    .copied()
                    .unwrap_or(0),
                self.tx_v2_last_used_seq.get(idx as usize).copied().unwrap_or(0),
            );
        }
        let sample_len = core::cmp::min(self.tx_v2_free.len(), 8);
        let mut sample = HeaplessVec::<u16, 8>::new();
        for idx in 0..sample_len {
            if let Some(id) = self.tx_v2_free.get(idx).copied() {
                let _ = sample.push(id);
            }
        }
        info!(
            target: "net-console",
            "[virtio-net][forensics] tx-v2 summary free={} posted={} free_sample={:?}",
            free,
            posted,
            sample,
        );
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
            let Some(desc) = self.rx_queue.read_descriptor_checked(head) else {
                info!(
                    target: "virtio-forensics",
                    "[virtio-forensics] rx avail[{ring_slot}] head={head} desc=invalid",
                );
                continue;
            };
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

    fn dump_rx_window_small(&self) {
        let qsize = usize::from(self.rx_queue.size);
        if qsize == 0 {
            return;
        }
        let avail = self.rx_queue.avail.as_ptr();
        let avail_idx = unsafe { read_volatile(&(*avail).idx) };
        let window = core::cmp::min(qsize, 8);
        let start = avail_idx.wrapping_sub(window as u16);
        for offset in 0..window {
            let slot_idx = start.wrapping_add(offset as u16);
            let ring_slot = (slot_idx as usize) % qsize;
            let ring_ptr = unsafe { (*avail).ring.as_ptr().add(ring_slot) as *const u16 };
            let head = unsafe { read_volatile(ring_ptr) };
            let Some(desc) = self.rx_queue.read_descriptor_checked(head) else {
                info!(
                    target: "virtio-net",
                    "[virtio-net][forensics] rx avail[{ring_slot}] -> head={head} desc=invalid",
                );
                continue;
            };
            info!(
                target: "virtio-forensics",
                "[virtio-forensics] rx avail[{ring_slot}] head={head} desc.addr=0x{addr:016x} len={len} flags=0x{flags:04x} next={next}",
                addr = desc.addr,
                len = desc.len,
                flags = desc.flags,
                next = desc.next,
            );
        }
    }

    fn dump_tx_v2_states_compact(&self) {
        let qsize = usize::from(self.tx_queue.size);
        let mut posted = 0usize;
        for idx in 0..qsize {
            if self.tx_v2_state[idx] == TxV2State::Posted {
                posted += 1;
            }
        }
        info!(
            target: "virtio-forensics",
            "[virtio-forensics] tx-v2 state summary free={} posted={} last_pub(head={},len={},seq={})",
            self.tx_v2_free.len(),
            posted,
            self.tx_v2_last_published_head,
            self.tx_v2_last_published_len,
            self.tx_v2_last_published_seq_latest,
        );
        let sample_len = core::cmp::min(qsize, 8);
        for idx in 0..sample_len {
            let state = self.tx_v2_state[idx];
            let gen = self.tx_v2_gen[idx];
            let len = self.tx_v2_len[idx];
            let pub_seq = self.tx_v2_last_published_seq[idx];
            let used_seq = self.tx_v2_last_used_seq[idx];
            info!(
                target: "virtio-forensics",
                "[virtio-forensics] tx-v2 id={} state={:?} gen={} len={} pub_seq={} used_seq={}",
                idx,
                state,
                gen,
                len,
                pub_seq,
                used_seq,
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
            self.tx_in_flight,
            self.tx_free.len(),
            self.tx_gen,
            pending,
            free_entries,
            self.device_faulted,
            forensics_frozen(),
        );
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
            self.tx_free.len(),
            self.tx_in_flight,
            self.tx_gen,
        );
        self.dump_tx_descriptor_table_once();
        self.dump_tx_used_window_once();
        self.dump_tx_states();
        self.dump_tx_recent_entries();
        self.freeze_and_capture("tx_anomaly");
    }

    fn validate_tx_reclaim_state(&mut self, id: u16) -> Result<(), ()> {
        if self.tx_free.iter().any(|&entry| entry == id) {
            self.tx_anomaly(TxAnomalyReason::FreeListCorrupt, "tx_reclaim_already_free");
            return Err(());
        }

        match self.tx_states.get(id as usize) {
            Some(TxState::Posted {
                len: posted_len, ..
            }) => {
                if *posted_len == 0 {
                    self.tx_anomaly(
                        TxAnomalyReason::RingIndexUnexpected,
                        "tx_reclaim_zero_len_state",
                    );
                }
                Ok(())
            }
            Some(TxState::Free) => {
                self.tx_anomaly(TxAnomalyReason::FreeListCorrupt, "tx_reclaim_free_state");
                Err(())
            }
            None => {
                self.tx_anomaly(TxAnomalyReason::RingIndexUnexpected, "tx_reclaim_oob_state");
                Err(())
            }
        }
    }

    fn record_tx_post(&mut self, head_id: u16, desc: &DescSpec) -> Result<(), ()> {
        let Some(state) = self.tx_states.get_mut(head_id as usize) else {
            return self.tx_state_violation("tx_id_oob", head_id, None);
        };

        if desc.len == 0 || desc.addr == 0 {
            return self.tx_state_violation("tx_post_zero", head_id, None);
        }

        match state {
            TxState::Free => {
                *state = TxState::Posted {
                    len: desc.len,
                    addr: desc.addr,
                    gen: self.tx_gen,
                };
                self.tx_gen = self.tx_gen.wrapping_add(1);
                self.tx_in_flight = self.tx_in_flight.wrapping_add(1);
                Ok(())
            }
            _ => self.tx_state_violation("tx_double_post", head_id, None),
        }
    }

    fn record_tx_complete(&mut self, id: u16) -> Result<(), ()> {
        let Some(state) = self.tx_states.get_mut(id as usize) else {
            return self.tx_state_violation("tx_complete_oob", id, None);
        };
        match state {
            TxState::Posted { .. } => {
                *state = TxState::Free;
                if self.tx_in_flight > 0 {
                    self.tx_in_flight -= 1;
                }
                Ok(())
            }
            TxState::Free => self.tx_state_violation("tx_complete_free", id, None),
        }
    }

    fn rollback_tx_post(&mut self, id: u16) {
        if let Some(state) = self.tx_states.get_mut(id as usize) {
            if matches!(state, TxState::Posted { .. }) {
                *state = TxState::Free;
                if self.tx_in_flight > 0 {
                    self.tx_in_flight -= 1;
                }
            }
        }
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

        self.validate_chain_nonzero(
            "RX",
            head_id,
            descs,
            Some(header_len),
            Some(payload_len),
            Some(frame_capacity),
            used_len,
        )?;

        let mut resolved_descs: HeaplessVec<DescSpec, RX_QUEUE_SIZE> = HeaplessVec::new();
        for (idx, spec) in descs.iter().enumerate() {
            let desc_index = head_id.wrapping_add(idx as u16);
            let next = if idx + 1 < descs.len() {
                Some(head_id.wrapping_add((idx + 1) as u16))
            } else {
                spec.next
            };
            self.rx_queue
                .setup_descriptor(desc_index, spec.addr, spec.len, spec.flags, next);
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

        fence(AtomicOrdering::Release);
        self.verify_descriptor_write("RX", QueueKind::Rx, head_id, &resolved_descs)?;
        self.rx_queue.sync_descriptor_table_for_device();
        self.log_pre_publish_if_suspicious("RX", QueueKind::Rx, head_id, &resolved_descs)?;
        fence(AtomicOrdering::Release);
        if let Err(fault) = validate_chain_pre_publish(
            "RX",
            self.rx_queue.size,
            self.rx_queue.desc.as_ptr(),
            head_id,
        ) {
            return self.handle_forensic_fault(fault);
        }

        let Some((slot, avail_idx, old_idx)) = self.rx_queue.push_avail(head_id) else {
            self.device_faulted = true;
            self.last_error.get_or_insert("rx_avail_write_failed");
            return Err(());
        };
        self.rx_queue.sync_avail_ring_for_device();
        self.log_publish_transaction(
            "RX",
            QueueKind::Rx,
            old_idx,
            avail_idx,
            slot,
            head_id,
            false,
        );
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
            self.notify_queue(RX_QUEUE_INDEX, "RX");
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
        self.check_device_health();
        if self.device_faulted || forensics_frozen() {
            return Err(());
        }

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
        self.validate_chain_nonzero("TX", head_id, descs, None, None, None, used_len)?;

        let mut resolved_descs: HeaplessVec<DescSpec, TX_QUEUE_SIZE> = HeaplessVec::new();
        for (idx, spec) in descs.iter().enumerate() {
            let desc_index = head_id.wrapping_add(idx as u16);
            let next = if idx + 1 < descs.len() {
                Some(head_id.wrapping_add((idx + 1) as u16))
            } else {
                spec.next
            };
            self.tx_queue
                .setup_descriptor(desc_index, spec.addr, spec.len, spec.flags, next);
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
        let header_fields = self.inspect_tx_header(head_id, header_len);
        let payload_overlaps = resolved_descs.get(0).map_or(false, |desc| {
            let header_end = desc.addr.saturating_add(header_len as u64);
            let payload_addr = desc.addr.saturating_add(header_len as u64);
            payload_len > 0 && payload_addr < header_end
        });

        fence(AtomicOrdering::Release);
        self.verify_descriptor_write("TX", QueueKind::Tx, head_id, &resolved_descs)?;
        self.tx_queue.sync_descriptor_table_for_device();
        self.log_tx_descriptor_readback(head_id, &resolved_descs);
        self.log_pre_publish_if_suspicious("TX", QueueKind::Tx, head_id, &resolved_descs)?;
        fence(AtomicOrdering::Release);
        if let Err(fault) = validate_chain_pre_publish(
            "TX",
            self.tx_queue.size,
            self.tx_queue.desc.as_ptr(),
            head_id,
        ) {
            return self.handle_forensic_fault(fault);
        }

        if self.record_tx_post(head_id, &resolved_descs[0]).is_err() {
            return Err(());
        }

        let buffer_range = self
            .clean_tx_buffer_for_device(
                head_id,
                resolved_descs[0].len as usize,
                self.tx_anomaly_logged,
            )
            .unwrap_or((0, 0));
        dma_barrier();

        let Some((slot, avail_idx, old_idx)) = self.tx_queue.push_avail(head_id) else {
            self.device_faulted = true;
            self.last_error.get_or_insert("tx_avail_write_failed");
            self.rollback_tx_post(head_id);
            return Err(());
        };
        if slot == 0 && !self.tx_wrap_logged {
            self.tx_wrap_logged = true;
            info!(
                target: "virtio-net",
                "[virtio-net][tx-wrap] avail_idx {}->{} slot={} head={} free={} in_flight={}",
                old_idx,
                avail_idx,
                slot,
                head_id,
                self.tx_free.len(),
                self.tx_in_flight,
            );
        }
        self.verify_tx_publish(slot, head_id, &resolved_descs[0])?;
        self.tx_queue.sync_avail_ring_for_device();
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
            self.notify_queue(TX_QUEUE_INDEX, "TX");
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
                    .min(buffer.as_slice().len()) as u32;
                let desc = [DescSpec {
                    addr: buffer.paddr() as u64,
                    len: total_len,
                    flags: VIRTQ_DESC_F_WRITE,
                    next: None,
                }];

                Self::sync_rx_slot_for_device(buffer, header_len, payload_len, self.dma_cacheable);

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

        log::info!(
            target: "net-console",
            "[virtio-net] TX queue initialised: size={} buffers={} free_entries={}",
            self.tx_queue.size,
            self.tx_buffers.len(),
            self.tx_free.len(),
        );
    }

    fn sync_rx_slot_for_device(
        buffer: &RamFrame,
        header_len: usize,
        payload_len: usize,
        cacheable: bool,
    ) {
        let header_ptr = buffer.ptr().as_ptr();
        let payload_ptr = unsafe { header_ptr.add(header_len) };
        let payload_len = core::cmp::min(
            payload_len,
            buffer.as_slice().len().saturating_sub(header_len),
        );
        dma_clean(header_ptr, header_len, cacheable, "clean rx buffer header");
        dma_clean(
            payload_ptr,
            payload_len,
            cacheable,
            "clean rx buffer payload",
        );
    }

    fn sync_rx_slot_for_cpu(
        buffer: &RamFrame,
        header_len: usize,
        written_len: usize,
        cacheable: bool,
    ) {
        let header_len = core::cmp::min(header_len, written_len);
        let header_ptr = buffer.ptr().as_ptr();
        dma_invalidate(
            header_ptr,
            header_len,
            cacheable,
            "invalidate rx buffer header",
        );
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
            );
        }
    }

    fn poll_interrupts(&mut self) {
        let (status, isr_ack) = self.regs.acknowledge_interrupts();
        log::debug!(
            target: "virtio-net",
            "ISR status=0x{:02x}, ISRACK=0x{:02x}",
            status,
            isr_ack,
        );
        self.check_device_health();
        if self.device_faulted {
            return;
        }
        self.reclaim_tx();
    }

    fn check_device_health(&mut self) {
        let status = self.regs.status();
        if (status & (STATUS_DEVICE_NEEDS_RESET | STATUS_FAILED)) != 0 {
            if !self.bad_status_logged {
                warn!(
                    target: "net-console",
                    "[virtio-net] entered bad status (0x{status:02x}); continuing until forensic log captured"
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
        if NET_VIRTIO_TX_V2 {
            self.reclaim_tx_v2();
            return;
        }
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
                self.tx_in_flight,
                self.tx_free.len(),
                self.tx_gen,
            );
            self.tx_last_used_seen = used_idx;
        }
        self.tx_progress_log_gate = self.tx_progress_log_gate.wrapping_add(1);
        loop {
            match self.tx_queue.pop_used("TX", true) {
                Ok(Some((id, len))) => {
                    let len_u32 = len;
                    self.record_tx_used_entry(id, len_u32);
                    if len_u32 == 0 {
                        self.tx_used_zero_streak = self.tx_used_zero_streak.saturating_add(1);
                        if self.tx_used_zero_streak == 1 || self.tx_used_zero_streak % 16 == 0 {
                            warn!(
                                target: "net-console",
                                "[virtio-net] TX used len zero (id={} streak={})",
                                id,
                                self.tx_used_zero_streak,
                            );
                        }
                    } else {
                        self.tx_used_zero_streak = 0;
                    }
                    if self.validate_tx_reclaim_state(id).is_err() {
                        break;
                    }
                    if self.record_tx_complete(id).is_err() {
                        break;
                    }
                    self.tx_used_count = self.tx_used_count.wrapping_add(1);
                    self.note_progress();
                    if self.tx_free.push(id).is_err() {
                        self.tx_drops = self.tx_drops.saturating_add(1);
                        self.last_error = Some("tx_free_overflow");
                    }
                }
                Ok(None) => break,
                Err(fault) => {
                    let _ = self.handle_forensic_fault(fault);
                    break;
                }
            }
        }
    }

    fn reclaim_tx_v2(&mut self) {
        let used = self.tx_queue.used.as_ptr();
        self.tx_queue.invalidate_used_header_for_cpu();
        let used_idx = unsafe { read_volatile(&(*used).idx) };
        let qsize = usize::from(self.tx_queue.size);

        assert!(qsize != 0, "virtqueue size must be non-zero");

        while self.tx_v2_last_used != used_idx {
            let ring_slot = (self.tx_v2_last_used as usize) % qsize;
            self.tx_queue.invalidate_used_elem_for_cpu(ring_slot);
            let elem_ptr = unsafe { (*used).ring.as_ptr().add(ring_slot) as *const VirtqUsedElem };
            let elem = unsafe { read_volatile(elem_ptr) };
            let id = elem.id as u16;
            if id >= self.tx_queue.size {
                self.last_error.get_or_insert("tx_v2_used_id_oob");
                self.device_faulted = true;
                break;
            }
            if elem.len == 0 {
                let now_ms = crate::hal::timebase().now_ms();
                if now_ms.saturating_sub(self.tx_v2_used_zero_log_ms) >= 1_000 {
                    self.tx_v2_used_zero_log_ms = now_ms;
                    warn!(
                        target: "net-console",
                        "[virtio-net][tx-v2] used len zero id={} gen={} posted_len={}",
                        id,
                        self.tx_v2_gen.get(id as usize).copied().unwrap_or(0),
                        self.tx_v2_len.get(id as usize).copied().unwrap_or(0),
                    );
                }
                self.tx_v2_used_zero_streak = self.tx_v2_used_zero_streak.saturating_add(1);
                if self.tx_v2_used_zero_streak >= 8 && rl(RlTag::UsedZero, 8) {
                    self.log_tx_v2_zero_len_snapshot(id);
                }
            } else {
                self.tx_v2_used_zero_streak = 0;
            }
            if !self.tx_v2_mark_free(id) {
                break;
            }
            self.tx_complete = self.tx_complete.wrapping_add(1);
            self.tx_used_count = self.tx_used_count.wrapping_add(1);
            if self.tx_v2_free.push(id).is_err() {
                self.tx_drops = self.tx_drops.saturating_add(1);
                self.last_error.get_or_insert("tx_v2_free_overflow");
            }
            self.tx_v2_last_used = self.tx_v2_last_used.wrapping_add(1);
            self.note_progress();
        }
        self.tx_queue.last_used = self.tx_v2_last_used;

        self.log_tx_v2_invariants();
    }

    fn tx_v2_in_flight_count(&self) -> u16 {
        let limit = usize::from(self.tx_queue.size);
        self.tx_v2_state
            .iter()
            .take(limit)
            .filter(|&&state| state == TxV2State::Posted)
            .count() as u16
    }

    fn log_tx_v2_invariants(&mut self) {
        let now_ms = crate::hal::timebase().now_ms();
        if now_ms.saturating_sub(self.tx_v2_log_ms) < 1_000 {
            return;
        }
        self.tx_v2_log_ms = now_ms;
        let in_flight = self.tx_v2_in_flight_count();
        let free = self.tx_v2_free.len() as u16;
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
        info!(
            target: "net-console",
            "[virtio-net][tx-v2] stats free={} in_flight={} submit={} complete={} zero_len={} double_submit={}",
            free,
            in_flight,
            self.tx_submit,
            self.tx_complete,
            self.tx_zero_len_attempt,
            self.tx_double_submit,
        );
    }

    fn log_tx_v2_zero_len_snapshot(&mut self, id: u16) {
        let status = self.regs.status();
        let isr = self.regs.isr_status();
        let (used_idx, avail_idx) = self.tx_queue.indices();
        let in_flight = self.tx_v2_in_flight_count();
        let free = self.tx_v2_free.len();
        info!(
            target: "virtio-forensics",
            "[virtio-forensics] tx-v2 used.len==0 snapshot id={} status=0x{:02x} isr=0x{:02x} avail.idx={} used.idx={} last_used={} free={} in_flight={} last_pub(head={},len={},seq={})",
            id,
            status,
            isr,
            avail_idx,
            used_idx,
            self.tx_queue.last_used,
            free,
            in_flight,
            self.tx_v2_last_published_head,
            self.tx_v2_last_published_len,
            self.tx_v2_last_published_seq_latest,
        );
    }

    fn tx_v2_mark_posted(&mut self, id: u16, len: u32) -> bool {
        let Some(state) = self.tx_v2_state.get_mut(id as usize) else {
            self.last_error.get_or_insert("tx_v2_state_oob");
            return false;
        };
        if *state != TxV2State::Free {
            self.tx_double_submit = self.tx_double_submit.wrapping_add(1);
            self.tx_v2_invariant_violation("tx_v2_post_not_free", id);
            return false;
        }
        *state = TxV2State::Posted;
        let gen = self.tx_v2_gen_counter;
        self.tx_v2_gen_counter = self.tx_v2_gen_counter.wrapping_add(1);
        if let Some(entry) = self.tx_v2_gen.get_mut(id as usize) {
            *entry = gen;
        }
        if let Some(entry) = self.tx_v2_len.get_mut(id as usize) {
            *entry = len;
        }
        true
    }

    fn tx_v2_mark_free(&mut self, id: u16) -> bool {
        let Some(state) = self.tx_v2_state.get_mut(id as usize) else {
            self.last_error.get_or_insert("tx_v2_state_oob");
            return false;
        };
        if *state != TxV2State::Posted {
            self.tx_v2_invariant_violation("tx_v2_reclaim_not_posted", id);
            return false;
        }
        *state = TxV2State::Free;
        if let Some(entry) = self.tx_v2_last_used_seq.get_mut(id as usize) {
            let last_pub = self
                .tx_v2_last_published_seq
                .get(id as usize)
                .copied()
                .unwrap_or(0);
            *entry = last_pub;
        }
        true
    }

    fn tx_v2_invariant_violation(&mut self, reason: &'static str, id: u16) {
        warn!(
            target: "net-console",
            "[virtio-net][tx-v2] invariant violation reason={} id={} state={:?} gen={} len={} pub_seq={} used_seq={}",
            reason,
            id,
            self.tx_v2_state.get(id as usize).copied().unwrap_or(TxV2State::Free),
            self.tx_v2_gen.get(id as usize).copied().unwrap_or(0),
            self.tx_v2_len.get(id as usize).copied().unwrap_or(0),
            self.tx_v2_last_published_seq
                .get(id as usize)
                .copied()
                .unwrap_or(0),
            self.tx_v2_last_used_seq.get(id as usize).copied().unwrap_or(0),
        );
        self.log_tx_v2_recent();
        self.freeze_and_capture(reason);
    }

    fn tx_v2_avail_contains(&mut self, id: u16) -> bool {
        let avail = self.tx_queue.avail.as_ptr();
        let avail_idx = unsafe { read_volatile(&(*avail).idx) };
        let qsize = usize::from(self.tx_queue.size);
        let distance = avail_idx.wrapping_sub(self.tx_v2_last_used);
        if distance > self.tx_queue.size {
            return false;
        }
        for step in 0..distance {
            let idx = self.tx_v2_last_used.wrapping_add(step);
            let ring_slot = (idx as usize) % qsize;
            let ring_ptr = unsafe { (*avail).ring.as_ptr().add(ring_slot) as *const u16 };
            let head = unsafe { read_volatile(ring_ptr) };
            if head == id {
                return true;
            }
        }
        false
    }

    fn log_tx_v2_recent(&self) {
        let sample_len = core::cmp::min(self.tx_v2_free.len(), 8);
        let mut sample = HeaplessVec::<u16, 8>::new();
        for idx in 0..sample_len {
            if let Some(entry) = self.tx_v2_free.get(idx).copied() {
                let _ = sample.push(entry);
            }
        }
        info!(
            target: "virtio-forensics",
            "[virtio-forensics] tx-v2 recent publishes free_sample={:?}",
            sample,
        );
        for idx in 0..self.tx_v2_recent_head.len() {
            let head = self.tx_v2_recent_head[idx];
            let len = self.tx_v2_recent_len[idx];
            let seq = self.tx_v2_recent_seq[idx];
            if seq == 0 {
                continue;
            }
            info!(
                target: "virtio-forensics",
                "[virtio-forensics] tx-v2 recent idx={} seq={} head={} len={}",
                idx,
                seq,
                head,
                len,
            );
        }
    }

    fn pop_rx(&mut self) -> Option<(u16, usize)> {
        if forensics_frozen() {
            return None;
        }
        match self.rx_queue.pop_used("RX", false) {
            Ok(Some((id, len))) => {
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
                    self.note_progress();
                    self.requeue_rx(id, Some(len));
                    return None;
                }
                if let Some(buffer) = self.rx_buffers.get_mut(id as usize) {
                    Self::sync_rx_slot_for_cpu(buffer, header_len, len, self.dma_cacheable);
                }
                self.last_used_idx_debug = self.rx_queue.last_used;
                let (used_idx, avail_idx) = self.rx_queue.indices();
                log::debug!(
                    target: "virtio-net",
                    "[RX] consumed used_idx={} avail_idx={}",
                    used_idx,
                    avail_idx,
                );
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

            let total_len = frame_capacity.min(buffer.as_slice().len()) as u32;

            let desc = [DescSpec {
                addr: buffer.paddr() as u64,
                len: total_len,
                flags: VIRTQ_DESC_F_WRITE,
                next: None,
            }];

            Self::sync_rx_slot_for_device(buffer, header_len, payload_len, self.dma_cacheable);

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
        if NET_VIRTIO_TX_V2 {
            while let Some(id) = self.tx_v2_free.pop() {
                if self.tx_v2_state.get(id as usize) != Some(&TxV2State::Free) {
                    self.tx_v2_invariant_violation("tx_v2_alloc_not_free", id);
                    continue;
                }
                return VirtioTxToken::new(driver_ptr, Some(id));
            }
            return VirtioTxToken::new(driver_ptr, None);
        }
        if let Some(id) = self.tx_free.pop() {
            if let Some(state) = self.tx_states.get(id as usize) {
                if !matches!(state, TxState::Free) {
                    self.tx_anomaly(
                        TxAnomalyReason::FreeListCorrupt,
                        "prepare_tx_token_inflight",
                    );
                }
            }
            VirtioTxToken::new(driver_ptr, Some(id))
        } else {
            VirtioTxToken::new(driver_ptr, None)
        }
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

    fn submit_tx(&mut self, id: u16, len: usize) {
        if NET_VIRTIO_TX_V2 {
            self.submit_tx_v2(id, len);
            return;
        }
        self.check_device_health();
        if self.device_faulted {
            return;
        }
        if forensics_frozen() {
            return;
        }
        if let Some((length, addr)) = self
            .tx_buffers
            .get(id as usize)
            .map(|buffer| (len.min(buffer.as_slice().len()), buffer.paddr()))
        {
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
                return;
            }
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
        self.check_device_health();
        if self.device_faulted {
            return;
        }
        if forensics_frozen() {
            return;
        }
        if len == 0 {
            self.tx_zero_len_attempt = self.tx_zero_len_attempt.wrapping_add(1);
            debug_assert!(len != 0, "tx-v2 zero-length submit");
            return;
        }
        if id >= self.tx_queue.size {
            self.last_error.get_or_insert("tx_v2_id_oob");
            return;
        }
        if self.tx_v2_state.get(id as usize) != Some(&TxV2State::Free) {
            self.tx_double_submit = self.tx_double_submit.wrapping_add(1);
            self.tx_v2_invariant_violation("tx_v2_double_submit", id);
            return;
        }
        self.log_mmio_state_once("first_tx");

        let Some(buffer) = self.tx_buffers.get_mut(id as usize) else {
            self.last_error.get_or_insert("tx_v2_buffer_missing");
            return;
        };
        let buffer_len = buffer.as_slice().len();
        let capped_len = len.min(buffer_len);
        let addr = buffer.paddr() as u64;
        if capped_len == 0 {
            self.tx_zero_len_attempt = self.tx_zero_len_attempt.wrapping_add(1);
            debug_assert!(capped_len != 0, "tx-v2 zero-length buffer");
            return;
        }

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

        if cfg!(debug_assertions) || rl(RlTag::AvailScan, 64) {
            if self.tx_v2_avail_contains(id) {
                self.tx_v2_invariant_violation("tx_v2_dup_in_avail", id);
                return;
            }
        }

        let publish_seq = self.tx_v2_publish_seq;
        self.tx_v2_publish_seq = self.tx_v2_publish_seq.wrapping_add(1);
        self.tx_v2_last_published_head = id;
        self.tx_v2_last_published_len = capped_len as u32;
        self.tx_v2_last_published_seq_latest = publish_seq;
        if let Some(entry) = self.tx_v2_last_published_seq.get_mut(id as usize) {
            *entry = publish_seq;
        }
        let recent_idx = (publish_seq as usize) % self.tx_v2_recent_head.len();
        self.tx_v2_recent_head[recent_idx] = id;
        self.tx_v2_recent_len[recent_idx] = capped_len as u32;
        self.tx_v2_recent_seq[recent_idx] = publish_seq;

        if !self.tx_v2_mark_posted(id, capped_len as u32) {
            return;
        }

        let _buffer_range =
            self.clean_tx_buffer_for_device(id, capped_len, self.tx_anomaly_logged);
        self.tx_queue
            .setup_descriptor(id, addr, capped_len as u32, 0, None);
        self.tx_queue.sync_descriptor_table_for_device();
        if !self.log_tx_desc_summary(id) {
            return;
        }
        // Avail ring publish is the final step so the device never observes a
        // descriptor before buffer and descriptor cache maintenance completes.
        if let Some((slot, _avail_idx, _old_idx)) = self.tx_queue.push_avail(id) {
            self.tx_queue.sync_avail_ring_for_device();
            self.dma_overkill_tx(id, slot, _buffer_range);
            self.tx_submit = self.tx_submit.wrapping_add(1);
            if rl(RlTag::TxPublish, 256) {
                info!(
                    target: "virtio-forensics",
                    "[virtio-forensics] tx publish id={} len={} slot={}",
                    id,
                    capped_len,
                    slot,
                );
            }
            if slot == 0 && !self.tx_wrap_logged {
                self.tx_wrap_logged = true;
            }
            self.notify_queue(TX_QUEUE_INDEX, "TX");
        } else {
            self.tx_v2_mark_free(id);
            let _ = self.tx_v2_free.push(id);
            self.last_error.get_or_insert("tx_v2_avail_write_failed");
        }
    }

    fn log_tx_desc_summary(&mut self, head_id: u16) -> bool {
        let Some(desc) = self.tx_queue.read_descriptor_checked(head_id) else {
            warn!(
                target: "virtio-forensics",
                "[virtio-forensics] tx desc invalid id={} (out of range)",
                head_id,
            );
            self.freeze_and_capture("tx_desc_oob");
            return false;
        };
        if rl(RlTag::DescRaw, 256) {
            info!(
                target: "virtio-forensics",
                "[virtio-forensics] tx desc id={} addr=0x{:016x} len={} flags=0x{:04x} next={}",
                head_id,
                desc.addr,
                desc.len,
                desc.flags,
                desc.next,
            );
        }
        if desc.addr == 0 || desc.len == 0 {
            warn!(
                target: "virtio-forensics",
                "[virtio-forensics] tx desc invalid id={} addr=0x{:016x} len={}",
                head_id,
                desc.addr,
                desc.len,
            );
            self.freeze_and_capture("tx_desc_zero");
            return false;
        }
        if desc.flags != 0 || desc.next != 0 {
            warn!(
                target: "virtio-forensics",
                "[virtio-forensics] tx desc flags invalid id={} flags=0x{:04x} next={}",
                head_id,
                desc.flags,
                desc.next,
            );
            self.freeze_and_capture("tx_desc_flags_invalid");
            return false;
        }
        true
    }

    fn dma_overkill_tx(
        &mut self,
        _head_id: u16,
        _ring_slot: u16,
        _buffer_range: Option<(usize, usize)>,
    ) {
        if !VIRTIO_DMA_OVERKILL {
            return;
        }
        if !DMA_OVERKILL_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            info!(
                target: "virtio-forensics",
                "[virtio-forensics] dma_overkill=on (disabled by default)",
            );
        }
    }

    fn clean_tx_buffer_for_device(
        &mut self,
        head_id: u16,
        len: usize,
        force_log: bool,
    ) -> Option<(usize, usize)> {
        self.tx_buffers.get(head_id as usize).map(|buffer| {
            let capped_len = len.min(buffer.as_slice().len());
            let ptr = buffer.ptr().as_ptr();
            let start = ptr as usize;
            dma_clean(ptr, capped_len, self.dma_cacheable, "clean tx buffer");
            let payload_start = start.saturating_add(self.rx_header_len);
            let payload_end = payload_start.saturating_add(capped_len.saturating_sub(self.rx_header_len));
            if force_log || !self.tx_dma_log_once {
                self.tx_dma_log_once = true;
                info!(
                    target: "virtio-net",
                    "[virtio-net][dma] tx buffer clean head={} header=0x{start:016x}..0x{hdr_end:016x} payload=0x{payload_start:016x}..0x{payload_end:016x}",
                    head_id,
                    hdr_end = start.saturating_add(self.rx_header_len),
                    payload_start = payload_start,
                    payload_end = payload_end,
                );
            }
            (start, start.saturating_add(capped_len))
        })
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
            Some((rx, tx))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        self.poll_interrupts();
        if self.device_faulted {
            return None;
        }
        if NET_VIRTIO_TX_V2 {
            while let Some(id) = self.tx_v2_free.pop() {
                if self.tx_v2_state.get(id as usize) != Some(&TxV2State::Free) {
                    self.tx_v2_invariant_violation("tx_v2_alloc_not_free", id);
                    continue;
                }
                return Some(VirtioTxToken::new(self as *mut _, Some(id)));
            }
            return None;
        }
        if let Some(id) = self.tx_free.pop() {
            Some(VirtioTxToken::new(self as *mut _, Some(id)))
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

    fn mac(&self) -> EthernetAddress {
        self.mac
    }

    fn tx_drop_count(&self) -> u32 {
        self.tx_drops
    }

    fn counters(&self) -> NetDeviceCounters {
        let tx_free = if NET_VIRTIO_TX_V2 {
            self.tx_v2_free.len() as u64
        } else {
            self.tx_free.len() as u64
        };
        let tx_in_flight = if NET_VIRTIO_TX_V2 {
            self.tx_v2_in_flight_count() as u64
        } else {
            self.tx_in_flight as u64
        };
        let (tx_submit, tx_complete, tx_double_submit, tx_zero_len_attempt) = if NET_VIRTIO_TX_V2
        {
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

    fn debug_dump(&mut self) {
        VirtioNet::debug_dump_forensics(self);
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
    desc: Option<u16>,
}

impl VirtioTxToken {
    fn new(driver: *mut VirtioNet, desc: Option<u16>) -> Self {
        Self { driver, desc }
    }
}

impl TxToken for VirtioTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let driver = unsafe { &mut *self.driver };
        let attempt_seq = driver.next_tx_attempt_seq();
        if len == 0 {
            if NET_VIRTIO_TX_V2 {
                driver.tx_zero_len_attempt = driver.tx_zero_len_attempt.wrapping_add(1);
            }
            driver.tx_drops = driver.tx_drops.saturating_add(1);
            driver.log_tx_attempt(attempt_seq, len, 0, 0);
            return f(&mut []);
        }
        if let Some(id) = self.desc {
            let buffer = driver
                .tx_buffers
                .get_mut(id as usize)
                .expect("tx descriptor out of range");
            let header_len = driver.rx_header_len;
            let max_len = buffer.as_mut_slice().len();
            if max_len <= header_len {
                driver.tx_drops = driver.tx_drops.saturating_add(1);
                return f(&mut []);
            }

            let payload_len = len
                .min(MAX_FRAME_LEN)
                .min(max_len.saturating_sub(header_len));
            let total_len = payload_len + header_len;
            let result;
            let written_len;
            {
                let mut_slice = &mut buffer.as_mut_slice()[..total_len];
                let (header, payload) =
                    mut_slice.split_at_mut(core::cmp::min(header_len, total_len));
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
            let total_len = if NET_VIRTIO_TX_V2 {
                if written_len == 0 {
                    driver.tx_zero_len_attempt = driver.tx_zero_len_attempt.wrapping_add(1);
                    driver.tx_drops = driver.tx_drops.saturating_add(1);
                    return result;
                }
                header_len.saturating_add(written_len)
            } else {
                total_len
            };
            if NET_VIRTIO_TX_V2 && total_len <= header_len {
                driver.tx_drops = driver.tx_drops.saturating_add(1);
                return result;
            }
            driver.submit_tx(id, total_len);
            driver.tx_packets = driver.tx_packets.saturating_add(1);
            result
        } else {
            let length = len.min(MAX_FRAME_LEN);
            let scratch = [0u8; MAX_FRAME_LEN];
            let result = {
                let mut payload = scratch;
                let payload_slice = &mut payload[..length];
                let mut snapshot = [0u8; MAX_FRAME_LEN];
                snapshot[..length].copy_from_slice(payload_slice);
                let result = f(payload_slice);
                let written_len =
                    VirtioNet::compute_written_len(length, &snapshot[..length], payload_slice);
                driver.log_tx_attempt(attempt_seq, len, length, written_len);
                result
            };
            driver.tx_drops = driver.tx_drops.saturating_add(1);
            result
        }
    }

    fn set_meta(&mut self, _meta: smoltcp::phy::PacketMeta) {}
}

struct VirtioRegs {
    mmio: DeviceFrame,
    mode: VirtioMmioMode,
}

#[derive(Clone, Copy, Debug)]
struct QueueRegs {
    index: u32,
    sel_requested: u32,
    sel: u32,
    size: u16,
    ready: u32,
    desc_lo: u32,
    desc_hi: u32,
    driver_lo: u32,
    driver_hi: u32,
    device_lo: u32,
    device_hi: u32,
    align: u32,
    pfn: u32,
}

impl QueueRegs {
    fn desc_addr(self) -> u64 {
        (u64::from(self.desc_hi) << 32) | u64::from(self.desc_lo)
    }

    fn driver_addr(self) -> u64 {
        (u64::from(self.driver_hi) << 32) | u64::from(self.driver_lo)
    }

    fn device_addr(self) -> u64 {
        (u64::from(self.device_hi) << 32) | u64::from(self.device_lo)
    }
}

impl VirtioRegs {
    fn mmio_paddr(&self) -> usize {
        self.mmio.paddr()
    }

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

    fn queue_sel(&self) -> u32 {
        self.read32(Registers::QueueSel)
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

    fn read_queue_regs(&mut self, index: u32) -> QueueRegs {
        let sel_before = self.queue_sel();
        self.select_queue(index);
        let sel_readback = self.queue_sel();
        if sel_readback != index {
            warn!(
                target: "virtio-forensics",
                "[virtio-forensics] mmio queue_sel mismatch requested={} readback={}",
                index,
                sel_readback
            );
        }
        let regs = QueueRegs {
            index,
            sel_requested: index,
            sel: sel_readback,
            size: self.read32(Registers::QueueNum) as u16,
            ready: self.read32(Registers::QueueReady),
            desc_lo: self.read32(Registers::QueueDescLow),
            desc_hi: self.read32(Registers::QueueDescHigh),
            driver_lo: self.read32(Registers::QueueDriverLow),
            driver_hi: self.read32(Registers::QueueDriverHigh),
            device_lo: self.read32(Registers::QueueDeviceLow),
            device_hi: self.read32(Registers::QueueDeviceHigh),
            align: self.read32(Registers::QueueAlign),
            pfn: self.read32(Registers::QueuePfn),
        };
        info!(
            target: "virtio-forensics",
            "[virtio-forensics] mmio queue_regs sel_req={} sel={} size={} ready={} desc=0x{:016x} (hi=0x{:08x} lo=0x{:08x}) avail=0x{:016x} (hi=0x{:08x} lo=0x{:08x}) used=0x{:016x} (hi=0x{:08x} lo=0x{:08x}) align={} pfn=0x{:08x}",
            regs.sel_requested,
            regs.sel,
            regs.size,
            regs.ready,
            regs.desc_addr(),
            regs.desc_hi,
            regs.desc_lo,
            regs.driver_addr(),
            regs.driver_hi,
            regs.driver_lo,
            regs.device_addr(),
            regs.device_hi,
            regs.device_lo,
            regs.align,
            regs.pfn,
        );
        if matches!(self.mode, VirtioMmioMode::Modern) {
            let suspicious = regs.size == 0
                || regs.ready == 0
                || regs.desc_addr() == 0
                || regs.driver_addr() == 0
                || regs.device_addr() == 0;
            if suspicious {
                warn!(
                    target: "virtio-forensics",
                    "[virtio-forensics] mmio_readback_suspicious queue={} sel={} size={} ready={} desc=0x{:016x} avail=0x{:016x} used=0x{:016x}",
                    regs.index,
                    regs.sel,
                    regs.size,
                    regs.ready,
                    regs.desc_addr(),
                    regs.driver_addr(),
                    regs.device_addr(),
                );
            }
        }
        self.select_queue(sel_before);
        regs
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
    use heapless::String as HeaplessString;

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
    cacheable: bool,
    used_zero_len_head: Option<u16>,
}

impl VirtQueue {
    fn new(
        regs: &mut VirtioRegs,
        mut frame: RamFrame,
        index: u32,
        size: usize,
        mode: VirtioMmioMode,
        cacheable: bool,
    ) -> Result<Self, DriverError> {
        let queue_size = size as u16;
        let base_paddr = frame.paddr();
        let base_ptr = frame.ptr();
        let page_bytes = 1usize << seL4_PageBits;

        if queue_size == 0 {
            uart_ring_oob(
                "queue_size_zero",
                b"queue_size=0x",
                queue_size as usize,
                b"frame=0x",
                page_bytes,
                b"index=0x",
                index as usize,
            );
            panic!("VIRTIO_RING_OOB");
        }

        if index == RX_QUEUE_INDEX {
            bootinfo_probe("net.mmio.q0.desc.zero.before");
        }
        let frame_capacity = {
            let frame_slice = frame.as_mut_slice();
            let capacity = frame_slice.len();

            frame_slice.fill(0);
            capacity
        };

        unsafe {
            core::ptr::write_bytes(base_ptr.as_ptr(), 0, frame_capacity);
        }
        if index == RX_QUEUE_INDEX {
            bootinfo_probe("net.mmio.q0.desc.zero.after");
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

        if base_paddr & (page_bytes - 1) != 0 {
            error!(
                target: "net-console",
                "[virtio-net] virtqueue backing not page aligned: paddr=0x{base_paddr:x}"
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue backing not page aligned",
            ));
        }

        const LEGACY_QUEUE_ALIGN: usize = 4;
        let layout = VirtqLayout::compute_vq_layout(queue_size, false)?;

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
            uart_ring_oob(
                "layout_frame",
                b"used_end=0x",
                used_end,
                b"cap=0x",
                frame_capacity,
                b"qsize=0x",
                queue_size as usize,
            );
            panic!("VIRTIO_RING_OOB");
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

        let avail_idx = unsafe { read_volatile(&(*avail_ptr.as_ptr()).idx) };
        if index == RX_QUEUE_INDEX {
            bootinfo_probe("net.mmio.q0.avail.idx");
        }
        let used_idx = unsafe { read_volatile(&(*used_ptr.as_ptr()).idx) };
        if index == RX_QUEUE_INDEX {
            bootinfo_probe("net.mmio.q0.used.idx");
        }

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
            vaddr = base_ptr.as_ptr() as usize,
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
        regs.queue_ready(1);
        if index == RX_QUEUE_INDEX {
            bootinfo_probe("net.mmio.q0.config.done");
        }
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
            cacheable,
            used_zero_len_head: None,
        })
    }

    fn setup_descriptor(&self, index: u16, addr: u64, len: u32, flags: u16, next: Option<u16>) {
        if index >= self.size {
            uart_ring_oob(
                "desc_index",
                b"index=0x",
                index as usize,
                b"size=0x",
                self.size as usize,
                b"desc=0x",
                self.layout.desc_len,
            );
            panic!("VIRTIO_RING_OOB");
        }
        let desc_bytes = self.size as usize * core::mem::size_of::<VirtqDesc>();
        let desc_offset = index as usize * core::mem::size_of::<VirtqDesc>();
        if desc_offset + core::mem::size_of::<VirtqDesc>() > desc_bytes {
            uart_ring_oob(
                "desc_offset",
                b"offset=0x",
                desc_offset,
                b"bytes=0x",
                desc_bytes,
                b"index=0x",
                index as usize,
            );
            panic!("VIRTIO_RING_OOB");
        }
        let flags = match next {
            Some(_) => flags | VIRTQ_DESC_F_NEXT,
            None => flags,
        };
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
    }

    fn push_avail(&self, index: u16) -> Option<(u16, u16, u16)> {
        let avail = self.avail.as_ptr();
        let qsize = usize::from(self.size);

        if qsize == 0 {
            uart_ring_oob(
                "avail_qsize_zero",
                b"size=0x",
                qsize,
                b"index=0x",
                index as usize,
                b"avail=0x",
                self.layout.avail_len,
            );
            panic!("VIRTIO_RING_OOB");
        }
        if index >= self.size {
            uart_ring_oob(
                "avail_index",
                b"index=0x",
                index as usize,
                b"size=0x",
                self.size as usize,
                b"avail=0x",
                self.layout.avail_len,
            );
            panic!("VIRTIO_RING_OOB");
        }

        let idx = unsafe { read_volatile(&(*avail).idx) };
        let ring_slot = (idx as usize) % qsize;
        assert!(ring_slot < qsize, "avail ring slot out of range");
        let ring_offset = ring_slot * core::mem::size_of::<u16>();
        let ring_bytes = qsize * core::mem::size_of::<u16>();
        if ring_offset + core::mem::size_of::<u16>() > ring_bytes {
            uart_ring_oob(
                "avail_offset",
                b"offset=0x",
                ring_offset,
                b"bytes=0x",
                ring_bytes,
                b"slot=0x",
                ring_slot,
            );
            panic!("VIRTIO_RING_OOB");
        }
        unsafe {
            let ring_ptr = (*avail).ring.as_ptr().add(ring_slot as usize) as *mut u16;
            write_volatile(ring_ptr, index);
            fence(AtomicOrdering::Release);
            let new_idx = idx.wrapping_add(1);
            write_volatile(&mut (*avail).idx, new_idx);
            fence(AtomicOrdering::Release);
            Some((ring_slot as u16, new_idx, idx))
        }
    }

    fn notify(&mut self, regs: &mut VirtioRegs, queue: u32) {
        // Ensure descriptors and avail ring updates are visible to the device before the kick.
        dma_barrier();
        regs.notify(queue);
        let notify_flag = match queue {
            RX_QUEUE_INDEX => Some(&RX_NOTIFY_LOGGED),
            TX_QUEUE_INDEX => Some(&TX_NOTIFY_LOGGED),
            _ => None,
        };
        if let Some(flag) = notify_flag {
            if !flag.swap(true, AtomicOrdering::AcqRel) {
                let label = if queue == TX_QUEUE_INDEX { "TX" } else { "RX" };
                info!(target: "virtio-net", "[virtio-net] notify queue={queue} ({label})");
            }
        }
    }

    fn sync_descriptor_table_for_device(&self) {
        dma_clean(
            self.desc.as_ptr() as *const u8,
            self.layout.desc_len,
            self.cacheable,
            "clean descriptor table",
        );
    }

    fn sync_avail_ring_for_device(&self) {
        dma_clean(
            self.avail.as_ptr() as *const u8,
            self.layout.avail_len,
            self.cacheable,
            "clean avail ring",
        );
    }

    fn invalidate_used_header_for_cpu(&self) {
        let used_ptr = self.used.as_ptr() as *const u8;
        dma_invalidate(
            used_ptr,
            core::mem::size_of::<u16>() * 2,
            self.cacheable,
            "invalidate used ring header",
        );
        fence(AtomicOrdering::SeqCst);
    }

    fn invalidate_used_elem_for_cpu(&self, ring_slot: usize) {
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
        );
        fence(AtomicOrdering::SeqCst);
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

    fn read_descriptor_checked(&self, index: u16) -> Option<VirtqDesc> {
        if index >= self.size {
            return None;
        }
        Some(unsafe { read_volatile(self.desc.as_ptr().add(index as usize)) })
    }

    fn indices(&self) -> (u16, u16) {
        let used = self.used.as_ptr();
        let avail = self.avail.as_ptr();

        self.invalidate_used_header_for_cpu();
        let used_idx = unsafe { read_volatile(&(*used).idx) };
        let avail_idx = unsafe { read_volatile(&(*avail).idx) };

        (used_idx, avail_idx)
    }

    fn indices_no_sync(&self) -> (u16, u16) {
        let used = self.used.as_ptr();
        let avail = self.avail.as_ptr();

        let used_idx = unsafe { read_volatile(&(*used).idx) };
        let avail_idx = unsafe { read_volatile(&(*avail).idx) };

        (used_idx, avail_idx)
    }

    pub fn debug_dump(&self, label: &str) {
        let used = self.used.as_ptr();
        let avail = self.avail.as_ptr();

        self.invalidate_used_header_for_cpu();
        let used_idx = unsafe { read_volatile(&(*used).idx) };
        let avail_idx = unsafe { read_volatile(&(*avail).idx) };

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
    ) -> Result<Option<(u16, u32)>, ForensicFault> {
        let qsize = usize::from(self.size);
        if qsize == 0 {
            uart_ring_oob(
                "used_qsize_zero",
                b"size=0x",
                qsize,
                b"used=0x",
                self.layout.used_len,
                b"last=0x",
                self.last_used as usize,
            );
            panic!("VIRTIO_RING_OOB");
        }
        let used = self.used.as_ptr();
        self.invalidate_used_header_for_cpu();
        let idx = unsafe { read_volatile(&(*used).idx) };
        if self.last_used == idx {
            return Ok(None);
        }
        let distance = idx.wrapping_sub(self.last_used);
        if distance > self.size {
            uart_ring_oob(
                "used_distance",
                b"distance=0x",
                distance as usize,
                b"size=0x",
                qsize,
                b"idx=0x",
                idx as usize,
            );
            panic!("VIRTIO_RING_OOB");
        }

        let ring_slot = (self.last_used as usize) % qsize;
        assert!(ring_slot < qsize, "used ring slot out of range");
        let ring_offset = ring_slot * core::mem::size_of::<VirtqUsedElem>();
        let ring_bytes = qsize * core::mem::size_of::<VirtqUsedElem>();
        if ring_offset + core::mem::size_of::<VirtqUsedElem>() > ring_bytes {
            uart_ring_oob(
                "used_offset",
                b"offset=0x",
                ring_offset,
                b"bytes=0x",
                ring_bytes,
                b"slot=0x",
                ring_slot,
            );
            panic!("VIRTIO_RING_OOB");
        }
        let elem_ptr = unsafe { (*used).ring.as_ptr().add(ring_slot) as *const VirtqUsedElem };
        self.invalidate_used_elem_for_cpu(ring_slot);
        let elem = unsafe { read_volatile(elem_ptr) };
        if elem.len == 0 {
            let head_id = elem.id as u16;
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
                        len: elem.len,
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
                    elem.len,
                    addr = desc.addr,
                    len = desc.len,
                    flags = desc.flags,
                    next = desc.next,
                );
                return Ok(None);
            }
        }
        if elem.len != 0 {
            self.used_zero_len_head = None;
        }
        if elem.id >= u32::from(self.size) {
            uart_ring_oob(
                "used_head",
                b"head=0x",
                elem.id as usize,
                b"size=0x",
                qsize,
                b"idx=0x",
                idx as usize,
            );
            panic!("VIRTIO_RING_OOB");
        }
        let desc_idx = elem.id as usize;
        let desc = unsafe { read_volatile(self.desc.as_ptr().add(desc_idx)) };
        if desc.addr == 0 || desc.len == 0 {
            return Err(ForensicFault {
                queue_name: queue_label,
                qsize: self.size,
                head: elem.id as u16,
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
            elem.id,
            elem.len,
        );
        self.last_used = self.last_used.wrapping_add(1);
        Ok(Some((elem.id as u16, elem.len)))
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

fn ranges_overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start < b_end && b_start < a_end
}

fn desc_fingerprint_safe(queue: &VirtQueue) -> u64 {
    let count = core::cmp::min(queue.size as usize, 2);
    let mut acc = 0u64;
    for idx in 0..count {
        let Some(desc) = queue.read_descriptor_checked(idx as u16) else {
            continue;
        };
        acc ^= desc.addr
            ^ u64::from(desc.len)
            ^ u64::from(desc.flags)
            ^ u64::from(desc.next);
    }
    acc
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn dma_clean(ptr: *const u8, len: usize, cacheable: bool, reason: &str) {
    if len == 0 {
        return;
    }
    if !cacheable {
        if !DMA_SKIP_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            warn!(
                target: "virtio-net",
                "[virtio-net][dma] cache ops disabled (mapping not proven cacheable or feature off)",
            );
        }
        return;
    }
    let log_once = !DMA_CLEAN_LOGGED.swap(true, AtomicOrdering::AcqRel);
    info!(
        target: "virtio-net",
        "[virtio-net][dma] cache op reason={reason}",
    );
    if log_once {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] clean enter ptr=0x{ptr:016x} len={len}",
            ptr = ptr as usize,
            len = len,
        );
    }
    compiler_fence(AtomicOrdering::Release);
    if let Err(err) = cache_clean(seL4_CapInitThreadVSpace, ptr as usize, len) {
        warn!(
            target: "virtio-net",
            "[virtio-net][dma] clean syscall failed err={}",
            err,
        );
    }
    if log_once {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] clean exit ptr=0x{ptr:016x} len={len}",
            ptr = ptr as usize,
            len = len,
        );
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn dma_invalidate(ptr: *const u8, len: usize, cacheable: bool, reason: &str) {
    if len == 0 {
        return;
    }
    if !cacheable {
        if !DMA_SKIP_LOGGED.swap(true, AtomicOrdering::AcqRel) {
            warn!(
                target: "virtio-net",
                "[virtio-net][dma] cache ops disabled (mapping not proven cacheable or feature off)",
            );
        }
        return;
    }
    let log_once = !DMA_INVALIDATE_LOGGED.swap(true, AtomicOrdering::AcqRel);
    info!(
        target: "virtio-net",
        "[virtio-net][dma] cache op reason={reason}",
    );
    if log_once {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] invalidate enter ptr=0x{ptr:016x} len={len}",
            ptr = ptr as usize,
            len = len,
        );
    }
    compiler_fence(AtomicOrdering::SeqCst);
    if let Err(err) = cache_invalidate(seL4_CapInitThreadVSpace, ptr as usize, len) {
        warn!(
            target: "virtio-net",
            "[virtio-net][dma] invalidate syscall failed err={}",
            err,
        );
    }
    if log_once {
        info!(
            target: "virtio-net",
            "[virtio-net][dma] invalidate exit ptr=0x{ptr:016x} len={len}",
            ptr = ptr as usize,
            len = len,
        );
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn dma_barrier() {
    compiler_fence(AtomicOrdering::Release);
    unsafe {
        asm!("dsb ishst", "isb", options(nostack, preserves_flags));
    }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn dma_clean(_ptr: *const u8, _len: usize, _cacheable: bool, _reason: &str) {}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn dma_invalidate(_ptr: *const u8, _len: usize, _cacheable: bool, _reason: &str) {}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn dma_barrier() {
    compiler_fence(AtomicOrdering::Release);
}
