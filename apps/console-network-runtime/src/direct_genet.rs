// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Exchange bounded CPU-only Ethernet rings with the isolated Pi GENET owner.
// Author: Lukas Bower

//! Bounded console-network endpoint for the Pi GENET direct link.
//!
//! The isolated GENET child remains the sole MMIO, DMA, and IRQ owner. This
//! endpoint consumes its CPU-only RX ring and produces the reciprocal TX ring.
//! Shared payload bytes use relaxed atomic access so raced readers and writers
//! never create a Rust data race; every aligned generation, cursor, and slot
//! commit adds the required acquire/release publication. A raced cursor
//! replacement retains the exact operation for a later service unit; stable
//! corruption, generation drift, or poison remains terminal.

use core::sync::atomic::{fence, AtomicU64, AtomicU8, Ordering};

use console_network_runtime::abi::{
    DirectGenetConsumerCommit, DirectGenetControlPage, DirectGenetCursorRole,
    DirectGenetCursorState, DirectGenetDirection, DirectGenetError, DirectGenetLayout,
    DirectGenetProducerCommit, DirectGenetRingSnapshot, DirectGenetSlotPage, DirectGenetSlotRecord,
    DIRECT_GENET_CONTROL_HEADER_BYTES, DIRECT_GENET_CURSOR_STATE_BYTES,
    DIRECT_GENET_POISON_INVALID_CONTROL, DIRECT_GENET_POISON_INVALID_CURSOR,
    DIRECT_GENET_POISON_INVALID_SLOT, DIRECT_GENET_POISON_STALE_GENERATION,
    DIRECT_GENET_RX_CONSUMER_STATE_OFFSET, DIRECT_GENET_RX_PRODUCER_STATE_OFFSET,
    DIRECT_GENET_RX_SLOT_COUNT, DIRECT_GENET_SLOT_COMMIT_OFFSET, DIRECT_GENET_SLOT_HEADER_BYTES,
    DIRECT_GENET_SLOT_PAYLOAD_OFFSET, DIRECT_GENET_TX_CONSUMER_STATE_OFFSET,
    DIRECT_GENET_TX_PRODUCER_STATE_OFFSET, DIRECT_GENET_TX_SLOT_COUNT, ETHERNET_FRAME_BYTES,
    SHARED_PAGE_BYTES,
};

const CURSOR_COMMIT_WITHIN_STATE: usize = DIRECT_GENET_CURSOR_STATE_BYTES - 8;
const CONTROL_GENERATION_OFFSET: usize = 8;
const CONTROL_SEAL_OFFSET: usize = DIRECT_GENET_CONTROL_HEADER_BYTES - 8;

const _: () = assert!(DIRECT_GENET_CONTROL_HEADER_BYTES == 64);
const _: () = assert!(DIRECT_GENET_CURSOR_STATE_BYTES == 64);
const _: () = assert!(DIRECT_GENET_SLOT_HEADER_BYTES == 64);
const _: () = assert!(DIRECT_GENET_SLOT_PAYLOAD_OFFSET == DIRECT_GENET_SLOT_HEADER_BYTES);
const _: () = assert!(DIRECT_GENET_SLOT_PAYLOAD_OFFSET + ETHERNET_FRAME_BYTES <= SHARED_PAGE_BYTES);

#[derive(Clone, Copy)]
struct PendingReceive {
    initial: DirectGenetRingSnapshot,
    record: DirectGenetSlotRecord,
}

/// One bounded CPU-only SPSC endpoint owned by the console-network child.
pub struct DirectGenetLink {
    generation: u64,
    control_vaddr: usize,
    rx_vaddrs: [usize; DIRECT_GENET_RX_SLOT_COUNT],
    tx_vaddrs: [usize; DIRECT_GENET_TX_SLOT_COUNT],
    peer_wake_notification_slot: u32,
    pending_receive: Option<PendingReceive>,
    ready_receive: Option<DirectGenetSlotRecord>,
    transmit_credit: Option<DirectGenetRingSnapshot>,
    pending_transmit: Option<DirectGenetRingSnapshot>,
    peer_wake_pending: bool,
    work_pending: bool,
    rx_data_pending: bool,
    transient_retry_pending: bool,
}

impl DirectGenetLink {
    /// Construct one endpoint from a descriptor-validated sealed layout.
    pub fn new(layout: DirectGenetLayout) -> Result<Self, DirectGenetError> {
        layout
            .validate()
            .map_err(|_| DirectGenetError::InvalidLayout)?;
        let control_vaddr = checked_page_address(layout.control_vaddr)?;
        let mut rx_vaddrs = [0usize; DIRECT_GENET_RX_SLOT_COUNT];
        let mut index = 0usize;
        while index < rx_vaddrs.len() {
            rx_vaddrs[index] = checked_page_address(layout.rx_vaddrs[index])?;
            index += 1;
        }
        let mut tx_vaddrs = [0usize; DIRECT_GENET_TX_SLOT_COUNT];
        index = 0;
        while index < tx_vaddrs.len() {
            tx_vaddrs[index] = checked_page_address(layout.tx_vaddrs[index])?;
            index += 1;
        }
        Ok(Self {
            generation: layout.generation,
            control_vaddr,
            rx_vaddrs,
            tx_vaddrs,
            peer_wake_notification_slot: layout.peer_wake_notification_slot,
            pending_receive: None,
            ready_receive: None,
            transmit_credit: None,
            pending_transmit: None,
            peer_wake_pending: false,
            work_pending: true,
            rx_data_pending: false,
            transient_retry_pending: false,
        })
    }

    /// Advance exact retained cursor commits and validate both live rings.
    pub fn poll(&mut self) -> Result<(), DirectGenetError> {
        self.work_pending = false;
        self.rx_data_pending = false;
        self.transient_retry_pending = false;
        self.finish_pending_transmit()?;
        self.finish_pending_receive()?;

        match self.snapshot(DirectGenetDirection::Rx) {
            Ok(snapshot) => {
                self.rx_data_pending = snapshot.occupancy() != 0;
                self.work_pending |= self.rx_data_pending;
            }
            Err(DirectGenetError::StateChanged) => {
                self.work_pending = true;
                self.transient_retry_pending = true;
            }
            Err(error) => return Err(error),
        }
        match self.snapshot(DirectGenetDirection::Tx) {
            Ok(_) => {}
            Err(DirectGenetError::StateChanged) => {
                self.work_pending = true;
                self.transient_retry_pending = true;
            }
            Err(error) => return Err(error),
        }
        self.work_pending |= self.pending_receive.is_some()
            || self.ready_receive.is_some()
            || self.transmit_credit.is_some()
            || self.pending_transmit.is_some();
        Ok(())
    }

    /// Copy at most one committed RX frame after its consumer cursor is durable.
    pub fn receive(&mut self, output: &mut [u8]) -> Result<Option<usize>, DirectGenetError> {
        if output.len() < ETHERNET_FRAME_BYTES {
            return Err(DirectGenetError::InvalidBound);
        }
        if self.ready_receive.is_none() && self.pending_receive.is_none() {
            let initial = match self.snapshot(DirectGenetDirection::Rx) {
                Ok(snapshot) => snapshot,
                Err(DirectGenetError::StateChanged) => {
                    self.work_pending = true;
                    self.transient_retry_pending = true;
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            let (sequence, slot_index) = match initial.next_consumer() {
                Ok(next) => next,
                Err(DirectGenetError::Empty) => return Ok(None),
                Err(error) => return Err(error),
            };
            let record = self.read_slot(
                self.rx_vaddrs[slot_index],
                DirectGenetDirection::Rx,
                sequence,
            )?;
            self.pending_receive = Some(PendingReceive { initial, record });
            self.work_pending = true;
            self.finish_pending_receive()?;
        }

        let Some(record) = self.ready_receive.take() else {
            return Ok(None);
        };
        let frame = record.frame();
        output[..frame.len()].copy_from_slice(frame);
        Ok(Some(frame.len()))
    }

    /// Whether the reciprocal TX ring can accept one complete frame.
    pub fn can_transmit(&mut self) -> Result<bool, DirectGenetError> {
        if self.pending_transmit.is_some() {
            self.work_pending = true;
            return Ok(false);
        }
        if self.transmit_credit.is_some() {
            return Ok(true);
        }
        match self.snapshot(DirectGenetDirection::Tx) {
            Ok(snapshot) => match snapshot.next_producer() {
                Ok(_) => {
                    self.transmit_credit = Some(snapshot);
                    Ok(true)
                }
                Err(DirectGenetError::Backpressure) => Ok(false),
                Err(error) => Err(error),
            },
            Err(DirectGenetError::StateChanged) => {
                self.work_pending = true;
                self.transient_retry_pending = true;
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    /// Stage one TX frame and retain its exact cursor commit across a race.
    pub fn transmit(&mut self, frame: &[u8]) -> Result<(), DirectGenetError> {
        if frame.is_empty() || frame.len() > ETHERNET_FRAME_BYTES {
            return Err(DirectGenetError::InvalidBound);
        }
        if self.pending_transmit.is_some() {
            return Err(DirectGenetError::Backpressure);
        }
        let Some(initial) = self.transmit_credit.take() else {
            return Err(DirectGenetError::Backpressure);
        };
        let (_, slot_index) = initial.next_producer()?;
        self.publish_slot(
            self.tx_vaddrs[slot_index],
            DirectGenetDirection::Tx,
            initial.producer_cursor,
            frame,
        )?;
        self.pending_transmit = Some(initial);
        self.work_pending = true;
        self.finish_pending_transmit()
    }

    /// Whether a retained cursor transition or committed RX frame needs service.
    #[must_use]
    #[cfg(test)]
    pub const fn work_pending(&self) -> bool {
        self.work_pending
    }

    /// Whether local polling can make progress without a new peer wake.
    /// Committed RX data is actionable only while smoltcp has ingress credit;
    /// retained cursor reconciliation remains actionable regardless.
    #[must_use]
    pub const fn actionable_work_pending(&self, ingress_available: bool) -> bool {
        self.transient_retry_pending
            || self.pending_receive.is_some()
            || self.pending_transmit.is_some()
            || (ingress_available && (self.rx_data_pending || self.ready_receive.is_some()))
    }

    /// Return the admitted peer signal slot when one coalesced wake is due.
    pub fn take_peer_wake(&mut self) -> Option<u32> {
        if core::mem::take(&mut self.peer_wake_pending) {
            Some(self.peer_wake_notification_slot)
        } else {
            None
        }
    }

    /// Fence both console-owned directions before entering the standard fault path.
    ///
    /// Each valid owned cursor line is poisoned independently so peer progress,
    /// corruption, or poison on the reciprocal line cannot prevent the console
    /// from publishing its terminal state. The peer wake remains mandatory even
    /// when an already-corrupt owned line cannot be advanced safely.
    pub fn fail_closed(&mut self, error: DirectGenetError) {
        let reason = direct_genet_poison_reason(error);
        let _ = self.poison_owned_cursor(DirectGenetCursorRole::RxConsumer, reason);
        let _ = self.poison_owned_cursor(DirectGenetCursorRole::TxProducer, reason);
        self.pending_receive = None;
        self.ready_receive = None;
        self.transmit_credit = None;
        self.pending_transmit = None;
        self.work_pending = false;
        self.rx_data_pending = false;
        self.transient_retry_pending = false;
        self.peer_wake_pending = true;
    }

    fn finish_pending_receive(&mut self) -> Result<(), DirectGenetError> {
        let Some(pending) = self.pending_receive else {
            return Ok(());
        };
        let receipt = match self.commit_consumer(pending.initial) {
            Ok(receipt) => receipt,
            Err(DirectGenetError::StateChanged) => {
                self.work_pending = true;
                self.transient_retry_pending = true;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let expected_sequence = pending.initial.consumer_cursor + 1;
        if receipt.sequence != expected_sequence
            || receipt.slot_index != ((expected_sequence - 1) % pending.initial.capacity) as usize
            || pending.record.sequence() != expected_sequence
            || pending.record.direction() != DirectGenetDirection::Rx
        {
            return Err(DirectGenetError::InvalidCursor);
        }
        self.peer_wake_pending |= receipt.producer_rearm_due;
        self.work_pending |= receipt.work_remaining;
        self.ready_receive = Some(pending.record);
        self.pending_receive = None;
        Ok(())
    }

    fn finish_pending_transmit(&mut self) -> Result<(), DirectGenetError> {
        let Some(initial) = self.pending_transmit else {
            return Ok(());
        };
        let receipt = match self.commit_producer(initial) {
            Ok(receipt) => receipt,
            Err(DirectGenetError::StateChanged) => {
                self.work_pending = true;
                self.transient_retry_pending = true;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        Self::validate_producer_receipt(initial, receipt)?;
        self.peer_wake_pending |= receipt.data_notification_due;
        self.pending_transmit = None;
        Ok(())
    }

    fn validate_producer_receipt(
        initial: DirectGenetRingSnapshot,
        receipt: DirectGenetProducerCommit,
    ) -> Result<(), DirectGenetError> {
        let expected_sequence = initial
            .producer_cursor
            .checked_add(1)
            .ok_or(DirectGenetError::InvalidCursor)?;
        if receipt.sequence != expected_sequence
            || receipt.slot_index != ((expected_sequence - 1) % initial.capacity) as usize
        {
            return Err(DirectGenetError::InvalidCursor);
        }
        Ok(())
    }

    fn snapshot(
        &self,
        direction: DirectGenetDirection,
    ) -> Result<DirectGenetRingSnapshot, DirectGenetError> {
        let (_, snapshot) = self.control_sample(direction)?;
        Ok(snapshot)
    }

    fn control_sample(
        &self,
        direction: DirectGenetDirection,
    ) -> Result<([u8; SHARED_PAGE_BYTES], DirectGenetRingSnapshot), DirectGenetError> {
        let (producer_offset, consumer_offset) = match direction {
            DirectGenetDirection::Rx => (
                DIRECT_GENET_RX_PRODUCER_STATE_OFFSET,
                DIRECT_GENET_RX_CONSUMER_STATE_OFFSET,
            ),
            DirectGenetDirection::Tx => (
                DIRECT_GENET_TX_PRODUCER_STATE_OFFSET,
                DIRECT_GENET_TX_CONSUMER_STATE_OFFSET,
            ),
        };
        let generation = self.read_shared_atomic_u64(
            self.control_vaddr,
            CONTROL_GENERATION_OFFSET,
            Ordering::Acquire,
        );
        let seal =
            self.read_shared_atomic_u64(self.control_vaddr, CONTROL_SEAL_OFFSET, Ordering::Acquire);
        let producer_commit = self.read_shared_atomic_u64(
            self.control_vaddr,
            producer_offset + CURSOR_COMMIT_WITHIN_STATE,
            Ordering::Acquire,
        );
        let consumer_commit = self.read_shared_atomic_u64(
            self.control_vaddr,
            consumer_offset + CURSOR_COMMIT_WITHIN_STATE,
            Ordering::Acquire,
        );
        if generation == 0 || seal == 0 || producer_commit == 0 || consumer_commit == 0 {
            return Err(DirectGenetError::StateChanged);
        }
        let mut snapshot = [0u8; SHARED_PAGE_BYTES];
        self.copy_control_header_without_commits(&mut snapshot);
        snapshot[CONTROL_GENERATION_OFFSET..CONTROL_GENERATION_OFFSET + 8]
            .copy_from_slice(&generation.to_le_bytes());
        snapshot[CONTROL_SEAL_OFFSET..CONTROL_SEAL_OFFSET + 8].copy_from_slice(&seal.to_le_bytes());
        for (offset, commit) in [
            (producer_offset, producer_commit),
            (consumer_offset, consumer_commit),
        ] {
            self.copy_from_shared(
                self.control_vaddr,
                offset,
                &mut snapshot[offset..offset + CURSOR_COMMIT_WITHIN_STATE],
            );
            snapshot[offset + CURSOR_COMMIT_WITHIN_STATE..offset + DIRECT_GENET_CURSOR_STATE_BYTES]
                .copy_from_slice(&commit.to_le_bytes());
        }
        if self.read_shared_atomic_u64(
            self.control_vaddr,
            CONTROL_GENERATION_OFFSET,
            Ordering::Acquire,
        ) != generation
            || self.read_shared_atomic_u64(
                self.control_vaddr,
                CONTROL_SEAL_OFFSET,
                Ordering::Acquire,
            ) != seal
            || self.read_shared_atomic_u64(
                self.control_vaddr,
                producer_offset + CURSOR_COMMIT_WITHIN_STATE,
                Ordering::Acquire,
            ) != producer_commit
            || self.read_shared_atomic_u64(
                self.control_vaddr,
                consumer_offset + CURSOR_COMMIT_WITHIN_STATE,
                Ordering::Acquire,
            ) != consumer_commit
        {
            return Err(DirectGenetError::StateChanged);
        }
        let ring = DirectGenetControlPage::snapshot(&snapshot, self.generation, direction)?;
        Ok((snapshot, ring))
    }

    fn owner_sample(
        &self,
        role: DirectGenetCursorRole,
    ) -> Result<([u8; SHARED_PAGE_BYTES], DirectGenetCursorState), DirectGenetError> {
        let state_offset = role.offset();
        let generation = self.read_shared_atomic_u64(
            self.control_vaddr,
            CONTROL_GENERATION_OFFSET,
            Ordering::Acquire,
        );
        let seal =
            self.read_shared_atomic_u64(self.control_vaddr, CONTROL_SEAL_OFFSET, Ordering::Acquire);
        let state_commit = self.read_shared_atomic_u64(
            self.control_vaddr,
            state_offset + CURSOR_COMMIT_WITHIN_STATE,
            Ordering::Acquire,
        );
        if generation == 0 || seal == 0 || state_commit == 0 {
            return Err(DirectGenetError::StateChanged);
        }
        let mut snapshot = [0u8; SHARED_PAGE_BYTES];
        self.copy_control_header_without_commits(&mut snapshot);
        snapshot[CONTROL_GENERATION_OFFSET..CONTROL_GENERATION_OFFSET + 8]
            .copy_from_slice(&generation.to_le_bytes());
        snapshot[CONTROL_SEAL_OFFSET..CONTROL_SEAL_OFFSET + 8].copy_from_slice(&seal.to_le_bytes());
        self.copy_from_shared(
            self.control_vaddr,
            state_offset,
            &mut snapshot[state_offset..state_offset + CURSOR_COMMIT_WITHIN_STATE],
        );
        snapshot[state_offset + CURSOR_COMMIT_WITHIN_STATE
            ..state_offset + DIRECT_GENET_CURSOR_STATE_BYTES]
            .copy_from_slice(&state_commit.to_le_bytes());
        if self.read_shared_atomic_u64(
            self.control_vaddr,
            CONTROL_GENERATION_OFFSET,
            Ordering::Acquire,
        ) != generation
            || self.read_shared_atomic_u64(
                self.control_vaddr,
                CONTROL_SEAL_OFFSET,
                Ordering::Acquire,
            ) != seal
            || self.read_shared_atomic_u64(
                self.control_vaddr,
                state_offset + CURSOR_COMMIT_WITHIN_STATE,
                Ordering::Acquire,
            ) != state_commit
        {
            return Err(DirectGenetError::StateChanged);
        }
        let state = DirectGenetControlPage::cursor_state(&snapshot, self.generation, role)?;
        Ok((snapshot, state))
    }

    fn poison_owned_cursor(
        &self,
        role: DirectGenetCursorRole,
        reason: u32,
    ) -> Result<(), DirectGenetError> {
        let (mut page, state) = self.owner_sample(role)?;
        let _ = DirectGenetControlPage::poison_owner(
            &mut page,
            self.generation,
            role,
            state.state_sequence,
            reason,
        )?;
        let state_offset = role.offset();
        self.publish_sequence_last_region(
            self.control_vaddr,
            state_offset,
            &page[state_offset..state_offset + DIRECT_GENET_CURSOR_STATE_BYTES],
            CURSOR_COMMIT_WITHIN_STATE,
            DIRECT_GENET_CURSOR_STATE_BYTES,
        );
        Ok(())
    }

    fn commit_consumer(
        &self,
        initial: DirectGenetRingSnapshot,
    ) -> Result<DirectGenetConsumerCommit, DirectGenetError> {
        let mut page = self.control_snapshot_for_commit(initial.direction)?;
        let staged = DirectGenetControlPage::commit_consumer(&mut page, initial)?;
        self.publish_sequence_last_region(
            self.control_vaddr,
            DIRECT_GENET_RX_CONSUMER_STATE_OFFSET,
            &page[DIRECT_GENET_RX_CONSUMER_STATE_OFFSET
                ..DIRECT_GENET_RX_CONSUMER_STATE_OFFSET + DIRECT_GENET_CURSOR_STATE_BYTES],
            CURSOR_COMMIT_WITHIN_STATE,
            DIRECT_GENET_CURSOR_STATE_BYTES,
        );
        let final_state = self.snapshot(initial.direction)?;
        initial.reconcile_consumer_commit(final_state, staged.sequence)
    }

    fn commit_producer(
        &self,
        initial: DirectGenetRingSnapshot,
    ) -> Result<DirectGenetProducerCommit, DirectGenetError> {
        let mut page = self.control_snapshot_for_commit(initial.direction)?;
        let staged = DirectGenetControlPage::commit_producer(&mut page, initial)?;
        self.publish_sequence_last_region(
            self.control_vaddr,
            DIRECT_GENET_TX_PRODUCER_STATE_OFFSET,
            &page[DIRECT_GENET_TX_PRODUCER_STATE_OFFSET
                ..DIRECT_GENET_TX_PRODUCER_STATE_OFFSET + DIRECT_GENET_CURSOR_STATE_BYTES],
            CURSOR_COMMIT_WITHIN_STATE,
            DIRECT_GENET_CURSOR_STATE_BYTES,
        );
        let final_state = self.snapshot(initial.direction)?;
        initial.reconcile_producer_commit(final_state, staged.sequence)
    }

    fn control_snapshot_for_commit(
        &self,
        direction: DirectGenetDirection,
    ) -> Result<[u8; SHARED_PAGE_BYTES], DirectGenetError> {
        let stable = self.snapshot(direction)?;
        let (page, rechecked) = self.control_sample(direction)?;
        if rechecked != stable {
            return Err(DirectGenetError::StateChanged);
        }
        Ok(page)
    }

    fn read_slot(
        &self,
        address: usize,
        direction: DirectGenetDirection,
        sequence: u64,
    ) -> Result<DirectGenetSlotRecord, DirectGenetError> {
        self.read_slot_with_interleave(address, direction, sequence, || {})
    }

    fn read_slot_with_interleave(
        &self,
        address: usize,
        direction: DirectGenetDirection,
        sequence: u64,
        before_commit_recheck: impl FnOnce(),
    ) -> Result<DirectGenetSlotRecord, DirectGenetError> {
        let commit = self.read_shared_atomic_u64(
            address,
            DIRECT_GENET_SLOT_COMMIT_OFFSET,
            Ordering::Acquire,
        );
        if commit == 0 || commit != sequence {
            return Err(DirectGenetError::InvalidSequence);
        }
        let mut page = [0u8; SHARED_PAGE_BYTES];
        self.copy_from_shared(address, 0, &mut page[..DIRECT_GENET_SLOT_COMMIT_OFFSET]);
        page[DIRECT_GENET_SLOT_COMMIT_OFFSET..DIRECT_GENET_SLOT_COMMIT_OFFSET + 8]
            .copy_from_slice(&commit.to_le_bytes());
        let frame_len = usize::from(u16::from_le_bytes([page[10], page[11]]));
        if frame_len <= ETHERNET_FRAME_BYTES {
            self.copy_from_shared(
                address,
                DIRECT_GENET_SLOT_PAYLOAD_OFFSET,
                &mut page[DIRECT_GENET_SLOT_PAYLOAD_OFFSET
                    ..DIRECT_GENET_SLOT_PAYLOAD_OFFSET + frame_len],
            );
        }
        before_commit_recheck();
        if self.read_shared_atomic_u64(address, DIRECT_GENET_SLOT_COMMIT_OFFSET, Ordering::Acquire)
            != commit
        {
            return Err(DirectGenetError::StateChanged);
        }
        DirectGenetSlotPage::decode_next(&page, direction, self.generation, sequence - 1)
    }

    fn publish_slot(
        &self,
        address: usize,
        direction: DirectGenetDirection,
        after_cursor: u64,
        frame: &[u8],
    ) -> Result<(), DirectGenetError> {
        let mut page = [0u8; SHARED_PAGE_BYTES];
        DirectGenetSlotPage::publish_next_into(
            &mut page,
            direction,
            self.generation,
            after_cursor,
            frame,
        )?;
        self.publish_sequence_last_region(
            address,
            0,
            &page,
            DIRECT_GENET_SLOT_COMMIT_OFFSET,
            DIRECT_GENET_SLOT_PAYLOAD_OFFSET + frame.len(),
        );
        Ok(())
    }

    fn copy_from_shared(&self, address: usize, offset: usize, output: &mut [u8]) {
        let mut index = 0usize;
        while index < output.len() && (address + offset + index) & 7 != 0 {
            // SAFETY: The sealed direct-link layout validates one mapped
            // 4-KiB page at `address`; every caller bounds this prefix to it,
            // and shared body bytes are accessed atomically by both children.
            let atomic = unsafe { &*((address + offset + index) as *const AtomicU8) };
            output[index] = atomic.load(Ordering::Relaxed);
            index += 1;
        }
        while index + 8 <= output.len() {
            // SAFETY: The address is now naturally aligned, and the complete
            // eight-byte word remains in the caller-bounded page region. Both
            // children use atomic accesses for every overlapping word.
            let atomic = unsafe { &*((address + offset + index) as *const AtomicU64) };
            let word = atomic.load(Ordering::Relaxed);
            output[index..index + 8].copy_from_slice(&u64::from_le(word).to_le_bytes());
            index += 8;
        }
        while index < output.len() {
            // SAFETY: Same admitted page and bounded tail as the word loop.
            let atomic = unsafe { &*((address + offset + index) as *const AtomicU8) };
            output[index] = atomic.load(Ordering::Relaxed);
            index += 1;
        }
    }

    fn copy_control_header_without_commits(&self, output: &mut [u8; SHARED_PAGE_BYTES]) {
        self.copy_from_shared(
            self.control_vaddr,
            0,
            &mut output[..CONTROL_GENERATION_OFFSET],
        );
        self.copy_from_shared(
            self.control_vaddr,
            CONTROL_GENERATION_OFFSET + 8,
            &mut output[CONTROL_GENERATION_OFFSET + 8..DIRECT_GENET_CONTROL_HEADER_BYTES - 8],
        );
    }

    fn publish_sequence_last_region(
        &self,
        address: usize,
        offset: usize,
        input: &[u8],
        commit_offset: usize,
        initialized_bytes: usize,
    ) {
        let commit_address = address + offset + commit_offset;
        self.write_shared_atomic_u64(commit_address, 0, Ordering::Release);
        // The invalid marker must become globally ordered before any body byte
        // can be replaced; the final release commit then publishes the body.
        fence(Ordering::SeqCst);
        let mut index = 0usize;
        while index < initialized_bytes {
            if index == commit_offset {
                index += 8;
            } else if index + 8 <= initialized_bytes && (address + offset + index) & 7 == 0 {
                let word = u64::from_le_bytes([
                    input[index],
                    input[index + 1],
                    input[index + 2],
                    input[index + 3],
                    input[index + 4],
                    input[index + 5],
                    input[index + 6],
                    input[index + 7],
                ]);
                // SAFETY: The initialized prefix is bounded to the validated
                // shared page. Only this child writes the selected slot or
                // cursor state, so the sequence-last commit grants visibility.
                let atomic = unsafe { &*((address + offset + index) as *const AtomicU64) };
                atomic.store(word.to_le(), Ordering::Relaxed);
                index += 8;
            } else {
                // SAFETY: Same exclusive page owner and bounded initialized
                // tail as the aligned word path above.
                let atomic = unsafe { &*((address + offset + index) as *const AtomicU8) };
                atomic.store(input[index], Ordering::Relaxed);
                index += 1;
            }
        }
        let commit = u64::from_le_bytes([
            input[commit_offset],
            input[commit_offset + 1],
            input[commit_offset + 2],
            input[commit_offset + 3],
            input[commit_offset + 4],
            input[commit_offset + 5],
            input[commit_offset + 6],
            input[commit_offset + 7],
        ]);
        self.write_shared_atomic_u64(commit_address, commit, Ordering::Release);
    }

    fn read_shared_atomic_u64(&self, address: usize, offset: usize, ordering: Ordering) -> u64 {
        let atomic_address = address + offset;
        // SAFETY: The sealed ABI validates a live page mapping, and every
        // synchronized word is fixed at an eight-byte-aligned offset. Both
        // children access these words exclusively through AtomicU64 while the
        // generation is live.
        let atomic = unsafe { &*(atomic_address as *const AtomicU64) };
        u64::from_le(atomic.load(ordering))
    }

    fn write_shared_atomic_u64(&self, address: usize, value: u64, ordering: Ordering) {
        // SAFETY: The caller supplies the already-computed aligned address of
        // a commit word owned solely by this SPSC endpoint. The peer only loads
        // the same word atomically.
        let atomic = unsafe { &*(address as *const AtomicU64) };
        atomic.store(value.to_le(), ordering);
    }
}

fn checked_page_address(address: u64) -> Result<usize, DirectGenetError> {
    let address = usize::try_from(address).map_err(|_| DirectGenetError::InvalidLayout)?;
    address
        .checked_add(SHARED_PAGE_BYTES - 1)
        .ok_or(DirectGenetError::InvalidLayout)?;
    Ok(address)
}

const fn direct_genet_poison_reason(error: DirectGenetError) -> u32 {
    match error {
        DirectGenetError::InvalidIdentity | DirectGenetError::InvalidLayout => {
            DIRECT_GENET_POISON_INVALID_CONTROL
        }
        DirectGenetError::StaleGeneration => DIRECT_GENET_POISON_STALE_GENERATION,
        DirectGenetError::InvalidCursor
        | DirectGenetError::Empty
        | DirectGenetError::Backpressure => DIRECT_GENET_POISON_INVALID_CURSOR,
        DirectGenetError::InvalidSequence
        | DirectGenetError::InvalidBound
        | DirectGenetError::StateChanged => DIRECT_GENET_POISON_INVALID_SLOT,
        DirectGenetError::Poisoned(poison) => poison.reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATION: u64 = 0x4745_4e45_5400_0001;

    #[repr(C, align(4096))]
    struct SharedPage([u8; SHARED_PAGE_BYTES]);

    fn atomic_store_u64(address: usize, value: u64, ordering: Ordering) {
        // SAFETY: The test page is live and 4-KiB aligned, and callers use only
        // ABI-declared eight-byte-aligned offsets.
        let atomic = unsafe { &*(address as *const AtomicU64) };
        atomic.store(value.to_le(), ordering);
    }

    fn atomic_store_u8(address: usize, value: u8) {
        // SAFETY: The byte lies in the live test page and all raced test access
        // to it uses AtomicU8.
        let atomic = unsafe { &*(address as *const AtomicU8) };
        atomic.store(value, Ordering::Relaxed);
    }

    #[test]
    fn acquire_recheck_rejects_a_slot_replacement_interleaving() {
        let mut slot = Box::new(SharedPage([0; SHARED_PAGE_BYTES]));
        DirectGenetSlotPage::initialize_into(&mut slot.0).expect("slot initializes");
        DirectGenetSlotPage::publish_next_into(
            &mut slot.0,
            DirectGenetDirection::Rx,
            GENERATION,
            0,
            &[0x11; 64],
        )
        .expect("first sequence publishes");
        let address = slot.0.as_mut_ptr() as usize;
        let link = DirectGenetLink {
            generation: GENERATION,
            control_vaddr: address,
            rx_vaddrs: [address; DIRECT_GENET_RX_SLOT_COUNT],
            tx_vaddrs: [address; DIRECT_GENET_TX_SLOT_COUNT],
            peer_wake_notification_slot: 0,
            pending_receive: None,
            ready_receive: None,
            transmit_credit: None,
            pending_transmit: None,
            peer_wake_pending: false,
            work_pending: false,
            rx_data_pending: false,
            transient_retry_pending: false,
        };

        let raced = link.read_slot_with_interleave(address, DirectGenetDirection::Rx, 1, || {
            atomic_store_u64(
                address + DIRECT_GENET_SLOT_COMMIT_OFFSET,
                0,
                Ordering::Release,
            );
            fence(Ordering::SeqCst);
            for index in 0..64 {
                atomic_store_u8(address + DIRECT_GENET_SLOT_PAYLOAD_OFFSET + index, 0x22);
            }
            atomic_store_u64(
                address + DIRECT_GENET_SLOT_COMMIT_OFFSET,
                2,
                Ordering::Release,
            );
        });
        assert_eq!(raced, Err(DirectGenetError::StateChanged));
    }

    #[test]
    fn blocked_ingress_does_not_turn_rx_occupancy_into_a_busy_poll() {
        let mut slot = Box::new(SharedPage([0; SHARED_PAGE_BYTES]));
        let address = slot.0.as_mut_ptr() as usize;
        let mut link = DirectGenetLink {
            generation: GENERATION,
            control_vaddr: address,
            rx_vaddrs: [address; DIRECT_GENET_RX_SLOT_COUNT],
            tx_vaddrs: [address; DIRECT_GENET_TX_SLOT_COUNT],
            peer_wake_notification_slot: 0,
            pending_receive: None,
            ready_receive: None,
            transmit_credit: None,
            pending_transmit: None,
            peer_wake_pending: false,
            work_pending: true,
            rx_data_pending: true,
            transient_retry_pending: false,
        };

        assert!(!link.actionable_work_pending(false));
        assert!(link.actionable_work_pending(true));

        link.transient_retry_pending = true;
        assert!(link.actionable_work_pending(false));
    }
}
