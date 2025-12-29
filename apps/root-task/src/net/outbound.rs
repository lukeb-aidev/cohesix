// Author: Lukas Bower
// Purpose: Outbound net-console throttle and coalescer enforcing rate limits and batching.

use core::cmp::min;

use heapless::{Deque, Vec as HeaplessVec};

use crate::debug::maybe_report_str_write;
use crate::serial::DEFAULT_LINE_CAPACITY;

pub const MAX_PAYLOAD: usize = 1200;
const MAX_FRAMES_PER_POLL: usize = 2;
const MAX_BYTES_PER_POLL: usize = 1_600;
const LOG_Q_CAP: usize = 64;
const CTRL_Q_CAP: usize = 16;
const LINE_CAP: usize = DEFAULT_LINE_CAPACITY;
const TRUNCATION_SUFFIX: &[u8] = b"...";
const RATE_BPS: u32 = 32_000;
const BURST: u32 = 4_000;

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
        let head_len = if truncated {
            copy_len.saturating_sub(TRUNCATION_SUFFIX.len())
        } else {
            copy_len
        };
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
    log_q: Deque<LineBuf, LOG_Q_CAP>,
    ctrl_q: Deque<LineBuf, CTRL_Q_CAP>,
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
    #[must_use]
    pub fn new() -> Self {
        Self {
            log_q: Deque::new(),
            ctrl_q: Deque::new(),
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
        self.tokens = BURST;
        self.last_refill_ms = 0;
        self.queued_lines = 0;
        self.queued_bytes = 0;
        self.drops = 0;
        self.frames_sent = 0;
        self.bytes_sent = 0;
        self.would_block = 0;
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
        if line.is_empty() {
            return Ok(());
        }
        if self.ctrl_q.is_full() {
            return Err(());
        }
        let mut stored = LineBuf::new();
        stored.store(line);
        self.ctrl_q.push_back(stored).map_err(|_| ())?;
        self.queued_lines = self.queued_lines.saturating_add(1);
        self.queued_bytes = self.queued_bytes.saturating_add(stored.len as u32);
        Ok(())
    }

    pub fn enqueue_log(&mut self, line: &[u8]) {
        if line.is_empty() {
            return;
        }
        if self.log_q.is_full() {
            self.drops = self.drops.saturating_add(1);
            return;
        }
        let mut stored = LineBuf::new();
        stored.store(line);
        if stored.is_empty() {
            return;
        }
        if self.log_q.push_back(stored).is_ok() {
            self.queued_lines = self.queued_lines.saturating_add(1);
            self.queued_bytes = self.queued_bytes.saturating_add(stored.len as u32);
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

        while let Some(line) = iter.next() {
            let line_slice = line.as_slice();
            let line_len = line_slice.len();
            if line_len == 0 {
                consumed = consumed.saturating_add(1);
                continue;
            }
            let required = if payload.is_empty() {
                line_len
            } else {
                line_len.saturating_add(1)
            };
            if payload.len().saturating_add(required) > MAX_PAYLOAD {
                break;
            }
            if !payload.is_empty() {
                if payload.push(b'\n').is_err() {
                    break;
                }
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
            OutboundLane::Control => Self::pop_front_batch(&mut self.ctrl_q, plan.consumed_lines),
            OutboundLane::Log => Self::pop_front_batch(&mut self.log_q, plan.consumed_lines),
        }
        self.queued_lines = self.queued_lines.saturating_sub(plan.consumed_lines as u32);
        self.queued_bytes = self.queued_bytes.saturating_sub(plan.consumed_bytes as u32);
        if lane == OutboundLane::Log {
            self.tokens = self.tokens.saturating_sub(plan.payload_len as u32);
        }
        self.frames_sent = self.frames_sent.saturating_add(1);
        self.bytes_sent = self.bytes_sent.saturating_add(plan.payload_len as u64);
    }

    fn pop_front_batch<const N: usize>(queue: &mut Deque<LineBuf, N>, count: usize) {
        for _ in 0..count {
            let _ = queue.pop_front();
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::{clear_watches, watch_hint_for, watch_range};

    const LOG_LINE: &[u8] = b"line";

    #[test]
    fn linebuf_store_reports_overlap() {
        clear_watches();
        let mut buf = LineBuf::new();
        let ptr = buf.buf.as_mut_ptr();
        watch_range("linebuf", ptr as *const u8, LINE_CAP);
        let payload = [b'x'; LINE_CAP + 8];
        buf.store(&payload);
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
        assert_eq!(payload_seen[0].as_slice(), b"line\nline");
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
}
