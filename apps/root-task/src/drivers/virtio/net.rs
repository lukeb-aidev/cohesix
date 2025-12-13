// Author: Lukas Bower
//! Virtio MMIO network device driver used by the root task.
//!
//! Virtio MMIO network device driver used by the root task on the ARM `virt`
//! platform. RX descriptor handling and smoltcp integration are instrumented to
//! aid debugging end-to-end TCP console flows.
#![cfg(all(feature = "kernel", feature = "net-console"))]
#![allow(unsafe_code)]

use core::fmt::{self, Write as FmtWrite};
use core::ptr::{read_volatile, write_volatile, NonNull};
use core::sync::atomic::{compiler_fence, fence, AtomicBool, Ordering as AtomicOrdering};

use heapless::{String as HeaplessString, Vec as HeaplessVec};
use log::{debug, error, info, warn};
use sel4_sys::{seL4_Error, seL4_NotEnoughMemory};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;

use crate::hal::{self, alloc_dma, DmaBuf, HalError, Hardware, MmioRegion};
use crate::net::{NetDevice, NetDeviceCounters, NetDriverError, CONSOLE_TCP_PORT};
use crate::net_consts::MAX_FRAME_LEN;
use crate::sel4::PAGE_SIZE;

const VIRTIO_MMIO_BASE: usize = 0x0a00_0000;
const VIRTIO_MMIO_STRIDE: usize = 0x200;
const VIRTIO_MMIO_SLOTS: usize = 8;

const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976;
const VIRTIO_MMIO_VERSION_MODERN: u32 = 2;
const VIRTIO_DEVICE_ID_NET: u32 = 1;

const DEVICE_FRAME_BITS: usize = 12;

const STATUS_ACKNOWLEDGE: u32 = 1 << 0;
const STATUS_DRIVER: u32 = 1 << 1;
const STATUS_DRIVER_OK: u32 = 1 << 2;
const STATUS_FEATURES_OK: u32 = 1 << 3;
const STATUS_FAILED: u32 = 1 << 7;

const VIRTQ_DESC_F_WRITE: u16 = 1 << 1;

const VIRTIO_F_VERSION_1: u64 = 1 << 32;

const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const SUPPORTED_FEATURES: u64 = VIRTIO_NET_F_MAC | VIRTIO_F_VERSION_1;

const RX_QUEUE_INDEX: u32 = 0;
const TX_QUEUE_INDEX: u32 = 1;

const RX_QUEUE_SIZE: usize = 16;
const TX_QUEUE_SIZE: usize = 16;
const VIRTIO_NET_HEADER_LEN: usize = core::mem::size_of::<VirtioNetHdr>();
const FRAME_BUFFER_LEN: usize = MAX_FRAME_LEN + VIRTIO_NET_HEADER_LEN;
static LOG_TCP_DEST_PORT: AtomicBool = AtomicBool::new(true);

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
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sel4(err) => write!(f, "seL4 error {err:?}"),
            Self::Hal(err) => write!(f, "hal error: {err}"),
            Self::NoDevice => f.write_str("virtio-net device not found"),
            Self::NoQueue => f.write_str("virtio-net queues unavailable"),
            Self::BufferExhausted => f.write_str("virtio-net DMA buffer exhausted"),
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
    rx_queue: VirtQueue,
    tx_queue: VirtQueue,
    rx_buffers: HeaplessVec<DmaBuf, RX_QUEUE_SIZE>,
    tx_buffers: HeaplessVec<DmaBuf, TX_QUEUE_SIZE>,
    tx_free: HeaplessVec<u16, TX_QUEUE_SIZE>,
    tx_drops: u32,
    tx_packets: u64,
    tx_used_count: u64,
    rx_packets: u64,
    mac: EthernetAddress,
    rx_poll_count: u64,
    rx_used_count: u64,
    last_used_idx_debug: u16,
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
        info!(
            "[net-console] virtio-mmio device located: base=0x{base:08x}",
            base = regs.base().as_ptr() as usize
        );

        info!("[net-console] resetting virtio-net status register");
        regs.reset_status();
        regs.set_status(STATUS_ACKNOWLEDGE);
        info!(
            target: "net-console",
            "[net-console] status set to ACKNOWLEDGE: 0x{:02x}",
            regs.status()
        );
        regs.set_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        info!(
            target: "net-console",
            "[net-console] status set to DRIVER: 0x{:02x}",
            regs.status()
        );

        info!("[net-console] querying queue sizes");
        let rx_max = regs.queue_num_max(RX_QUEUE_INDEX);
        let tx_max = regs.queue_num_max(TX_QUEUE_INDEX);
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
        let host_features = regs.device_features_64();
        let negotiated_features = host_features & SUPPORTED_FEATURES;
        if negotiated_features & VIRTIO_F_VERSION_1 == 0 {
            regs.set_status(STATUS_FAILED);
            error!(
                target: "net-console",
                "[virtio-net] VIRTIO_F_VERSION_1 not offered by device; host_features=0x{host:016x}",
                host = host_features
            );
            return Err(DriverError::NoQueue);
        }
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
        regs.set_driver_features_64(negotiated_features);
        info!(
            target: "net-console",
            "[net-console] guest features set: status=0x{:02x}",
            regs.status()
        );
        let mut status = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
        regs.set_status(status);
        let status_after_features = regs.status();
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

        let queue_mem_rx = alloc_dma(hal, PAGE_SIZE, PAGE_SIZE).map_err(|err| {
            regs.set_status(STATUS_FAILED);
            DriverError::from(err)
        })?;

        let queue_mem_tx = {
            let mut attempt = 0;
            loop {
                let frame = alloc_dma(hal, PAGE_SIZE, PAGE_SIZE).map_err(|err| {
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
        let rx_queue = VirtQueue::new(&mut regs, queue_mem_rx, RX_QUEUE_INDEX, rx_size)?;
        info!(
            "[net-console] provisioning TX descriptors ({} entries)",
            tx_size
        );
        let tx_queue = VirtQueue::new(&mut regs, queue_mem_tx, TX_QUEUE_INDEX, tx_size)?;

        let mut rx_buffers = HeaplessVec::<DmaBuf, RX_QUEUE_SIZE>::new();
        for _ in 0..rx_size {
            let frame = alloc_dma(hal, PAGE_SIZE, PAGE_SIZE).map_err(|err| {
                regs.set_status(STATUS_FAILED);
                DriverError::from(err)
            })?;
            rx_buffers.push(frame).map_err(|_| {
                regs.set_status(STATUS_FAILED);
                DriverError::BufferExhausted
            })?;
        }

        let mut tx_buffers = HeaplessVec::<DmaBuf, TX_QUEUE_SIZE>::new();
        for _ in 0..tx_size {
            let frame = alloc_dma(hal, PAGE_SIZE, PAGE_SIZE).map_err(|err| {
                regs.set_status(STATUS_FAILED);
                DriverError::from(err)
            })?;
            tx_buffers.push(frame).map_err(|_| {
                regs.set_status(STATUS_FAILED);
                DriverError::BufferExhausted
            })?;
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

        let mut driver = Self {
            regs,
            rx_queue,
            tx_queue,
            rx_buffers,
            tx_buffers,
            tx_free,
            tx_drops: 0,
            tx_packets: 0,
            tx_used_count: 0,
            rx_packets: 0,
            mac,
            rx_poll_count: 0,
            rx_used_count: 0,
            last_used_idx_debug: 0,
        };
        driver.initialise_queues();

        let status_reg_value = driver.regs.status();
        log::info!(
            target: "net-console",
            "[virtio-net] post-setup: rx_desc=0x{:x}, tx_desc=0x{:x}, status=0x{:02x}",
            driver.rx_queue.desc_paddr,
            driver.tx_queue.desc_paddr,
            status_reg_value,
        );

        status |= STATUS_DRIVER_OK;
        driver.regs.set_status(status);
        info!(
            "[net-console] driver status set to DRIVER_OK (status=0x{:02x})",
            driver.regs.status()
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
        let isr = self.regs.isr_status();
        let status = self.regs.status();

        self.rx_queue.debug_dump("rx");
        self.tx_queue.debug_dump("tx");

        log::info!(
            target: "net-console",
            "[virtio-net] debug_snapshot: status=0x{:02x} isr=0x{:02x} last_used_idx_debug={} rx_used_count={} rx_poll_count={}",
            status,
            isr,
            self.last_used_idx_debug,
            self.rx_used_count,
            self.rx_poll_count,
        );
    }

    fn initialise_queues(&mut self) {
        for (index, buffer) in self.rx_buffers.iter().enumerate() {
            let idx = index as u16;
            self.rx_queue.setup_descriptor(
                idx,
                buffer.paddr(),
                FRAME_BUFFER_LEN as u32,
                VIRTQ_DESC_F_WRITE,
            );
            self.rx_queue.push_avail(idx);
        }
        let (used_idx, avail_idx) = self.rx_queue.indices();
        log::debug!(
            target: "virtio-net",
            "[RX] posted buffers={} used_idx={} avail_idx={}",
            self.rx_buffers.len(),
            used_idx,
            avail_idx,
        );
        self.rx_queue.notify(&mut self.regs, RX_QUEUE_INDEX);
        info!(
            "[virtio-net] RX queue armed: size={} buffers={} last_used={}",
            self.rx_queue.size,
            self.rx_buffers.len(),
            self.rx_queue.last_used,
        );
        info!(
            target: "net-console",
            "[virtio-net] RX queue initialised: size={} buffers={}",
            self.rx_queue.size,
            self.rx_buffers.len(),
        );

        log::info!(
            target: "net-console",
            "[virtio-net] TX queue initialised: size={} buffers={} free_entries={}",
            self.tx_queue.size,
            self.tx_buffers.len(),
            self.tx_free.len(),
        );
    }

    fn poll_interrupts(&mut self) {
        let (status, isr_ack) = self.regs.acknowledge_interrupts();
        log::debug!(
            target: "virtio-net",
            "ISR status=0x{:02x}, ISRACK=0x{:02x}",
            status,
            isr_ack,
        );
        self.reclaim_tx();
    }

    fn reclaim_tx(&mut self) {
        while let Some((id, _len)) = self.tx_queue.pop_used() {
            self.tx_used_count = self.tx_used_count.wrapping_add(1);
            if self.tx_free.push(id).is_err() {
                self.tx_drops = self.tx_drops.saturating_add(1);
            }
        }
    }

    fn pop_rx(&mut self) -> Option<(u16, usize)> {
        let result = self.rx_queue.pop_used().map(|(id, len)| (id, len as usize));

        if result.is_some() {
            self.last_used_idx_debug = self.rx_queue.last_used;
            let (used_idx, avail_idx) = self.rx_queue.indices();
            log::debug!(
                target: "virtio-net",
                "[RX] consumed used_idx={} avail_idx={}",
                used_idx,
                avail_idx,
            );
        }

        result
    }

    fn requeue_rx(&mut self, id: u16) {
        if let Some(buffer) = self.rx_buffers.get_mut(id as usize) {
            self.rx_queue.setup_descriptor(
                id,
                buffer.paddr(),
                FRAME_BUFFER_LEN as u32,
                VIRTQ_DESC_F_WRITE,
            );
            self.rx_queue.push_avail(id);
            let (used_idx, avail_idx) = self.rx_queue.indices();
            log::debug!(
                target: "virtio-net",
                "[RX] posted buffers={} used_idx={} avail_idx={}",
                1,
                used_idx,
                avail_idx,
            );
            self.rx_queue.notify(&mut self.regs, RX_QUEUE_INDEX);
        }
        debug!(target: "net-console", "[virtio-net] requeue_rx: id={id}");
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
        if let Some(buffer) = self.tx_buffers.get_mut(id as usize) {
            let length = len.min(buffer.as_mut_slice().len());
            self.tx_queue
                .setup_descriptor(id, buffer.paddr(), length as u32, 0);
            self.tx_queue.push_avail(id);
            self.tx_queue.notify(&mut self.regs, TX_QUEUE_INDEX);
            let slice = buffer.as_mut_slice();
            for byte in &mut slice[length..] {
                *byte = 0;
            }
            info!(
                target: "net-console",
                "[virtio-net] TX descriptor posted: id={} len={}",
                id,
                length
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

        let (used_idx, avail_idx) = self.rx_queue.indices();
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
            let (used_idx, avail_idx) = self.rx_queue.indices();
            log::info!(
                target: "net-console",
                "[virtio-net] RX: used descriptor received: id={} len={} used.idx={} avail.idx={} (rx_used_count={})",
                id,
                len,
                used_idx,
                avail_idx,
                self.rx_used_count,
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
        let buffer = driver
            .rx_buffers
            .get_mut(self.id as usize)
            .expect("rx descriptor out of range");
        let available = core::cmp::min(self.len, buffer.as_mut_slice().len());
        if available < VIRTIO_NET_HEADER_LEN {
            warn!(
                "[virtio-net] RX: frame too small for virtio-net header (len={})",
                available
            );
            driver.requeue_rx(self.id);
            return f(&[]);
        }

        let payload_len = available - VIRTIO_NET_HEADER_LEN;
        let mut_slice = &mut buffer.as_mut_slice()[..available];
        let payload = &mut mut_slice[VIRTIO_NET_HEADER_LEN..VIRTIO_NET_HEADER_LEN + payload_len];
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
        driver.requeue_rx(self.id);
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
        if let Some(id) = self.desc {
            let buffer = driver
                .tx_buffers
                .get_mut(id as usize)
                .expect("tx descriptor out of range");
            let max_len = buffer.as_mut_slice().len();
            if max_len <= VIRTIO_NET_HEADER_LEN {
                driver.tx_drops = driver.tx_drops.saturating_add(1);
                return f(&mut []);
            }

            let payload_len = len
                .min(MAX_FRAME_LEN)
                .min(max_len.saturating_sub(VIRTIO_NET_HEADER_LEN));
            let total_len = payload_len + VIRTIO_NET_HEADER_LEN;
            let result = {
                let mut_slice = &mut buffer.as_mut_slice()[..total_len];
                let (header, payload) =
                    mut_slice.split_at_mut(core::cmp::min(VIRTIO_NET_HEADER_LEN, total_len));
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
    mmio: MmioRegion,
    config_base: NonNull<u8>,
    version: u32,
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
            let region = match hal::map_mmio(hal, base, VIRTIO_MMIO_STRIDE) {
                Ok(region) => region,
                Err(HalError::Sel4(err)) if err == seL4_NotEnoughMemory => {
                    log::trace!(
                        "virtio-mmio: slot {slot} @ 0x{base:08x} unavailable (no device coverage)",
                    );
                    continue;
                }
                Err(err) => return Err(DriverError::from(err)),
            };
            let config_base =
                unsafe { NonNull::new_unchecked(region.as_ptr().add(Registers::Config as usize)) };
            let mut regs = VirtioRegs {
                mmio: region,
                config_base,
                version: 0,
            };
            let magic = regs.magic_value();
            let version = regs.read_version();
            let device_id = regs.device_id();
            let vendor_id = regs.vendor_id();
            regs.version = version;
            info!(
                "[net-console] slot={slot} mmio=0x{base:08x} id=0x{device_id:04x} vendor=0x{vendor_id:04x} magic=0x{magic:08x} version={version}",
                slot = slot,
                base = base,
                device_id = device_id,
                vendor_id = vendor_id,
                version = version
            );
            let header_valid = magic == VIRTIO_MMIO_MAGIC && version == VIRTIO_MMIO_VERSION_MODERN;
            if magic == VIRTIO_MMIO_MAGIC && version != VIRTIO_MMIO_VERSION_MODERN {
                warn!(
                    target: "net-console",
                    "[virtio-net] virtio-mmio device present but not modern (version={version}); skipping"
                );
                continue;
            }
            if header_valid && device_id == 0 {
                warn!(
                    "[net-console] slot={} has virtio header but device_id=0 (no usable device)",
                    slot
                );
                continue;
            }
            if header_valid && device_id != VIRTIO_DEVICE_ID_NET {
                warn!(
                    "[net-console] slot={} hosts non-net virtio device (id=0x{device_id:04x}); skipping",
                    slot
                );
                continue;
            }
            if header_valid && device_id == VIRTIO_DEVICE_ID_NET && vendor_id != 0 {
                info!(
                    target: "net-console",
                    "[virtio-net] found device: slot={} paddr=0x{base:08x} vaddr=0x{vaddr:08x} config_vaddr=0x{config:08x} device_id=0x{device_id:04x} vendor=0x{vendor_id:04x}",
                    slot,
                    base = base,
                    vaddr = regs.base().as_ptr() as usize,
                    config = regs.config_base().as_ptr() as usize,
                    device_id = device_id,
                    vendor_id = vendor_id,
                );
                return Ok(regs);
            }
        }
        error!("[net-console] no virtio-net device found on virtio-mmio bus; TCP console disabled");
        Err(DriverError::NoDevice)
    }

    fn base(&self) -> NonNull<u8> {
        unsafe { NonNull::new_unchecked(self.mmio.as_ptr()) }
    }

    fn config_base(&self) -> NonNull<u8> {
        self.config_base
    }

    fn paddr(&self) -> usize {
        self.mmio.paddr()
    }

    fn read_reg32(&self, offset: Registers) -> u32 {
        compiler_fence(AtomicOrdering::SeqCst);
        let value = unsafe { hal::mmio::read32(&self.mmio, offset as usize) };
        compiler_fence(AtomicOrdering::SeqCst);
        value
    }

    fn write_reg32(&mut self, offset: Registers, value: u32) {
        compiler_fence(AtomicOrdering::SeqCst);
        unsafe { hal::mmio::write32(&mut self.mmio, offset as usize, value) };
        compiler_fence(AtomicOrdering::SeqCst);
    }

    fn magic_value(&self) -> u32 {
        self.read_reg32(Registers::MagicValue)
    }

    fn read_version(&self) -> u32 {
        self.read_reg32(Registers::Version)
    }

    fn device_id(&self) -> u32 {
        self.read_reg32(Registers::DeviceId)
    }

    fn vendor_id(&self) -> u32 {
        self.read_reg32(Registers::VendorId)
    }

    fn device_features(&mut self, select: u32) -> u32 {
        self.write_reg32(Registers::DeviceFeaturesSel, select);
        self.read_reg32(Registers::DeviceFeatures)
    }

    fn driver_features(&mut self, select: u32, value: u32) {
        self.write_reg32(Registers::DriverFeaturesSel, select);
        self.write_reg32(Registers::DriverFeatures, value);
    }

    fn set_driver_features_64(&mut self, features: u64) {
        self.driver_features(0, features as u32);
        self.driver_features(1, (features >> 32) as u32);
    }

    fn device_features_64(&mut self) -> u64 {
        let lower = self.device_features(0) as u64;
        let upper = self.device_features(1) as u64;
        (upper << 32) | lower
    }

    fn reset_status(&mut self) {
        self.write_reg32(Registers::Status, 0);
    }

    fn set_status(&mut self, status: u32) {
        self.write_reg32(Registers::Status, status);
    }

    fn status(&self) -> u32 {
        self.read_reg32(Registers::Status)
    }

    fn write_queue_sel(&mut self, index: u32) {
        self.write_reg32(Registers::QueueSel, index);
    }

    fn queue_num_max(&mut self, index: u32) -> u32 {
        self.write_queue_sel(index);
        self.read_reg32(Registers::QueueNumMax)
    }

    fn write_queue_num(&mut self, index: u32, size: u16) {
        self.write_queue_sel(index);
        self.write_reg32(Registers::QueueNum, size as u32);
    }

    fn read_queue_num(&mut self, index: u32) -> u32 {
        self.write_queue_sel(index);
        self.read_reg32(Registers::QueueNum)
    }

    fn write_queue_desc(&mut self, index: u32, paddr: u64) {
        self.write_queue_sel(index);
        self.write_reg32(Registers::QueueDescLow, paddr as u32);
        self.write_reg32(Registers::QueueDescHigh, (paddr >> 32) as u32);
    }

    fn write_queue_avail(&mut self, index: u32, paddr: u64) {
        self.write_queue_sel(index);
        self.write_reg32(Registers::QueueAvailLow, paddr as u32);
        self.write_reg32(Registers::QueueAvailHigh, (paddr >> 32) as u32);
    }

    fn write_queue_used(&mut self, index: u32, paddr: u64) {
        self.write_queue_sel(index);
        self.write_reg32(Registers::QueueUsedLow, paddr as u32);
        self.write_reg32(Registers::QueueUsedHigh, (paddr >> 32) as u32);
    }

    fn write_queue_ready(&mut self, index: u32, ready: u32) {
        self.write_queue_sel(index);
        self.write_reg32(Registers::QueueReady, ready);
    }

    fn read_queue_ready(&mut self, index: u32) -> u32 {
        self.write_queue_sel(index);
        self.read_reg32(Registers::QueueReady)
    }

    fn configure_queue(
        &mut self,
        index: u32,
        size: u16,
        desc_paddr: u64,
        avail_paddr: u64,
        used_paddr: u64,
    ) -> Result<u16, DriverError> {
        info!("[virtio-net] queue{index}: QueueSel <- {index}");
        self.write_reg32(Registers::QueueSel, index);
        self.trace_queue_regs(index, "after select");

        let max = self.read_reg32(Registers::QueueNumMax);
        info!("[virtio-net] queue{index}: QueueNumMax -> {max}");
        if size == 0 || size as u32 > max {
            error!(
                target: "net-console",
                "[virtio-net] queue {index} unsupported size: requested={} max={}",
                size,
                max
            );
            self.set_status(self.status() | STATUS_FAILED);
            return Err(DriverError::NoQueue);
        }

        self.write_reg32(Registers::QueueNum, size as u32);
        self.trace_queue_regs(index, "after queue num write");
        info!("[virtio-net] queue{index}: QueueNum <- {size}");
        let queue_num = self.read_reg32(Registers::QueueNum);
        self.trace_queue_regs(index, "after queue num read");
        info!("[virtio-net] queue{index}: QueueNum readback -> {queue_num}");
        if queue_num == 0 {
            error!(
                target: "net-console",
                "[virtio-net] queue {index} rejected size; readback was zero (max={max})",
            );
            self.set_status(self.status() | STATUS_FAILED);
            return Err(DriverError::NoQueue);
        }

        let configured_size = queue_num as u16;
        if configured_size != size {
            if queue_num > max || configured_size < 2 {
                let snapshot = self.queue_register_snapshot(index);
                error!(
                    target: "net-console",
                    "[virtio-net] queue {index} size write failed: expected={} readback={} max={} sel={} ready={} status=0x{:02x}",
                    size,
                    queue_num,
                    snapshot.queue_num_max,
                    snapshot.queue_sel,
                    snapshot.queue_ready,
                    snapshot.status,
                );
                self.set_status(self.status() | STATUS_FAILED);
                return Err(DriverError::NoQueue);
            }

            warn!(
                target: "net-console",
                "[virtio-net] queue {index} size clamped by device: requested={} configured={} max={}",
                size,
                configured_size,
                max,
            );
        }

        self.write_reg32(Registers::QueueDescLow, desc_paddr as u32);
        self.write_reg32(Registers::QueueDescHigh, (desc_paddr >> 32) as u32);
        self.write_reg32(Registers::QueueAvailLow, avail_paddr as u32);
        self.write_reg32(Registers::QueueAvailHigh, (avail_paddr >> 32) as u32);
        self.write_reg32(Registers::QueueUsedLow, used_paddr as u32);
        self.write_reg32(Registers::QueueUsedHigh, (used_paddr >> 32) as u32);
        self.trace_queue_regs(index, "after queue addr program");
        info!("[virtio-net] queue{index}: DESC <- 0x{desc_paddr:016x}");
        info!("[virtio-net] queue{index}: AVAIL <- 0x{avail_paddr:016x}");
        info!("[virtio-net] queue{index}: USED <- 0x{used_paddr:016x}");
        hal::dma_wmb();
        self.write_reg32(Registers::QueueReady, 1);
        hal::dma_wmb();
        self.trace_queue_regs(index, "after ready set");
        info!("[virtio-net] queue{index}: QueueReady <- 1");
        let ready = self.read_reg32(Registers::QueueReady);
        info!("[virtio-net] queue{index}: QueueReady readback -> {ready}");
        if ready != 1 {
            error!(
                target: "net-console",
                "[virtio-net] queue {index} failed to enter ready state: ready_readback={}",
                ready,
            );
            self.set_status(self.status() | STATUS_FAILED);
            return Err(DriverError::NoQueue);
        }

        Ok(configured_size)
    }

    fn queue_register_snapshot(&mut self, index: u32) -> QueueRegisterSnapshot {
        QueueRegisterSnapshot {
            queue_sel: {
                self.write_queue_sel(index);
                self.read_reg32(Registers::QueueSel)
            },
            queue_num_max: self.queue_num_max(index),
            queue_num: self.read_queue_num(index),
            queue_ready: self.read_queue_ready(index),
            status: self.read_reg32(Registers::Status),
        }
    }

    #[cfg(feature = "bootstrap-trace")]
    fn trace_queue_regs(&mut self, index: u32, stage: &str) {
        if index != RX_QUEUE_INDEX {
            return;
        }

        let snapshot = self.queue_register_snapshot(index);
        info!(
            target: "net-console",
            "[virtio-net:trace] queue{index} {stage}: sel={} num={} max={} ready={} status=0x{:02x}",
            snapshot.queue_sel,
            snapshot.queue_num,
            snapshot.queue_num_max,
            snapshot.queue_ready,
            snapshot.status,
        );
    }

    #[cfg(not(feature = "bootstrap-trace"))]
    fn trace_queue_regs(&mut self, _index: u32, _stage: &str) {}

    fn notify(&mut self, queue: u32) {
        self.write_reg32(Registers::QueueNotify, queue);
    }

    fn acknowledge_interrupts(&mut self) -> (u32, u32) {
        let status = self.read_reg32(Registers::InterruptStatus);
        let mut ack = 0;
        if status != 0 {
            self.write_reg32(Registers::InterruptAck, status);
            ack = status;
        }
        (status, ack)
    }

    fn isr_status(&self) -> u32 {
        self.read_reg32(Registers::InterruptStatus)
    }

    fn read_mac(&self) -> Option<EthernetAddress> {
        let mut bytes = [0u8; 6];
        compiler_fence(AtomicOrdering::SeqCst);
        for (idx, byte) in bytes.iter_mut().enumerate() {
            *byte = unsafe {
                read_volatile(
                    self.config_base().as_ptr().add(VIRTIO_NET_CONFIG_MAC + idx) as *const u8
                )
            };
        }
        compiler_fence(AtomicOrdering::SeqCst);
        if bytes.iter().all(|&b| b == 0) {
            None
        } else {
            Some(EthernetAddress::from_bytes(&bytes))
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum Registers {
    MagicValue = 0x000,
    Version = 0x004,
    DeviceId = 0x008,
    VendorId = 0x00c,
    DeviceFeatures = 0x010,
    DeviceFeaturesSel = 0x014,
    DriverFeatures = 0x020,
    DriverFeaturesSel = 0x024,
    QueueSel = 0x030,
    QueueNumMax = 0x034,
    QueueNum = 0x038,
    QueueReady = 0x044,
    QueueNotify = 0x050,
    InterruptStatus = 0x060,
    InterruptAck = 0x064,
    Status = 0x070,
    QueueDescLow = 0x080,
    QueueDescHigh = 0x084,
    QueueAvailLow = 0x090,
    QueueAvailHigh = 0x094,
    QueueUsedLow = 0x0a0,
    QueueUsedHigh = 0x0a4,
    Config = 0x100,
}

struct QueueRegisterSnapshot {
    queue_sel: u32,
    queue_num_max: u32,
    queue_num: u32,
    queue_ready: u32,
    status: u32,
}

const VIRTIO_NET_CONFIG_MAC: usize = 0x00;

struct VirtQueue {
    _frame: DmaBuf,
    layout: VirtqLayout,
    size: u16,
    desc: NonNull<VirtqDesc>,
    avail: NonNull<VirtqAvail>,
    used: NonNull<VirtqUsed>,
    last_used: u16,
    base_paddr: u64,
    desc_paddr: u64,
    avail_paddr: u64,
    used_paddr: u64,
}

impl VirtQueue {
    fn new(
        regs: &mut VirtioRegs,
        mut frame: DmaBuf,
        index: u32,
        size: usize,
    ) -> Result<Self, DriverError> {
        let queue_size = size as u16;
        let base_ptr = frame.vaddr();
        frame.as_mut_slice().fill(0);

        let frame_capacity = frame.as_mut_slice().len();
        let mut layout = VirtqLayout::compute(queue_size, frame_capacity)?;

        let base_paddr = frame.paddr();
        let desc_paddr = base_paddr;
        let mut avail_paddr = base_paddr + layout.avail_offset as u64;
        let mut used_paddr = base_paddr + layout.used_offset as u64;

        layout.validate(base_paddr as usize, frame_capacity)?;

        let mut configured_size =
            regs.configure_queue(index, queue_size, desc_paddr, avail_paddr, used_paddr)?;
        if configured_size != queue_size {
            layout = VirtqLayout::compute(configured_size, frame_capacity)?;
            avail_paddr = base_paddr + layout.avail_offset as u64;
            used_paddr = base_paddr + layout.used_offset as u64;
            layout.validate(base_paddr as usize, frame_capacity)?;
            configured_size =
                regs.configure_queue(index, configured_size, desc_paddr, avail_paddr, used_paddr)?;
        }

        let desc_ptr = base_ptr.cast::<VirtqDesc>();
        let avail_ptr = unsafe {
            NonNull::new_unchecked(base_ptr.as_ptr().add(layout.avail_offset) as *mut VirtqAvail)
        };
        let used_ptr = unsafe {
            NonNull::new_unchecked(base_ptr.as_ptr().add(layout.used_offset) as *mut VirtqUsed)
        };
        info!(
            target: "net-console",
            "[virtio-net] queue {} layout: base_paddr=0x{:x} desc=0x{:x} avail=0x{:x} used=0x{:x} desc_len={} avail_len={} used_len={} total_len={}",
            index,
            base_paddr,
            desc_paddr,
            avail_paddr,
            used_paddr,
            layout.desc_len,
            layout.avail_len,
            layout.used_len,
            layout.total_len,
        );

        Ok(Self {
            _frame: frame,
            layout,
            size: configured_size,
            desc: desc_ptr,
            avail: avail_ptr,
            used: used_ptr,
            last_used: 0,
            base_paddr,
            desc_paddr,
            avail_paddr,
            used_paddr,
        })
    }

    fn setup_descriptor(&self, index: u16, addr: u64, len: u32, flags: u16) {
        let desc = unsafe { &mut *self.desc.as_ptr().add(index as usize) };
        desc.addr = addr;
        desc.len = len;
        desc.flags = flags;
        desc.next = 0;
    }

    fn push_avail(&self, index: u16) {
        let avail = self.avail.as_ptr();
        let idx = unsafe { read_volatile(&(*avail).idx) };
        let ring_slot = idx % self.size;
        unsafe {
            let ring_ptr = (*avail).ring.as_ptr().add(ring_slot as usize) as *mut u16;
            write_volatile(ring_ptr, index);
            fence(AtomicOrdering::Release);
            write_volatile(&mut (*avail).idx, idx.wrapping_add(1));
        }
    }

    fn notify(&mut self, regs: &mut VirtioRegs, queue: u32) {
        fence(AtomicOrdering::Release);
        regs.notify(queue);
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

    fn indices(&self) -> (u16, u16) {
        let used = self.used.as_ptr();
        let avail = self.avail.as_ptr();

        let used_idx = unsafe { read_volatile(&(*used).idx) };
        let avail_idx = unsafe { read_volatile(&(*avail).idx) };

        (used_idx, avail_idx)
    }

    pub fn debug_dump(&self, label: &str) {
        let used = self.used.as_ptr();
        let avail = self.avail.as_ptr();

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

    fn pop_used(&mut self) -> Option<(u16, u32)> {
        let used = self.used.as_ptr();
        let idx = unsafe { read_volatile(&(*used).idx) };
        if self.last_used == idx {
            return None;
        }
        fence(AtomicOrdering::Acquire);
        let ring_slot = self.last_used % self.size;
        let elem_ptr =
            unsafe { (*used).ring.as_ptr().add(ring_slot as usize) as *const VirtqUsedElem };
        let elem = unsafe { read_volatile(elem_ptr) };
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
        Some((elem.id as u16, elem.len))
    }
}

#[repr(C)]
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
    fn compute(queue_size: u16, frame_capacity: usize) -> Result<Self, DriverError> {
        let size = queue_size as usize;
        let desc_len = core::mem::size_of::<VirtqDesc>()
            .checked_mul(size)
            .ok_or(DriverError::NoQueue)?;
        let avail_offset = align_up(desc_len, 2);
        let avail_len = 6usize
            .checked_add(core::mem::size_of::<u16>() * size)
            .ok_or(DriverError::NoQueue)?;
        let used_offset = align_up(
            avail_offset
                .checked_add(avail_len)
                .ok_or(DriverError::NoQueue)?,
            4,
        );
        let used_len = 6usize
            .checked_add(core::mem::size_of::<VirtqUsedElem>() * size)
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

    fn validate(&self, base_paddr: usize, frame_capacity: usize) -> Result<(), DriverError> {
        let desc_paddr = base_paddr;
        let avail_paddr = base_paddr
            .checked_add(self.avail_offset)
            .ok_or(DriverError::NoQueue)?;
        let used_paddr = base_paddr
            .checked_add(self.used_offset)
            .ok_or(DriverError::NoQueue)?;
        let frame_end = base_paddr
            .checked_add(frame_capacity)
            .ok_or(DriverError::NoQueue)?;

        let desc_end = desc_paddr
            .checked_add(self.desc_len)
            .ok_or(DriverError::NoQueue)?;
        let avail_end = avail_paddr
            .checked_add(self.avail_len)
            .ok_or(DriverError::NoQueue)?;
        let used_end = used_paddr
            .checked_add(self.used_len)
            .ok_or(DriverError::NoQueue)?;

        let regions = [
            ("desc", desc_paddr, desc_end, 16usize),
            ("avail", avail_paddr, avail_end, 2usize),
            ("used", used_paddr, used_end, 4usize),
        ];

        for (label, start, end, align) in regions {
            if start == 0 || start % align != 0 || end > frame_end {
                error!(
                    target: "net-console",
                    "[virtio-net] virtqueue {label} region invalid: start=0x{start:x} end=0x{end:x} align={} frame_end=0x{frame_end:x}",
                    align
                );
                return Err(DriverError::NoQueue);
            }
        }

        if desc_end > avail_paddr || avail_end > used_paddr {
            error!(
                target: "net-console",
                "[virtio-net] virtqueue regions overlap: desc_end=0x{desc_end:x} avail_start=0x{avail_paddr:x} used_start=0x{used_paddr:x}",
            );
            return Err(DriverError::NoQueue);
        }

        info!(
            target: "net-console",
            "[virtio-net] virtqueue layout validated: desc=0x{:x}-0x{:x} avail=0x{:x}-0x{:x} used=0x{:x}-0x{:x} total_len={} frame_len={}",
            desc_paddr,
            desc_end,
            avail_paddr,
            avail_end,
            used_paddr,
            used_end,
            self.total_len,
            frame_capacity,
        );

        Ok(())
    }
}

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn virtq_layout_respects_alignment_for_common_sizes() {
        let base_paddr = 0x2000usize;

        let layout_small =
            VirtqLayout::compute(16, 4096).expect("layout for 16 entries should fit");
        assert_eq!(layout_small.desc_len, 16 * size_of::<VirtqDesc>());
        assert_eq!(layout_small.avail_offset, 256);
        assert_eq!(layout_small.avail_len, 38);
        assert_eq!(layout_small.used_offset, 296);
        assert_eq!(layout_small.used_len, 134);
        assert_eq!(layout_small.total_len, 430);
        layout_small
            .validate(base_paddr, 4096)
            .expect("validated small layout");

        let layout_large = VirtqLayout::compute(256, 8192)
            .expect("layout for 256 entries should fit in 8 KiB frame");
        assert_eq!(layout_large.desc_len, 256 * size_of::<VirtqDesc>());
        assert_eq!(layout_large.avail_offset, 4096);
        assert_eq!(layout_large.avail_len, 518);
        assert_eq!(layout_large.used_offset, 4616);
        assert_eq!(layout_large.used_len, 2054);
        assert_eq!(layout_large.total_len, 6670);
        layout_large
            .validate(base_paddr, 8192)
            .expect("validated large layout");
    }

    #[test]
    fn set_queue_size_writes_32bit_mmio_word() {
        let mut backing = [0u32; 64];
        let ptr = NonNull::new(backing.as_mut_ptr() as *mut u8).expect("non-null backing slice");
        let mmio_len = backing.len() * size_of::<u32>();
        let mmio = unsafe { MmioRegion::from_raw_parts_for_test(ptr.as_ptr(), mmio_len) };
        let config_base =
            unsafe { NonNull::new_unchecked(ptr.as_ptr().add(Registers::Config as usize)) };
        let mut regs = VirtioRegs {
            mmio,
            config_base,
            version: VIRTIO_MMIO_VERSION_MODERN,
        };

        regs.write_queue_num(0, 0x1234);
        let queue_num_slot = Registers::QueueNum as usize / size_of::<u32>();
        assert_eq!(backing[queue_num_slot], 0x1234);
        assert_eq!(regs.read_queue_num(0), 0x1234);
    }
}
