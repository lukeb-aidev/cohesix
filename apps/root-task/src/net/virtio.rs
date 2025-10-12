#![cfg(target_os = "none")]
#![allow(unsafe_code)]

use core::ptr::{read_volatile, write_volatile, NonNull};

use heapless::{Deque, String as HeaplessString, Vec as HeaplessVec};
use sel4_sys::seL4_Error;
use smoltcp::iface::{Config as IfaceConfig, Interface, SocketHandle, SocketSet, SocketStorage};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp::{Socket as TcpSocket, SocketBuffer as TcpSocketBuffer, State as TcpState};
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpListenEndpoint, Ipv4Address,
};

use super::{NetPoller, NetTelemetry, CONSOLE_QUEUE_DEPTH, MAX_FRAME_LEN};
use crate::sel4::{DeviceFrame, KernelEnv, RamFrame};
use crate::serial::DEFAULT_LINE_CAPACITY;

const VIRTIO_MMIO_BASE: usize = 0x0a00_0000;
const VIRTIO_MMIO_STRIDE: usize = 0x200;
const VIRTIO_MMIO_SLOTS: usize = 16;

const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976;
const VIRTIO_MMIO_VERSION_LEGACY: u32 = 1;
const VIRTIO_DEVICE_ID_NET: u32 = 1;

const STATUS_ACKNOWLEDGE: u32 = 1 << 0;
const STATUS_DRIVER: u32 = 1 << 1;
const STATUS_DRIVER_OK: u32 = 1 << 2;
const STATUS_FEATURES_OK: u32 = 1 << 3;
const STATUS_FAILED: u32 = 1 << 7;

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 1 << 1;

const RX_QUEUE_INDEX: u32 = 0;
const TX_QUEUE_INDEX: u32 = 1;

const RX_QUEUE_SIZE: usize = 16;
const TX_QUEUE_SIZE: usize = 16;

const TCP_LISTEN_PORT: u16 = 31337;
const TCP_RX_BUFFER: usize = 2048;
const TCP_TX_BUFFER: usize = 2048;
const SOCKET_CAPACITY: usize = 4;

static SOCKET_STORAGE_IN_USE: portable_atomic::AtomicBool =
    portable_atomic::AtomicBool::new(false);
static mut SOCKET_STORAGE: [SocketStorage<'static>; SOCKET_CAPACITY] =
    [SocketStorage::EMPTY; SOCKET_CAPACITY];
static TCP_RX_STORAGE_IN_USE: portable_atomic::AtomicBool =
    portable_atomic::AtomicBool::new(false);
static mut TCP_RX_STORAGE: [u8; TCP_RX_BUFFER] = [0u8; TCP_RX_BUFFER];
static TCP_TX_STORAGE_IN_USE: portable_atomic::AtomicBool =
    portable_atomic::AtomicBool::new(false);
static mut TCP_TX_STORAGE: [u8; TCP_TX_BUFFER] = [0u8; TCP_TX_BUFFER];

/// Shared monotonic clock for the interface.
#[derive(Debug, Default)]
pub struct NetworkClock {
    ticks_ms: portable_atomic::AtomicU64,
}

impl NetworkClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            ticks_ms: portable_atomic::AtomicU64::new(0),
        }
    }

    pub fn advance(&self, delta_ms: u32) -> Instant {
        let delta = u64::from(delta_ms);
        let updated = self
            .ticks_ms
            .fetch_add(delta, portable_atomic::Ordering::Relaxed)
            .saturating_add(delta);
        let millis = i64::try_from(updated).unwrap_or(i64::MAX);
        Instant::from_millis(millis)
    }

    #[must_use]
    pub fn now(&self) -> Instant {
        let current = self.ticks_ms.load(portable_atomic::Ordering::Relaxed);
        let millis = i64::try_from(current).unwrap_or(i64::MAX);
        Instant::from_millis(millis)
    }
}

pub struct NetStack {
    clock: NetworkClock,
    device: VirtioNet,
    interface: Interface,
    sockets: SocketSet<'static>,
    tcp_handle: SocketHandle,
    line_buffer: HeaplessString<DEFAULT_LINE_CAPACITY>,
    console_lines: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, CONSOLE_QUEUE_DEPTH>,
    telemetry: NetTelemetry,
}

impl NetStack {
    pub fn new(env: &mut KernelEnv, ip: Ipv4Address) -> Self {
        let device = VirtioNet::new(env);
        let mut device = device.expect("virtio-net device not found");
        let mac = device.mac();

        let clock = NetworkClock::new();
        let mut config = IfaceConfig::new(HardwareAddress::Ethernet(mac));
        config.random_seed = 0x5a5a_5a5a_1234_5678;

        let mut interface = Interface::new(config, &mut device, clock.now());
        interface.update_ip_addrs(|addrs| {
            if addrs.push(IpCidr::new(IpAddress::from(ip), 24)).is_err() {
                addrs[0] = IpCidr::new(IpAddress::from(ip), 24);
            }
        });

        assert!(
            !SOCKET_STORAGE_IN_USE.swap(true, portable_atomic::Ordering::AcqRel),
            "virtio-net socket storage already initialised"
        );
        let sockets = SocketSet::new(unsafe { &mut SOCKET_STORAGE[..] });
        let mut stack = Self {
            clock,
            device,
            interface,
            sockets,
            tcp_handle: SocketHandle::default(),
            line_buffer: HeaplessString::new(),
            console_lines: Deque::new(),
            telemetry: NetTelemetry::default(),
        };
        stack.initialise_socket();
        stack
    }

    fn initialise_socket(&mut self) {
        assert!(
            !TCP_RX_STORAGE_IN_USE.swap(true, portable_atomic::Ordering::AcqRel),
            "virtio-net TCP RX storage already initialised"
        );
        assert!(
            !TCP_TX_STORAGE_IN_USE.swap(true, portable_atomic::Ordering::AcqRel),
            "virtio-net TCP TX storage already initialised"
        );
        let rx_buffer = unsafe { TcpSocketBuffer::new(&mut TCP_RX_STORAGE[..]) };
        let tx_buffer = unsafe { TcpSocketBuffer::new(&mut TCP_TX_STORAGE[..]) };
        let tcp_socket = TcpSocket::new(rx_buffer, tx_buffer);
        self.tcp_handle = self.sockets.add(tcp_socket);
    }

    pub fn poll_with_time(&mut self, now_ms: u64) -> bool {
        let last = self.telemetry.last_poll_ms;
        let delta = now_ms.saturating_sub(last);
        let delta_ms = core::cmp::min(delta, u64::from(u32::MAX)) as u32;
        let timestamp = if delta_ms == 0 {
            self.clock.now()
        } else {
            self.clock.advance(delta_ms)
        };

        let changed = self
            .interface
            .poll(timestamp, &mut self.device, &mut self.sockets);
        let mut activity = changed;
        activity |= self.process_tcp();

        self.telemetry.last_poll_ms = now_ms;
        if activity {
            self.telemetry.link_up = true;
        }
        self.telemetry.tx_drops = self.device.tx_drop_count();
        activity
    }

    fn process_tcp(&mut self) -> bool {
        let mut activity = false;
        let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);

        if !socket.is_open() {
            let _ = socket.listen(IpListenEndpoint::from(TCP_LISTEN_PORT));
            self.line_buffer.clear();
        }

        if socket.can_recv() {
            let mut temp = [0u8; 256];
            while socket.can_recv() {
                match socket.recv_slice(&mut temp) {
                    Ok(count) if count > 0 => {
                        for &byte in &temp[..count] {
                            match byte {
                                b'\r' => {}
                                b'\n' => {
                                    if !self.line_buffer.is_empty() {
                                        let line = self.line_buffer.clone();
                                        let _ = self.console_lines.push_back(line);
                                        self.line_buffer.clear();
                                        activity = true;
                                    }
                                }
                                0x08 | 0x7f => {
                                    self.line_buffer.pop();
                                }
                                b if b.is_ascii() && !b.is_ascii_control() => {
                                    let _ = self.line_buffer.push(b as char);
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => break,
                }
            }
        }

        if socket.can_send() && !self.console_lines.is_empty() {
            let _ = socket.send_slice(b"");
        }

        match socket.state() {
            TcpState::CloseWait | TcpState::Closed => {
                socket.close();
                self.line_buffer.clear();
            }
            _ => {}
        }

        activity
    }

    #[must_use]
    pub fn hardware_address(&self) -> EthernetAddress {
        self.device.mac()
    }

    #[must_use]
    pub fn telemetry(&self) -> NetTelemetry {
        self.telemetry
    }
}

impl NetPoller for NetStack {
    fn poll(&mut self, now_ms: u64) -> bool {
        self.poll_with_time(now_ms)
    }

    fn telemetry(&self) -> NetTelemetry {
        self.telemetry()
    }

    fn drain_console_lines(
        &mut self,
        visitor: &mut dyn FnMut(HeaplessString<DEFAULT_LINE_CAPACITY>),
    ) {
        while let Some(line) = self.console_lines.pop_front() {
            visitor(line);
        }
    }
}

impl Drop for NetStack {
    fn drop(&mut self) {
        SOCKET_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
        TCP_RX_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
        TCP_TX_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
    }
}

struct VirtioNet {
    regs: VirtioRegs,
    rx_queue: VirtQueue,
    tx_queue: VirtQueue,
    rx_buffers: HeaplessVec<RamFrame, RX_QUEUE_SIZE>,
    tx_buffers: HeaplessVec<RamFrame, TX_QUEUE_SIZE>,
    tx_free: HeaplessVec<u16, TX_QUEUE_SIZE>,
    tx_drops: u32,
    mac: EthernetAddress,
}

impl VirtioNet {
    fn new(env: &mut KernelEnv) -> Result<Self, DriverError> {
        let mut regs = VirtioRegs::probe(env)?;
        regs.reset_status();
        regs.set_status(STATUS_ACKNOWLEDGE);
        regs.set_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        regs.select_queue(RX_QUEUE_INDEX);
        let rx_max = regs.queue_num_max();
        regs.select_queue(TX_QUEUE_INDEX);
        let tx_max = regs.queue_num_max();
        let rx_size = core::cmp::min(rx_max as usize, RX_QUEUE_SIZE);
        let tx_size = core::cmp::min(tx_max as usize, TX_QUEUE_SIZE);
        if rx_size == 0 || tx_size == 0 {
            return Err(DriverError::NoQueue);
        }

        regs.set_guest_features(0);
        regs.set_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);

        let queue_mem_rx = env.alloc_dma_frame().map_err(DriverError::Sel4)?;
        let queue_mem_tx = env.alloc_dma_frame().map_err(DriverError::Sel4)?;

        let rx_queue = VirtQueue::new(&mut regs, queue_mem_rx, RX_QUEUE_INDEX, rx_size)?;
        let tx_queue = VirtQueue::new(&mut regs, queue_mem_tx, TX_QUEUE_INDEX, tx_size)?;

        let mut rx_buffers = HeaplessVec::<RamFrame, RX_QUEUE_SIZE>::new();
        for _ in 0..rx_size {
            let frame = env.alloc_dma_frame().map_err(DriverError::Sel4)?;
            rx_buffers
                .push(frame)
                .map_err(|_| DriverError::BufferExhausted)?;
        }

        let mut tx_buffers = HeaplessVec::<RamFrame, TX_QUEUE_SIZE>::new();
        for _ in 0..tx_size {
            let frame = env.alloc_dma_frame().map_err(DriverError::Sel4)?;
            tx_buffers
                .push(frame)
                .map_err(|_| DriverError::BufferExhausted)?;
        }

        let mut tx_free = HeaplessVec::<u16, TX_QUEUE_SIZE>::new();
        for idx in 0..tx_size {
            tx_free
                .push(idx as u16)
                .map_err(|_| DriverError::BufferExhausted)?;
        }

        let mac = regs
            .read_mac()
            .unwrap_or_else(|| EthernetAddress::from_bytes(&[0x02, 0, 0, 0, 0, 1]));

        let mut driver = Self {
            regs,
            rx_queue,
            tx_queue,
            rx_buffers,
            tx_buffers,
            tx_free,
            tx_drops: 0,
            mac,
        };
        driver.initialise_queues();

        driver
            .regs
            .set_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK);
        Ok(driver)
    }

    fn initialise_queues(&mut self) {
        for (index, buffer) in self.rx_buffers.iter().enumerate() {
            let idx = index as u16;
            self.rx_queue.setup_descriptor(
                idx,
                buffer.paddr() as u64,
                MAX_FRAME_LEN as u32,
                VIRTQ_DESC_F_WRITE,
            );
            self.rx_queue.push_avail(idx);
        }
        self.rx_queue.notify(&mut self.regs, RX_QUEUE_INDEX);
    }

    fn mac(&self) -> EthernetAddress {
        self.mac
    }

    fn tx_drop_count(&self) -> u32 {
        self.tx_drops
    }

    fn reclaim_tx(&mut self) {
        while let Some((id, _len)) = self.tx_queue.pop_used() {
            if self.tx_free.push(id).is_err() {
                self.tx_drops = self.tx_drops.saturating_add(1);
            }
        }
    }

    fn pop_rx(&mut self) -> Option<(u16, usize)> {
        self.rx_queue.pop_used().map(|(id, len)| (id, len as usize))
    }

    fn requeue_rx(&mut self, id: u16) {
        if let Some(buffer) = self.rx_buffers.get_mut(id as usize) {
            self.rx_queue.setup_descriptor(
                id,
                buffer.paddr() as u64,
                MAX_FRAME_LEN as u32,
                VIRTQ_DESC_F_WRITE,
            );
            self.rx_queue.push_avail(id);
            self.rx_queue.notify(&mut self.regs, RX_QUEUE_INDEX);
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
        if let Some(buffer) = self.tx_buffers.get_mut(id as usize) {
            let length = len.min(MAX_FRAME_LEN);
            self.tx_queue
                .setup_descriptor(id, buffer.paddr() as u64, length as u32, 0);
            self.tx_queue.push_avail(id);
            self.tx_queue.notify(&mut self.regs, TX_QUEUE_INDEX);
            // zero the unused portion to avoid leaking stale data.
            let slice = buffer.as_mut_slice();
            for byte in &mut slice[length..] {
                *byte = 0;
            }
        }
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
        self.reclaim_tx();
        if let Some((id, len)) = self.pop_rx() {
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
        self.reclaim_tx();
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

pub struct VirtioRxToken {
    driver: *mut VirtioNet,
    id: u16,
    len: usize,
}

impl RxToken for VirtioRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let driver = unsafe { &mut *self.driver };
        let buffer = driver
            .rx_buffers
            .get_mut(self.id as usize)
            .expect("rx descriptor out of range");
        let mut_slice = &mut buffer.as_mut_slice()[..self.len.min(MAX_FRAME_LEN)];
        let result = f(mut_slice);
        driver.requeue_rx(self.id);
        result
    }
}

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
            let length = len.min(MAX_FRAME_LEN);
            let result = {
                let buffer = driver
                    .tx_buffers
                    .get_mut(id as usize)
                    .expect("tx descriptor out of range");
                let slice = &mut buffer.as_mut_slice()[..length];
                f(slice)
            };
            driver.submit_tx(id, length);
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

#[derive(Debug)]
enum DriverError {
    Sel4(seL4_Error),
    NoDevice,
    NoQueue,
    BufferExhausted,
}

struct VirtioRegs {
    mmio: DeviceFrame,
}

impl VirtioRegs {
    fn probe(env: &mut KernelEnv) -> Result<Self, DriverError> {
        for slot in 0..VIRTIO_MMIO_SLOTS {
            let base = VIRTIO_MMIO_BASE + slot * VIRTIO_MMIO_STRIDE;
            let frame = env.map_device(base).map_err(DriverError::Sel4)?;
            let regs = VirtioRegs { mmio: frame };
            let magic = regs.read32(Registers::MAGIC_VALUE);
            let version = regs.read32(Registers::VERSION);
            let device_id = regs.read32(Registers::DEVICE_ID);
            if magic == VIRTIO_MMIO_MAGIC
                && version == VIRTIO_MMIO_VERSION_LEGACY
                && device_id == VIRTIO_DEVICE_ID_NET
            {
                return Ok(regs);
            }
        }
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

    fn read16(&self, offset: Registers) -> u16 {
        unsafe { read_volatile(self.base().as_ptr().add(offset as usize) as *const u16) }
    }

    fn write16(&mut self, offset: Registers, value: u16) {
        unsafe { write_volatile(self.base().as_ptr().add(offset as usize) as *mut u16, value) };
    }

    fn reset_status(&mut self) {
        self.write32(Registers::STATUS, 0);
    }

    fn set_status(&mut self, status: u32) {
        self.write32(Registers::STATUS, status);
    }

    fn select_queue(&mut self, index: u32) {
        self.write32(Registers::QUEUE_SEL, index);
    }

    fn queue_num_max(&self) -> u32 {
        self.read32(Registers::QUEUE_NUM_MAX)
    }

    fn set_queue_size(&mut self, size: u16) {
        self.write16(Registers::QUEUE_NUM, size);
    }

    fn set_queue_align(&mut self, align: u32) {
        self.write32(Registers::QUEUE_ALIGN, align);
    }

    fn set_queue_pfn(&mut self, pfn: u32) {
        self.write32(Registers::QUEUE_PFN, pfn);
    }

    fn queue_ready(&mut self, ready: u32) {
        self.write32(Registers::QUEUE_READY, ready);
    }

    fn notify(&mut self, queue: u32) {
        self.write32(Registers::QUEUE_NOTIFY, queue);
    }

    fn set_guest_features(&mut self, features: u32) {
        self.write32(Registers::GUEST_FEATURES_SEL, 0);
        self.write32(Registers::GUEST_FEATURES, features);
    }

    fn read_mac(&self) -> Option<EthernetAddress> {
        let mut bytes = [0u8; 6];
        let base = Registers::CONFIG as usize;
        for (idx, byte) in bytes.iter_mut().enumerate() {
            *byte = unsafe {
                read_volatile(self.base().as_ptr().add(base + idx) as *const u8)
            };
        }
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
    MAGIC_VALUE = 0x000,
    VERSION = 0x004,
    DEVICE_ID = 0x008,
    VENDOR_ID = 0x00c,
    HOST_FEATURES = 0x010,
    HOST_FEATURES_SEL = 0x014,
    GUEST_FEATURES = 0x020,
    GUEST_FEATURES_SEL = 0x024,
    QUEUE_SEL = 0x030,
    QUEUE_NUM_MAX = 0x034,
    QUEUE_NUM = 0x038,
    QUEUE_ALIGN = 0x03c,
    QUEUE_PFN = 0x040,
    QUEUE_READY = 0x044,
    QUEUE_NOTIFY = 0x050,
    INTERRUPT_STATUS = 0x060,
    INTERRUPT_ACK = 0x064,
    STATUS = 0x070,
    CONFIG = 0x100,
}

struct VirtQueue {
    frame: RamFrame,
    size: u16,
    desc: NonNull<VirtqDesc>,
    avail: NonNull<VirtqAvail>,
    used: NonNull<VirtqUsed>,
    last_used: u16,
}

impl VirtQueue {
    fn new(
        regs: &mut VirtioRegs,
        mut frame: RamFrame,
        index: u32,
        size: usize,
    ) -> Result<Self, DriverError> {
        let queue_size = size as u16;
        let base_ptr = frame.ptr();
        frame.as_mut_slice().fill(0);

        let desc_ptr = base_ptr.cast::<VirtqDesc>();
        let desc_bytes = core::mem::size_of::<VirtqDesc>() * size;
        let avail_offset = desc_bytes;
        let avail_ptr = unsafe {
            NonNull::new_unchecked(base_ptr.as_ptr().add(avail_offset) as *mut VirtqAvail)
        };

        let avail_bytes = 4 + 2 * size + 2;
        let used_offset = align_up(desc_bytes + avail_bytes, 4);
        let used_ptr =
            unsafe { NonNull::new_unchecked(base_ptr.as_ptr().add(used_offset) as *mut VirtqUsed) };

        regs.select_queue(index);
        regs.set_queue_size(queue_size);
        regs.set_queue_align(4096);
        regs.set_queue_pfn((frame.paddr() >> 12) as u32);
        regs.queue_ready(1);

        Ok(Self {
            frame,
            size: queue_size,
            desc: desc_ptr,
            avail: avail_ptr,
            used: used_ptr,
            last_used: 0,
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
            write_volatile(&mut (*avail).idx, idx.wrapping_add(1));
        }
    }

    fn notify(&mut self, regs: &mut VirtioRegs, queue: u32) {
        regs.notify(queue);
    }

    fn pop_used(&mut self) -> Option<(u16, u32)> {
        let used = self.used.as_ptr();
        let idx = unsafe { read_volatile(&(*used).idx) };
        if self.last_used == idx {
            return None;
        }
        let ring_slot = self.last_used % self.size;
        let elem_ptr =
            unsafe { (*used).ring.as_ptr().add(ring_slot as usize) as *const VirtqUsedElem };
        let elem = unsafe { read_volatile(elem_ptr) };
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

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}
