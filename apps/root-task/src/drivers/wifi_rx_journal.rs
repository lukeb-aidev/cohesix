// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Author: Lukas Bower
// Purpose: Retain bounded TCP header and receive-timestamp evidence without packet payload or device access.

use core::fmt::Write;
use heapless::String;
use spin::Mutex;

use super::driver_task_net::Cyw43RxDequeueTcpTuple;
use crate::serial::DEFAULT_LINE_CAPACITY;

const CAPACITY: usize = 96;
pub(crate) const PAGE_COUNT: u8 = 6;
const ROWS: usize = CAPACITY / PAGE_COUNT as usize;

/// Timestamps are observations, never scheduling or packet-admission authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RxReceipt {
    pub tuple: Cyw43RxDequeueTcpTuple,
    pub ip_id: u16,
    pub source: Option<u32>,
    pub stages_q11: Option<u32>,
    pub root_copy: Option<u32>,
    pub dequeue: Option<u64>,
}

struct Journal {
    generation: u32,
    flow: Option<Cyw43RxDequeueTcpTuple>,
    syn_sequence: Option<u32>,
    entries: [Option<RxReceipt>; CAPACITY],
    written: u64,
    ignored_flow: u64,
}

impl Journal {
    const fn new() -> Self {
        Self {
            generation: 0,
            flow: None,
            syn_sequence: None,
            entries: [None; CAPACITY],
            written: 0,
            ignored_flow: 0,
        }
    }

    fn record(&mut self, generation: u32, receipt: RxReceipt) {
        let tuple = receipt.tuple;
        if generation == 0 || (tuple.payload_bytes == 0 && tuple.flags & 0x07 == 0) {
            return;
        }
        let same_flow = self.flow.is_some_and(|prior| {
            (
                prior.source_ipv4,
                prior.destination_ipv4,
                prior.source_port,
                prior.destination_port,
            ) == (
                tuple.source_ipv4,
                tuple.destination_ipv4,
                tuple.source_port,
                tuple.destination_port,
            )
        });
        let syn = tuple.flags & 0x02 != 0;
        let new_syn = syn && (!same_flow || self.syn_sequence != Some(tuple.sequence));
        if self.generation != generation || self.flow.is_none() || new_syn {
            *self = Self::new();
            self.generation = generation;
            self.flow = Some(tuple);
            self.syn_sequence = syn.then_some(tuple.sequence);
        } else if !same_flow {
            self.ignored_flow = self.ignored_flow.saturating_add(1);
            return;
        }
        // Stop rather than wrap the absolute receipt identity at u64::MAX.
        if self.written == u64::MAX {
            return;
        }
        self.entries[(self.written % CAPACITY as u64) as usize] = Some(receipt);
        self.written += 1;
    }

    fn page(&self, page: u8) -> Option<[String<DEFAULT_LINE_CAPACITY>; ROWS + 2]> {
        if page >= PAGE_COUNT {
            return None;
        }
        let mut lines = core::array::from_fn(|_| String::new());
        let first = self.written.saturating_sub(CAPACITY as u64);
        let _ = write!(lines[0], "wifi: rx_trace schema=v1 gen={} page={}/{} first={} next={} ignored={} payload=none clock=cntvct",
            self.generation, page, PAGE_COUNT, first, self.written, self.ignored_flow);
        if let Some(flow) = self.flow {
            let _ = write!(
                lines[1],
                "wifi: rx_trace_flow src={:08x}:{} dst={:08x}:{} syn={}",
                flow.source_ipv4,
                flow.source_port,
                flow.destination_ipv4,
                flow.destination_port,
                HexWord(self.syn_sequence.map(u64::from))
            );
        } else {
            let _ = write!(lines[1], "wifi: rx_trace_flow absent=yes");
        }
        for row in 0..ROWS {
            let ordinal = first.checked_add(u64::from(page) * ROWS as u64 + row as u64)?;
            if ordinal >= self.written {
                break;
            }
            let entry = self.entries[(ordinal % CAPACITY as u64) as usize]?;
            let _ = write!(lines[row + 2], "wifi: rx_trace_row n={} ip={:04x} seq={:08x} ack={:08x} f={:03x} len={} s={} q={} r={} d={}",
                ordinal, entry.ip_id, entry.tuple.sequence, entry.tuple.acknowledgment,
                entry.tuple.flags, entry.tuple.payload_bytes,
                HexWord(entry.source.map(u64::from)), HexWord(entry.stages_q11.map(u64::from)),
                HexWord(entry.root_copy.map(u64::from)), HexWord(entry.dequeue));
        }
        Some(lines)
    }
}

struct HexWord(Option<u64>);
impl core::fmt::Display for HexWord {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(value) => write!(f, "{value:x}"),
            None => f.write_str("none"),
        }
    }
}

static JOURNAL: Mutex<Journal> = Mutex::new(Journal::new());

pub(crate) fn parse_page(word: &str) -> Option<u8> {
    match word.as_bytes() {
        [digit @ b'0'..=b'5'] => Some(digit - b'0'),
        _ => None,
    }
}

pub(crate) fn record(generation: u32, receipt: RxReceipt) {
    JOURNAL.lock().record(generation, receipt);
}

pub(crate) fn page(page: u8) -> Option<[String<DEFAULT_LINE_CAPACITY>; ROWS + 2]> {
    JOURNAL.lock().page(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(sequence: u32, flags: u16, payload_bytes: u16) -> RxReceipt {
        RxReceipt {
            tuple: Cyw43RxDequeueTcpTuple {
                source_ipv4: 0xc0000201,
                destination_ipv4: 0xc0000202,
                source_port: 40000,
                destination_port: 31337,
                sequence,
                acknowledgment: 17,
                flags,
                payload_bytes,
            },
            ip_id: 7,
            source: Some(0),
            stages_q11: None,
            root_copy: Some(3),
            dequeue: Some(4),
        }
    }

    #[test]
    fn repeated_syn_and_data_preserve_independent_packet_receipts() {
        let mut journal = Journal::new();
        journal.record(1, receipt(10, 2, 0));
        journal.record(1, receipt(10, 2, 0));
        journal.record(1, receipt(11, 0x18, 5));
        let mut repeated = receipt(11, 0x18, 5);
        repeated.ip_id = 8;
        journal.record(1, repeated);
        journal.record(1, receipt(16, 0x10, 0));
        assert_eq!(journal.written, 4);
        assert_eq!(journal.entries[3], Some(repeated));
        let page = journal.page(0).unwrap();
        assert!(page[2].contains("s=0 q=none r=3 d=4"));
    }

    #[test]
    fn bounded_eviction_flow_and_generation_are_explicit() {
        let mut journal = Journal::new();
        for n in 0..100 {
            journal.record(1, receipt(n, 0x18, 5));
        }
        assert!(journal.page(0).unwrap()[0].contains("first=4 next=100"));
        assert!(journal.page(5).unwrap()[17].contains("n=99 "));
        assert!(journal.page(6).is_none());
        let mut alien = receipt(1, 0x18, 5);
        alien.tuple.source_port += 1;
        journal.record(1, alien);
        assert_eq!((journal.written, journal.ignored_flow), (100, 1));
        alien.tuple.flags = 2;
        alien.tuple.payload_bytes = 0;
        journal.record(1, alien);
        assert_eq!(journal.written, 1);
        journal.record(2, receipt(20, 0x18, 5));
        assert_eq!((journal.generation, journal.written), (2, 1));
        journal.record(0, receipt(21, 0x18, 5));
        assert_eq!((journal.generation, journal.written), (2, 1));
    }

    #[test]
    fn maximum_width_rows_retain_the_final_timestamp() {
        let mut journal = Journal::new();
        let mut entry = receipt(u32::MAX, 0x1ff, u16::MAX);
        entry.tuple.acknowledgment = u32::MAX;
        entry.ip_id = u16::MAX;
        entry.source = Some(u32::MAX);
        entry.stages_q11 = Some(u32::MAX);
        entry.root_copy = Some(u32::MAX);
        entry.dequeue = Some(u64::MAX);
        journal.record(u32::MAX, entry);
        let lines = journal.page(0).unwrap();
        assert!(lines[2].ends_with("d=ffffffffffffffff"));
        assert!(lines[2].len() < DEFAULT_LINE_CAPACITY);
    }

    #[test]
    fn page_argument_is_one_bounded_ascii_digit() {
        assert_eq!(parse_page("0"), Some(0));
        assert_eq!(parse_page("5"), Some(5));
        for invalid in ["", "6", "-1", "+1", "01", "1 ", "١"] {
            assert_eq!(parse_page(invalid), None);
        }
    }
}
