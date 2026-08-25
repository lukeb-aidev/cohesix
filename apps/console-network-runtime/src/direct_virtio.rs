// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Own the bounded QEMU VirtIO-net data path inside the isolated console-network child.
// Author: Lukas Bower

//! Minimal modern VirtIO-MMIO network ownership for the QEMU child.
//!
//! Root admits one MMIO page and fixed queue/DMA pages. This driver performs
//! no allocation, discovery walk, retry loop, or blocking operation. Each
//! public data-path call reclaims or transfers at most one fixed queue.

use core::ptr::{copy_nonoverlapping, read_volatile, write_bytes, write_volatile};

use console_network_runtime::abi::{
    DirectVirtioLayout, DIRECT_VIRTIO_BUFFER_COUNT, DIRECT_VIRTIO_PAGE_BYTES,
    DIRECT_VIRTIO_QUEUE_SIZE, ETHERNET_FRAME_BYTES,
};

const VIRTIO_MAGIC: u32 = 0x7472_6976;
const VIRTIO_VERSION_MODERN: u32 = 2;
const VIRTIO_DEVICE_NETWORK: u32 = 1;

const REG_MAGIC: usize = 0x000;
const REG_VERSION: usize = 0x004;
const REG_DEVICE_ID: usize = 0x008;
const REG_DEVICE_FEATURES: usize = 0x010;
const REG_DEVICE_FEATURES_SEL: usize = 0x014;
const REG_DRIVER_FEATURES: usize = 0x020;
const REG_DRIVER_FEATURES_SEL: usize = 0x024;
const REG_QUEUE_SEL: usize = 0x030;
const REG_QUEUE_NUM_MAX: usize = 0x034;
const REG_QUEUE_NUM: usize = 0x038;
const REG_QUEUE_READY: usize = 0x044;
const REG_QUEUE_NOTIFY: usize = 0x050;
const REG_INTERRUPT_STATUS: usize = 0x060;
const REG_INTERRUPT_ACK: usize = 0x064;
const REG_STATUS: usize = 0x070;
const REG_QUEUE_DESC_LOW: usize = 0x080;
const REG_QUEUE_DESC_HIGH: usize = 0x084;
const REG_QUEUE_DRIVER_LOW: usize = 0x090;
const REG_QUEUE_DRIVER_HIGH: usize = 0x094;
const REG_QUEUE_DEVICE_LOW: usize = 0x0a0;
const REG_QUEUE_DEVICE_HIGH: usize = 0x0a4;
const REG_CONFIG: usize = 0x100;

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;
const STATUS_FAILED: u32 = 128;
const INTERRUPT_USED_BUFFER: u32 = 1;
const INTERRUPT_CONFIG_CHANGE: u32 = 2;
const INTERRUPT_MASK: u32 = INTERRUPT_USED_BUFFER | INTERRUPT_CONFIG_CHANGE;

const FEATURE_MAC: u64 = 1 << 5;
const FEATURE_MRG_RXBUF: u64 = 1 << 15;
const FEATURE_VERSION_1: u64 = 1 << 32;
const REQUIRED_FEATURES: u64 = FEATURE_MAC | FEATURE_MRG_RXBUF | FEATURE_VERSION_1;

const RX_QUEUE: usize = 0;
const TX_QUEUE: usize = 1;
const RX_NOTIFY_PENDING: u8 = 1 << RX_QUEUE;
const TX_NOTIFY_PENDING: u8 = 1 << TX_QUEUE;
const DESC_BYTES: usize = 16;
const AVAIL_OFFSET: usize = DIRECT_VIRTIO_QUEUE_SIZE * DESC_BYTES;
const AVAIL_FLAGS_OFFSET: usize = AVAIL_OFFSET;
const AVAIL_IDX_OFFSET: usize = AVAIL_OFFSET + 2;
const AVAIL_RING_OFFSET: usize = AVAIL_OFFSET + 4;
const USED_OFFSET: usize = 296;
const USED_IDX_OFFSET: usize = USED_OFFSET + 2;
const USED_RING_OFFSET: usize = USED_OFFSET + 4;
const USED_ELEMENT_BYTES: usize = 8;
const DESC_FLAG_WRITE: u16 = 2;
// QEMU's modern VirtIO-net contract uses `virtio_net_hdr_mrg_rxbuf` for
// VERSION_1, including the trailing `num_buffers` word. Each admitted RX page
// is large enough for one complete bounded frame, so multi-buffer packets are
// rejected rather than extending the descriptor walk.
const NET_HEADER_BYTES: usize = 12;
const NET_HEADER_NUM_BUFFERS_OFFSET: usize = 10;
const ALL_BUFFERS_FREE: u32 = (1u32 << DIRECT_VIRTIO_BUFFER_COUNT) - 1;

const _: () = assert!(DIRECT_VIRTIO_QUEUE_SIZE == DIRECT_VIRTIO_BUFFER_COUNT);
const _: () = assert!(USED_RING_OFFSET + DIRECT_VIRTIO_QUEUE_SIZE * USED_ELEMENT_BYTES < 4096);
const _: () = assert!(NET_HEADER_BYTES + ETHERNET_FRAME_BYTES <= DIRECT_VIRTIO_PAGE_BYTES);

/// Direct-device initialization or bounded queue-integrity failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectVirtioError {
    /// The admitted MMIO page is not a modern VirtIO network device.
    InvalidDevice,
    /// Required modern/MAC features were unavailable or rejected.
    FeatureNegotiation,
    /// A fixed RX or TX queue cannot hold the compiled descriptor count.
    QueueUnavailable,
    /// Device-owned queue metadata named an undeclared descriptor or length.
    QueueCorrupt,
    /// The RX used ring named a descriptor outside the admitted fixed pool.
    RxDescriptorCorrupt,
    /// The RX used ring advanced without a completed byte count.
    RxLengthZero,
    /// The RX used ring reported only a partial or header-only completion.
    RxLengthHeaderOnly,
    /// The RX used ring reported more than one bounded Ethernet frame.
    RxLengthTooLong,
    /// The negotiated mergeable RX header did not describe one buffer.
    RxBufferCountCorrupt,
    /// Device MAC disagrees with the root-sealed QEMU identity.
    MacMismatch,
    /// Caller supplied an empty or oversized Ethernet frame.
    FrameBound,
    /// All bounded TX descriptors are in flight.
    TxBackpressure,
}

/// One fixed modern VirtIO-net instance with 16 RX and 16 TX DMA pages.
pub struct DirectVirtioNet {
    mmio: *mut u8,
    queue_vaddrs: [usize; 2],
    queue_paddrs: [u64; 2],
    rx_vaddr: usize,
    tx_vaddr: usize,
    rx_paddrs: [u64; DIRECT_VIRTIO_BUFFER_COUNT],
    tx_paddrs: [u64; DIRECT_VIRTIO_BUFFER_COUNT],
    rx_last_used: u16,
    rx_avail_idx: u16,
    tx_last_used: u16,
    tx_avail_idx: u16,
    tx_free_mask: u32,
    notify_pending: u8,
    mac: [u8; 6],
}

impl DirectVirtioNet {
    /// Reset, negotiate, and populate both fixed queues.
    pub fn new(
        layout: DirectVirtioLayout,
        expected_mac: [u8; 6],
    ) -> Result<Self, DirectVirtioError> {
        layout
            .validate()
            .map_err(|_| DirectVirtioError::InvalidDevice)?;
        let mut device = Self {
            mmio: layout.mmio_vaddr as *mut u8,
            queue_vaddrs: [
                layout.queue_vaddrs[RX_QUEUE] as usize,
                layout.queue_vaddrs[TX_QUEUE] as usize,
            ],
            queue_paddrs: layout.queue_paddrs,
            rx_vaddr: layout.rx_vaddr as usize,
            tx_vaddr: layout.tx_vaddr as usize,
            rx_paddrs: layout.rx_paddrs,
            tx_paddrs: layout.tx_paddrs,
            rx_last_used: 0,
            rx_avail_idx: DIRECT_VIRTIO_QUEUE_SIZE as u16,
            tx_last_used: 0,
            tx_avail_idx: 0,
            tx_free_mask: ALL_BUFFERS_FREE,
            notify_pending: 0,
            mac: [0; 6],
        };
        if device.read_mmio(REG_MAGIC) != VIRTIO_MAGIC
            || device.read_mmio(REG_VERSION) != VIRTIO_VERSION_MODERN
            || device.read_mmio(REG_DEVICE_ID) != VIRTIO_DEVICE_NETWORK
        {
            return Err(DirectVirtioError::InvalidDevice);
        }

        device.write_mmio(REG_STATUS, 0);
        device.write_mmio(REG_STATUS, STATUS_ACKNOWLEDGE);
        device.write_mmio(REG_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        let offered = device.read_features();
        if offered & REQUIRED_FEATURES != REQUIRED_FEATURES {
            device.fail_device();
            return Err(DirectVirtioError::FeatureNegotiation);
        }
        device.write_features(REQUIRED_FEATURES);
        let feature_status = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
        device.write_mmio(REG_STATUS, feature_status);
        if device.read_mmio(REG_STATUS) & STATUS_FEATURES_OK == 0 {
            device.fail_device();
            return Err(DirectVirtioError::FeatureNegotiation);
        }

        let mut mac = [0u8; 6];
        let mut byte = 0usize;
        while byte < mac.len() {
            mac[byte] = device.read_mmio_u8(REG_CONFIG + byte);
            byte += 1;
        }
        if mac != expected_mac {
            device.fail_device();
            return Err(DirectVirtioError::MacMismatch);
        }
        device.mac = mac;

        device.initialize_queue(RX_QUEUE, true)?;
        device.initialize_queue(TX_QUEUE, false)?;
        device.write_mmio(REG_STATUS, feature_status | STATUS_DRIVER_OK);
        device.notify(RX_QUEUE);
        Ok(device)
    }

    /// Device-proven Ethernet MAC.
    #[must_use]
    pub const fn mac(&self) -> [u8; 6] {
        self.mac
    }

    /// Reclaim completed TX records from device-owned queue memory.
    ///
    /// IRQ delivery is the activation source; an active bounded quantum polls
    /// used indices directly without another MMIO status read per service unit.
    pub fn poll(&mut self) -> Result<(), DirectVirtioError> {
        self.reclaim_tx()
    }

    /// Clear one direct VirtIO interrupt source before acknowledging seL4.
    pub fn acknowledge_interrupt(&self) -> Result<(), DirectVirtioError> {
        let status = self.read_mmio(REG_INTERRUPT_STATUS);
        if status == 0 || status & !INTERRUPT_MASK != 0 {
            return Err(DirectVirtioError::QueueCorrupt);
        }
        self.write_mmio(REG_INTERRUPT_ACK, status);
        Ok(())
    }

    /// Copy at most one device-completed Ethernet frame into `output`.
    pub fn receive(&mut self, output: &mut [u8]) -> Result<Option<usize>, DirectVirtioError> {
        if output.len() < ETHERNET_FRAME_BYTES {
            return Err(DirectVirtioError::FrameBound);
        }
        let used_idx = self.queue_read_u16(RX_QUEUE, USED_IDX_OFFSET);
        device_acquire_barrier();
        if self.rx_last_used == used_idx {
            return Ok(None);
        }
        let slot = usize::from(self.rx_last_used) % DIRECT_VIRTIO_QUEUE_SIZE;
        let id = self.queue_read_u32(RX_QUEUE, USED_RING_OFFSET + slot * USED_ELEMENT_BYTES);
        let used_len = self
            .queue_read_u32(RX_QUEUE, USED_RING_OFFSET + slot * USED_ELEMENT_BYTES + 4)
            as usize;
        let id = usize::try_from(id).map_err(|_| DirectVirtioError::RxDescriptorCorrupt)?;
        if id >= DIRECT_VIRTIO_BUFFER_COUNT {
            return Err(DirectVirtioError::RxDescriptorCorrupt);
        }
        if used_len == 0 {
            return Err(DirectVirtioError::RxLengthZero);
        }
        if used_len <= NET_HEADER_BYTES {
            return Err(DirectVirtioError::RxLengthHeaderOnly);
        }
        if used_len > NET_HEADER_BYTES + ETHERNET_FRAME_BYTES {
            return Err(DirectVirtioError::RxLengthTooLong);
        }
        let header = self.rx_vaddr + id * DIRECT_VIRTIO_PAGE_BYTES;
        let num_buffers = read_dma_u16(header + NET_HEADER_NUM_BUFFERS_OFFSET);
        if num_buffers != 1 {
            return Err(DirectVirtioError::RxBufferCountCorrupt);
        }
        let frame_len = used_len - NET_HEADER_BYTES;
        let source = (header + NET_HEADER_BYTES) as *const u8;
        // SAFETY: The sealed layout validates a mapped page for every admitted
        // descriptor. Device completion bounds `used_len` to that page, and
        // `output` was checked to cover the copied Ethernet prefix.
        unsafe {
            copy_nonoverlapping(source, output.as_mut_ptr(), frame_len);
        }

        self.rx_last_used = self.rx_last_used.wrapping_add(1);
        let avail_slot = usize::from(self.rx_avail_idx) % DIRECT_VIRTIO_QUEUE_SIZE;
        self.queue_write_u16(RX_QUEUE, AVAIL_RING_OFFSET + avail_slot * 2, id as u16);
        device_release_barrier();
        self.rx_avail_idx = self.rx_avail_idx.wrapping_add(1);
        self.queue_write_u16(RX_QUEUE, AVAIL_IDX_OFFSET, self.rx_avail_idx);
        self.notify_pending |= RX_NOTIFY_PENDING;
        Ok(Some(frame_len))
    }

    /// Whether one bounded TX page can be populated without overwriting DMA.
    pub fn can_transmit(&mut self) -> Result<bool, DirectVirtioError> {
        self.reclaim_tx()?;
        Ok(self.tx_free_mask != 0)
    }

    /// Copy and publish one Ethernet frame into one free TX descriptor.
    pub fn transmit(&mut self, frame: &[u8]) -> Result<(), DirectVirtioError> {
        if frame.is_empty() || frame.len() > ETHERNET_FRAME_BYTES {
            return Err(DirectVirtioError::FrameBound);
        }
        self.reclaim_tx()?;
        if self.tx_free_mask == 0 {
            return Err(DirectVirtioError::TxBackpressure);
        }
        let id = self.tx_free_mask.trailing_zeros() as usize;
        self.tx_free_mask &= !(1u32 << id);
        let buffer = (self.tx_vaddr + id * DIRECT_VIRTIO_PAGE_BYTES) as *mut u8;
        // SAFETY: `id` is selected from the fixed free mask and the sealed
        // layout maps one exclusive TX page at this address. Header plus frame
        // is compile-time bounded below one page.
        unsafe {
            write_bytes(buffer, 0, NET_HEADER_BYTES);
            copy_nonoverlapping(frame.as_ptr(), buffer.add(NET_HEADER_BYTES), frame.len());
        }
        let desc = id * DESC_BYTES;
        self.queue_write_u64(TX_QUEUE, desc, self.tx_paddrs[id]);
        self.queue_write_u32(TX_QUEUE, desc + 8, (NET_HEADER_BYTES + frame.len()) as u32);
        self.queue_write_u16(TX_QUEUE, desc + 12, 0);
        self.queue_write_u16(TX_QUEUE, desc + 14, 0);
        let avail_slot = usize::from(self.tx_avail_idx) % DIRECT_VIRTIO_QUEUE_SIZE;
        self.queue_write_u16(TX_QUEUE, AVAIL_RING_OFFSET + avail_slot * 2, id as u16);
        device_release_barrier();
        self.tx_avail_idx = self.tx_avail_idx.wrapping_add(1);
        self.queue_write_u16(TX_QUEUE, AVAIL_IDX_OFFSET, self.tx_avail_idx);
        self.notify_pending |= TX_NOTIFY_PENDING;
        Ok(())
    }

    /// Publish every queue advanced during the current bounded service quantum.
    ///
    /// The available indices are committed before this call. Coalescing all
    /// same-quantum descriptor advances into at most one kick per queue avoids
    /// an HVF/KVM MMIO exit per packet while preserving FIFO order and the
    /// fixed 16-descriptor backpressure bound.
    pub fn flush_notifications(&mut self) {
        let pending = core::mem::take(&mut self.notify_pending);
        if pending & RX_NOTIFY_PENDING != 0 {
            self.notify(RX_QUEUE);
        }
        if pending & TX_NOTIFY_PENDING != 0 {
            self.notify(TX_QUEUE);
        }
    }

    fn initialize_queue(&mut self, queue: usize, receive: bool) -> Result<(), DirectVirtioError> {
        self.write_mmio(REG_QUEUE_SEL, queue as u32);
        if self.read_mmio(REG_QUEUE_NUM_MAX) < DIRECT_VIRTIO_QUEUE_SIZE as u32 {
            self.fail_device();
            return Err(DirectVirtioError::QueueUnavailable);
        }
        self.zero_queue(queue);
        self.queue_write_u16(queue, AVAIL_FLAGS_OFFSET, 0);
        let mut id = 0usize;
        while id < DIRECT_VIRTIO_QUEUE_SIZE {
            let desc = id * DESC_BYTES;
            let paddr = if receive {
                self.rx_paddrs[id]
            } else {
                self.tx_paddrs[id]
            };
            self.queue_write_u64(queue, desc, paddr);
            self.queue_write_u32(
                queue,
                desc + 8,
                if receive {
                    DIRECT_VIRTIO_PAGE_BYTES as u32
                } else {
                    0
                },
            );
            self.queue_write_u16(queue, desc + 12, if receive { DESC_FLAG_WRITE } else { 0 });
            self.queue_write_u16(queue, desc + 14, 0);
            if receive {
                self.queue_write_u16(queue, AVAIL_RING_OFFSET + id * 2, id as u16);
            }
            id += 1;
        }
        if receive {
            self.queue_write_u16(queue, AVAIL_IDX_OFFSET, DIRECT_VIRTIO_QUEUE_SIZE as u16);
        }
        device_release_barrier();
        self.write_mmio(REG_QUEUE_NUM, DIRECT_VIRTIO_QUEUE_SIZE as u32);
        let paddr = self.queue_paddrs[queue];
        self.write_mmio(REG_QUEUE_DESC_LOW, paddr as u32);
        self.write_mmio(REG_QUEUE_DESC_HIGH, (paddr >> 32) as u32);
        let avail = paddr + AVAIL_OFFSET as u64;
        self.write_mmio(REG_QUEUE_DRIVER_LOW, avail as u32);
        self.write_mmio(REG_QUEUE_DRIVER_HIGH, (avail >> 32) as u32);
        let used = paddr + USED_OFFSET as u64;
        self.write_mmio(REG_QUEUE_DEVICE_LOW, used as u32);
        self.write_mmio(REG_QUEUE_DEVICE_HIGH, (used >> 32) as u32);
        self.write_mmio(REG_QUEUE_READY, 1);
        Ok(())
    }

    fn reclaim_tx(&mut self) -> Result<(), DirectVirtioError> {
        let used_idx = self.queue_read_u16(TX_QUEUE, USED_IDX_OFFSET);
        device_acquire_barrier();
        let mut reclaimed = 0usize;
        while self.tx_last_used != used_idx && reclaimed < DIRECT_VIRTIO_QUEUE_SIZE {
            let slot = usize::from(self.tx_last_used) % DIRECT_VIRTIO_QUEUE_SIZE;
            let id = self.queue_read_u32(TX_QUEUE, USED_RING_OFFSET + slot * USED_ELEMENT_BYTES);
            let id = usize::try_from(id).map_err(|_| DirectVirtioError::QueueCorrupt)?;
            if id >= DIRECT_VIRTIO_BUFFER_COUNT || self.tx_free_mask & (1u32 << id) != 0 {
                return Err(DirectVirtioError::QueueCorrupt);
            }
            self.tx_free_mask |= 1u32 << id;
            self.tx_last_used = self.tx_last_used.wrapping_add(1);
            reclaimed += 1;
        }
        if self.tx_last_used != used_idx {
            return Err(DirectVirtioError::QueueCorrupt);
        }
        Ok(())
    }

    fn read_features(&mut self) -> u64 {
        self.write_mmio(REG_DEVICE_FEATURES_SEL, 0);
        let low = u64::from(self.read_mmio(REG_DEVICE_FEATURES));
        self.write_mmio(REG_DEVICE_FEATURES_SEL, 1);
        let high = u64::from(self.read_mmio(REG_DEVICE_FEATURES));
        low | (high << 32)
    }

    fn write_features(&mut self, features: u64) {
        self.write_mmio(REG_DRIVER_FEATURES_SEL, 0);
        self.write_mmio(REG_DRIVER_FEATURES, features as u32);
        self.write_mmio(REG_DRIVER_FEATURES_SEL, 1);
        self.write_mmio(REG_DRIVER_FEATURES, (features >> 32) as u32);
    }

    fn notify(&self, queue: usize) {
        device_release_barrier();
        self.write_mmio(REG_QUEUE_NOTIFY, queue as u32);
    }

    fn fail_device(&self) {
        let status = self.read_mmio(REG_STATUS);
        self.write_mmio(REG_STATUS, status | STATUS_FAILED);
    }

    fn zero_queue(&self, queue: usize) {
        let base = self.queue_vaddrs[queue] as *mut u8;
        let mut offset = 0usize;
        while offset < DIRECT_VIRTIO_PAGE_BYTES {
            // SAFETY: The sealed layout maps an exclusive full queue page at
            // `base`; the fixed loop writes each byte exactly once in bounds.
            unsafe {
                write_volatile(base.add(offset), 0);
            }
            offset += 1;
        }
    }

    fn read_mmio(&self, offset: usize) -> u32 {
        // SAFETY: Construction validates one page-aligned MMIO mapping and all
        // register offsets used by this driver are within that 4-KiB page.
        unsafe { read_volatile(self.mmio.add(offset).cast::<u32>()) }
    }

    fn read_mmio_u8(&self, offset: usize) -> u8 {
        // SAFETY: Same admitted MMIO page invariant as `read_mmio`; MAC bytes
        // occupy the standard device-config window within the page.
        unsafe { read_volatile(self.mmio.add(offset)) }
    }

    fn write_mmio(&self, offset: usize, value: u32) {
        // SAFETY: Same admitted MMIO page invariant as `read_mmio`; callers use
        // only writable standard VirtIO-MMIO register offsets.
        unsafe {
            write_volatile(self.mmio.add(offset).cast::<u32>(), value);
        }
    }

    fn queue_read_u16(&self, queue: usize, offset: usize) -> u16 {
        let base = self.queue_vaddrs[queue] as *const u8;
        // SAFETY: Queue index and every fixed offset are compile-time bounded
        // to one sealed queue page and naturally aligned for the access size.
        unsafe { read_volatile(base.add(offset).cast::<u16>()) }
    }

    fn queue_read_u32(&self, queue: usize, offset: usize) -> u32 {
        let base = self.queue_vaddrs[queue] as *const u8;
        // SAFETY: Same queue-page and alignment invariant as `queue_read_u16`.
        unsafe { read_volatile(base.add(offset).cast::<u32>()) }
    }

    fn queue_write_u16(&self, queue: usize, offset: usize, value: u16) {
        let base = self.queue_vaddrs[queue] as *mut u8;
        // SAFETY: Same queue-page and alignment invariant as `queue_read_u16`;
        // queue pages are child-writable and exclusively owned.
        unsafe {
            write_volatile(base.add(offset).cast::<u16>(), value);
        }
    }

    fn queue_write_u32(&self, queue: usize, offset: usize, value: u32) {
        let base = self.queue_vaddrs[queue] as *mut u8;
        // SAFETY: Same queue-page and alignment invariant as `queue_read_u16`.
        unsafe {
            write_volatile(base.add(offset).cast::<u32>(), value);
        }
    }

    fn queue_write_u64(&self, queue: usize, offset: usize, value: u64) {
        let base = self.queue_vaddrs[queue] as *mut u8;
        // SAFETY: Descriptor address fields are naturally aligned, and the
        // fixed descriptor array remains inside one sealed queue page.
        unsafe {
            write_volatile(base.add(offset).cast::<u64>(), value);
        }
    }
}

#[inline(always)]
fn device_release_barrier() {
    // SAFETY: This barrier has no operands or memory references. It orders
    // normal uncached queue writes before the following device-visible index
    // or notification write, as required by VirtIO on AArch64.
    unsafe {
        core::arch::asm!("dmb oshst", options(nostack, preserves_flags));
    }
}

#[inline(always)]
fn device_acquire_barrier() {
    // SAFETY: This barrier has no operands or memory references. It orders a
    // device-updated used index before subsequent queue and DMA-buffer reads.
    unsafe {
        core::arch::asm!("dmb oshld", options(nostack, preserves_flags));
    }
}

#[inline(always)]
fn read_dma_u16(address: usize) -> u16 {
    // SAFETY: Callers derive this naturally aligned word from one sealed RX
    // DMA page after the device's used-ring completion and acquire barrier.
    u16::from_le(unsafe { read_volatile(address as *const u16) })
}
