// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Outbound net-console throttle and coalescer enforcing rate limits and batching.
// Author: Lukas Bower

use core::{cmp::min, mem};

use heapless::{Deque, Vec as HeaplessVec};
use log::info;
use portable_atomic::{AtomicBool, Ordering};
use static_assertions::const_assert;

use crate::debug::maybe_report_str_write;
use crate::serial::DEFAULT_LINE_CAPACITY;

pub const MAX_PAYLOAD: usize = 1200;
const MAX_FRAMES_PER_POLL: usize = 2;
const MAX_BYTES_PER_POLL: usize = 1_600;
const LOG_Q_CAP: usize = 64;
const CTRL_Q_CAP: usize = 16;
const LINE_CAP: usize = DEFAULT_LINE_CAPACITY;
const TOTAL_QUEUE_CAP: usize = LOG_Q_CAP + CTRL_Q_CAP;
// The boot logs show the net-storage window is ~0x2d39 bytes (~11.6 KiB); a
// 4 KiB outbound ring plus compact metadata keeps this coalescer well below
// that ceiling and the budget guard prevents regressions back into that arena.
const OUTBOUND_RING_CAP: usize = 4096;
const OUTBOUND_SIZE_BUDGET: usize = 8 * 1024;
const TRUNCATION_SUFFIX: &[u8] = b"...";
const RATE_BPS: u32 = 32_000;
const BURST: u32 = 4_000;
type SlotIdx = u8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboundLane {
    Control,
    Log,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendError {
    WouldBlock,
    Fault,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OutboundStats {
    pub queued_lines: u32,
    pub queued_bytes: u32,
    pub drops: u64,
    pub frames_sent: u64,
    pub bytes_sent: u64,
    pub would_block: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlushOutcome {
    pub sent_frames: u32,
    pub sent_bytes: u32,
    pub would_block: bool,
    pub blocked_for_tokens: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LineSlot {
    offset: u16,
    len: u16,
    in_use: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LineBuf {
    len: u16,
    buf: [u8; LINE_CAP],
}

impl LineBuf {
    fn new() -> Self {
        Self {
            len: 0,
            buf: [0u8; LINE_CAP],
        }
    }

    fn utf8_prefix_len(line: &[u8], max_len: usize) -> usize {
        let cap = min(line.len(), max_len);
        if cap == 0 {
            return 0;
        }
        match core::str::from_utf8(&line[..cap]) {
            Ok(_) => cap,
            Err(err) => err.valid_up_to(),
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.buf[..usize::from(self.len)]
    }

    fn store(&mut self, line: &[u8]) {
        let mut truncated = false;
        let mut copy_len = min(line.len(), MAX_PAYLOAD);
        if copy_len > LINE_CAP {
            copy_len = LINE_CAP;
        }
        if line.len() > copy_len {
            truncated = true;
        }
        let mut head_len = if truncated {
            copy_len.saturating_sub(TRUNCATION_SUFFIX.len())
        } else {
            copy_len
        };
        if truncated {
            head_len = Self::utf8_prefix_len(line, head_len);
        }
        let _ = maybe_report_str_write(
            self.buf.as_mut_ptr(),
            head_len,
            line.as_ptr(),
            line.len(),
            "linebuf.store.head",
        );
        self.buf[..head_len].copy_from_slice(&line[..head_len]);
        let mut written = head_len;
        if truncated && written < LINE_CAP {
            let suffix_len = min(TRUNCATION_SUFFIX.len(), LINE_CAP.saturating_sub(written));
            let _ = maybe_report_str_write(
                self.buf[written..].as_mut_ptr(),
                suffix_len,
                TRUNCATION_SUFFIX.as_ptr(),
                suffix_len,
                "linebuf.store.suffix",
            );
            self.buf[written..written + suffix_len]
                .copy_from_slice(&TRUNCATION_SUFFIX[..suffix_len]);
            written = written.saturating_add(suffix_len);
        }
        self.len = written as u16;
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for LineBuf {
    fn default() -> Self {
        Self::new()
    }
}

pub struct OutboundCoalescer {
    ring: [u8; OUTBOUND_RING_CAP],
    slots: [LineSlot; TOTAL_QUEUE_CAP],
    alloc_order: Deque<SlotIdx, TOTAL_QUEUE_CAP>,
    free_list: Deque<SlotIdx, TOTAL_QUEUE_CAP>,
    log_q: Deque<SlotIdx, LOG_Q_CAP>,
    ctrl_q: Deque<SlotIdx, CTRL_Q_CAP>,
    head: u16,
    tail: u16,
    used: u16,
    tokens: u32,
    last_refill_ms: u64,
    drops: u64,
    frames_sent: u64,
    bytes_sent: u64,
    would_block: u64,
    queued_lines: u32,
    queued_bytes: u32,
}

impl OutboundCoalescer {
    fn rollback_ring(&mut self, prev_head: u16, prev_used: u16) {
        self.head = prev_head;
        self.used = prev_used;
    }

    fn reserve_ring(&mut self, len: usize) -> Option<u16> {
        if len == 0 || len > OUTBOUND_RING_CAP {
            return None;
        }
        if usize::from(self.used).saturating_add(len) > OUTBOUND_RING_CAP {
            return None;
        }
        if self.used == 0 {
            self.head = self.tail;
        }
        let head = self.head as usize;
        let tail = self.tail as usize;
        if head.saturating_add(len) <= OUTBOUND_RING_CAP {
            let new_head = head.saturating_add(len);
            self.head = if new_head == OUTBOUND_RING_CAP {
                0
            } else {
                new_head as u16
            };
            self.used = self.used.saturating_add(len as u16);
            return Some(head as u16);
        }
        if len <= tail {
            self.head = len as u16;
            self.used = self.used.saturating_add(len as u16);
            return Some(0);
        }
        None
    }

    fn release_slot(&mut self, slot_idx: SlotIdx) {
        if let Some(slot) = self.slots.get_mut(slot_idx as usize) {
            slot.in_use = false;
        }
    }

    fn reclaim_ring(&mut self) {
        while let Some(front) = self.alloc_order.front().copied() {
            let slot = &self.slots[front as usize];
            if slot.in_use {
                break;
            }
            let len = slot.len;
            let offset = slot.offset;
            let _ = self.alloc_order.pop_front();
            if len == 0 {
                self.slots[front as usize] = LineSlot::default();
                let _ = self.free_list.push_back(front);
                continue;
            }
            self.used = self.used.saturating_sub(len);
            let next_tail = usize::from(offset).saturating_add(usize::from(len));
            self.tail = if next_tail >= OUTBOUND_RING_CAP {
                0
            } else {
                next_tail as u16
            };
            self.slots[front as usize] = LineSlot::default();
            let _ = self.free_list.push_back(front);
        }
    }

    fn slot_slice(&self, slot_idx: SlotIdx) -> Option<&[u8]> {
        let slot = self.slots.get(slot_idx as usize)?;
        if !slot.in_use || slot.len == 0 {
            return None;
        }
        let start = usize::from(slot.offset);
        let end = start.saturating_add(usize::from(slot.len));
        if end > OUTBOUND_RING_CAP {
            return None;
        }
        Some(&self.ring[start..end])
    }

    fn try_enqueue_line(&mut self, lane: OutboundLane, line: &[u8]) -> Result<(), ()> {
        if line.is_empty() {
            return Ok(());
        }
        let queue_full = match lane {
            OutboundLane::Control => self.ctrl_q.is_full(),
            OutboundLane::Log => self.log_q.is_full(),
        };
        if queue_full {
            return Err(());
        }
        let Some(slot_idx) = self.free_list.pop_front() else {
            return Err(());
        };
        let mut stored = LineBuf::new();
        stored.store(line);
        let len = usize::from(stored.len);
        if len == 0 {
            let _ = self.free_list.push_front(slot_idx);
            return Ok(());
        }
        let prev_head = self.head;
        let prev_used = self.used;
        let Some(offset) = self.reserve_ring(len) else {
            let _ = self.free_list.push_front(slot_idx);
            return Err(());
        };
        let slot = &mut self.slots[slot_idx as usize];
        slot.offset = offset;
        slot.len = stored.len;
        slot.in_use = true;
        let start = usize::from(offset);
        let end = start.saturating_add(len);
        if end > OUTBOUND_RING_CAP {
            *slot = LineSlot::default();
            self.rollback_ring(prev_head, prev_used);
            let _ = self.free_list.push_front(slot_idx);
            return Err(());
        }
        let mut written = 0usize;
        for byte in &stored.buf[..len] {
            self.ring[start + written] = *byte;
            written = written.saturating_add(1);
        }
        let push_result = match lane {
            OutboundLane::Control => self.ctrl_q.push_back(slot_idx),
            OutboundLane::Log => self.log_q.push_back(slot_idx),
        };
        if push_result.is_err() {
            *slot = LineSlot::default();
            self.rollback_ring(prev_head, prev_used);
            let _ = self.free_list.push_front(slot_idx);
            return Err(());
        }
        if self.alloc_order.push_back(slot_idx).is_err() {
            match lane {
                OutboundLane::Control => {
                    let _ = self.ctrl_q.pop_back();
                }
                OutboundLane::Log => {
                    let _ = self.log_q.pop_back();
                }
            }
            *slot = LineSlot::default();
            self.rollback_ring(prev_head, prev_used);
            let _ = self.free_list.push_front(slot_idx);
            return Err(());
        }
        self.queued_lines = self.queued_lines.saturating_add(1);
        self.queued_bytes = self.queued_bytes.saturating_add(slot.len as u32);
        Ok(())
    }

    pub fn log_buffer_addresses_once(&mut self, marker: &'static str) {
        static LOGGED: AtomicBool = AtomicBool::new(false);
        if LOGGED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let ring_ptr = self.ring.as_ptr() as usize;
        info!(
            target: "net-console",
            "[net-console] addr marker={marker} label=outbound-ring ptr=0x{ring_ptr:016x} len=0x{len:04x}",
            len = OUTBOUND_RING_CAP,
        );
        let slot_ptr = self.slots.as_ptr() as usize;
        info!(
            target: "net-console",
            "[net-console] addr marker={marker} label=outbound-slots ptr=0x{slot_ptr:016x} count={} slot_size=0x{slot_size:04x}",
            TOTAL_QUEUE_CAP,
            slot_size = mem::size_of::<LineSlot>(),
        );
    }

    #[must_use]
    pub fn new() -> Self {
        const_assert!(TOTAL_QUEUE_CAP < SlotIdx::MAX as usize);
        const_assert!(OUTBOUND_RING_CAP <= u16::MAX as usize);
        let mut free_list: Deque<SlotIdx, TOTAL_QUEUE_CAP> = Deque::new();
        for idx in 0..TOTAL_QUEUE_CAP {
            let _ = free_list.push_back(idx as SlotIdx);
        }
        Self {
            ring: [0u8; OUTBOUND_RING_CAP],
            slots: [LineSlot::default(); TOTAL_QUEUE_CAP],
            alloc_order: Deque::new(),
            free_list,
            log_q: Deque::new(),
            ctrl_q: Deque::new(),
            head: 0,
            tail: 0,
            used: 0,
            tokens: BURST,
            last_refill_ms: 0,
            drops: 0,
            frames_sent: 0,
            bytes_sent: 0,
            would_block: 0,
            queued_lines: 0,
            queued_bytes: 0,
        }
    }

    pub fn reset(&mut self) {
        self.log_q.clear();
        self.ctrl_q.clear();
        self.alloc_order.clear();
        self.slots = [LineSlot::default(); TOTAL_QUEUE_CAP];
        self.free_list.clear();
        for idx in 0..TOTAL_QUEUE_CAP {
            let _ = self.free_list.push_back(idx as SlotIdx);
        }
        self.tokens = BURST;
        self.last_refill_ms = 0;
        self.queued_lines = 0;
        self.queued_bytes = 0;
        self.drops = 0;
        self.frames_sent = 0;
        self.bytes_sent = 0;
        self.would_block = 0;
        self.head = 0;
        self.tail = 0;
        self.used = 0;
    }

    #[inline]
    pub fn has_pending(&self) -> bool {
        !self.log_q.is_empty() || !self.ctrl_q.is_empty()
    }

    #[inline]
    pub fn stats(&self) -> OutboundStats {
        OutboundStats {
            queued_lines: self.queued_lines,
            queued_bytes: self.queued_bytes,
            drops: self.drops,
            frames_sent: self.frames_sent,
            bytes_sent: self.bytes_sent,
            would_block: self.would_block,
        }
    }

    pub fn enqueue_control(&mut self, line: &[u8]) -> Result<(), ()> {
        self.try_enqueue_line(OutboundLane::Control, line)
    }

    pub fn enqueue_log(&mut self, line: &[u8]) {
        if self.try_enqueue_line(OutboundLane::Log, line).is_err() {
            self.drops = self.drops.saturating_add(1);
        }
    }

    pub fn flush<F>(&mut self, now_ms: u64, mut send: F) -> FlushOutcome
    where
        F: FnMut(&[u8], OutboundLane) -> Result<(), SendError>,
    {
        self.refill(now_ms);
        let mut outcome = FlushOutcome::default();
        let mut bytes_sent_this_poll: usize = 0;

        for _ in 0..MAX_FRAMES_PER_POLL {
            if bytes_sent_this_poll >= MAX_BYTES_PER_POLL {
                break;
            }
            let lane = if self.ctrl_q.is_empty() {
                if self.log_q.is_empty() {
                    break;
                }
                OutboundLane::Log
            } else {
                OutboundLane::Control
            };

            if lane == OutboundLane::Log && self.tokens == 0 {
                outcome.blocked_for_tokens = true;
                break;
            }

            let Some(plan) = self.prepare_payload(lane) else {
                break;
            };
            if lane == OutboundLane::Log
                && u32::try_from(plan.payload_len).unwrap_or(u32::MAX) > self.tokens
            {
                outcome.blocked_for_tokens = true;
                break;
            }
            if bytes_sent_this_poll.saturating_add(plan.payload_len) > MAX_BYTES_PER_POLL {
                break;
            }

            match send(&plan.payload[..plan.payload_len], lane) {
                Ok(()) => {
                    self.commit_payload(&plan, lane);
                    outcome.sent_frames = outcome.sent_frames.saturating_add(1);
                    outcome.sent_bytes = outcome.sent_bytes.saturating_add(plan.payload_len as u32);
                    bytes_sent_this_poll = bytes_sent_this_poll.saturating_add(plan.payload_len);
                }
                Err(SendError::WouldBlock) => {
                    self.would_block = self.would_block.saturating_add(1);
                    outcome.would_block = true;
                    break;
                }
                Err(SendError::Fault) => {
                    break;
                }
            }
        }

        outcome
    }

    fn prepare_payload(&self, lane: OutboundLane) -> Option<PlannedPayload> {
        let mut payload: HeaplessVec<u8, MAX_PAYLOAD> = HeaplessVec::new();
        let mut consumed = 0usize;
        let mut consumed_bytes = 0usize;
        let mut iter = match lane {
            OutboundLane::Control => self.ctrl_q.iter(),
            OutboundLane::Log => self.log_q.iter(),
        };

        while let Some(slot_idx) = iter.next() {
            let Some(line_slice) = self.slot_slice(*slot_idx) else {
                consumed = consumed.saturating_add(1);
                continue;
            };
            let line_len = line_slice.len();
            if line_len == 0 {
                consumed = consumed.saturating_add(1);
                continue;
            }
            let required = line_len.saturating_add(4);
            if payload.len().saturating_add(required) > MAX_PAYLOAD {
                break;
            }
            let total_len: u32 = line_len
                .saturating_add(4)
                .try_into()
                .unwrap_or(u32::MAX);
            if payload.extend_from_slice(&total_len.to_le_bytes()).is_err() {
                break;
            }
            if payload.extend_from_slice(line_slice).is_err() {
                break;
            }
            consumed = consumed.saturating_add(1);
            consumed_bytes = consumed_bytes.saturating_add(line_len);
        }

        if consumed == 0 || payload.is_empty() {
            None
        } else {
            let payload_len = payload.len();
            Some(PlannedPayload {
                payload,
                payload_len,
                consumed_lines: consumed,
                consumed_bytes,
            })
        }
    }

    fn commit_payload(&mut self, plan: &PlannedPayload, lane: OutboundLane) {
        match lane {
            OutboundLane::Control => {
                for _ in 0..plan.consumed_lines {
                    if let Some(slot_idx) = self.ctrl_q.pop_front() {
                        self.release_slot(slot_idx);
                    }
                }
            }
            OutboundLane::Log => {
                for _ in 0..plan.consumed_lines {
                    if let Some(slot_idx) = self.log_q.pop_front() {
                        self.release_slot(slot_idx);
                    }
                }
            }
        }
        self.reclaim_ring();
        self.queued_lines = self.queued_lines.saturating_sub(plan.consumed_lines as u32);
        self.queued_bytes = self.queued_bytes.saturating_sub(plan.consumed_bytes as u32);
        if lane == OutboundLane::Log {
            self.tokens = self.tokens.saturating_sub(plan.payload_len as u32);
        }
        self.frames_sent = self.frames_sent.saturating_add(1);
        self.bytes_sent = self.bytes_sent.saturating_add(plan.payload_len as u64);
    }

    fn refill(&mut self, now_ms: u64) {
        if self.last_refill_ms == 0 {
            self.last_refill_ms = now_ms;
            self.tokens = BURST;
            return;
        }
        let delta = now_ms.saturating_sub(self.last_refill_ms);
        if delta == 0 {
            return;
        }
        let add = (delta.saturating_mul(u64::from(RATE_BPS)) / 1_000) as u32;
        self.tokens = min(BURST, self.tokens.saturating_add(add));
        self.last_refill_ms = now_ms;
    }
}

#[derive(Debug)]
struct PlannedPayload {
    payload: HeaplessVec<u8, MAX_PAYLOAD>,
    payload_len: usize,
    consumed_lines: usize,
    consumed_bytes: usize,
}

const_assert!(mem::size_of::<OutboundCoalescer>() <= OUTBOUND_SIZE_BUDGET);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::{clear_watches, trip_on_overlap, watch_hint_for, watch_range};

    const LOG_LINE: &[u8] = b"line";

    #[test]
    fn linebuf_store_reports_overlap() {
        clear_watches();
        trip_on_overlap(false);
        let mut buf = LineBuf::new();
        let ptr = buf.buf.as_mut_ptr();
        watch_range("linebuf", ptr as *const u8, LINE_CAP);
        let payload = [b'x'; LINE_CAP + 8];
        buf.store(&payload);
        trip_on_overlap(true);
        let hint = watch_hint_for(ptr as usize, LINE_CAP);
        assert!(
            hint.is_some(),
            "watcher should record overlap during line buffer store"
        );
    }

    #[test]
    fn coalesces_multiple_lines() {
        let mut coalescer = OutboundCoalescer::new();
        coalescer.enqueue_log(LOG_LINE);
        coalescer.enqueue_log(LOG_LINE);
        let mut payload_seen: HeaplessVec<HeaplessVec<u8, MAX_PAYLOAD>, 2> = HeaplessVec::new();
        let outcome = coalescer.flush(0, |payload, _lane| {
            let mut buf = HeaplessVec::<u8, MAX_PAYLOAD>::new();
            let _ = buf.extend_from_slice(payload);
            let _ = payload_seen.push(buf);
            Ok(())
        });
        assert_eq!(outcome.sent_frames, 1);
        assert_eq!(payload_seen.len(), 1);
        let expected = [
            8u8, 0, 0, 0, b'l', b'i', b'n', b'e', 8u8, 0, 0, 0, b'l', b'i', b'n', b'e',
        ];
        assert_eq!(payload_seen[0].as_slice(), expected);
    }

    #[test]
    fn truncates_oversized_line() {
        let mut coalescer = OutboundCoalescer::new();
        let mut long_line = [b'a'; MAX_PAYLOAD + 10];
        coalescer.enqueue_log(&long_line);
        let outcome = coalescer.flush(0, |_payload, _lane| Ok(()));
        assert_eq!(outcome.sent_frames, 1);
        let stats = coalescer.stats();
        assert!(stats.bytes_sent <= MAX_PAYLOAD as u64);
    }

    #[test]
    fn truncation_preserves_utf8_boundary() {
        let head_len = LINE_CAP.saturating_sub(TRUNCATION_SUFFIX.len());
        let mut line = [b'a'; LINE_CAP + 8];
        let idx = head_len.saturating_sub(1);
        line[idx] = 0xe2;
        line[idx + 1] = 0x86;
        line[idx + 2] = 0x92;
        let mut buf = LineBuf::new();
        buf.store(&line);
        assert!(core::str::from_utf8(buf.as_slice()).is_ok());
        assert!(buf.as_slice().ends_with(TRUNCATION_SUFFIX));
    }

    #[test]
    fn token_bucket_blocks_logs() {
        let mut coalescer = OutboundCoalescer::new();
        coalescer.tokens = 0;
        coalescer.enqueue_log(LOG_LINE);
        let outcome = coalescer.flush(0, |_payload, _lane| Ok(()));
        assert_eq!(outcome.sent_frames, 0);
        assert!(outcome.blocked_for_tokens);
        assert!(coalescer.has_pending());
    }

    #[test]
    fn control_flushes_before_logs() {
        let mut coalescer = OutboundCoalescer::new();
        coalescer.enqueue_log(LOG_LINE);
        coalescer.enqueue_control(b"control").unwrap();
        let mut lanes: HeaplessVec<OutboundLane, 2> = HeaplessVec::new();
        let _ = coalescer.flush(0, |_, lane| {
            let _ = lanes.push(lane);
            Ok(())
        });
        assert_eq!(lanes.first().copied(), Some(OutboundLane::Control));
    }

    #[test]
    fn stops_on_would_block() {
        let mut coalescer = OutboundCoalescer::new();
        coalescer.enqueue_log(LOG_LINE);
        coalescer.enqueue_log(LOG_LINE);
        let mut first = true;
        let outcome = coalescer.flush(0, |_payload, _lane| {
            if first {
                first = false;
                Err(SendError::WouldBlock)
            } else {
                Ok(())
            }
        });
        assert!(outcome.would_block);
        assert!(coalescer.has_pending());
    }

    #[test]
    fn drops_when_ring_full() {
        let mut coalescer = OutboundCoalescer::new();
        let line = [b'x'; LINE_CAP];
        for _ in 0..LOG_Q_CAP {
            coalescer.enqueue_log(&line);
        }
        let stats = coalescer.stats();
        assert!(usize::from(stats.queued_bytes) <= OUTBOUND_RING_CAP);
        assert!(stats.drops > 0);
    }
}
