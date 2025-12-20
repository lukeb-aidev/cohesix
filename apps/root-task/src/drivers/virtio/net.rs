// Author: Lukas Bower
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
use core::ptr::{read_volatile, write_volatile, NonNull};
use core::sync::atomic::{fence, AtomicBool, Ordering as AtomicOrdering};

use heapless::{String as HeaplessString, Vec as HeaplessVec};
use log::{debug, error, info, warn};
use sel4_sys::{seL4_Error, seL4_NotEnoughMemory, seL4_PageBits};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;

use crate::hal::{HalError, Hardware};
use crate::net::{NetDevice, NetDeviceCounters, NetDriverError, CONSOLE_TCP_PORT};
use crate::net_consts::MAX_FRAME_LEN;
use crate::sel4::{DeviceFrame, RamFrame};

const FORENSICS: bool = true;
const FORENSICS_PUBLISH_LOG_LIMIT: usize = 64;

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
static RING_SLOT_CANARY_LOGGED: [AtomicBool; VIRTIO_MMIO_SLOTS] =
    [const { AtomicBool::new(false) }; VIRTIO_MMIO_SLOTS];
static FORENSICS_FROZEN: AtomicBool = AtomicBool::new(false);
static FORENSICS_DUMPED: AtomicBool = AtomicBool::new(false);

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
    negotiated_features: u64,
    rx_header_len: usize,
    rx_payload_capacity: usize,
    rx_frame_capacity: usize,
    rx_buffer_capacity: usize,
    tx_drops: u32,
    tx_packets: u64,
    tx_used_count: u64,
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
    rx_zero_len_logged: bool,
    tx_zero_len_logged: bool,
    rx_header_zero_logged: bool,
    rx_payload_zero_logged: bool,
    rx_underflow_logged_ids: HeaplessVec<u16, RX_QUEUE_SIZE>,
    last_error: Option<&'static str>,
    rx_requeue_logged_ids: HeaplessVec<u16, RX_QUEUE_SIZE>,
    rx_publish_log_count: u32,
    tx_publish_log_count: u32,
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
        if status_after_features & STATUS_FEATURES_OK == 0 {
            regs.set_status(STATUS_FAILED);
            error!(
                target: "net-console",
                "[virtio-net] device rejected FEATURES_OK: status=0x{status_after_features:02x}"
            );
            return Err(DriverError::NoQueue);
        }

        info!("[net-console] allocating virtqueue backing memory");

        let queue_mem_rx = hal.alloc_dma_frame().map_err(|err| {
            regs.set_status(STATUS_FAILED);
            DriverError::from(err)
        })?;

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

        info!(
            "[net-console] provisioning RX descriptors ({} entries)",
            rx_size
        );
        let rx_queue = VirtQueue::new(&mut regs, queue_mem_rx, RX_QUEUE_INDEX, rx_size, mmio_mode)
            .map_err(|err| {
                regs.set_status(STATUS_FAILED);
                err
            })?;
        info!(
            "[net-console] provisioning TX descriptors ({} entries)",
            tx_size
        );
        let tx_queue = VirtQueue::new(&mut regs, queue_mem_tx, TX_QUEUE_INDEX, tx_size, mmio_mode)
            .map_err(|err| {
                regs.set_status(STATUS_FAILED);
                err
            })?;

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
            negotiated_features,
            rx_header_len: net_header_len,
            rx_payload_capacity,
            rx_frame_capacity,
            rx_buffer_capacity,
            tx_drops: 0,
            tx_packets: 0,
            tx_used_count: 0,
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
            rx_zero_len_logged: false,
            tx_zero_len_logged: false,
            rx_header_zero_logged: false,
            rx_payload_zero_logged: false,
            rx_underflow_logged_ids: HeaplessVec::new(),
            last_error: None,
            rx_requeue_logged_ids: HeaplessVec::new(),
            rx_publish_log_count: 0,
            tx_publish_log_count: 0,
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
        }
    }

    fn log_forensic_fault(&self, fault: &ForensicFault) {
        error!(
            target: "net-console",
            "[virtio-net][forensics] fault queue={} head={} idx={} qsize={} addr=0x{addr:016x} len={} flags=0x{flags:04x} next={next} reason={reason}",
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
        self.dump_descriptor_table("rx", &self.rx_queue);
        self.dump_descriptor_table("tx", &self.tx_queue);
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
        let should_log = force || (*counter as usize) < FORENSICS_PUBLISH_LOG_LIMIT;
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
            self.rx_queue.notify(&mut self.regs, RX_QUEUE_INDEX);
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
        self.check_device_health();
        if self.device_faulted || forensics_frozen() {
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

        fence(AtomicOrdering::Release);
        self.verify_descriptor_write("TX", QueueKind::Tx, head_id, &resolved_descs)?;
        self.tx_queue.sync_descriptor_table_for_device();
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

        let Some((slot, avail_idx, old_idx)) = self.tx_queue.push_avail(head_id) else {
            self.device_faulted = true;
            self.last_error.get_or_insert("tx_avail_write_failed");
            return Err(());
        };
        self.tx_queue.sync_avail_ring_for_device();
        self.log_publish_transaction(
            "TX",
            QueueKind::Tx,
            old_idx,
            avail_idx,
            slot,
            head_id,
            false,
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
        if notify && !self.device_faulted {
            self.tx_queue.notify(&mut self.regs, TX_QUEUE_INDEX);
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

                Self::sync_rx_slot_for_device(buffer, header_len, payload_len);

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
        let (used_idx, avail_idx) = self.rx_queue.indices();
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

    fn sync_rx_slot_for_device(buffer: &RamFrame, header_len: usize, payload_len: usize) {
        let header_ptr = buffer.ptr().as_ptr();
        let payload_ptr = unsafe { header_ptr.add(header_len) };
        let payload_len = core::cmp::min(
            payload_len,
            buffer.as_slice().len().saturating_sub(header_len),
        );
        dma_sync_for_device(header_ptr, header_len);
        dma_sync_for_device(payload_ptr, payload_len);
    }

    fn sync_rx_slot_for_cpu(buffer: &RamFrame, header_len: usize, written_len: usize) {
        let header_len = core::cmp::min(header_len, written_len);
        let header_ptr = buffer.ptr().as_ptr();
        dma_sync_for_cpu(header_ptr, header_len);
        let payload_len = written_len
            .saturating_sub(header_len)
            .min(buffer.as_slice().len().saturating_sub(header_len));
        if payload_len > 0 {
            let payload_ptr = unsafe { header_ptr.add(header_len) };
            dma_sync_for_cpu(payload_ptr, payload_len);
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
        loop {
            match self.tx_queue.pop_used("TX") {
                Ok(Some((id, _len))) => {
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

    fn pop_rx(&mut self) -> Option<(u16, usize)> {
        if forensics_frozen() {
            return None;
        }
        match self.rx_queue.pop_used("RX") {
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
                    Self::sync_rx_slot_for_cpu(buffer, header_len, len);
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

            Self::sync_rx_slot_for_device(buffer, header_len, payload_len);

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
        if let Some(id) = self.tx_free.pop() {
            VirtioTxToken::new(driver_ptr, Some(id))
        } else {
            VirtioTxToken::new(driver_ptr, None)
        }
    }

    fn submit_tx(&mut self, id: u16, len: usize) {
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
        NetDeviceCounters {
            rx_packets: self.rx_packets,
            tx_packets: self.tx_packets,
            rx_used_advances: self.rx_used_count,
            tx_used_advances: self.tx_used_count,
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
        if len == 0 {
            driver.tx_drops = driver.tx_drops.saturating_add(1);
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
            let result = {
                let mut_slice = &mut buffer.as_mut_slice()[..total_len];
                let (header, payload) =
                    mut_slice.split_at_mut(core::cmp::min(header_len, total_len));
                header.fill(0);
                let result = f(payload);
                log_tcp_trace("TX", payload);
                result
            };
            driver.submit_tx(id, total_len);
            driver.tx_packets = driver.tx_packets.saturating_add(1);
            result
        } else {
            let length = len.min(MAX_FRAME_LEN);
            let mut scratch = [0u8; MAX_FRAME_LEN];
            let result = f(&mut scratch[..length]);
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
}

impl VirtQueue {
    fn new(
        regs: &mut VirtioRegs,
        mut frame: RamFrame,
        index: u32,
        size: usize,
        mode: VirtioMmioMode,
    ) -> Result<Self, DriverError> {
        let queue_size = size as u16;
        let base_paddr = frame.paddr();
        let base_ptr = frame.ptr();
        let page_bytes = 1usize << seL4_PageBits;

        let frame_capacity = {
            let frame_slice = frame.as_mut_slice();
            let capacity = frame_slice.len();

            frame_slice.fill(0);
            capacity
        };

        unsafe {
            core::ptr::write_bytes(base_ptr.as_ptr(), 0, frame_capacity);
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

        let queue_align = core::mem::size_of::<VirtqUsedElem>();
        let layout = VirtqLayout::compute(size, queue_align, frame_capacity)?;

        if layout.avail_offset + layout.avail_len > frame_capacity
            || layout.used_offset + layout.used_len > frame_capacity
        {
            error!(
                target: "net-console",
                "[virtio-net] virtqueue layout exceeds backing: avail_end={} used_end={} capacity={}",
                layout.avail_offset + layout.avail_len,
                layout.used_offset + layout.used_len,
                frame_capacity,
            );
            return Err(DriverError::QueueInvariant(
                "virtqueue layout exceeds backing",
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
        let used_idx = unsafe { read_volatile(&(*used_ptr.as_ptr()).idx) };

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
            "[virtio-net] queue {index} layout: base_vaddr=0x{vaddr:016x} base_paddr=0x{paddr:016x} desc=+0x{desc_off:03x} avail=+0x{avail_off:03x} used=+0x{used_off:03x} total_len={total}",
            vaddr = base_ptr.as_ptr() as usize,
            paddr = base_paddr,
            desc_off = 0,
            avail_off = layout.avail_offset,
            used_off = layout.used_offset,
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
                regs.set_queue_align(queue_align as u32);
                regs.set_queue_pfn(queue_pfn);
            }
        }
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
            last_used: 0,
            pfn: queue_pfn,
            base_paddr,
        })
    }

    fn setup_descriptor(&self, index: u16, addr: u64, len: u32, flags: u16, next: Option<u16>) {
        if index >= self.size {
            error!(
                target: "net-console",
                "[virtio-net] descriptor index out of range: index={} size={}",
                index,
                self.size,
            );
            return;
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
        if index >= self.size {
            error!(
                target: "net-console",
                "[virtio-net] avail ring index out of range: index={} size={}",
                index,
                self.size,
            );
            return None;
        }
        let avail = self.avail.as_ptr();
        let qsize = usize::from(self.size);

        assert!(qsize != 0, "virtqueue size must be non-zero");

        let idx = unsafe { read_volatile(&(*avail).idx) };
        let ring_slot = (idx as usize) % qsize;
        assert!(ring_slot < qsize, "avail ring slot out of range");
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
        fence(AtomicOrdering::Release);
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
        dma_sync_for_device(self.desc.as_ptr() as *const u8, self.layout.desc_len);
    }

    fn sync_avail_ring_for_device(&self) {
        dma_sync_for_device(self.avail.as_ptr() as *const u8, self.layout.avail_len);
    }

    fn sync_used_ring_for_cpu(&self) {
        dma_sync_for_cpu(self.used.as_ptr() as *const u8, self.layout.used_len);
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

    fn indices(&self) -> (u16, u16) {
        let used = self.used.as_ptr();
        let avail = self.avail.as_ptr();

        self.sync_used_ring_for_cpu();
        let used_idx = unsafe { read_volatile(&(*used).idx) };
        let avail_idx = unsafe { read_volatile(&(*avail).idx) };

        (used_idx, avail_idx)
    }

    pub fn debug_dump(&self, label: &str) {
        let used = self.used.as_ptr();
        let avail = self.avail.as_ptr();

        self.sync_used_ring_for_cpu();
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

    fn pop_used(&mut self, queue_label: &'static str) -> Result<Option<(u16, u32)>, ForensicFault> {
        let used = self.used.as_ptr();
        self.sync_used_ring_for_cpu();
        let idx = unsafe { read_volatile(&(*used).idx) };
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

        assert!(qsize != 0, "virtqueue size must be non-zero");

        let ring_slot = (self.last_used as usize) % qsize;
        assert!(ring_slot < qsize, "used ring slot out of range");
        let elem_ptr = unsafe { (*used).ring.as_ptr().add(ring_slot) as *const VirtqUsedElem };
        let elem = unsafe { read_volatile(elem_ptr) };
        if elem.id >= u32::from(self.size) {
            return Err(ForensicFault {
                queue_name: queue_label,
                qsize: self.size,
                head: elem.id as u16,
                idx: self.last_used,
                addr: 0,
                len: elem.len,
                flags: 0,
                next: 0,
                reason: ForensicFaultReason::UsedIdOutOfRange,
            });
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
    desc_len: usize,
    avail_offset: usize,
    avail_len: usize,
    used_offset: usize,
    used_len: usize,
    total_len: usize,
}

impl VirtqLayout {
    fn compute(
        queue_size: usize,
        queue_align: usize,
        frame_capacity: usize,
    ) -> Result<Self, DriverError> {
        debug_assert!(queue_align.is_power_of_two());

        let desc_len = core::mem::size_of::<VirtqDesc>()
            .checked_mul(queue_size)
            .ok_or(DriverError::NoQueue)?;
        let avail_len = core::mem::size_of::<VirtqAvail>()
            .checked_add(core::mem::size_of::<u16>() * queue_size)
            .ok_or(DriverError::NoQueue)?;
        let avail_offset = desc_len;
        let used_offset = align_up(desc_len + avail_len, queue_align);
        let used_len = core::mem::size_of::<VirtqUsed>()
            .checked_add(core::mem::size_of::<VirtqUsedElem>() * queue_size)
            .ok_or(DriverError::NoQueue)?;
        let total_len = used_offset
            .checked_add(used_len)
            .ok_or(DriverError::NoQueue)?;

        if total_len > frame_capacity {
            error!(
                target: "net-console",
                "[net-console] virtqueue layout overflows frame: frame_len={} frame_capacity={}",
                total_len,
                frame_capacity,
            );
            return Err(DriverError::NoQueue);
        }

        Ok(Self {
            desc_len,
            avail_offset,
            avail_len,
            used_offset,
            used_len,
            total_len,
        })
    }
}

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn cache_line_len() -> usize {
    64
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn dma_sync_for_device(ptr: *const u8, len: usize) {
    let line = cache_line_len();
    let start = ptr as usize;
    let end = start.saturating_add(len);
    let aligned_start = start & !(line - 1);
    let aligned_end = (end + line - 1) & !(line - 1);
    let mut addr = aligned_start;
    unsafe {
        while addr < aligned_end {
            asm!("dc cvac, {addr}", addr = in(reg) addr, options(nostack, preserves_flags));
            addr = addr.saturating_add(line);
        }
        asm!("dsb ishst", "isb", options(nostack, preserves_flags));
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn dma_sync_for_cpu(ptr: *const u8, len: usize) {
    let line = cache_line_len();
    let start = ptr as usize;
    let end = start.saturating_add(len);
    let aligned_start = start & !(line - 1);
    let aligned_end = (end + line - 1) & !(line - 1);
    let mut addr = aligned_start;
    unsafe {
        while addr < aligned_end {
            asm!("dc civac, {addr}", addr = in(reg) addr, options(nostack, preserves_flags));
            addr = addr.saturating_add(line);
        }
        asm!("dsb ish", "isb", options(nostack, preserves_flags));
    }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn dma_sync_for_device(_ptr: *const u8, _len: usize) {}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn dma_sync_for_cpu(_ptr: *const u8, _len: usize) {}
