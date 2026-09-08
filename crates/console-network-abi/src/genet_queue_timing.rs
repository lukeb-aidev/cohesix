// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Author: Lukas Bower
// Purpose: Define optional, non-authoritative GENET slot-to-consumer timing records.

//! Observational extensions in otherwise unused direct-GENET page tails.
//! Missing, raced, or invalid observations never affect packet acceptance.

use crate::{DirectGenetDirection, ETHERNET_FRAME_BYTES, SHARED_PAGE_BYTES};

/// Producer-owned stamp in each packet page, outside the Ethernet payload.
pub const GENET_QUEUE_STAMP_OFFSET: usize = 2048;
/// Four little-endian words: magic/version, generation, sequence, CNTVCT.
pub const GENET_QUEUE_STAMP_BYTES: usize = 32;
/// Console-owned RX timing publication in the control page.
pub const GENET_RX_QUEUE_TIMING_OFFSET: usize = 640;
/// GENET-owned TX timing publication in the control page.
pub const GENET_TX_QUEUE_TIMING_OFFSET: usize = 768;
/// Exact diagnostic record size; the last word is its sequence-last commit.
pub const GENET_QUEUE_TIMING_BYTES: usize = 128;
/// Offset of the diagnostic commit word.
pub const GENET_QUEUE_TIMING_COMMIT_OFFSET: usize = 120;
const STAMP_ID: u64 = 0x0001_0000_434e_4751;
const RECORD_ID: u64 = 0x0080_0001_434e_4754;

const _: () = assert!(
    crate::DIRECT_GENET_SLOT_PAYLOAD_OFFSET + ETHERNET_FRAME_BYTES <= GENET_QUEUE_STAMP_OFFSET
);
const _: () = assert!(GENET_QUEUE_STAMP_OFFSET + GENET_QUEUE_STAMP_BYTES <= SHARED_PAGE_BYTES);
const _: () = assert!(
    crate::DIRECT_GENET_RUNTIME_DIAGNOSTIC_OFFSET + crate::DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES
        <= GENET_RX_QUEUE_TIMING_OFFSET
);
const _: () = assert!(
    GENET_RX_QUEUE_TIMING_OFFSET + GENET_QUEUE_TIMING_BYTES == GENET_TX_QUEUE_TIMING_OFFSET
);
const _: () = assert!(GENET_TX_QUEUE_TIMING_OFFSET + GENET_QUEUE_TIMING_BYTES <= SHARED_PAGE_BYTES);

/// Encode a producer stamp before publishing the corresponding packet slot.
#[must_use]
pub fn genet_queue_stamp(generation: u64, sequence: u64, ticks: u64) -> [u8; 32] {
    let mut bytes = [0; 32];
    for (index, value) in [STAMP_ID, generation, sequence, ticks]
        .into_iter()
        .enumerate()
    {
        bytes[index * 8..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn word(bytes: &[u8], index: usize) -> u64 {
    let offset = index * 8;
    u64::from_le_bytes(core::array::from_fn(|i| bytes[offset + i]))
}

/// Peak and accumulated stamped-slot-to-stable-copy time for one TCP flow.
///
/// Only IPv4 TCP frames containing data are counted. TCP headers identify the
/// peak; no payload is retained. Time includes producer commit/reconciliation,
/// scheduling, notification handling and the consumer's bounded frame copy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GenetQueueTiming {
    /// Current direct-link generation.
    pub generation: u64,
    /// Monotonic diagnostic publication sequence, independent of flow resets.
    pub publication: u64,
    /// Valid data-bearing TCP observations for the current directional flow.
    pub samples: u64,
    /// Saturating sum of observation intervals in CNTVCT ticks.
    pub total_ticks: u64,
    /// Producer stamp for the longest observed interval.
    pub produced_ticks: u64,
    /// Consumer stable-copy timestamp for that interval.
    pub copied_ticks: u64,
    /// Exact packet-ring sequence of the peak.
    pub ring_sequence: u64,
    /// IPv4 source address in network-order numeric representation.
    pub source_ipv4: u32,
    /// IPv4 destination address in network-order numeric representation.
    pub destination_ipv4: u32,
    /// TCP sequence number of the peak.
    pub tcp_sequence: u32,
    /// TCP acknowledgement number of the peak.
    pub tcp_ack: u32,
    /// Source TCP port of the current flow.
    pub source_port: u16,
    /// Destination TCP port of the current flow.
    pub destination_port: u16,
    /// TCP flags of the peak frame.
    pub tcp_flags: u16,
    /// Complete Ethernet length of the peak frame.
    pub frame_len: u16,
}

impl GenetQueueTiming {
    /// Unpublished observation used by statically allocated driver state.
    pub const EMPTY: Self = Self {
        generation: 0,
        publication: 0,
        samples: 0,
        total_ticks: 0,
        produced_ticks: 0,
        copied_ticks: 0,
        ring_sequence: 0,
        source_ipv4: 0,
        destination_ipv4: 0,
        tcp_sequence: 0,
        tcp_ack: 0,
        source_port: 0,
        destination_port: 0,
        tcp_flags: 0,
        frame_len: 0,
    };

    /// Sole consumer's cache-line-aligned publication region.
    #[must_use]
    pub const fn offset(direction: DirectGenetDirection) -> usize {
        match direction {
            DirectGenetDirection::Rx => GENET_RX_QUEUE_TIMING_OFFSET,
            DirectGenetDirection::Tx => GENET_TX_QUEUE_TIMING_OFFSET,
        }
    }

    /// Observe an already validated, privately copied frame. A bad observation
    /// returns false without changing either diagnostics or transport state.
    pub fn observe(
        &mut self,
        stamp: &[u8; 32],
        generation: u64,
        sequence: u64,
        copied: u64,
        frame: &[u8],
    ) -> bool {
        if generation == 0
            || sequence == 0
            || word(stamp, 0) != STAMP_ID
            || word(stamp, 1) != generation
            || word(stamp, 2) != sequence
            || word(stamp, 3) == 0
            || copied < word(stamp, 3)
            || frame.len() < 54
            || frame.len() > ETHERNET_FRAME_BYTES
            || frame[12..14] != [0x08, 0x00]
            || frame[14] >> 4 != 4
            || frame[23] != 6
        {
            return false;
        }
        let ip_len = usize::from(frame[14] & 15) * 4;
        let ip_total = usize::from(u16::from_be_bytes([frame[16], frame[17]]));
        let tcp = 14 + ip_len;
        if ip_len < 20
            || tcp + 20 > frame.len()
            || ip_total + 14 > frame.len()
            || u16::from_be_bytes([frame[20], frame[21]]) & 0x3fff != 0
        {
            return false;
        }
        let tcp_len = usize::from(frame[tcp + 12] >> 4) * 4;
        if tcp_len < 20 || ip_total <= ip_len + tcp_len {
            return false;
        }
        let src = u32::from_be_bytes(core::array::from_fn(|i| frame[26 + i]));
        let dst = u32::from_be_bytes(core::array::from_fn(|i| frame[30 + i]));
        let sport = u16::from_be_bytes([frame[tcp], frame[tcp + 1]]);
        let dport = u16::from_be_bytes([frame[tcp + 2], frame[tcp + 3]]);
        let Some(publication) = self.publication.checked_add(1) else {
            return false;
        };
        let new_flow = self.generation != generation
            || self.source_ipv4 != src
            || self.destination_ipv4 != dst
            || self.source_port != sport
            || self.destination_port != dport;
        if new_flow {
            *self = Self {
                generation,
                publication: self.publication,
                source_ipv4: src,
                destination_ipv4: dst,
                source_port: sport,
                destination_port: dport,
                ..Self::default()
            };
        }
        let produced = word(stamp, 3);
        let elapsed = copied - produced;
        if self.samples == 0 || elapsed > self.copied_ticks - self.produced_ticks {
            self.produced_ticks = produced;
            self.copied_ticks = copied;
            self.ring_sequence = sequence;
            self.tcp_sequence = u32::from_be_bytes(core::array::from_fn(|i| frame[tcp + 4 + i]));
            self.tcp_ack = u32::from_be_bytes(core::array::from_fn(|i| frame[tcp + 8 + i]));
            self.tcp_flags = u16::from(frame[tcp + 13]) | (u16::from(frame[tcp + 12] & 1) << 8);
            self.frame_len = frame.len() as u16;
        }
        self.publication = publication;
        self.samples = self.samples.saturating_add(1);
        self.total_ticks = self.total_ticks.saturating_add(elapsed);
        true
    }

    /// Serialize the exact fixed record; callers publish word15 last.
    #[must_use]
    pub fn encode(self) -> [u8; GENET_QUEUE_TIMING_BYTES] {
        let words = [
            RECORD_ID,
            self.generation,
            self.publication,
            self.samples,
            self.total_ticks,
            self.produced_ticks,
            self.copied_ticks,
            self.ring_sequence,
            u64::from(self.source_ipv4) | (u64::from(self.destination_ipv4) << 32),
            u64::from(self.tcp_sequence) | (u64::from(self.tcp_ack) << 32),
            u64::from(self.source_port)
                | (u64::from(self.destination_port) << 16)
                | (u64::from(self.tcp_flags) << 32)
                | (u64::from(self.frame_len) << 48),
            0,
            0,
            0,
            0,
            self.publication,
        ];
        let mut bytes = [0; GENET_QUEUE_TIMING_BYTES];
        for (i, value) in words.into_iter().enumerate() {
            bytes[i * 8..i * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// Decode a stable generation-bound observation; never grant packet credit.
    #[must_use]
    pub fn decode(bytes: &[u8; GENET_QUEUE_TIMING_BYTES], generation: u64) -> Option<Self> {
        if generation == 0
            || word(bytes, 0) != RECORD_ID
            || word(bytes, 1) != generation
            || word(bytes, 2) == 0
            || word(bytes, 15) != word(bytes, 2)
            || word(bytes, 3) == 0
            || word(bytes, 3) > word(bytes, 2)
            || word(bytes, 5) == 0
            || word(bytes, 6) < word(bytes, 5)
            || word(bytes, 7) == 0
            || (11..15).any(|i| word(bytes, i) != 0)
            || word(bytes, 4) < word(bytes, 6) - word(bytes, 5)
        {
            return None;
        }
        let ports = word(bytes, 10);
        let frame_len = (ports >> 48) as u16;
        if !(55..=ETHERNET_FRAME_BYTES as u16).contains(&frame_len)
            || ((ports >> 32) as u16) & !0x1ff != 0
        {
            return None;
        }
        Some(Self {
            generation,
            publication: word(bytes, 2),
            samples: word(bytes, 3),
            total_ticks: word(bytes, 4),
            produced_ticks: word(bytes, 5),
            copied_ticks: word(bytes, 6),
            ring_sequence: word(bytes, 7),
            source_ipv4: word(bytes, 8) as u32,
            destination_ipv4: (word(bytes, 8) >> 32) as u32,
            tcp_sequence: word(bytes, 9) as u32,
            tcp_ack: (word(bytes, 9) >> 32) as u32,
            source_port: ports as u16,
            destination_port: (ports >> 16) as u16,
            tcp_flags: (ports >> 32) as u16,
            frame_len,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independently specified Ethernet/IPv4/TCP frame with one data byte.
    fn frame() -> [u8; 55] {
        let mut f = [0; 55];
        f[12..14].copy_from_slice(&[8, 0]);
        f[14] = 0x45;
        f[16..18].copy_from_slice(&41u16.to_be_bytes());
        f[23] = 6;
        f[26..34].copy_from_slice(&[192, 168, 10, 1, 192, 168, 10, 50]);
        f[34..38].copy_from_slice(&[0xd4, 0x31, 0x7a, 0x69]);
        f[38..46].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        f[46] = 0x50;
        f[47] = 0x18;
        f[54] = 0x99;
        f
    }

    fn stamp() -> [u8; 32] {
        [
            0x51, 0x47, 0x4e, 0x43, 0, 0, 1, 0, 7, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0,
            100, 0, 0, 0, 0, 0, 0, 0,
        ]
    }

    #[test]
    fn queue_timing_wire_fixture_and_reserved_regions() {
        assert_eq!(genet_queue_stamp(7, 9, 100), stamp());
        assert_eq!(
            (GENET_QUEUE_STAMP_OFFSET, GENET_QUEUE_STAMP_BYTES),
            (2048, 32)
        );
        assert_eq!(
            (GENET_RX_QUEUE_TIMING_OFFSET, GENET_TX_QUEUE_TIMING_OFFSET),
            (640, 768)
        );
        let mut timing = GenetQueueTiming::EMPTY;
        assert!(timing.observe(&stamp(), 7, 9, 140, &frame()));
        let expected = [
            0x0080_0001_434e_4754,
            7,
            1,
            1,
            40,
            100,
            140,
            9,
            0xc0a8_0a32_c0a8_0a01,
            0x0506_0708_0102_0304,
            0x0037_0018_7a69_d431,
            0,
            0,
            0,
            0,
            1u64,
        ];
        let mut bytes = [0; 128];
        for (i, w) in expected.into_iter().enumerate() {
            bytes[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
        }
        assert_eq!(timing.encode(), bytes);
        assert_eq!(GenetQueueTiming::decode(&bytes, 7), Some(timing));
        assert_eq!(GenetQueueTiming::decode(&bytes, 8), None);
        for index in [0, 8, 16, 24, 40, 56, 88, 96, 104, 112, 120] {
            let mut invalid = bytes;
            if index == 56 {
                invalid[56..64].fill(0);
            } else {
                invalid[index] ^= 0x80;
            }
            assert_eq!(
                GenetQueueTiming::decode(&invalid, 7),
                None,
                "offset {index}"
            );
        }
    }

    #[test]
    fn queue_timing_invalid_observations_cannot_mutate_state() {
        let mut timing = GenetQueueTiming::EMPTY;
        assert!(timing.observe(&stamp(), 7, 9, 140, &frame()));
        let before = timing;
        for (g, s, t) in [(0, 9, 140), (8, 9, 140), (7, 10, 140), (7, 9, 99)] {
            assert!(!timing.observe(&stamp(), g, s, t, &frame()));
            assert_eq!(timing, before);
        }
        for len in 0..55 {
            assert!(!timing.observe(&stamp(), 7, 9, 140, &frame()[..len]));
        }
        for (index, value) in [
            (12, 0),
            (14, 0x65),
            (14, 0x44),
            (17, 40),
            (17, 255),
            (20, 0x20),
            (21, 1),
            (23, 17),
            (46, 0x40),
            (46, 0xf0),
        ] {
            let mut invalid = frame();
            invalid[index] = value;
            assert!(!timing.observe(&stamp(), 7, 9, 140, &invalid));
            assert_eq!(timing, before);
        }
        assert!(!timing.observe(&[0; 32], 7, 9, 140, &frame()));
        assert!(!timing.observe(&genet_queue_stamp(7, 9, 0), 7, 9, 140, &frame()));
        assert_eq!(timing, before);
    }

    #[test]
    fn queue_timing_peak_flow_reset_and_exhaustion_are_bounded() {
        let mut timing = GenetQueueTiming::EMPTY;
        assert!(timing.observe(&stamp(), 7, 9, 140, &frame()));
        assert!(timing.observe(&genet_queue_stamp(7, 10, 200), 7, 10, 220, &frame()));
        assert_eq!(
            (timing.samples, timing.total_ticks, timing.ring_sequence),
            (2, 60, 9)
        );
        assert!(timing.observe(&genet_queue_stamp(7, 11, 300), 7, 11, 350, &frame()));
        assert_eq!(
            (timing.samples, timing.total_ticks, timing.ring_sequence),
            (3, 110, 11)
        );
        let mut different = frame();
        different[35] += 1;
        assert!(timing.observe(&genet_queue_stamp(7, 12, 400), 7, 12, 400, &different));
        assert_eq!(
            (
                timing.publication,
                timing.samples,
                timing.total_ticks,
                timing.ring_sequence
            ),
            (4, 1, 0, 12)
        );
        timing.total_ticks = u64::MAX - 1;
        assert!(timing.observe(&genet_queue_stamp(7, 13, 500), 7, 13, 510, &different));
        assert_eq!(timing.total_ticks, u64::MAX);
        timing.publication = u64::MAX;
        let exhausted = timing;
        assert!(!timing.observe(&genet_queue_stamp(7, 14, 600), 7, 14, 610, &different));
        assert_eq!(timing, exhausted);
    }
}
