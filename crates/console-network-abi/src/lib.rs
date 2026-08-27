// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define the pointer-free fixed-page ABI for the isolated console-network service.
// Author: Lukas Bower

#![no_std]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! Fixed-layout `console-network-service/v5` records.
//!
//! The root remains the sole owner of operator policy and command execution.
//! The child owns Ethernet/IP/TCP processing, transport authentication, and
//! framing. Four single-producer/single-consumer pages carry one durable packet,
//! control request, or event at a time. A producer stages a complete body and
//! writes `committed_sequence` last before signaling the peer. The consumer
//! validates the generation and identical sequence fields before use. Page
//! tails are construction-zeroed and containment-scrubbed. The sealed
//! descriptor requires completion watermarks: the final 16 bytes of the
//! child-owned event page carry exact ingress/control consumption sequences;
//! all other tail bytes remain non-authoritative padding.

use core::mem::{align_of, size_of};
use core::sync::atomic::{fence, Ordering};

/// Runtime-init magic (`CNI1`).
pub const RUNTIME_INIT_MAGIC: u32 = 0x434e_4931;
/// Ethernet page magic (`CNP1`).
pub const PACKET_PAGE_MAGIC: u32 = 0x434e_5031;
/// Console exchange page magic (`CNE1`).
pub const EXCHANGE_PAGE_MAGIC: u32 = 0x434e_4531;
/// ABI version.
pub const ABI_VERSION: u16 = 5;
/// Exact child `Ready` payload for this ABI generation.
pub const CONSOLE_NETWORK_SERVICE_IDENTITY: &[u8] = b"console-network-service/v5";
/// Shared page size and alignment.
pub const SHARED_PAGE_BYTES: usize = 4096;
/// Maximum copied Ethernet frame, including VLAN headroom.
pub const ETHERNET_FRAME_BYTES: usize = 1536;
/// Maximum framed console payload crossing the service boundary.
///
/// The command side must carry one complete compiler-bounded host-ticket
/// record plus the existing console verb and path. Ordinary response lines
/// retain the smaller [`CONSOLE_OUTPUT_BYTES`] bound.
pub const CONSOLE_PAYLOAD_BYTES: usize = 2368;
/// Maximum root-authorized response line emitted by the child.
pub const CONSOLE_OUTPUT_BYTES: usize = 512;
/// Binary root-to-child response-batch encoding version.
pub const SEND_BATCH_ENCODING_VERSION: u16 = 1;
/// Exact bytes in the response-batch header.
pub const SEND_BATCH_HEADER_BYTES: usize = 8;
/// Exact bytes in each response-batch record header.
pub const SEND_BATCH_RECORD_HEADER_BYTES: usize = 2;
/// Maximum response records authorized by one root control.
pub const SEND_BATCH_MAX_RECORDS: usize = 8;
/// Maximum UTF-8 bytes in one batched root response line.
pub const SEND_BATCH_LINE_BYTES: usize = 256;
/// Binary child-to-root command-batch encoding version.
pub const COMMAND_BATCH_ENCODING_VERSION: u16 = 1;
/// Exact bytes in the command-batch header.
pub const COMMAND_BATCH_HEADER_BYTES: usize = 8;
/// Exact bytes in each command-batch record header (`now_ms`, `command_len`).
pub const COMMAND_BATCH_RECORD_HEADER_BYTES: usize = 10;
/// Maximum authenticated commands carried by one child publication.
pub const COMMAND_BATCH_MAX_RECORDS: usize = 8;
/// Maximum authenticated command bytes admitted to root's parser.
pub const COMMAND_LINE_BYTES: usize = 2304;
/// Maximum authentication token bytes passed only to the restricted child.
pub const AUTH_TOKEN_BYTES: usize = 64;
/// Exact serialized runtime-init descriptor bytes.
pub const RUNTIME_INIT_DESCRIPTOR_BYTES: usize = 224;
/// Byte offset of the optional QEMU direct-VirtIO layout in the read-only init page.
pub const DIRECT_VIRTIO_LAYOUT_OFFSET: usize = 512;
/// Direct-VirtIO layout magic (`CNV1`).
pub const DIRECT_VIRTIO_LAYOUT_MAGIC: u32 = 0x434e_5631;
/// Direct-VirtIO layout version.
pub const DIRECT_VIRTIO_LAYOUT_VERSION: u16 = 1;
/// Exact encoded direct-VirtIO layout bytes.
pub const DIRECT_VIRTIO_LAYOUT_BYTES: usize = 344;
/// Fixed VirtIO queue count (RX and TX).
pub const DIRECT_VIRTIO_QUEUE_COUNT: usize = 2;
/// Fixed descriptor count in each direct VirtIO queue.
pub const DIRECT_VIRTIO_QUEUE_SIZE: usize = 16;
/// Fixed RX and TX DMA-buffer count.
pub const DIRECT_VIRTIO_BUFFER_COUNT: usize = 16;
/// Exact page bytes for every direct VirtIO MMIO, queue, and DMA mapping.
pub const DIRECT_VIRTIO_PAGE_BYTES: usize = 4096;
/// Byte offset of the optional Pi GENET direct-link layout in the read-only init page.
pub const DIRECT_GENET_LAYOUT_OFFSET: usize = 1024;
/// Pi GENET direct-link layout magic (`CNG1`).
pub const DIRECT_GENET_LAYOUT_MAGIC: u32 = 0x434e_4731;
/// Pi GENET direct-link layout version.
pub const DIRECT_GENET_LAYOUT_VERSION: u16 = 1;
/// Exact encoded Pi GENET direct-link layout bytes.
pub const DIRECT_GENET_LAYOUT_BYTES: usize = 296;
/// Direct-link layout flag: every shared page is CPU-only and never DMA-visible.
pub const DIRECT_GENET_LAYOUT_FLAG_CPU_ONLY: u32 = 1 << 0;
/// Direct-link layout flag: the pages are reused only after bootstrap releases them.
pub const DIRECT_GENET_LAYOUT_FLAG_POST_BOOTSTRAP_REUSE: u32 = 1 << 1;
/// Exact required Pi GENET direct-link layout flags.
pub const DIRECT_GENET_LAYOUT_FLAGS: u32 =
    DIRECT_GENET_LAYOUT_FLAG_CPU_ONLY | DIRECT_GENET_LAYOUT_FLAG_POST_BOOTSTRAP_REUSE;
/// Exact CPU-only page population shared by the GENET and console-network children.
pub const DIRECT_GENET_SHARED_PAGE_COUNT: usize = 32;
/// Shared-page index of the direct-link control page.
pub const DIRECT_GENET_CONTROL_PAGE_INDEX: usize = 0;
/// First shared-page index in the GENET-to-console RX ring.
pub const DIRECT_GENET_RX_FIRST_PAGE_INDEX: usize = 1;
/// Exact GENET-to-console RX slot count.
pub const DIRECT_GENET_RX_SLOT_COUNT: usize = 15;
/// First shared-page index in the console-to-GENET TX ring.
pub const DIRECT_GENET_TX_FIRST_PAGE_INDEX: usize = 16;
/// Exact console-to-GENET TX slot count.
pub const DIRECT_GENET_TX_SLOT_COUNT: usize = 16;
/// Direct-GENET peer-notification cap slot, mutually exclusive with direct VirtIO.
pub const DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT: u32 = 7;
/// Fixed child CSpace cardinality.
pub const CHILD_CSPACE_SLOTS: u32 = 16;
/// Child wait cap for the root-to-child wake notification.
pub const CHILD_WAKE_NOTIFICATION_SLOT: u32 = 2;
/// Child signal cap for packet-TX wakes to the supervisor.
pub const PACKET_TX_WAKE_NOTIFICATION_SLOT: u32 = 3;
/// Child signal cap for console-event wakes to the supervisor.
pub const SUPERVISOR_WAKE_NOTIFICATION_SLOT: u32 = 4;
/// Reserved child slot corresponding to the root-held fault identity.
///
/// The child receives no endpoint capability here; seL4 installs the
/// root-held fault and timeout-fault caps directly on its TCB.
pub const FAULT_ENDPOINT_SLOT: u32 = 5;
/// Direct-QEMU VirtIO IRQHandler cap slot; empty for non-direct transports.
pub const DIRECT_VIRTIO_IRQ_HANDLER_SLOT: u32 = 7;

/// Root has published one packet in the RX page.
pub const WAKE_PACKET_RX: u64 = 1;
/// Root has published one control or requests one bounded service tick.
pub const WAKE_CONTROL: u64 = 2;
/// Root requests bounded shutdown after queued output drains.
pub const WAKE_SHUTDOWN: u64 = 4;
/// Root revokes this generation immediately.
pub const WAKE_REVOKE: u64 = 8;
/// Child has published one packet in the TX page.
pub const WAKE_PACKET_TX_READY: u64 = 16;
/// Child has published one event in the event page.
pub const WAKE_EVENT_READY: u64 = 32;
/// Root has accepted the preceding child publication and grants one new slot credit.
pub const WAKE_PUBLICATION_ACK: u64 = 64;
/// QEMU VirtIO queue/config interrupt delivered directly to the child.
pub const WAKE_DIRECT_VIRTIO_IRQ: u64 = 128;
/// Pi GENET direct-link state changed; durable cursors remain authoritative.
pub const WAKE_DIRECT_GENET_LINK: u64 = 256;
/// All allowed root-to-child notification bits.
pub const ROOT_WAKE_MASK: u64 =
    WAKE_PACKET_RX | WAKE_CONTROL | WAKE_SHUTDOWN | WAKE_REVOKE | WAKE_PUBLICATION_ACK;
/// All allowed child-to-root notification bits.
pub const CHILD_WAKE_MASK: u64 = WAKE_PACKET_TX_READY | WAKE_EVENT_READY;

/// Init flags declare a copied, pointer-free, single-listener service.
pub const INIT_FLAG_POINTER_FREE: u32 = 1 << 0;
/// Shared records use sequence-last durable publication.
pub const INIT_FLAG_SEQUENCE_LAST: u32 = 1 << 1;
/// The child accepts exactly one TCP listener.
pub const INIT_FLAG_SINGLE_LISTENER: u32 = 1 << 2;
/// The child owns transport authentication and framing.
pub const INIT_FLAG_CHILD_AUTH_FRAMING: u32 = 1 << 3;
/// Root observation explicitly acknowledges one child publication slot.
pub const INIT_FLAG_PUBLICATION_ACK: u32 = 1 << 4;
/// Child reports consumed root inputs through exact event-page watermarks.
pub const INIT_FLAG_COMPLETION_WATERMARKS: u32 = 1 << 5;
/// The QEMU child owns its admitted VirtIO MMIO and DMA data path directly.
pub const INIT_FLAG_DIRECT_VIRTIO: u32 = 1 << 6;
/// The Pi child exchanges CPU-only packet slots directly with the isolated GENET owner.
pub const INIT_FLAG_DIRECT_GENET: u32 = 1 << 7;
/// Exact required runtime flags.
pub const REQUIRED_INIT_FLAGS: u32 = INIT_FLAG_POINTER_FREE
    | INIT_FLAG_SEQUENCE_LAST
    | INIT_FLAG_SINGLE_LISTENER
    | INIT_FLAG_CHILD_AUTH_FRAMING
    | INIT_FLAG_PUBLICATION_ACK
    | INIT_FLAG_COMPLETION_WATERMARKS;
/// Every recognized runtime flag.
pub const ALLOWED_INIT_FLAGS: u32 =
    REQUIRED_INIT_FLAGS | INIT_FLAG_DIRECT_VIRTIO | INIT_FLAG_DIRECT_GENET;

const PACKET_RESERVED_BYTES: usize = SHARED_PAGE_BYTES - 40 - ETHERNET_FRAME_BYTES;
const EXCHANGE_RESERVED_BYTES: usize = SHARED_PAGE_BYTES - 64 - CONSOLE_PAYLOAD_BYTES;
/// Byte offset of the packet page's sequence-last commit word.
pub const PACKET_COMMIT_OFFSET: usize = 24;
/// Exact bytes occupied by the packet page header.
pub const PACKET_HEADER_BYTES: usize = 40;
/// Byte offset of the packet page's active-length field.
pub const PACKET_LENGTH_OFFSET: usize = 32;
/// Byte offset of the packet payload within its shared page.
pub const PACKET_PAYLOAD_OFFSET: usize = PACKET_HEADER_BYTES;
/// Byte offset of the exchange page's sequence-last commit word.
pub const EXCHANGE_COMMIT_OFFSET: usize = 56;
/// Exact bytes occupied by the exchange page header.
pub const EXCHANGE_HEADER_BYTES: usize = 64;
/// Byte offset of the exchange page's active-length field.
pub const EXCHANGE_LENGTH_OFFSET: usize = 10;
/// Byte offset of the exchange payload within its shared page.
pub const EXCHANGE_PAYLOAD_OFFSET: usize = EXCHANGE_HEADER_BYTES;
/// Event-page offset of the child-published ingress-consumption watermark.
pub const INGRESS_CONSUMED_SEQUENCE_OFFSET: usize = SHARED_PAGE_BYTES - 16;
/// Event-page offset of the child-published control-consumption watermark.
pub const CONTROL_CONSUMED_SEQUENCE_OFFSET: usize = SHARED_PAGE_BYTES - 8;
const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// ABI validation failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbiError {
    /// A record magic or ABI version is wrong.
    InvalidIdentity,
    /// A fixed record length, alignment, or mapping is wrong.
    InvalidLayout,
    /// Flags, badges, or cap slots do not match the contract.
    InvalidAuthority,
    /// A required field is zero or outside its deterministic bound.
    InvalidBound,
    /// A sequence is zero, stale, or not committed sequence-last.
    InvalidSequence,
    /// A record belongs to another child generation.
    StaleGeneration,
    /// A runtime-init seal does not bind the complete descriptor.
    InvalidSeal,
    /// The record kind is not legal for its direction.
    InvalidKind,
}

/// Stable child-published consumption state from the event-page trailer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompletionWatermarks {
    /// Newest exact ingress packet sequence consumed by the child.
    pub ingress_sequence: u64,
    /// Newest exact root-control sequence consumed by the child.
    pub control_sequence: u64,
}

/// Direction of an Ethernet packet page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum PacketDirection {
    /// Admitted driver or virtual NIC to the child.
    Ingress = 1,
    /// Child to the admitted driver or virtual NIC.
    Egress = 2,
}

impl PacketDirection {
    /// Decode a raw direction.
    pub const fn from_raw(value: u16) -> Result<Self, AbiError> {
        match value {
            1 => Ok(Self::Ingress),
            2 => Ok(Self::Egress),
            _ => Err(AbiError::InvalidKind),
        }
    }
}

/// Root-to-child and child-to-root console record kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ExchangeKind {
    /// Root queues an already-authorized console response line.
    SendLine = 1,
    /// Root asks the active connection to close after output drain.
    Disconnect = 2,
    /// Root queues up to eight already-authorized response lines atomically.
    SendBatch = 3,
    /// Child reports TCP accept before authentication.
    Connected = 16,
    /// Child reports successful transport authentication.
    Authenticated = 17,
    /// Child provides one authenticated command line for root policy.
    Command = 18,
    /// Child reports a terminal connection close.
    Disconnected = 19,
    /// Child reports bounded queue pressure; no command was admitted.
    Backpressure = 20,
    /// Child reports that runtime construction completed.
    Ready = 21,
    /// Child confirms shutdown and terminal record publication.
    ShutdownComplete = 22,
    /// Child reports a fail-closed protocol rejection.
    Rejected = 23,
    /// Child durably acknowledges one exact ingress packet sequence.
    PacketConsumed = 24,
    /// Child durably completes one exact root control sequence.
    ControlCompleted = 25,
    /// Child confirms one control's TCP bytes have left the send queue.
    OutputDrained = 26,
    /// Child publishes up to eight authenticated commands atomically.
    CommandBatch = 27,
}

/// Compact validated metadata for one packet-page publication.
///
/// Unlike [`PacketPage`], this record does not carry page alignment or the
/// reserved tail. It lets target runtimes copy only the declared packet bytes
/// while retaining the fixed 4-KiB wire layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct PacketPageHeader {
    /// [`PACKET_PAGE_MAGIC`].
    pub magic: u32,
    /// [`ABI_VERSION`].
    pub abi_version: u16,
    /// Raw [`PacketDirection`].
    pub direction: u16,
    /// Nonzero child generation.
    pub generation: u64,
    /// Producer sequence.
    pub sequence: u64,
    /// Sequence-last publication word.
    pub committed_sequence: u64,
    /// Initialized packet bytes.
    pub packet_len: u16,
    /// Reserved flags; zero.
    pub flags: u16,
    /// Reserved; zero.
    pub reserved0: u32,
}

impl PacketPageHeader {
    /// Build one uncommitted packet header after validating its bounds.
    pub fn staged(
        direction: PacketDirection,
        generation: u64,
        sequence: u64,
        packet_len: usize,
    ) -> Result<Self, AbiError> {
        if generation == 0 || sequence == 0 || packet_len == 0 || packet_len > ETHERNET_FRAME_BYTES
        {
            return Err(AbiError::InvalidBound);
        }
        Ok(Self {
            magic: PACKET_PAGE_MAGIC,
            abi_version: ABI_VERSION,
            direction: direction as u16,
            generation,
            sequence,
            committed_sequence: 0,
            packet_len: packet_len as u16,
            flags: 0,
            reserved0: 0,
        })
    }

    /// Validate identity, generation, sequence, direction, and packet bound.
    pub fn validate(
        self,
        generation: u64,
        after_sequence: u64,
    ) -> Result<(PacketDirection, usize), AbiError> {
        if self.magic != PACKET_PAGE_MAGIC || self.abi_version != ABI_VERSION {
            return Err(AbiError::InvalidIdentity);
        }
        if self.generation != generation {
            return Err(AbiError::StaleGeneration);
        }
        if self.sequence == 0
            || self.sequence <= after_sequence
            || self.committed_sequence != self.sequence
        {
            return Err(AbiError::InvalidSequence);
        }
        let packet_len = self.packet_len as usize;
        if packet_len == 0
            || packet_len > ETHERNET_FRAME_BYTES
            || self.flags != 0
            || self.reserved0 != 0
        {
            return Err(AbiError::InvalidBound);
        }
        Ok((PacketDirection::from_raw(self.direction)?, packet_len))
    }
}

/// Compact owned packet copied from one stable shared-page publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PacketRecord {
    direction: PacketDirection,
    sequence: u64,
    packet_len: u16,
    packet: [u8; ETHERNET_FRAME_BYTES],
}

impl PacketRecord {
    /// Validated packet direction.
    #[must_use]
    pub const fn direction(&self) -> PacketDirection {
        self.direction
    }

    /// Exact producer sequence.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Initialized packet bytes only.
    #[must_use]
    pub fn packet(&self) -> &[u8] {
        &self.packet[..self.packet_len as usize]
    }
}

/// Compact validated metadata for one console exchange publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct ExchangePageHeader {
    /// [`EXCHANGE_PAGE_MAGIC`].
    pub magic: u32,
    /// [`ABI_VERSION`].
    pub abi_version: u16,
    /// Raw [`ExchangeKind`].
    pub kind: u16,
    /// Exact fixed page size.
    pub record_bytes: u16,
    /// Initialized payload bytes.
    pub payload_len: u16,
    /// Reserved; zero.
    pub reserved0: u32,
    /// Nonzero child generation.
    pub generation: u64,
    /// Producer sequence.
    pub sequence: u64,
    /// Exact connection identity, or zero for READY.
    pub connection_id: u64,
    /// Monotonic observation timestamp supplied by the producer.
    pub now_ms: u64,
    /// Exact packet/control sequence acknowledged by completion kinds.
    pub related_sequence: u64,
    /// Sequence-last publication word.
    pub committed_sequence: u64,
}

impl ExchangePageHeader {
    /// Build one uncommitted exchange header after validating its metadata.
    pub fn staged(
        kind: ExchangeKind,
        generation: u64,
        sequence: u64,
        connection_id: u64,
        now_ms: u64,
        related_sequence: u64,
        payload_len: usize,
    ) -> Result<Self, AbiError> {
        validate_exchange_metadata(
            kind,
            generation,
            sequence,
            connection_id,
            related_sequence,
            payload_len,
        )?;
        Ok(Self {
            magic: EXCHANGE_PAGE_MAGIC,
            abi_version: ABI_VERSION,
            kind: kind as u16,
            record_bytes: SHARED_PAGE_BYTES as u16,
            payload_len: payload_len as u16,
            reserved0: 0,
            generation,
            sequence,
            connection_id,
            now_ms,
            related_sequence,
            committed_sequence: 0,
        })
    }

    /// Validate identity, direction, generation, sequence, and payload bound.
    pub fn validate(
        self,
        generation: u64,
        after_sequence: u64,
        root_to_child: bool,
    ) -> Result<(ExchangeKind, usize), AbiError> {
        if self.magic != EXCHANGE_PAGE_MAGIC || self.abi_version != ABI_VERSION {
            return Err(AbiError::InvalidIdentity);
        }
        if self.record_bytes as usize != SHARED_PAGE_BYTES || self.reserved0 != 0 {
            return Err(AbiError::InvalidLayout);
        }
        if self.generation != generation {
            return Err(AbiError::StaleGeneration);
        }
        if self.sequence == 0
            || self.sequence <= after_sequence
            || self.committed_sequence != self.sequence
        {
            return Err(AbiError::InvalidSequence);
        }
        let kind = ExchangeKind::from_raw(self.kind)?;
        if kind.root_to_child() != root_to_child {
            return Err(AbiError::InvalidKind);
        }
        validate_exchange_metadata(
            kind,
            self.generation,
            self.sequence,
            self.connection_id,
            self.related_sequence,
            self.payload_len as usize,
        )?;
        Ok((kind, self.payload_len as usize))
    }
}

/// Compact owned exchange copied from one stable shared-page publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExchangeRecord {
    kind: ExchangeKind,
    sequence: u64,
    connection_id: u64,
    now_ms: u64,
    related_sequence: u64,
    payload_len: u16,
    payload: [u8; CONSOLE_PAYLOAD_BYTES],
}

impl ExchangeRecord {
    /// Validated exchange kind.
    #[must_use]
    pub const fn kind(&self) -> ExchangeKind {
        self.kind
    }

    /// Exact producer sequence.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Exact child connection identity.
    #[must_use]
    pub const fn connection_id(&self) -> u64 {
        self.connection_id
    }

    /// Producer observation time.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// Exact related packet/control sequence.
    #[must_use]
    pub const fn related_sequence(&self) -> u64 {
        self.related_sequence
    }

    /// Initialized payload bytes validated according to [`Self::kind`].
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len as usize]
    }
}

impl ExchangeKind {
    /// Decode a raw record kind.
    pub const fn from_raw(value: u16) -> Result<Self, AbiError> {
        match value {
            1 => Ok(Self::SendLine),
            2 => Ok(Self::Disconnect),
            3 => Ok(Self::SendBatch),
            16 => Ok(Self::Connected),
            17 => Ok(Self::Authenticated),
            18 => Ok(Self::Command),
            19 => Ok(Self::Disconnected),
            20 => Ok(Self::Backpressure),
            21 => Ok(Self::Ready),
            22 => Ok(Self::ShutdownComplete),
            23 => Ok(Self::Rejected),
            24 => Ok(Self::PacketConsumed),
            25 => Ok(Self::ControlCompleted),
            26 => Ok(Self::OutputDrained),
            27 => Ok(Self::CommandBatch),
            _ => Err(AbiError::InvalidKind),
        }
    }

    /// Whether this kind may be produced by root.
    #[must_use]
    pub const fn root_to_child(self) -> bool {
        matches!(self, Self::SendLine | Self::Disconnect | Self::SendBatch)
    }

    /// Whether this kind may be produced by the child.
    #[must_use]
    pub const fn child_to_root(self) -> bool {
        !self.root_to_child()
    }
}

/// Incremental encoder for one bounded [`ExchangeKind::SendBatch`] payload.
///
/// The caller supplies the existing exchange-page payload storage. Only the
/// returned active prefix is authoritative; the unused suffix is untouched.
pub struct SendBatchBuilder<'a> {
    output: &'a mut [u8; CONSOLE_PAYLOAD_BYTES],
    cursor: usize,
    record_count: u16,
}

impl<'a> SendBatchBuilder<'a> {
    /// Begin an empty response batch in caller-owned bounded storage.
    #[must_use]
    pub fn new(output: &'a mut [u8; CONSOLE_PAYLOAD_BYTES]) -> Self {
        output[..SEND_BATCH_HEADER_BYTES].fill(0);
        Self {
            output,
            cursor: SEND_BATCH_HEADER_BYTES,
            record_count: 0,
        }
    }

    /// Append one canonical response line.
    ///
    /// Returns `Ok(false)` without mutation when the valid line would exceed
    /// the eight-record or shared-payload bound. Terminal CR/LF bytes are
    /// removed exactly as for the legacy single-line control.
    pub fn try_push_line(&mut self, line: &str) -> Result<bool, AbiError> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty()
            || line.len() > SEND_BATCH_LINE_BYTES
            || line
                .as_bytes()
                .iter()
                .any(|byte| matches!(byte, b'\r' | b'\n'))
        {
            return Err(AbiError::InvalidBound);
        }
        if usize::from(self.record_count) >= SEND_BATCH_MAX_RECORDS {
            return Ok(false);
        }
        let Some(record_end) = self
            .cursor
            .checked_add(SEND_BATCH_RECORD_HEADER_BYTES)
            .and_then(|offset| offset.checked_add(line.len()))
        else {
            return Err(AbiError::InvalidBound);
        };
        if record_end > self.output.len() {
            return Ok(false);
        }
        let line_len = line.len() as u16;
        self.output[self.cursor..self.cursor + SEND_BATCH_RECORD_HEADER_BYTES]
            .copy_from_slice(&line_len.to_le_bytes());
        let line_start = self.cursor + SEND_BATCH_RECORD_HEADER_BYTES;
        self.output[line_start..record_end].copy_from_slice(line.as_bytes());
        self.cursor = record_end;
        self.record_count = self.record_count.saturating_add(1);
        Ok(true)
    }

    /// Number of lines already encoded.
    #[must_use]
    pub const fn record_count(&self) -> usize {
        self.record_count as usize
    }

    /// Finish the batch and return its exact active payload prefix.
    pub fn finish(self) -> Result<&'a [u8], AbiError> {
        if self.record_count == 0 {
            return Err(AbiError::InvalidBound);
        }
        let used_bytes = self.cursor.saturating_sub(SEND_BATCH_HEADER_BYTES);
        if used_bytes == 0 || used_bytes > u16::MAX as usize {
            return Err(AbiError::InvalidBound);
        }
        self.output[0..2].copy_from_slice(&SEND_BATCH_ENCODING_VERSION.to_le_bytes());
        self.output[2..4].copy_from_slice(&self.record_count.to_le_bytes());
        self.output[4..6].copy_from_slice(&(used_bytes as u16).to_le_bytes());
        self.output[6..8].copy_from_slice(&0u16.to_le_bytes());
        Ok(&self.output[..self.cursor])
    }
}

/// Validated cursor over one private copy of a response-batch payload.
///
/// The cursor owns no bytes and contains no pointer. A consumer may therefore
/// validate shared input once, copy the exact active prefix into private
/// storage, and retain this cursor across bounded service turns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SendBatchCursor {
    next_offset: u16,
    end_offset: u16,
    remaining: u16,
}

impl SendBatchCursor {
    /// Validate the complete binary batch and return its initial cursor.
    pub fn validate(payload: &[u8]) -> Result<Self, AbiError> {
        if payload.len() < SEND_BATCH_HEADER_BYTES + SEND_BATCH_RECORD_HEADER_BYTES + 1
            || payload.len() > CONSOLE_PAYLOAD_BYTES
        {
            return Err(AbiError::InvalidBound);
        }
        if read_u16(payload, 0) != SEND_BATCH_ENCODING_VERSION {
            return Err(AbiError::InvalidIdentity);
        }
        let record_count = read_u16(payload, 2);
        if record_count == 0 || usize::from(record_count) > SEND_BATCH_MAX_RECORDS {
            return Err(AbiError::InvalidBound);
        }
        let used_bytes = usize::from(read_u16(payload, 4));
        if read_u16(payload, 6) != 0
            || used_bytes != payload.len().saturating_sub(SEND_BATCH_HEADER_BYTES)
        {
            return Err(AbiError::InvalidLayout);
        }
        let initial = Self {
            next_offset: SEND_BATCH_HEADER_BYTES as u16,
            end_offset: payload.len() as u16,
            remaining: record_count,
        };
        let mut scan = initial;
        while scan.next_line(payload)?.is_some() {}
        Ok(initial)
    }

    /// Number of response lines not yet consumed.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.remaining as usize
    }

    /// Whether every validated record has been consumed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.remaining == 0
    }

    /// Validate and advance over one exact UTF-8 response line.
    pub fn next_line<'a>(&mut self, payload: &'a [u8]) -> Result<Option<&'a str>, AbiError> {
        let end_offset = usize::from(self.end_offset);
        let next_offset = usize::from(self.next_offset);
        if payload.len() != end_offset || next_offset > end_offset {
            return Err(AbiError::InvalidBound);
        }
        if self.remaining == 0 {
            if next_offset != end_offset {
                return Err(AbiError::InvalidLayout);
            }
            return Ok(None);
        }
        let Some(line_start) = next_offset.checked_add(SEND_BATCH_RECORD_HEADER_BYTES) else {
            return Err(AbiError::InvalidBound);
        };
        if line_start > end_offset {
            return Err(AbiError::InvalidBound);
        }
        let line_len = usize::from(read_u16(payload, next_offset));
        if line_len == 0 || line_len > SEND_BATCH_LINE_BYTES {
            return Err(AbiError::InvalidBound);
        }
        let Some(line_end) = line_start.checked_add(line_len) else {
            return Err(AbiError::InvalidBound);
        };
        if line_end > end_offset {
            return Err(AbiError::InvalidBound);
        }
        let line_bytes = &payload[line_start..line_end];
        let line = core::str::from_utf8(line_bytes).map_err(|_| AbiError::InvalidBound)?;
        if line_bytes.iter().any(|byte| matches!(byte, b'\r' | b'\n')) {
            return Err(AbiError::InvalidBound);
        }
        let remaining = self.remaining - 1;
        if (remaining == 0 && line_end != end_offset) || (remaining != 0 && line_end >= end_offset)
        {
            return Err(AbiError::InvalidLayout);
        }
        self.next_offset = line_end as u16;
        self.remaining = remaining;
        Ok(Some(line))
    }
}

/// Incremental encoder for one bounded [`ExchangeKind::CommandBatch`] payload.
///
/// Commands retain the exact UTF-8 bytes accepted by the legacy single-command
/// record. The caller supplies fixed exchange-page storage, and the builder
/// stops before either the eight-command or shared-payload bound.
pub struct CommandBatchBuilder<'a> {
    output: &'a mut [u8; CONSOLE_PAYLOAD_BYTES],
    cursor: usize,
    record_count: u16,
}

impl<'a> CommandBatchBuilder<'a> {
    /// Begin an empty command batch in caller-owned bounded storage.
    #[must_use]
    pub fn new(output: &'a mut [u8; CONSOLE_PAYLOAD_BYTES]) -> Self {
        output[..COMMAND_BATCH_HEADER_BYTES].fill(0);
        Self {
            output,
            cursor: COMMAND_BATCH_HEADER_BYTES,
            record_count: 0,
        }
    }

    /// Append one exact authenticated command.
    ///
    /// Returns `Ok(false)` without mutation when the valid command would exceed
    /// the eight-record or shared-payload bound.
    pub fn try_push_command(&mut self, now_ms: u64, command: &str) -> Result<bool, AbiError> {
        if command.is_empty() || command.len() > COMMAND_LINE_BYTES {
            return Err(AbiError::InvalidBound);
        }
        if usize::from(self.record_count) >= COMMAND_BATCH_MAX_RECORDS {
            return Ok(false);
        }
        let Some(record_end) = self
            .cursor
            .checked_add(COMMAND_BATCH_RECORD_HEADER_BYTES)
            .and_then(|offset| offset.checked_add(command.len()))
        else {
            return Err(AbiError::InvalidBound);
        };
        if record_end > self.output.len() {
            return Ok(false);
        }
        let command_len = command.len() as u16;
        self.output[self.cursor..self.cursor + 8].copy_from_slice(&now_ms.to_le_bytes());
        self.output[self.cursor + 8..self.cursor + COMMAND_BATCH_RECORD_HEADER_BYTES]
            .copy_from_slice(&command_len.to_le_bytes());
        let command_start = self.cursor + COMMAND_BATCH_RECORD_HEADER_BYTES;
        self.output[command_start..record_end].copy_from_slice(command.as_bytes());
        self.cursor = record_end;
        self.record_count = self.record_count.saturating_add(1);
        Ok(true)
    }

    /// Number of commands already encoded.
    #[must_use]
    pub const fn record_count(&self) -> usize {
        self.record_count as usize
    }

    /// Finish the batch and return its exact active payload prefix.
    pub fn finish(self) -> Result<&'a [u8], AbiError> {
        if self.record_count == 0 {
            return Err(AbiError::InvalidBound);
        }
        let used_bytes = self.cursor.saturating_sub(COMMAND_BATCH_HEADER_BYTES);
        if used_bytes == 0 || used_bytes > u16::MAX as usize {
            return Err(AbiError::InvalidBound);
        }
        self.output[0..2].copy_from_slice(&COMMAND_BATCH_ENCODING_VERSION.to_le_bytes());
        self.output[2..4].copy_from_slice(&self.record_count.to_le_bytes());
        self.output[4..6].copy_from_slice(&(used_bytes as u16).to_le_bytes());
        self.output[6..8].copy_from_slice(&0u16.to_le_bytes());
        Ok(&self.output[..self.cursor])
    }
}

/// Validated cursor over one private copy of a command-batch payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandBatchCursor {
    next_offset: u16,
    end_offset: u16,
    remaining: u16,
}

impl CommandBatchCursor {
    /// Validate the complete binary batch and return its initial cursor.
    pub fn validate(payload: &[u8]) -> Result<Self, AbiError> {
        if payload.len() < COMMAND_BATCH_HEADER_BYTES + COMMAND_BATCH_RECORD_HEADER_BYTES + 1
            || payload.len() > CONSOLE_PAYLOAD_BYTES
        {
            return Err(AbiError::InvalidBound);
        }
        if read_u16(payload, 0) != COMMAND_BATCH_ENCODING_VERSION {
            return Err(AbiError::InvalidIdentity);
        }
        let record_count = read_u16(payload, 2);
        if record_count == 0 || usize::from(record_count) > COMMAND_BATCH_MAX_RECORDS {
            return Err(AbiError::InvalidBound);
        }
        let used_bytes = usize::from(read_u16(payload, 4));
        if read_u16(payload, 6) != 0
            || used_bytes != payload.len().saturating_sub(COMMAND_BATCH_HEADER_BYTES)
        {
            return Err(AbiError::InvalidLayout);
        }
        let initial = Self {
            next_offset: COMMAND_BATCH_HEADER_BYTES as u16,
            end_offset: payload.len() as u16,
            remaining: record_count,
        };
        let mut scan = initial;
        while scan.next_command(payload)?.is_some() {}
        Ok(initial)
    }

    /// Number of authenticated commands not yet consumed.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.remaining as usize
    }

    /// Whether every validated command has been consumed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.remaining == 0
    }

    /// Validate and advance over one exact UTF-8 command.
    pub fn next_command<'a>(
        &mut self,
        payload: &'a [u8],
    ) -> Result<Option<(u64, &'a str)>, AbiError> {
        let end_offset = usize::from(self.end_offset);
        let next_offset = usize::from(self.next_offset);
        if payload.len() != end_offset || next_offset > end_offset {
            return Err(AbiError::InvalidBound);
        }
        if self.remaining == 0 {
            if next_offset != end_offset {
                return Err(AbiError::InvalidLayout);
            }
            return Ok(None);
        }
        let Some(command_start) = next_offset.checked_add(COMMAND_BATCH_RECORD_HEADER_BYTES) else {
            return Err(AbiError::InvalidBound);
        };
        if command_start > end_offset {
            return Err(AbiError::InvalidBound);
        }
        let now_ms = read_u64(payload, next_offset);
        let command_len = usize::from(read_u16(payload, next_offset + 8));
        if command_len == 0 || command_len > COMMAND_LINE_BYTES {
            return Err(AbiError::InvalidBound);
        }
        let Some(command_end) = command_start.checked_add(command_len) else {
            return Err(AbiError::InvalidBound);
        };
        if command_end > end_offset {
            return Err(AbiError::InvalidBound);
        }
        let command = core::str::from_utf8(&payload[command_start..command_end])
            .map_err(|_| AbiError::InvalidBound)?;
        let remaining = self.remaining - 1;
        if (remaining == 0 && command_end != end_offset)
            || (remaining != 0 && command_end >= end_offset)
        {
            return Err(AbiError::InvalidLayout);
        }
        self.next_offset = command_end as u16;
        self.remaining = remaining;
        Ok(Some((now_ms, command)))
    }
}

/// Pointer-free, read-only QEMU VirtIO ownership layout.
///
/// Root constructs and maps every page before activation, then seals this
/// record into the child's existing init page. The child receives virtual and
/// physical addresses only; it receives no allocator, VSpace, IRQ-control, or
/// root capability authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C, align(8))]
pub struct DirectVirtioLayout {
    /// [`DIRECT_VIRTIO_LAYOUT_MAGIC`].
    pub magic: u32,
    /// [`DIRECT_VIRTIO_LAYOUT_VERSION`].
    pub version: u16,
    /// Exact [`DIRECT_VIRTIO_LAYOUT_BYTES`].
    pub layout_bytes: u16,
    /// Reserved flags; zero.
    pub flags: u32,
    /// Exact [`DIRECT_VIRTIO_QUEUE_SIZE`].
    pub queue_size: u16,
    /// Exact [`DIRECT_VIRTIO_BUFFER_COUNT`].
    pub buffer_count: u16,
    /// Child virtual address of the sole QEMU VirtIO-MMIO page.
    pub mmio_vaddr: u64,
    /// Physical address of the sole QEMU VirtIO-MMIO page.
    pub mmio_paddr: u64,
    /// Child virtual addresses of RX and TX queue pages.
    pub queue_vaddrs: [u64; DIRECT_VIRTIO_QUEUE_COUNT],
    /// Physical addresses of RX and TX queue pages.
    pub queue_paddrs: [u64; DIRECT_VIRTIO_QUEUE_COUNT],
    /// Child virtual base of the contiguous RX DMA-page window.
    pub rx_vaddr: u64,
    /// Child virtual base of the contiguous TX DMA-page window.
    pub tx_vaddr: u64,
    /// Physical address of every RX DMA page.
    pub rx_paddrs: [u64; DIRECT_VIRTIO_BUFFER_COUNT],
    /// Physical address of every TX DMA page.
    pub tx_paddrs: [u64; DIRECT_VIRTIO_BUFFER_COUNT],
    /// FNV-1a seal over every preceding field.
    pub seal: u64,
}

impl DirectVirtioLayout {
    /// Seal a fully populated layout.
    #[must_use]
    pub const fn sealed(mut self) -> Self {
        self.seal = self.expected_seal();
        self
    }

    /// Validate exact bounds, disjoint page identities, and the complete seal.
    pub const fn validate(self) -> Result<(), AbiError> {
        if self.magic != DIRECT_VIRTIO_LAYOUT_MAGIC || self.version != DIRECT_VIRTIO_LAYOUT_VERSION
        {
            return Err(AbiError::InvalidIdentity);
        }
        if self.layout_bytes as usize != DIRECT_VIRTIO_LAYOUT_BYTES || self.flags != 0 {
            return Err(AbiError::InvalidLayout);
        }
        if self.queue_size as usize != DIRECT_VIRTIO_QUEUE_SIZE
            || self.buffer_count as usize != DIRECT_VIRTIO_BUFFER_COUNT
        {
            return Err(AbiError::InvalidBound);
        }
        if !page_aligned_nonzero(self.mmio_vaddr)
            || !page_aligned_nonzero(self.mmio_paddr)
            || !page_aligned_nonzero(self.rx_vaddr)
            || !page_aligned_nonzero(self.tx_vaddr)
        {
            return Err(AbiError::InvalidLayout);
        }
        let rx_end = match self
            .rx_vaddr
            .checked_add((DIRECT_VIRTIO_BUFFER_COUNT * DIRECT_VIRTIO_PAGE_BYTES) as u64)
        {
            Some(end) => end,
            None => return Err(AbiError::InvalidLayout),
        };
        let tx_end = match self
            .tx_vaddr
            .checked_add((DIRECT_VIRTIO_BUFFER_COUNT * DIRECT_VIRTIO_PAGE_BYTES) as u64)
        {
            Some(end) => end,
            None => return Err(AbiError::InvalidLayout),
        };
        if ranges_overlap(self.rx_vaddr, rx_end, self.tx_vaddr, tx_end) {
            return Err(AbiError::InvalidLayout);
        }
        let mut page_index = 0usize;
        while page_index < DIRECT_VIRTIO_QUEUE_COUNT {
            if !page_aligned_nonzero(self.queue_vaddrs[page_index])
                || !page_aligned_nonzero(self.queue_paddrs[page_index])
            {
                return Err(AbiError::InvalidLayout);
            }
            page_index += 1;
        }
        page_index = 0;
        while page_index < DIRECT_VIRTIO_BUFFER_COUNT {
            if !page_aligned_nonzero(self.rx_paddrs[page_index])
                || !page_aligned_nonzero(self.tx_paddrs[page_index])
            {
                return Err(AbiError::InvalidLayout);
            }
            page_index += 1;
        }
        let virtual_pages = self.virtual_pages();
        let mut left = 0usize;
        while left < virtual_pages.len() {
            let mut right = left + 1;
            while right < virtual_pages.len() {
                if virtual_pages[left] == virtual_pages[right] {
                    return Err(AbiError::InvalidLayout);
                }
                right += 1;
            }
            left += 1;
        }
        let physical = self.physical_pages();
        left = 0;
        while left < physical.len() {
            let mut right = left + 1;
            while right < physical.len() {
                if physical[left] == physical[right] {
                    return Err(AbiError::InvalidLayout);
                }
                right += 1;
            }
            left += 1;
        }
        if self.seal == 0 || self.seal != self.expected_seal() {
            return Err(AbiError::InvalidSeal);
        }
        Ok(())
    }

    /// Encode the canonical layout without copying Rust padding.
    pub fn encode(&self, output: &mut [u8]) -> Result<(), AbiError> {
        if output.len() != DIRECT_VIRTIO_LAYOUT_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        self.validate()?;
        output.fill(0);
        output[0..4].copy_from_slice(&self.magic.to_le_bytes());
        output[4..6].copy_from_slice(&self.version.to_le_bytes());
        output[6..8].copy_from_slice(&self.layout_bytes.to_le_bytes());
        output[8..12].copy_from_slice(&self.flags.to_le_bytes());
        output[12..14].copy_from_slice(&self.queue_size.to_le_bytes());
        output[14..16].copy_from_slice(&self.buffer_count.to_le_bytes());
        output[16..24].copy_from_slice(&self.mmio_vaddr.to_le_bytes());
        output[24..32].copy_from_slice(&self.mmio_paddr.to_le_bytes());
        encode_u64_array(output, 32, &self.queue_vaddrs);
        encode_u64_array(output, 48, &self.queue_paddrs);
        output[64..72].copy_from_slice(&self.rx_vaddr.to_le_bytes());
        output[72..80].copy_from_slice(&self.tx_vaddr.to_le_bytes());
        encode_u64_array(output, 80, &self.rx_paddrs);
        encode_u64_array(output, 208, &self.tx_paddrs);
        output[336..344].copy_from_slice(&self.seal.to_le_bytes());
        Ok(())
    }

    /// Decode one canonical direct-VirtIO layout.
    pub fn decode(input: &[u8]) -> Result<Self, AbiError> {
        if input.len() != DIRECT_VIRTIO_LAYOUT_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        let layout = Self {
            magic: read_u32(input, 0),
            version: read_u16(input, 4),
            layout_bytes: read_u16(input, 6),
            flags: read_u32(input, 8),
            queue_size: read_u16(input, 12),
            buffer_count: read_u16(input, 14),
            mmio_vaddr: read_u64(input, 16),
            mmio_paddr: read_u64(input, 24),
            queue_vaddrs: decode_u64_array(input, 32),
            queue_paddrs: decode_u64_array(input, 48),
            rx_vaddr: read_u64(input, 64),
            tx_vaddr: read_u64(input, 72),
            rx_paddrs: decode_u64_array(input, 80),
            tx_paddrs: decode_u64_array(input, 208),
            seal: read_u64(input, 336),
        };
        layout.validate()?;
        Ok(layout)
    }

    const fn physical_pages(
        self,
    ) -> [u64; 1 + DIRECT_VIRTIO_QUEUE_COUNT + DIRECT_VIRTIO_BUFFER_COUNT * 2] {
        let mut pages = [0u64; 1 + DIRECT_VIRTIO_QUEUE_COUNT + DIRECT_VIRTIO_BUFFER_COUNT * 2];
        pages[0] = self.mmio_paddr;
        let mut index = 0usize;
        while index < DIRECT_VIRTIO_QUEUE_COUNT {
            pages[1 + index] = self.queue_paddrs[index];
            index += 1;
        }
        let mut buffer = 0usize;
        while buffer < DIRECT_VIRTIO_BUFFER_COUNT {
            pages[1 + DIRECT_VIRTIO_QUEUE_COUNT + buffer] = self.rx_paddrs[buffer];
            pages[1 + DIRECT_VIRTIO_QUEUE_COUNT + DIRECT_VIRTIO_BUFFER_COUNT + buffer] =
                self.tx_paddrs[buffer];
            buffer += 1;
        }
        pages
    }

    const fn virtual_pages(
        self,
    ) -> [u64; 1 + DIRECT_VIRTIO_QUEUE_COUNT + DIRECT_VIRTIO_BUFFER_COUNT * 2] {
        let mut pages = [0u64; 1 + DIRECT_VIRTIO_QUEUE_COUNT + DIRECT_VIRTIO_BUFFER_COUNT * 2];
        pages[0] = self.mmio_vaddr;
        let mut index = 0usize;
        while index < DIRECT_VIRTIO_QUEUE_COUNT {
            pages[1 + index] = self.queue_vaddrs[index];
            index += 1;
        }
        let mut buffer = 0usize;
        while buffer < DIRECT_VIRTIO_BUFFER_COUNT {
            pages[1 + DIRECT_VIRTIO_QUEUE_COUNT + buffer] =
                self.rx_vaddr + (buffer * DIRECT_VIRTIO_PAGE_BYTES) as u64;
            pages[1 + DIRECT_VIRTIO_QUEUE_COUNT + DIRECT_VIRTIO_BUFFER_COUNT + buffer] =
                self.tx_vaddr + (buffer * DIRECT_VIRTIO_PAGE_BYTES) as u64;
            buffer += 1;
        }
        pages
    }

    const fn expected_seal(self) -> u64 {
        let mut hash = FNV64_OFFSET;
        hash = hash_u32(hash, self.magic);
        hash = hash_u16(hash, self.version);
        hash = hash_u16(hash, self.layout_bytes);
        hash = hash_u32(hash, self.flags);
        hash = hash_u16(hash, self.queue_size);
        hash = hash_u16(hash, self.buffer_count);
        hash = hash_u64(hash, self.mmio_vaddr);
        hash = hash_u64(hash, self.mmio_paddr);
        hash = hash_u64_slice(hash, &self.queue_vaddrs);
        hash = hash_u64_slice(hash, &self.queue_paddrs);
        hash = hash_u64(hash, self.rx_vaddr);
        hash = hash_u64(hash, self.tx_vaddr);
        hash = hash_u64_slice(hash, &self.rx_paddrs);
        hash_u64_slice(hash, &self.tx_paddrs)
    }
}

/// Pointer-free, read-only layout for the optional Pi GENET direct link.
///
/// Root maps the same thirty-two CPU-only frames into the isolated GENET and
/// console-network children only after their bootstrap use has ended and the
/// complete frame population has been scrubbed. No address in this descriptor
/// is a DMA or MMIO address. The link notification is a scheduling hint; the
/// generation-bound control cursors are the sole packet authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C, align(8))]
pub struct DirectGenetLayout {
    /// [`DIRECT_GENET_LAYOUT_MAGIC`].
    pub magic: u32,
    /// [`DIRECT_GENET_LAYOUT_VERSION`].
    pub version: u16,
    /// Exact [`DIRECT_GENET_LAYOUT_BYTES`].
    pub layout_bytes: u16,
    /// Exact [`DIRECT_GENET_LAYOUT_FLAGS`].
    pub flags: u32,
    /// Exact [`SHARED_PAGE_BYTES`].
    pub shared_page_bytes: u16,
    /// Exact [`DIRECT_GENET_RX_SLOT_COUNT`].
    pub rx_slot_count: u8,
    /// Exact [`DIRECT_GENET_TX_SLOT_COUNT`].
    pub tx_slot_count: u8,
    /// Nonzero generation shared by both direct-link endpoints.
    pub generation: u64,
    /// Child cap slot used only to signal the isolated GENET peer.
    pub peer_wake_notification_slot: u32,
    /// Reserved; zero.
    pub reserved0: u32,
    /// Child virtual address of the sole direct-link control page.
    pub control_vaddr: u64,
    /// Child virtual addresses of GENET-produced RX slots.
    pub rx_vaddrs: [u64; DIRECT_GENET_RX_SLOT_COUNT],
    /// Child virtual addresses of console-network-produced TX slots.
    pub tx_vaddrs: [u64; DIRECT_GENET_TX_SLOT_COUNT],
    /// FNV-1a seal over every preceding field.
    pub seal: u64,
}

impl DirectGenetLayout {
    /// Seal a fully populated direct-link layout.
    #[must_use]
    pub const fn sealed(mut self) -> Self {
        self.seal = self.expected_seal();
        self
    }

    /// Validate the layout against its embedded nonzero generation.
    pub const fn validate(self) -> Result<(), AbiError> {
        self.validate_for(self.generation)
    }

    /// Validate exact bounds, generation, unique CPU mappings, and seal.
    pub const fn validate_for(self, generation: u64) -> Result<(), AbiError> {
        if self.magic != DIRECT_GENET_LAYOUT_MAGIC || self.version != DIRECT_GENET_LAYOUT_VERSION {
            return Err(AbiError::InvalidIdentity);
        }
        if self.layout_bytes as usize != DIRECT_GENET_LAYOUT_BYTES
            || self.flags != DIRECT_GENET_LAYOUT_FLAGS
            || self.shared_page_bytes as usize != SHARED_PAGE_BYTES
            || self.rx_slot_count as usize != DIRECT_GENET_RX_SLOT_COUNT
            || self.tx_slot_count as usize != DIRECT_GENET_TX_SLOT_COUNT
            || self.peer_wake_notification_slot != DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT
            || self.reserved0 != 0
        {
            return Err(AbiError::InvalidLayout);
        }
        if generation == 0 || self.generation != generation {
            return Err(AbiError::StaleGeneration);
        }
        let pages = self.virtual_pages();
        let mut left = 0usize;
        while left < pages.len() {
            if !page_aligned_nonzero(pages[left]) {
                return Err(AbiError::InvalidLayout);
            }
            let mut right = left + 1;
            while right < pages.len() {
                if pages[left] == pages[right] {
                    return Err(AbiError::InvalidLayout);
                }
                right += 1;
            }
            left += 1;
        }
        if self.seal == 0 || self.seal != self.expected_seal() {
            return Err(AbiError::InvalidSeal);
        }
        Ok(())
    }

    /// Encode the canonical layout without copying Rust padding.
    pub fn encode(&self, output: &mut [u8]) -> Result<(), AbiError> {
        if output.len() != DIRECT_GENET_LAYOUT_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        self.validate()?;
        output.fill(0);
        output[0..4].copy_from_slice(&self.magic.to_le_bytes());
        output[4..6].copy_from_slice(&self.version.to_le_bytes());
        output[6..8].copy_from_slice(&self.layout_bytes.to_le_bytes());
        output[8..12].copy_from_slice(&self.flags.to_le_bytes());
        output[12..14].copy_from_slice(&self.shared_page_bytes.to_le_bytes());
        output[14] = self.rx_slot_count;
        output[15] = self.tx_slot_count;
        output[16..24].copy_from_slice(&self.generation.to_le_bytes());
        output[24..28].copy_from_slice(&self.peer_wake_notification_slot.to_le_bytes());
        output[28..32].copy_from_slice(&self.reserved0.to_le_bytes());
        output[32..40].copy_from_slice(&self.control_vaddr.to_le_bytes());
        encode_u64_array(output, 40, &self.rx_vaddrs);
        encode_u64_array(output, 160, &self.tx_vaddrs);
        output[288..296].copy_from_slice(&self.seal.to_le_bytes());
        Ok(())
    }

    /// Decode one canonical Pi GENET direct-link layout.
    pub fn decode(input: &[u8]) -> Result<Self, AbiError> {
        if input.len() != DIRECT_GENET_LAYOUT_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        let layout = Self {
            magic: read_u32(input, 0),
            version: read_u16(input, 4),
            layout_bytes: read_u16(input, 6),
            flags: read_u32(input, 8),
            shared_page_bytes: read_u16(input, 12),
            rx_slot_count: input[14],
            tx_slot_count: input[15],
            generation: read_u64(input, 16),
            peer_wake_notification_slot: read_u32(input, 24),
            reserved0: read_u32(input, 28),
            control_vaddr: read_u64(input, 32),
            rx_vaddrs: decode_u64_array(input, 40),
            tx_vaddrs: decode_u64_array(input, 160),
            seal: read_u64(input, 288),
        };
        layout.validate()?;
        Ok(layout)
    }

    const fn virtual_pages(self) -> [u64; DIRECT_GENET_SHARED_PAGE_COUNT] {
        let mut pages = [0u64; DIRECT_GENET_SHARED_PAGE_COUNT];
        pages[DIRECT_GENET_CONTROL_PAGE_INDEX] = self.control_vaddr;
        let mut index = 0usize;
        while index < DIRECT_GENET_RX_SLOT_COUNT {
            pages[DIRECT_GENET_RX_FIRST_PAGE_INDEX + index] = self.rx_vaddrs[index];
            index += 1;
        }
        index = 0;
        while index < DIRECT_GENET_TX_SLOT_COUNT {
            pages[DIRECT_GENET_TX_FIRST_PAGE_INDEX + index] = self.tx_vaddrs[index];
            index += 1;
        }
        pages
    }

    const fn expected_seal(self) -> u64 {
        let mut hash = FNV64_OFFSET;
        hash = hash_u32(hash, self.magic);
        hash = hash_u16(hash, self.version);
        hash = hash_u16(hash, self.layout_bytes);
        hash = hash_u32(hash, self.flags);
        hash = hash_u16(hash, self.shared_page_bytes);
        hash = hash_byte(hash, self.rx_slot_count);
        hash = hash_byte(hash, self.tx_slot_count);
        hash = hash_u64(hash, self.generation);
        hash = hash_u32(hash, self.peer_wake_notification_slot);
        hash = hash_u32(hash, self.reserved0);
        hash = hash_u64(hash, self.control_vaddr);
        hash = hash_u64_slice(hash, &self.rx_vaddrs);
        hash_u64_slice(hash, &self.tx_vaddrs)
    }
}

/// Direct-link control-page magic (`CNGC`).
pub const DIRECT_GENET_CONTROL_MAGIC: u32 = 0x434e_4743;
/// Direct-link control-page version.
pub const DIRECT_GENET_CONTROL_VERSION: u16 = 1;
/// Immutable bytes at the front of the direct-link control page.
pub const DIRECT_GENET_CONTROL_HEADER_BYTES: usize = 64;
/// Control flag: the shared packet pages are CPU-only.
pub const DIRECT_GENET_CONTROL_FLAG_CPU_ONLY: u32 = 1 << 0;
/// Control flag: each ring has one immutable producer and consumer.
pub const DIRECT_GENET_CONTROL_FLAG_SPSC: u32 = 1 << 1;
/// Control flag: a poisoned owner state permanently fences this generation.
pub const DIRECT_GENET_CONTROL_FLAG_POISON_FAIL_CLOSED: u32 = 1 << 2;
/// Exact direct-link control flags.
pub const DIRECT_GENET_CONTROL_FLAGS: u32 = DIRECT_GENET_CONTROL_FLAG_CPU_ONLY
    | DIRECT_GENET_CONTROL_FLAG_SPSC
    | DIRECT_GENET_CONTROL_FLAG_POISON_FAIL_CLOSED;
/// Direct-link cursor-state magic (`CNGQ`).
pub const DIRECT_GENET_CURSOR_MAGIC: u32 = 0x434e_4751;
/// Direct-link cursor-state version.
pub const DIRECT_GENET_CURSOR_VERSION: u16 = 1;
/// Exact cache-line bytes in each single-writer cursor state.
pub const DIRECT_GENET_CURSOR_STATE_BYTES: usize = 64;
/// Exact cursor-state population in the control page.
pub const DIRECT_GENET_CURSOR_STATE_COUNT: usize = 4;
/// RX-producer state offset in the control page.
pub const DIRECT_GENET_RX_PRODUCER_STATE_OFFSET: usize = 64;
/// RX-consumer state offset in the control page.
pub const DIRECT_GENET_RX_CONSUMER_STATE_OFFSET: usize = 128;
/// TX-producer state offset in the control page.
pub const DIRECT_GENET_TX_PRODUCER_STATE_OFFSET: usize = 192;
/// TX-consumer state offset in the control page.
pub const DIRECT_GENET_TX_CONSUMER_STATE_OFFSET: usize = 256;
/// Cache-line-aligned offset of the GENET-owner diagnostic record.
///
/// The record is observational and carries no packet, IRQ, or command
/// authority. Root requests a refresh through exact idempotent DGHO replay;
/// the isolated GENET owner then publishes the complete record sequence-last.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_OFFSET: usize = 320;
/// Exact bytes in one direct-GENET runtime diagnostic record.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES: usize = 192;
/// Sequence-last commit offset within the runtime diagnostic record.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET: usize = 184;
/// Direct-GENET runtime diagnostic magic (`CNGD`).
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_MAGIC: u32 = 0x434e_4744;
/// Direct-GENET runtime diagnostic layout version.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_VERSION: u16 = 1;
/// Diagnostic flag: the isolated GENET runtime completed initialization.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_INITIALIZED: u32 = 1 << 0;
/// Diagnostic flag: the exact direct-link generation is active.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_ACTIVE: u32 = 1 << 1;
/// Diagnostic flag: the direct-link generation failed closed.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_FAULTED: u32 = 1 << 2;
/// Diagnostic flag: the seL4 IRQ handler remains deliberately unacknowledged.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_IRQ_ACK_PENDING: u32 = 1 << 3;
/// Diagnostic flag: one RX publication awaits cursor reconciliation.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_RX_COMMIT_PENDING: u32 = 1 << 4;
/// Diagnostic flag: one TX consumption awaits cursor reconciliation.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_TX_COMMIT_PENDING: u32 = 1 << 5;
/// Diagnostic flag: the RX producer/consumer cursor pair sampled exactly.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_RX_RING_VALID: u32 = 1 << 6;
/// Diagnostic flag: the TX producer/consumer cursor pair sampled exactly.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_TX_RING_VALID: u32 = 1 << 7;
/// Diagnostic flag: the owner sampled GENET IRQ and DMA registers.
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_MMIO_SAMPLED: u32 = 1 << 8;
/// Complete flag set admitted by [`DirectGenetRuntimeDiagnostic`].
pub const DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAGS: u32 =
    DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_INITIALIZED
        | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_ACTIVE
        | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_FAULTED
        | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_IRQ_ACK_PENDING
        | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_RX_COMMIT_PENDING
        | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_TX_COMMIT_PENDING
        | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_RX_RING_VALID
        | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_TX_RING_VALID
        | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_MMIO_SAMPLED;
/// Generation field offset within each direct-link cursor state.
pub const DIRECT_GENET_CURSOR_GENERATION_OFFSET: usize = 8;
/// Monotonic cursor field offset within each direct-link cursor state.
pub const DIRECT_GENET_CURSOR_OFFSET: usize = 16;
/// Owner-state sequence field offset within each direct-link cursor state.
pub const DIRECT_GENET_CURSOR_STATE_SEQUENCE_OFFSET: usize = 24;
/// Live/poison flags offset within each direct-link cursor state.
pub const DIRECT_GENET_CURSOR_FLAGS_OFFSET: usize = 32;
/// Poison-reason offset within each direct-link cursor state.
pub const DIRECT_GENET_CURSOR_POISON_REASON_OFFSET: usize = 36;
/// Sequence-last commit offset within each direct-link cursor state.
pub const DIRECT_GENET_CURSOR_COMMIT_OFFSET: usize = 56;
/// Cursor state is live and may advance exactly once per packet.
pub const DIRECT_GENET_CURSOR_FLAG_LIVE: u32 = 1 << 0;
/// Cursor owner has permanently poisoned this generation.
pub const DIRECT_GENET_CURSOR_FLAG_POISONED: u32 = 1 << 1;
/// Exact recognized cursor flags.
pub const DIRECT_GENET_CURSOR_FLAGS: u32 =
    DIRECT_GENET_CURSOR_FLAG_LIVE | DIRECT_GENET_CURSOR_FLAG_POISONED;
/// Poison reason: the immutable control record was invalid.
pub const DIRECT_GENET_POISON_INVALID_CONTROL: u32 = 1;
/// Poison reason: a producer or consumer cursor was invalid.
pub const DIRECT_GENET_POISON_INVALID_CURSOR: u32 = 2;
/// Poison reason: a packet-slot header or body was invalid.
pub const DIRECT_GENET_POISON_INVALID_SLOT: u32 = 3;
/// Poison reason: one endpoint observed another generation.
pub const DIRECT_GENET_POISON_STALE_GENERATION: u32 = 4;
/// Poison reason: a monotonic sequence could not advance without wrapping.
pub const DIRECT_GENET_POISON_SEQUENCE_EXHAUSTED: u32 = 5;
/// Direct-link packet-slot magic (`CNGS`).
pub const DIRECT_GENET_SLOT_MAGIC: u32 = 0x434e_4753;
/// Direct-link packet-slot version.
pub const DIRECT_GENET_SLOT_VERSION: u16 = 1;
/// Exact packet-slot header bytes.
pub const DIRECT_GENET_SLOT_HEADER_BYTES: usize = 64;
/// Byte offset of the slot's sequence-last commit word.
pub const DIRECT_GENET_SLOT_COMMIT_OFFSET: usize = 56;
/// Byte offset of the copied Ethernet frame in one direct-link slot.
pub const DIRECT_GENET_SLOT_PAYLOAD_OFFSET: usize = DIRECT_GENET_SLOT_HEADER_BYTES;
/// Byte offset of the active frame length within one direct-link slot.
pub const DIRECT_GENET_SLOT_LENGTH_OFFSET: usize = 10;
/// Byte offset of the generation within one direct-link slot.
pub const DIRECT_GENET_SLOT_GENERATION_OFFSET: usize = 16;
/// Byte offset of the monotonic sequence within one direct-link slot.
pub const DIRECT_GENET_SLOT_SEQUENCE_OFFSET: usize = 24;

/// Direction of one Pi GENET direct-link SPSC ring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum DirectGenetDirection {
    /// GENET produces received frames and console-network consumes them.
    Rx = 1,
    /// Console-network produces frames and GENET consumes them.
    Tx = 2,
}

impl DirectGenetDirection {
    /// Exact slot population for this direction.
    #[must_use]
    pub const fn slot_count(self) -> usize {
        match self {
            Self::Rx => DIRECT_GENET_RX_SLOT_COUNT,
            Self::Tx => DIRECT_GENET_TX_SLOT_COUNT,
        }
    }
}

/// Single writer responsible for one direct-link cursor cache line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum DirectGenetCursorRole {
    /// Isolated GENET child produces RX slots.
    RxProducer = 1,
    /// Console-network child consumes RX slots.
    RxConsumer = 2,
    /// Console-network child produces TX slots.
    TxProducer = 3,
    /// Isolated GENET child consumes TX slots.
    TxConsumer = 4,
}

impl DirectGenetCursorRole {
    /// Exact cache-line offset in the shared control page.
    #[must_use]
    pub const fn offset(self) -> usize {
        match self {
            Self::RxProducer => DIRECT_GENET_RX_PRODUCER_STATE_OFFSET,
            Self::RxConsumer => DIRECT_GENET_RX_CONSUMER_STATE_OFFSET,
            Self::TxProducer => DIRECT_GENET_TX_PRODUCER_STATE_OFFSET,
            Self::TxConsumer => DIRECT_GENET_TX_CONSUMER_STATE_OFFSET,
        }
    }
}

/// Exact poison receipt from one direct-link cursor owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectGenetPoison {
    /// Owner that fenced the generation.
    pub role: DirectGenetCursorRole,
    /// Nonzero stable poison reason.
    pub reason: u32,
}

/// Fail-closed Pi GENET direct-link validation errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectGenetError {
    /// A control, cursor, or slot identity is not exact.
    InvalidIdentity,
    /// A fixed size, reserved field, or slot mapping is invalid.
    InvalidLayout,
    /// The requested generation is zero or stale.
    StaleGeneration,
    /// Producer/consumer cursors are torn, regressed, overfull, or non-monotonic.
    InvalidCursor,
    /// A slot sequence is zero, stale, skipped, or not committed sequence-last.
    InvalidSequence,
    /// A packet or poison reason violates its deterministic bound.
    InvalidBound,
    /// No committed packet is available to the consumer.
    Empty,
    /// No slot credit is available to the producer.
    Backpressure,
    /// A bounded observation raced a legitimate sequence-last state transition.
    /// Retry the same exact operation; this is not a poison receipt.
    StateChanged,
    /// One owner permanently fenced this exact generation.
    Poisoned(DirectGenetPoison),
}

/// Sequence-last, child-owned direct-GENET diagnostic snapshot.
///
/// This record lives in a cache-line-isolated region of the CPU-only control
/// page. It is refreshed only by the physical owner while processing exact
/// DGHO replay, or immediately before terminal direct-link fault transfer.
/// None of its fields authorize packet service, IRQ acknowledgement, retry, or
/// recovery.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectGenetRuntimeDiagnostic {
    /// [`DIRECT_GENET_RUNTIME_DIAGNOSTIC_MAGIC`].
    pub magic: u32,
    /// [`DIRECT_GENET_RUNTIME_DIAGNOSTIC_VERSION`].
    pub version: u16,
    /// Exact [`DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES`].
    pub len: u16,
    /// Bounded diagnostic state flags.
    pub flags: u32,
    /// Reserved; zero.
    pub reserved0: u32,
    /// Exact nonzero direct-link generation.
    pub generation: u64,
    /// Monotonic owner-local publication sequence.
    pub publication_sequence: u64,
    /// Configured GENET IRQ badge.
    pub irq_badge: u32,
    /// IRQ notification wakes observed by the runtime.
    pub irq_wakes: u32,
    /// Successful seL4 IRQ-handler acknowledgements.
    pub irq_acks: u32,
    /// Failed seL4 IRQ-handler acknowledgements.
    pub irq_ack_failures: u32,
    /// GENET source-unmask readback failures.
    pub irq_unmask_failures: u32,
    /// Bounded GENET DPC turns.
    pub dpc_turns: u32,
    /// DPC turns that retained work at the service bound.
    pub dpc_budget_hits: u32,
    /// Final unmask/source rechecks before IRQ acknowledgement.
    pub dpc_final_rechecks: u32,
    /// Last owned GENET interrupt status observed by an IRQ wake.
    pub dpc_last_status: u32,
    /// Current raw owned INTRL2 status bits.
    pub irq_raw: u32,
    /// Current owned INTRL2 mask bits.
    pub irq_mask: u32,
    /// Current raw-and-unmasked owned INTRL2 status bits.
    pub irq_active: u32,
    /// Current hardware RDMA producer index.
    pub rdma_producer: u16,
    /// Current hardware RDMA consumer index.
    pub rdma_consumer: u16,
    /// Current hardware TDMA producer index.
    pub tdma_producer: u16,
    /// Current hardware TDMA consumer index.
    pub tdma_consumer: u16,
    /// RX packets committed by the direct-link producer.
    pub direct_rx_packets: u32,
    /// TX packets committed by the direct-link consumer.
    pub direct_tx_packets: u32,
    /// Console-to-GENET direct notification wakes observed.
    pub peer_wakes: u32,
    /// GENET-to-console direct notifications sent.
    pub peer_signals: u32,
    /// Stable cursor-sample races observed by the GENET owner.
    pub state_changes: u32,
    /// Reserved; zero.
    pub reserved1: u32,
    /// RX producer cursor when the RX-ring-valid flag is set.
    pub rx_producer_cursor: u64,
    /// RX consumer cursor when the RX-ring-valid flag is set.
    pub rx_consumer_cursor: u64,
    /// TX producer cursor when the TX-ring-valid flag is set.
    pub tx_producer_cursor: u64,
    /// TX consumer cursor when the TX-ring-valid flag is set.
    pub tx_consumer_cursor: u64,
    /// RX-producer poison reason when available, otherwise zero.
    pub rx_producer_poison: u32,
    /// RX-consumer poison reason when available, otherwise zero.
    pub rx_consumer_poison: u32,
    /// TX-producer poison reason when available, otherwise zero.
    pub tx_producer_poison: u32,
    /// TX-consumer poison reason when available, otherwise zero.
    pub tx_consumer_poison: u32,
    /// Reserved; zero.
    pub reserved: [u64; 3],
    /// Sequence-last commit, equal to [`Self::publication_sequence`].
    pub committed_sequence: u64,
}

impl DirectGenetRuntimeDiagnostic {
    /// Empty, unpublished record.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: DIRECT_GENET_RUNTIME_DIAGNOSTIC_MAGIC,
            version: DIRECT_GENET_RUNTIME_DIAGNOSTIC_VERSION,
            len: DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES as u16,
            flags: 0,
            reserved0: 0,
            generation: 0,
            publication_sequence: 0,
            irq_badge: 0,
            irq_wakes: 0,
            irq_acks: 0,
            irq_ack_failures: 0,
            irq_unmask_failures: 0,
            dpc_turns: 0,
            dpc_budget_hits: 0,
            dpc_final_rechecks: 0,
            dpc_last_status: 0,
            irq_raw: 0,
            irq_mask: 0,
            irq_active: 0,
            rdma_producer: 0,
            rdma_consumer: 0,
            tdma_producer: 0,
            tdma_consumer: 0,
            direct_rx_packets: 0,
            direct_tx_packets: 0,
            peer_wakes: 0,
            peer_signals: 0,
            state_changes: 0,
            reserved1: 0,
            rx_producer_cursor: 0,
            rx_consumer_cursor: 0,
            tx_producer_cursor: 0,
            tx_consumer_cursor: 0,
            rx_producer_poison: 0,
            rx_consumer_poison: 0,
            tx_producer_poison: 0,
            tx_consumer_poison: 0,
            reserved: [0; 3],
            committed_sequence: 0,
        }
    }

    /// Whether this record is exact, stable, and bound to `generation`.
    #[must_use]
    pub const fn valid_for(self, generation: u64) -> bool {
        let rx_valid = self.flags & DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_RX_RING_VALID != 0;
        let tx_valid = self.flags & DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_TX_RING_VALID != 0;
        let active = self.flags & DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_ACTIVE != 0;
        let faulted = self.flags & DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_FAULTED != 0;
        self.magic == DIRECT_GENET_RUNTIME_DIAGNOSTIC_MAGIC
            && self.version == DIRECT_GENET_RUNTIME_DIAGNOSTIC_VERSION
            && self.len as usize == DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES
            && self.flags & !DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAGS == 0
            && self.reserved0 == 0
            && self.reserved1 == 0
            && self.reserved[0] == 0
            && self.reserved[1] == 0
            && self.reserved[2] == 0
            && generation != 0
            && self.generation == generation
            && self.publication_sequence != 0
            && self.committed_sequence == self.publication_sequence
            && !(active && faulted)
            && self.irq_active == self.irq_raw & !self.irq_mask
            && (rx_valid || (self.rx_producer_cursor == 0 && self.rx_consumer_cursor == 0))
            && (tx_valid || (self.tx_producer_cursor == 0 && self.tx_consumer_cursor == 0))
            && (!rx_valid || (self.rx_producer_poison == 0 && self.rx_consumer_poison == 0))
            && (!tx_valid || (self.tx_producer_poison == 0 && self.tx_consumer_poison == 0))
            && self.rx_producer_poison <= DIRECT_GENET_POISON_SEQUENCE_EXHAUSTED
            && self.rx_consumer_poison <= DIRECT_GENET_POISON_SEQUENCE_EXHAUSTED
            && self.tx_producer_poison <= DIRECT_GENET_POISON_SEQUENCE_EXHAUSTED
            && self.tx_consumer_poison <= DIRECT_GENET_POISON_SEQUENCE_EXHAUSTED
    }

    /// Encode the complete record; callers still publish its commit word last.
    pub fn encode(&self, output: &mut [u8]) -> Result<(), DirectGenetError> {
        if output.len() != DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES || !self.valid_for(self.generation)
        {
            return Err(DirectGenetError::InvalidLayout);
        }
        output.fill(0);
        output[0..4].copy_from_slice(&self.magic.to_le_bytes());
        output[4..6].copy_from_slice(&self.version.to_le_bytes());
        output[6..8].copy_from_slice(&self.len.to_le_bytes());
        output[8..12].copy_from_slice(&self.flags.to_le_bytes());
        output[12..16].copy_from_slice(&self.reserved0.to_le_bytes());
        output[16..24].copy_from_slice(&self.generation.to_le_bytes());
        output[24..32].copy_from_slice(&self.publication_sequence.to_le_bytes());
        output[32..36].copy_from_slice(&self.irq_badge.to_le_bytes());
        output[36..40].copy_from_slice(&self.irq_wakes.to_le_bytes());
        output[40..44].copy_from_slice(&self.irq_acks.to_le_bytes());
        output[44..48].copy_from_slice(&self.irq_ack_failures.to_le_bytes());
        output[48..52].copy_from_slice(&self.irq_unmask_failures.to_le_bytes());
        output[52..56].copy_from_slice(&self.dpc_turns.to_le_bytes());
        output[56..60].copy_from_slice(&self.dpc_budget_hits.to_le_bytes());
        output[60..64].copy_from_slice(&self.dpc_final_rechecks.to_le_bytes());
        output[64..68].copy_from_slice(&self.dpc_last_status.to_le_bytes());
        output[68..72].copy_from_slice(&self.irq_raw.to_le_bytes());
        output[72..76].copy_from_slice(&self.irq_mask.to_le_bytes());
        output[76..80].copy_from_slice(&self.irq_active.to_le_bytes());
        output[80..82].copy_from_slice(&self.rdma_producer.to_le_bytes());
        output[82..84].copy_from_slice(&self.rdma_consumer.to_le_bytes());
        output[84..86].copy_from_slice(&self.tdma_producer.to_le_bytes());
        output[86..88].copy_from_slice(&self.tdma_consumer.to_le_bytes());
        output[88..92].copy_from_slice(&self.direct_rx_packets.to_le_bytes());
        output[92..96].copy_from_slice(&self.direct_tx_packets.to_le_bytes());
        output[96..100].copy_from_slice(&self.peer_wakes.to_le_bytes());
        output[100..104].copy_from_slice(&self.peer_signals.to_le_bytes());
        output[104..108].copy_from_slice(&self.state_changes.to_le_bytes());
        output[108..112].copy_from_slice(&self.reserved1.to_le_bytes());
        output[112..120].copy_from_slice(&self.rx_producer_cursor.to_le_bytes());
        output[120..128].copy_from_slice(&self.rx_consumer_cursor.to_le_bytes());
        output[128..136].copy_from_slice(&self.tx_producer_cursor.to_le_bytes());
        output[136..144].copy_from_slice(&self.tx_consumer_cursor.to_le_bytes());
        output[144..148].copy_from_slice(&self.rx_producer_poison.to_le_bytes());
        output[148..152].copy_from_slice(&self.rx_consumer_poison.to_le_bytes());
        output[152..156].copy_from_slice(&self.tx_producer_poison.to_le_bytes());
        output[156..160].copy_from_slice(&self.tx_consumer_poison.to_le_bytes());
        encode_u64_array(output, 160, &self.reserved);
        output[DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET
            ..DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET + 8]
            .copy_from_slice(&self.committed_sequence.to_le_bytes());
        Ok(())
    }

    /// Decode a previously acquired stable record.
    pub fn decode(input: &[u8], generation: u64) -> Result<Self, DirectGenetError> {
        if input.len() != DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES {
            return Err(DirectGenetError::InvalidLayout);
        }
        let record = Self {
            magic: read_u32(input, 0),
            version: read_u16(input, 4),
            len: read_u16(input, 6),
            flags: read_u32(input, 8),
            reserved0: read_u32(input, 12),
            generation: read_u64(input, 16),
            publication_sequence: read_u64(input, 24),
            irq_badge: read_u32(input, 32),
            irq_wakes: read_u32(input, 36),
            irq_acks: read_u32(input, 40),
            irq_ack_failures: read_u32(input, 44),
            irq_unmask_failures: read_u32(input, 48),
            dpc_turns: read_u32(input, 52),
            dpc_budget_hits: read_u32(input, 56),
            dpc_final_rechecks: read_u32(input, 60),
            dpc_last_status: read_u32(input, 64),
            irq_raw: read_u32(input, 68),
            irq_mask: read_u32(input, 72),
            irq_active: read_u32(input, 76),
            rdma_producer: read_u16(input, 80),
            rdma_consumer: read_u16(input, 82),
            tdma_producer: read_u16(input, 84),
            tdma_consumer: read_u16(input, 86),
            direct_rx_packets: read_u32(input, 88),
            direct_tx_packets: read_u32(input, 92),
            peer_wakes: read_u32(input, 96),
            peer_signals: read_u32(input, 100),
            state_changes: read_u32(input, 104),
            reserved1: read_u32(input, 108),
            rx_producer_cursor: read_u64(input, 112),
            rx_consumer_cursor: read_u64(input, 120),
            tx_producer_cursor: read_u64(input, 128),
            tx_consumer_cursor: read_u64(input, 136),
            rx_producer_poison: read_u32(input, 144),
            rx_consumer_poison: read_u32(input, 148),
            tx_producer_poison: read_u32(input, 152),
            tx_consumer_poison: read_u32(input, 156),
            reserved: decode_u64_array(input, 160),
            committed_sequence: read_u64(input, DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET),
        };
        if record.valid_for(generation) {
            Ok(record)
        } else {
            Err(DirectGenetError::InvalidLayout)
        }
    }
}

/// Immutable, sealed header at the front of the direct-link control page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C, align(8))]
pub struct DirectGenetControlHeader {
    /// [`DIRECT_GENET_CONTROL_MAGIC`].
    pub magic: u32,
    /// [`DIRECT_GENET_CONTROL_VERSION`].
    pub version: u16,
    /// Exact [`DIRECT_GENET_CONTROL_HEADER_BYTES`].
    pub header_bytes: u16,
    /// Nonzero direct-link generation.
    pub generation: u64,
    /// Exact [`SHARED_PAGE_BYTES`].
    pub shared_page_bytes: u16,
    /// Exact [`DIRECT_GENET_RX_SLOT_COUNT`].
    pub rx_slot_count: u8,
    /// Exact [`DIRECT_GENET_TX_SLOT_COUNT`].
    pub tx_slot_count: u8,
    /// Exact [`DIRECT_GENET_CURSOR_STATE_BYTES`].
    pub cursor_state_bytes: u16,
    /// Exact [`DIRECT_GENET_CURSOR_STATE_COUNT`].
    pub cursor_state_count: u16,
    /// Exact [`DIRECT_GENET_CONTROL_FLAGS`].
    pub flags: u32,
    /// Reserved; zero.
    pub reserved0: u32,
    /// Reserved; zero.
    pub reserved: [u64; 3],
    /// FNV-1a seal over every preceding field.
    pub seal: u64,
}

impl DirectGenetControlHeader {
    const fn new(generation: u64) -> Self {
        Self {
            magic: DIRECT_GENET_CONTROL_MAGIC,
            version: DIRECT_GENET_CONTROL_VERSION,
            header_bytes: DIRECT_GENET_CONTROL_HEADER_BYTES as u16,
            generation,
            shared_page_bytes: SHARED_PAGE_BYTES as u16,
            rx_slot_count: DIRECT_GENET_RX_SLOT_COUNT as u8,
            tx_slot_count: DIRECT_GENET_TX_SLOT_COUNT as u8,
            cursor_state_bytes: DIRECT_GENET_CURSOR_STATE_BYTES as u16,
            cursor_state_count: DIRECT_GENET_CURSOR_STATE_COUNT as u16,
            flags: DIRECT_GENET_CONTROL_FLAGS,
            reserved0: 0,
            reserved: [0; 3],
            seal: 0,
        }
        .sealed()
    }

    const fn sealed(mut self) -> Self {
        self.seal = self.expected_seal();
        self
    }

    fn validate_for(self, generation: u64) -> Result<(), DirectGenetError> {
        if self.magic != DIRECT_GENET_CONTROL_MAGIC || self.version != DIRECT_GENET_CONTROL_VERSION
        {
            return Err(DirectGenetError::InvalidIdentity);
        }
        if self.header_bytes as usize != DIRECT_GENET_CONTROL_HEADER_BYTES
            || self.shared_page_bytes as usize != SHARED_PAGE_BYTES
            || self.rx_slot_count as usize != DIRECT_GENET_RX_SLOT_COUNT
            || self.tx_slot_count as usize != DIRECT_GENET_TX_SLOT_COUNT
            || self.cursor_state_bytes as usize != DIRECT_GENET_CURSOR_STATE_BYTES
            || self.cursor_state_count as usize != DIRECT_GENET_CURSOR_STATE_COUNT
            || self.flags != DIRECT_GENET_CONTROL_FLAGS
            || self.reserved0 != 0
            || self.reserved != [0; 3]
        {
            return Err(DirectGenetError::InvalidLayout);
        }
        if generation == 0 || self.generation != generation {
            return Err(DirectGenetError::StaleGeneration);
        }
        if self.seal == 0 || self.seal != self.expected_seal() {
            return Err(DirectGenetError::InvalidLayout);
        }
        Ok(())
    }

    const fn expected_seal(self) -> u64 {
        let mut hash = FNV64_OFFSET;
        hash = hash_u32(hash, self.magic);
        hash = hash_u16(hash, self.version);
        hash = hash_u16(hash, self.header_bytes);
        hash = hash_u64(hash, self.generation);
        hash = hash_u16(hash, self.shared_page_bytes);
        hash = hash_byte(hash, self.rx_slot_count);
        hash = hash_byte(hash, self.tx_slot_count);
        hash = hash_u16(hash, self.cursor_state_bytes);
        hash = hash_u16(hash, self.cursor_state_count);
        hash = hash_u32(hash, self.flags);
        hash = hash_u32(hash, self.reserved0);
        hash_u64_slice(hash, &self.reserved)
    }
}

/// One cache-line-isolated, single-writer cursor publication.
///
/// `state_sequence` starts at one and advances exactly once with each cursor
/// increment. Poisoning advances it once without moving the cursor. The owner
/// clears `committed_sequence`, stages the complete body, performs a release
/// operation, and writes `committed_sequence` last. Peers acquire the commit
/// before reading and accept only the exact live relation
/// `state_sequence == cursor + 1`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C, align(64))]
pub struct DirectGenetCursorState {
    /// [`DIRECT_GENET_CURSOR_MAGIC`].
    pub magic: u32,
    /// [`DIRECT_GENET_CURSOR_VERSION`].
    pub version: u16,
    /// Raw [`DirectGenetCursorRole`].
    pub role: u16,
    /// Nonzero direct-link generation.
    pub generation: u64,
    /// Owner-written, non-wrapping packet cursor.
    pub cursor: u64,
    /// Owner state publication sequence.
    pub state_sequence: u64,
    /// Exactly one of live or poisoned.
    pub flags: u32,
    /// Stable nonzero reason only when poisoned.
    pub poison_reason: u32,
    /// Reserved; zero.
    pub reserved: [u8; 16],
    /// Sequence-last repetition of `state_sequence`.
    pub committed_sequence: u64,
}

impl DirectGenetCursorState {
    const fn initial(generation: u64, role: DirectGenetCursorRole) -> Self {
        Self {
            magic: DIRECT_GENET_CURSOR_MAGIC,
            version: DIRECT_GENET_CURSOR_VERSION,
            role: role as u16,
            generation,
            cursor: 0,
            state_sequence: 1,
            flags: DIRECT_GENET_CURSOR_FLAG_LIVE,
            poison_reason: 0,
            reserved: [0; 16],
            committed_sequence: 1,
        }
    }

    fn validate_shape(
        self,
        generation: u64,
        role: DirectGenetCursorRole,
    ) -> Result<(), DirectGenetError> {
        if self.magic != DIRECT_GENET_CURSOR_MAGIC
            || self.version != DIRECT_GENET_CURSOR_VERSION
            || self.role != role as u16
        {
            return Err(DirectGenetError::InvalidIdentity);
        }
        if generation == 0 || self.generation != generation {
            return Err(DirectGenetError::StaleGeneration);
        }
        if self.state_sequence == 0
            || self.committed_sequence != self.state_sequence
            || self.flags & !DIRECT_GENET_CURSOR_FLAGS != 0
            || !bytes_zero(&self.reserved)
        {
            return Err(DirectGenetError::InvalidCursor);
        }
        if self.flags == DIRECT_GENET_CURSOR_FLAG_LIVE {
            if self.poison_reason != 0
                || self.cursor.checked_add(1) != Some(self.state_sequence)
                || self.state_sequence == u64::MAX
            {
                return Err(DirectGenetError::InvalidCursor);
            }
        } else if self.flags == DIRECT_GENET_CURSOR_FLAG_POISONED {
            if self.poison_reason == 0 || self.cursor.checked_add(2) != Some(self.state_sequence) {
                return Err(DirectGenetError::InvalidCursor);
            }
        } else {
            return Err(DirectGenetError::InvalidCursor);
        }
        Ok(())
    }

    fn validate_live(
        self,
        generation: u64,
        role: DirectGenetCursorRole,
    ) -> Result<(), DirectGenetError> {
        self.validate_shape(generation, role)?;
        if self.flags == DIRECT_GENET_CURSOR_FLAG_POISONED {
            return Err(DirectGenetError::Poisoned(DirectGenetPoison {
                role,
                reason: self.poison_reason,
            }));
        }
        Ok(())
    }

    fn advanced(self) -> Result<Self, DirectGenetError> {
        let Some(cursor) = self.cursor.checked_add(1) else {
            return Err(DirectGenetError::InvalidCursor);
        };
        let Some(state_sequence) = self.state_sequence.checked_add(1) else {
            return Err(DirectGenetError::InvalidCursor);
        };
        if state_sequence == u64::MAX {
            return Err(DirectGenetError::InvalidCursor);
        }
        Ok(Self {
            cursor,
            state_sequence,
            committed_sequence: state_sequence,
            ..self
        })
    }

    fn poisoned(self, reason: u32) -> Result<Self, DirectGenetError> {
        if reason == 0 {
            return Err(DirectGenetError::InvalidBound);
        }
        let Some(state_sequence) = self.state_sequence.checked_add(1) else {
            return Err(DirectGenetError::InvalidCursor);
        };
        Ok(Self {
            state_sequence,
            flags: DIRECT_GENET_CURSOR_FLAG_POISONED,
            poison_reason: reason,
            committed_sequence: state_sequence,
            ..self
        })
    }
}

/// Stable live cursor state for one direct-link direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectGenetRingSnapshot {
    /// Ring direction.
    pub direction: DirectGenetDirection,
    /// Exact direct-link generation.
    pub generation: u64,
    /// Fixed ring capacity.
    pub capacity: u64,
    /// Producer's committed non-wrapping cursor.
    pub producer_cursor: u64,
    /// Consumer's committed non-wrapping cursor.
    pub consumer_cursor: u64,
    /// Producer's committed owner-state sequence.
    pub producer_state_sequence: u64,
    /// Consumer's committed owner-state sequence.
    pub consumer_state_sequence: u64,
}

impl DirectGenetRingSnapshot {
    /// Return the exact committed occupancy.
    #[must_use]
    pub const fn occupancy(self) -> u64 {
        self.producer_cursor - self.consumer_cursor
    }

    /// Return the exact next producer cursor and slot index.
    pub const fn next_producer(self) -> Result<(u64, usize), DirectGenetError> {
        if self.occupancy() == self.capacity {
            return Err(DirectGenetError::Backpressure);
        }
        let Some(sequence) = self.producer_cursor.checked_add(1) else {
            return Err(DirectGenetError::InvalidCursor);
        };
        match direct_genet_slot_index(self.direction, sequence) {
            Ok(index) => Ok((sequence, index)),
            Err(error) => Err(error),
        }
    }

    /// Return the exact next consumer cursor and slot index.
    pub const fn next_consumer(self) -> Result<(u64, usize), DirectGenetError> {
        if self.occupancy() == 0 {
            return Err(DirectGenetError::Empty);
        }
        let Some(sequence) = self.consumer_cursor.checked_add(1) else {
            return Err(DirectGenetError::InvalidCursor);
        };
        match direct_genet_slot_index(self.direction, sequence) {
            Ok(index) => Ok((sequence, index)),
            Err(error) => Err(error),
        }
    }

    /// Reconcile a producer commit against a live snapshot acquired after the
    /// owner published its cursor cache line.
    ///
    /// Runtimes stage the owned cursor through aligned atomic shared-memory
    /// accesses, publish its commit with release ordering, acquire a fresh
    /// complete control snapshot, then call this method. The resulting
    /// notification decision closes the consumer-drained-through-the-old-
    /// cursor lost-wake race.
    pub fn reconcile_producer_commit(
        self,
        final_state: Self,
        sequence: u64,
    ) -> Result<DirectGenetProducerCommit, DirectGenetError> {
        self.validate_producer_commit(final_state, sequence)
    }

    /// Reconcile a consumer commit against a live snapshot acquired after the
    /// owner published its cursor cache line.
    ///
    /// The returned rearm and work flags are authoritative only after this
    /// post-release peer recheck; computing them from a private pre-commit copy
    /// could lose a concurrent producer transition.
    pub fn reconcile_consumer_commit(
        self,
        final_state: Self,
        sequence: u64,
    ) -> Result<DirectGenetConsumerCommit, DirectGenetError> {
        self.validate_consumer_commit(final_state, sequence)
    }

    fn validate_identity(self, other: Self) -> Result<(), DirectGenetError> {
        if self.generation != other.generation {
            return Err(DirectGenetError::StaleGeneration);
        }
        if self.direction != other.direction || self.capacity != other.capacity {
            return Err(DirectGenetError::InvalidLayout);
        }
        Ok(())
    }

    fn validate_peer_progress(
        initial_cursor: u64,
        initial_state_sequence: u64,
        final_cursor: u64,
        final_state_sequence: u64,
    ) -> Result<(), DirectGenetError> {
        let Some(cursor_delta) = final_cursor.checked_sub(initial_cursor) else {
            return Err(DirectGenetError::InvalidCursor);
        };
        let Some(state_delta) = final_state_sequence.checked_sub(initial_state_sequence) else {
            return Err(DirectGenetError::InvalidCursor);
        };
        if cursor_delta != state_delta {
            return Err(DirectGenetError::InvalidCursor);
        }
        Ok(())
    }

    fn validate_producer_commit(
        self,
        final_state: Self,
        sequence: u64,
    ) -> Result<DirectGenetProducerCommit, DirectGenetError> {
        self.validate_identity(final_state)?;
        let (expected_sequence, _) = self.next_producer()?;
        let expected_state_sequence = self
            .producer_state_sequence
            .checked_add(1)
            .ok_or(DirectGenetError::InvalidCursor)?;
        if sequence != expected_sequence
            || final_state.producer_cursor != sequence
            || final_state.producer_state_sequence != expected_state_sequence
        {
            return Err(DirectGenetError::InvalidCursor);
        }
        Self::validate_peer_progress(
            self.consumer_cursor,
            self.consumer_state_sequence,
            final_state.consumer_cursor,
            final_state.consumer_state_sequence,
        )?;
        Ok(DirectGenetProducerCommit {
            sequence,
            slot_index: direct_genet_slot_index(self.direction, sequence)?,
            data_notification_due: self.occupancy() == 0
                || final_state.consumer_cursor == self.producer_cursor,
        })
    }

    fn validate_consumer_commit(
        self,
        final_state: Self,
        sequence: u64,
    ) -> Result<DirectGenetConsumerCommit, DirectGenetError> {
        self.validate_identity(final_state)?;
        let (expected_sequence, _) = self.next_consumer()?;
        let expected_state_sequence = self
            .consumer_state_sequence
            .checked_add(1)
            .ok_or(DirectGenetError::InvalidCursor)?;
        if sequence != expected_sequence
            || final_state.consumer_cursor != sequence
            || final_state.consumer_state_sequence != expected_state_sequence
        {
            return Err(DirectGenetError::InvalidCursor);
        }
        Self::validate_peer_progress(
            self.producer_cursor,
            self.producer_state_sequence,
            final_state.producer_cursor,
            final_state.producer_state_sequence,
        )?;
        let producer_distance = final_state
            .producer_cursor
            .checked_sub(self.consumer_cursor)
            .ok_or(DirectGenetError::InvalidCursor)?;
        Ok(DirectGenetConsumerCommit {
            sequence,
            slot_index: direct_genet_slot_index(self.direction, sequence)?,
            producer_rearm_due: self.occupancy() == self.capacity
                || producer_distance == self.capacity,
            work_remaining: final_state.producer_cursor > sequence,
        })
    }
}

/// Result of one fully rechecked producer cursor commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectGenetProducerCommit {
    /// Exact committed packet sequence.
    pub sequence: u64,
    /// Slot carrying this sequence.
    pub slot_index: usize,
    /// Whether the producer must send the coalescing data notification.
    pub data_notification_due: bool,
}

/// Result of one fully rechecked consumer cursor commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectGenetConsumerCommit {
    /// Exact consumed packet sequence.
    pub sequence: u64,
    /// Slot carrying this sequence.
    pub slot_index: usize,
    /// Whether the consumer must rearm a producer that may have observed full.
    pub producer_rearm_due: bool,
    /// Whether the final producer recheck proves more committed work exists.
    pub work_remaining: bool,
}

/// Fixed sequence-last header at the front of each direct-link packet slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C, align(8))]
pub struct DirectGenetSlotHeader {
    /// [`DIRECT_GENET_SLOT_MAGIC`].
    pub magic: u32,
    /// [`DIRECT_GENET_SLOT_VERSION`].
    pub version: u16,
    /// Raw [`DirectGenetDirection`].
    pub direction: u16,
    /// Exact [`DIRECT_GENET_SLOT_HEADER_BYTES`].
    pub header_bytes: u16,
    /// Initialized Ethernet frame bytes.
    pub frame_len: u16,
    /// Reserved; zero.
    pub flags: u32,
    /// Nonzero direct-link generation.
    pub generation: u64,
    /// Exact non-wrapping ring cursor for this packet.
    pub sequence: u64,
    /// Reserved; zero.
    pub reserved: [u64; 3],
    /// Sequence-last repetition of `sequence`.
    pub committed_sequence: u64,
}

impl DirectGenetSlotHeader {
    fn staged(
        direction: DirectGenetDirection,
        generation: u64,
        sequence: u64,
        frame_len: usize,
    ) -> Result<Self, DirectGenetError> {
        if generation == 0 {
            return Err(DirectGenetError::StaleGeneration);
        }
        if sequence == 0 {
            return Err(DirectGenetError::InvalidSequence);
        }
        if frame_len == 0 || frame_len > ETHERNET_FRAME_BYTES {
            return Err(DirectGenetError::InvalidBound);
        }
        Ok(Self {
            magic: DIRECT_GENET_SLOT_MAGIC,
            version: DIRECT_GENET_SLOT_VERSION,
            direction: direction as u16,
            header_bytes: DIRECT_GENET_SLOT_HEADER_BYTES as u16,
            frame_len: frame_len as u16,
            flags: 0,
            generation,
            sequence,
            reserved: [0; 3],
            committed_sequence: 0,
        })
    }

    fn validate(
        self,
        direction: DirectGenetDirection,
        generation: u64,
        expected_sequence: u64,
    ) -> Result<usize, DirectGenetError> {
        if self.magic != DIRECT_GENET_SLOT_MAGIC
            || self.version != DIRECT_GENET_SLOT_VERSION
            || self.direction != direction as u16
        {
            return Err(DirectGenetError::InvalidIdentity);
        }
        if self.header_bytes as usize != DIRECT_GENET_SLOT_HEADER_BYTES
            || self.flags != 0
            || self.reserved != [0; 3]
        {
            return Err(DirectGenetError::InvalidLayout);
        }
        if generation == 0 || self.generation != generation {
            return Err(DirectGenetError::StaleGeneration);
        }
        if expected_sequence == 0
            || self.sequence != expected_sequence
            || self.committed_sequence != self.sequence
        {
            return Err(DirectGenetError::InvalidSequence);
        }
        let frame_len = self.frame_len as usize;
        if frame_len == 0 || frame_len > ETHERNET_FRAME_BYTES {
            return Err(DirectGenetError::InvalidBound);
        }
        Ok(frame_len)
    }
}

/// Private stable copy of one direct-link packet publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectGenetSlotRecord {
    direction: DirectGenetDirection,
    sequence: u64,
    frame_len: u16,
    frame: [u8; ETHERNET_FRAME_BYTES],
}

impl DirectGenetSlotRecord {
    /// Ring direction that owns this record.
    #[must_use]
    pub const fn direction(&self) -> DirectGenetDirection {
        self.direction
    }

    /// Exact monotonic cursor carried by this record.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Initialized Ethernet frame bytes.
    #[must_use]
    pub fn frame(&self) -> &[u8] {
        &self.frame[..self.frame_len as usize]
    }
}

/// Byte-level helpers for the CPU-only direct-link control page.
pub struct DirectGenetControlPage;

impl DirectGenetControlPage {
    /// Scrub and initialize the complete control page before either child runs.
    pub fn initialize_into(output: &mut [u8], generation: u64) -> Result<(), DirectGenetError> {
        if output.len() != SHARED_PAGE_BYTES {
            return Err(DirectGenetError::InvalidLayout);
        }
        if generation == 0 {
            return Err(DirectGenetError::StaleGeneration);
        }
        output.fill(0);
        encode_direct_genet_control_header(
            &mut output[..DIRECT_GENET_CONTROL_HEADER_BYTES],
            DirectGenetControlHeader::new(generation),
        );
        for role in [
            DirectGenetCursorRole::RxProducer,
            DirectGenetCursorRole::RxConsumer,
            DirectGenetCursorRole::TxProducer,
            DirectGenetCursorRole::TxConsumer,
        ] {
            let offset = role.offset();
            encode_direct_genet_cursor_state(
                &mut output[offset..offset + DIRECT_GENET_CURSOR_STATE_BYTES],
                DirectGenetCursorState::initial(generation, role),
            )?;
        }
        Ok(())
    }

    /// Acquire one stable live producer/consumer snapshot.
    pub fn snapshot(
        input: &[u8],
        generation: u64,
        direction: DirectGenetDirection,
    ) -> Result<DirectGenetRingSnapshot, DirectGenetError> {
        if input.len() != SHARED_PAGE_BYTES {
            return Err(DirectGenetError::InvalidLayout);
        }
        let header = decode_direct_genet_control_header(input)?;
        header.validate_for(generation)?;
        let (producer_role, consumer_role) = match direction {
            DirectGenetDirection::Rx => (
                DirectGenetCursorRole::RxProducer,
                DirectGenetCursorRole::RxConsumer,
            ),
            DirectGenetDirection::Tx => (
                DirectGenetCursorRole::TxProducer,
                DirectGenetCursorRole::TxConsumer,
            ),
        };
        let producer = decode_direct_genet_cursor_state(input, generation, producer_role)?;
        let consumer = decode_direct_genet_cursor_state(input, generation, consumer_role)?;
        let Some(occupancy) = producer.cursor.checked_sub(consumer.cursor) else {
            return Err(DirectGenetError::InvalidCursor);
        };
        let capacity = direction.slot_count() as u64;
        if occupancy > capacity {
            return Err(DirectGenetError::InvalidCursor);
        }
        fence(Ordering::Acquire);
        if read_u64(input, 8) != generation
            || read_u64(input, 56) != header.seal
            || read_u64(input, producer_role.offset() + 56) != producer.committed_sequence
            || read_u64(input, consumer_role.offset() + 56) != consumer.committed_sequence
        {
            return Err(DirectGenetError::StateChanged);
        }
        Ok(DirectGenetRingSnapshot {
            direction,
            generation,
            capacity,
            producer_cursor: producer.cursor,
            consumer_cursor: consumer.cursor,
            producer_state_sequence: producer.state_sequence,
            consumer_state_sequence: consumer.state_sequence,
        })
    }

    /// Acquire one exact live owner cursor state.
    pub fn cursor_state(
        input: &[u8],
        generation: u64,
        role: DirectGenetCursorRole,
    ) -> Result<DirectGenetCursorState, DirectGenetError> {
        let header = decode_direct_genet_control_header(input)?;
        header.validate_for(generation)?;
        decode_direct_genet_cursor_state(input, generation, role)
    }

    /// Commit the producer cursor after the corresponding slot is committed.
    ///
    /// The returned result is based on a final generation/state recheck and
    /// closes the empty-to-nonempty lost-wake race. Callers signal only when
    /// `data_notification_due` is true.
    pub fn commit_producer(
        page: &mut [u8],
        initial: DirectGenetRingSnapshot,
    ) -> Result<DirectGenetProducerCommit, DirectGenetError> {
        let (sequence, _) = initial.next_producer()?;
        let expected_state_sequence = initial
            .producer_state_sequence
            .checked_add(1)
            .ok_or(DirectGenetError::InvalidCursor)?;
        let current = Self::snapshot(page, initial.generation, initial.direction)?;
        initial.validate_identity(current)?;
        if current.producer_cursor == sequence
            && current.producer_state_sequence == expected_state_sequence
        {
            return initial.validate_producer_commit(current, sequence);
        }
        if current.producer_cursor != initial.producer_cursor
            || current.producer_state_sequence != initial.producer_state_sequence
        {
            return Err(DirectGenetError::StateChanged);
        }
        DirectGenetRingSnapshot::validate_peer_progress(
            initial.consumer_cursor,
            initial.consumer_state_sequence,
            current.consumer_cursor,
            current.consumer_state_sequence,
        )?;
        let role = match initial.direction {
            DirectGenetDirection::Rx => DirectGenetCursorRole::RxProducer,
            DirectGenetDirection::Tx => DirectGenetCursorRole::TxProducer,
        };
        let state = decode_direct_genet_cursor_state(page, initial.generation, role)?;
        if state.state_sequence != initial.producer_state_sequence {
            return Err(DirectGenetError::StateChanged);
        }
        write_direct_genet_cursor_state(page, state.advanced()?)?;
        let final_state = Self::snapshot(page, initial.generation, initial.direction)?;
        initial.validate_producer_commit(final_state, sequence)
    }

    /// Commit the consumer cursor after copying and validating one exact slot.
    ///
    /// The returned result is based on a final generation/state recheck and
    /// closes both the full-to-not-full producer rearm race and the
    /// enqueue-before-sleep consumer race.
    pub fn commit_consumer(
        page: &mut [u8],
        initial: DirectGenetRingSnapshot,
    ) -> Result<DirectGenetConsumerCommit, DirectGenetError> {
        let (sequence, _) = initial.next_consumer()?;
        let expected_state_sequence = initial
            .consumer_state_sequence
            .checked_add(1)
            .ok_or(DirectGenetError::InvalidCursor)?;
        let current = Self::snapshot(page, initial.generation, initial.direction)?;
        initial.validate_identity(current)?;
        if current.consumer_cursor == sequence
            && current.consumer_state_sequence == expected_state_sequence
        {
            return initial.validate_consumer_commit(current, sequence);
        }
        if current.consumer_cursor != initial.consumer_cursor
            || current.consumer_state_sequence != initial.consumer_state_sequence
        {
            return Err(DirectGenetError::StateChanged);
        }
        DirectGenetRingSnapshot::validate_peer_progress(
            initial.producer_cursor,
            initial.producer_state_sequence,
            current.producer_cursor,
            current.producer_state_sequence,
        )?;
        let role = match initial.direction {
            DirectGenetDirection::Rx => DirectGenetCursorRole::RxConsumer,
            DirectGenetDirection::Tx => DirectGenetCursorRole::TxConsumer,
        };
        let state = decode_direct_genet_cursor_state(page, initial.generation, role)?;
        if state.state_sequence != initial.consumer_state_sequence {
            return Err(DirectGenetError::StateChanged);
        }
        write_direct_genet_cursor_state(page, state.advanced()?)?;
        let final_state = Self::snapshot(page, initial.generation, initial.direction)?;
        initial.validate_consumer_commit(final_state, sequence)
    }

    /// Permanently poison one owner state in the current generation.
    ///
    /// A peer that observes the committed poison receipt returns
    /// [`DirectGenetError::Poisoned`] and must contain or pair-restart the link;
    /// it may never skip the cursor or fall back to another physical issuer.
    pub fn poison_owner(
        page: &mut [u8],
        generation: u64,
        role: DirectGenetCursorRole,
        expected_state_sequence: u64,
        reason: u32,
    ) -> Result<DirectGenetPoison, DirectGenetError> {
        let state = decode_direct_genet_cursor_state(page, generation, role)?;
        if state.state_sequence != expected_state_sequence {
            return Err(DirectGenetError::StateChanged);
        }
        let poisoned = state.poisoned(reason)?;
        write_direct_genet_cursor_state(page, poisoned)?;
        let stable = decode_direct_genet_cursor_state_allow_poison(page, generation, role)?;
        if stable != poisoned {
            return Err(DirectGenetError::StateChanged);
        }
        Ok(DirectGenetPoison { role, reason })
    }
}

/// Byte-level helpers for one CPU-only direct-link packet slot.
pub struct DirectGenetSlotPage;

impl DirectGenetSlotPage {
    /// Scrub one reused slot page before either direct-link endpoint runs.
    pub fn initialize_into(output: &mut [u8]) -> Result<(), DirectGenetError> {
        if output.len() != SHARED_PAGE_BYTES {
            return Err(DirectGenetError::InvalidLayout);
        }
        output.fill(0);
        Ok(())
    }

    /// Publish exactly the cursor following `after_cursor` sequence-last.
    pub fn publish_next_into(
        output: &mut [u8],
        direction: DirectGenetDirection,
        generation: u64,
        after_cursor: u64,
        frame: &[u8],
    ) -> Result<u64, DirectGenetError> {
        if output.len() != SHARED_PAGE_BYTES {
            return Err(DirectGenetError::InvalidLayout);
        }
        let Some(sequence) = after_cursor.checked_add(1) else {
            return Err(DirectGenetError::InvalidSequence);
        };
        let header = DirectGenetSlotHeader::staged(direction, generation, sequence, frame.len())?;
        output[DIRECT_GENET_SLOT_COMMIT_OFFSET..DIRECT_GENET_SLOT_COMMIT_OFFSET + 8]
            .copy_from_slice(&0u64.to_le_bytes());
        fence(Ordering::Release);
        output[0..4].copy_from_slice(&header.magic.to_le_bytes());
        output[4..6].copy_from_slice(&header.version.to_le_bytes());
        output[6..8].copy_from_slice(&header.direction.to_le_bytes());
        output[8..10].copy_from_slice(&header.header_bytes.to_le_bytes());
        output[10..12].copy_from_slice(&header.frame_len.to_le_bytes());
        output[12..16].copy_from_slice(&header.flags.to_le_bytes());
        output[16..24].copy_from_slice(&header.generation.to_le_bytes());
        output[24..32].copy_from_slice(&header.sequence.to_le_bytes());
        encode_u64_array(output, 32, &header.reserved);
        output[DIRECT_GENET_SLOT_PAYLOAD_OFFSET..DIRECT_GENET_SLOT_PAYLOAD_OFFSET + frame.len()]
            .copy_from_slice(frame);
        fence(Ordering::Release);
        output[DIRECT_GENET_SLOT_COMMIT_OFFSET..DIRECT_GENET_SLOT_COMMIT_OFFSET + 8]
            .copy_from_slice(&sequence.to_le_bytes());
        Ok(sequence)
    }

    /// Acquire and privately copy exactly the cursor following `after_cursor`.
    pub fn decode_next(
        input: &[u8],
        direction: DirectGenetDirection,
        generation: u64,
        after_cursor: u64,
    ) -> Result<DirectGenetSlotRecord, DirectGenetError> {
        if input.len() != SHARED_PAGE_BYTES {
            return Err(DirectGenetError::InvalidLayout);
        }
        let expected_sequence = after_cursor
            .checked_add(1)
            .ok_or(DirectGenetError::InvalidSequence)?;
        let first = read_u64(input, DIRECT_GENET_SLOT_COMMIT_OFFSET);
        fence(Ordering::Acquire);
        let header = DirectGenetSlotHeader {
            magic: read_u32(input, 0),
            version: read_u16(input, 4),
            direction: read_u16(input, 6),
            header_bytes: read_u16(input, 8),
            frame_len: read_u16(input, 10),
            flags: read_u32(input, 12),
            generation: read_u64(input, 16),
            sequence: read_u64(input, 24),
            reserved: decode_u64_array(input, 32),
            committed_sequence: read_u64(input, DIRECT_GENET_SLOT_COMMIT_OFFSET),
        };
        let frame_len = header.validate(direction, generation, expected_sequence)?;
        if first != expected_sequence {
            return Err(DirectGenetError::InvalidSequence);
        }
        let mut frame = [0u8; ETHERNET_FRAME_BYTES];
        frame[..frame_len].copy_from_slice(
            &input[DIRECT_GENET_SLOT_PAYLOAD_OFFSET..DIRECT_GENET_SLOT_PAYLOAD_OFFSET + frame_len],
        );
        fence(Ordering::Acquire);
        if read_u64(input, DIRECT_GENET_SLOT_COMMIT_OFFSET) != first
            || read_u64(input, 16) != generation
            || read_u64(input, 24) != expected_sequence
            || read_u32(input, 12) != 0
        {
            return Err(DirectGenetError::InvalidSequence);
        }
        Ok(DirectGenetSlotRecord {
            direction,
            sequence: expected_sequence,
            frame_len: frame_len as u16,
            frame,
        })
    }
}

/// Return the exact page-slot index for one nonzero ring sequence.
pub const fn direct_genet_slot_index(
    direction: DirectGenetDirection,
    sequence: u64,
) -> Result<usize, DirectGenetError> {
    if sequence == 0 {
        return Err(DirectGenetError::InvalidSequence);
    }
    Ok(((sequence - 1) % direction.slot_count() as u64) as usize)
}

fn encode_direct_genet_control_header(output: &mut [u8], header: DirectGenetControlHeader) {
    output.fill(0);
    output[0..4].copy_from_slice(&header.magic.to_le_bytes());
    output[4..6].copy_from_slice(&header.version.to_le_bytes());
    output[6..8].copy_from_slice(&header.header_bytes.to_le_bytes());
    output[8..16].copy_from_slice(&header.generation.to_le_bytes());
    output[16..18].copy_from_slice(&header.shared_page_bytes.to_le_bytes());
    output[18] = header.rx_slot_count;
    output[19] = header.tx_slot_count;
    output[20..22].copy_from_slice(&header.cursor_state_bytes.to_le_bytes());
    output[22..24].copy_from_slice(&header.cursor_state_count.to_le_bytes());
    output[24..28].copy_from_slice(&header.flags.to_le_bytes());
    output[28..32].copy_from_slice(&header.reserved0.to_le_bytes());
    encode_u64_array(output, 32, &header.reserved);
    output[56..64].copy_from_slice(&header.seal.to_le_bytes());
}

fn decode_direct_genet_control_header(
    input: &[u8],
) -> Result<DirectGenetControlHeader, DirectGenetError> {
    if input.len() != SHARED_PAGE_BYTES {
        return Err(DirectGenetError::InvalidLayout);
    }
    Ok(DirectGenetControlHeader {
        magic: read_u32(input, 0),
        version: read_u16(input, 4),
        header_bytes: read_u16(input, 6),
        generation: read_u64(input, 8),
        shared_page_bytes: read_u16(input, 16),
        rx_slot_count: input[18],
        tx_slot_count: input[19],
        cursor_state_bytes: read_u16(input, 20),
        cursor_state_count: read_u16(input, 22),
        flags: read_u32(input, 24),
        reserved0: read_u32(input, 28),
        reserved: decode_u64_array(input, 32),
        seal: read_u64(input, 56),
    })
}

fn encode_direct_genet_cursor_state(
    output: &mut [u8],
    state: DirectGenetCursorState,
) -> Result<(), DirectGenetError> {
    let role = direct_genet_cursor_role(state.role)?;
    state.validate_shape(state.generation, role)?;
    output[56..64].copy_from_slice(&0u64.to_le_bytes());
    fence(Ordering::Release);
    output.fill(0);
    output[0..4].copy_from_slice(&state.magic.to_le_bytes());
    output[4..6].copy_from_slice(&state.version.to_le_bytes());
    output[6..8].copy_from_slice(&state.role.to_le_bytes());
    output[8..16].copy_from_slice(&state.generation.to_le_bytes());
    output[16..24].copy_from_slice(&state.cursor.to_le_bytes());
    output[24..32].copy_from_slice(&state.state_sequence.to_le_bytes());
    output[32..36].copy_from_slice(&state.flags.to_le_bytes());
    output[36..40].copy_from_slice(&state.poison_reason.to_le_bytes());
    output[40..56].copy_from_slice(&state.reserved);
    fence(Ordering::Release);
    output[56..64].copy_from_slice(&state.committed_sequence.to_le_bytes());
    Ok(())
}

fn decode_direct_genet_cursor_state(
    input: &[u8],
    generation: u64,
    role: DirectGenetCursorRole,
) -> Result<DirectGenetCursorState, DirectGenetError> {
    let state = decode_direct_genet_cursor_state_allow_poison(input, generation, role)?;
    state.validate_live(generation, role)?;
    Ok(state)
}

fn decode_direct_genet_cursor_state_allow_poison(
    input: &[u8],
    generation: u64,
    role: DirectGenetCursorRole,
) -> Result<DirectGenetCursorState, DirectGenetError> {
    if input.len() != SHARED_PAGE_BYTES {
        return Err(DirectGenetError::InvalidLayout);
    }
    let offset = role.offset();
    let first = read_u64(input, offset + 56);
    fence(Ordering::Acquire);
    let mut reserved = [0u8; 16];
    reserved.copy_from_slice(&input[offset + 40..offset + 56]);
    let state = DirectGenetCursorState {
        magic: read_u32(input, offset),
        version: read_u16(input, offset + 4),
        role: read_u16(input, offset + 6),
        generation: read_u64(input, offset + 8),
        cursor: read_u64(input, offset + 16),
        state_sequence: read_u64(input, offset + 24),
        flags: read_u32(input, offset + 32),
        poison_reason: read_u32(input, offset + 36),
        reserved,
        committed_sequence: read_u64(input, offset + 56),
    };
    fence(Ordering::Acquire);
    let second = read_u64(input, offset + 56);
    if first != second || first == 0 {
        return Err(DirectGenetError::StateChanged);
    }
    state.validate_shape(generation, role)?;
    Ok(state)
}

fn write_direct_genet_cursor_state(
    page: &mut [u8],
    state: DirectGenetCursorState,
) -> Result<(), DirectGenetError> {
    if page.len() != SHARED_PAGE_BYTES {
        return Err(DirectGenetError::InvalidLayout);
    }
    let role = direct_genet_cursor_role(state.role)?;
    let offset = role.offset();
    encode_direct_genet_cursor_state(
        &mut page[offset..offset + DIRECT_GENET_CURSOR_STATE_BYTES],
        state,
    )
}

const fn direct_genet_cursor_role(raw: u16) -> Result<DirectGenetCursorRole, DirectGenetError> {
    match raw {
        1 => Ok(DirectGenetCursorRole::RxProducer),
        2 => Ok(DirectGenetCursorRole::RxConsumer),
        3 => Ok(DirectGenetCursorRole::TxProducer),
        4 => Ok(DirectGenetCursorRole::TxConsumer),
        _ => Err(DirectGenetError::InvalidIdentity),
    }
}

/// Pointer-free runtime descriptor mapped read-only into the child.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C, align(8))]
pub struct RuntimeInitDescriptor {
    /// [`RUNTIME_INIT_MAGIC`].
    pub magic: u32,
    /// [`ABI_VERSION`].
    pub abi_version: u16,
    /// Exact `size_of::<Self>()`.
    pub descriptor_bytes: u16,
    /// Required flags plus recognized optional transport extensions.
    pub flags: u32,
    /// Reserved; zero.
    pub reserved0: u32,
    /// Nonzero supervisor generation.
    pub generation: u64,
    /// Child wait-notification cap slot.
    pub child_wake_notification_slot: u32,
    /// Child-to-supervisor packet-TX signal cap slot.
    pub packet_tx_wake_notification_slot: u32,
    /// Child-to-supervisor console-event signal cap slot.
    pub supervisor_wake_notification_slot: u32,
    /// Fault endpoint cap slot.
    pub fault_endpoint_slot: u32,
    /// Child CSpace cardinality.
    pub child_cspace_slots: u32,
    /// Root-to-child allowed notification bits.
    pub root_wake_mask: u64,
    /// Child-to-root allowed notification bits.
    pub child_wake_mask: u64,
    /// Child IPC-buffer mapping selected by the supervisor.
    pub ipc_buffer_vaddr: u64,
    /// Root-produced ingress packet page mapped read-only in the child.
    pub packet_rx_vaddr: u64,
    /// Child-produced egress packet page mapped read-write in the child.
    pub packet_tx_vaddr: u64,
    /// Root-produced control page mapped read-only in the child.
    pub command_vaddr: u64,
    /// Child-produced event page mapped read-write in the child.
    pub event_vaddr: u64,
    /// Exact [`SHARED_PAGE_BYTES`].
    pub shared_frame_bytes: u32,
    /// Exact [`ETHERNET_FRAME_BYTES`].
    pub ethernet_frame_bytes: u16,
    /// Exact sole listener port.
    pub listener_port: u16,
    /// Maximum packet records serviced per notification turn.
    pub max_packets_per_wake: u16,
    /// Maximum authenticated commands coalesced per event publication.
    pub max_commands_per_wake: u16,
    /// One root control may be outstanding.
    pub max_control_inflight: u8,
    /// IPv4 prefix length.
    pub prefix_len: u8,
    /// Authentication token length.
    pub auth_token_len: u8,
    /// Reserved; zero.
    pub reserved1: u8,
    /// Virtual NIC MAC address.
    pub mac: [u8; 6],
    /// Static IPv4 address for the QEMU service.
    pub ipv4: [u8; 4],
    /// Default gateway, or all zero when absent.
    pub gateway: [u8; 4],
    /// Reserved alignment bytes; zero.
    pub reserved2: [u8; 2],
    /// Authentication deadline in milliseconds.
    pub auth_timeout_ms: u32,
    /// Authenticated idle deadline in milliseconds.
    pub idle_timeout_ms: u32,
    /// Selected seL4 virtual-counter frequency.
    pub timer_clock_hz: u64,
    /// Authentication material visible only to this restricted child.
    pub auth_token: [u8; AUTH_TOKEN_BYTES],
    /// FNV-1a seal over every preceding field.
    pub seal: u64,
}

impl RuntimeInitDescriptor {
    /// Seal a fully populated descriptor.
    #[must_use]
    pub const fn sealed(mut self) -> Self {
        self.seal = self.expected_seal();
        self
    }

    /// Validate exact layout, least-authority slots, bounds, mappings, and seal.
    pub const fn validate(self) -> Result<(), AbiError> {
        if self.magic != RUNTIME_INIT_MAGIC || self.abi_version != ABI_VERSION {
            return Err(AbiError::InvalidIdentity);
        }
        if self.descriptor_bytes as usize != size_of::<Self>()
            || self.reserved0 != 0
            || self.reserved1 != 0
            || !bytes_zero(&self.reserved2)
        {
            return Err(AbiError::InvalidLayout);
        }
        if self.flags & REQUIRED_INIT_FLAGS != REQUIRED_INIT_FLAGS
            || self.flags & !ALLOWED_INIT_FLAGS != 0
            || (self.direct_virtio() && self.direct_genet())
            || self.child_cspace_slots != CHILD_CSPACE_SLOTS
            || self.child_wake_notification_slot != CHILD_WAKE_NOTIFICATION_SLOT
            || self.packet_tx_wake_notification_slot != PACKET_TX_WAKE_NOTIFICATION_SLOT
            || self.supervisor_wake_notification_slot != SUPERVISOR_WAKE_NOTIFICATION_SLOT
            || self.fault_endpoint_slot != FAULT_ENDPOINT_SLOT
            || self.root_wake_mask
                != (ROOT_WAKE_MASK
                    | if self.direct_virtio() {
                        WAKE_DIRECT_VIRTIO_IRQ
                    } else {
                        0
                    }
                    | if self.direct_genet() {
                        WAKE_DIRECT_GENET_LINK
                    } else {
                        0
                    })
            || self.child_wake_mask != CHILD_WAKE_MASK
        {
            return Err(AbiError::InvalidAuthority);
        }
        if self.generation == 0
            || self.shared_frame_bytes as usize != SHARED_PAGE_BYTES
            || self.ethernet_frame_bytes as usize != ETHERNET_FRAME_BYTES
            || self.listener_port != 31_337
            || self.max_packets_per_wake == 0
            || self.max_packets_per_wake > 16
            || self.max_commands_per_wake == 0
            || self.max_commands_per_wake as usize > COMMAND_BATCH_MAX_RECORDS
            || self.max_control_inflight != 1
            || self.prefix_len > 32
            || self.auth_token_len == 0
            || self.auth_token_len as usize > AUTH_TOKEN_BYTES
            || self.auth_timeout_ms == 0
            || self.idle_timeout_ms <= self.auth_timeout_ms
            || self.timer_clock_hz == 0
        {
            return Err(AbiError::InvalidBound);
        }
        let mappings = [
            self.ipc_buffer_vaddr,
            self.packet_rx_vaddr,
            self.packet_tx_vaddr,
            self.command_vaddr,
            self.event_vaddr,
        ];
        let mut left = 0usize;
        while left < mappings.len() {
            let alignment = if left == 0 {
                1024
            } else {
                SHARED_PAGE_BYTES as u64
            };
            if mappings[left] == 0 || mappings[left] & (alignment - 1) != 0 {
                return Err(AbiError::InvalidLayout);
            }
            let mut right = left + 1;
            while right < mappings.len() {
                if mappings[left] == mappings[right] {
                    return Err(AbiError::InvalidLayout);
                }
                right += 1;
            }
            left += 1;
        }
        let mut index = self.auth_token_len as usize;
        while index < self.auth_token.len() {
            if self.auth_token[index] != 0 {
                return Err(AbiError::InvalidBound);
            }
            index += 1;
        }
        if self.seal == 0 || self.seal != self.expected_seal() {
            return Err(AbiError::InvalidSeal);
        }
        Ok(())
    }

    /// Auth token as the bounded initialized byte prefix.
    #[must_use]
    pub fn auth_token(&self) -> &[u8] {
        &self.auth_token[..self.auth_token_len as usize]
    }

    /// Whether this QEMU service owns the admitted VirtIO device directly.
    #[must_use]
    pub const fn direct_virtio(self) -> bool {
        self.flags & INIT_FLAG_DIRECT_VIRTIO != 0
    }

    /// Enable the direct-VirtIO extension and reseal the descriptor.
    #[must_use]
    pub const fn with_direct_virtio(mut self) -> Self {
        self.flags |= INIT_FLAG_DIRECT_VIRTIO;
        self.root_wake_mask |= WAKE_DIRECT_VIRTIO_IRQ;
        self.sealed()
    }

    /// Whether this Pi service uses the CPU-only direct link to isolated GENET.
    #[must_use]
    pub const fn direct_genet(self) -> bool {
        self.flags & INIT_FLAG_DIRECT_GENET != 0
    }

    /// Enable the Pi GENET direct-link extension and reseal the descriptor.
    #[must_use]
    pub const fn with_direct_genet(mut self) -> Self {
        self.flags |= INIT_FLAG_DIRECT_GENET;
        self.root_wake_mask |= WAKE_DIRECT_GENET_LINK;
        self.sealed()
    }

    /// Validate the Pi extension against this descriptor and legacy mappings.
    pub fn validate_direct_genet_layout(self, layout: DirectGenetLayout) -> Result<(), AbiError> {
        self.validate()?;
        if !self.direct_genet() || self.direct_virtio() {
            return Err(AbiError::InvalidAuthority);
        }
        layout.validate_for(self.generation)?;
        let legacy_mappings = [
            (self.ipc_buffer_vaddr, 1024u64),
            (self.packet_rx_vaddr, SHARED_PAGE_BYTES as u64),
            (self.packet_tx_vaddr, SHARED_PAGE_BYTES as u64),
            (self.command_vaddr, SHARED_PAGE_BYTES as u64),
            (self.event_vaddr, SHARED_PAGE_BYTES as u64),
        ];
        let direct_pages = layout.virtual_pages();
        let mut page_index = 0usize;
        while page_index < direct_pages.len() {
            let direct_end = match direct_pages[page_index].checked_add(SHARED_PAGE_BYTES as u64) {
                Some(end) => end,
                None => return Err(AbiError::InvalidLayout),
            };
            let mut legacy_index = 0usize;
            while legacy_index < legacy_mappings.len() {
                let legacy_end = match legacy_mappings[legacy_index]
                    .0
                    .checked_add(legacy_mappings[legacy_index].1)
                {
                    Some(end) => end,
                    None => return Err(AbiError::InvalidLayout),
                };
                if ranges_overlap(
                    direct_pages[page_index],
                    direct_end,
                    legacy_mappings[legacy_index].0,
                    legacy_end,
                ) {
                    return Err(AbiError::InvalidLayout);
                }
                legacy_index += 1;
            }
            page_index += 1;
        }
        Ok(())
    }

    /// Encode the descriptor into its canonical little-endian wire form.
    ///
    /// This deliberately skips Rust's implicit alignment padding. Supervisors
    /// use it to populate the read-only init page without copying uninitialized
    /// bytes from a native structure.
    pub fn encode(&self, output: &mut [u8]) -> Result<(), AbiError> {
        if output.len() != RUNTIME_INIT_DESCRIPTOR_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        output.fill(0);
        output[0..4].copy_from_slice(&self.magic.to_le_bytes());
        output[4..6].copy_from_slice(&self.abi_version.to_le_bytes());
        output[6..8].copy_from_slice(&self.descriptor_bytes.to_le_bytes());
        output[8..12].copy_from_slice(&self.flags.to_le_bytes());
        output[12..16].copy_from_slice(&self.reserved0.to_le_bytes());
        output[16..24].copy_from_slice(&self.generation.to_le_bytes());
        output[24..28].copy_from_slice(&self.child_wake_notification_slot.to_le_bytes());
        output[28..32].copy_from_slice(&self.packet_tx_wake_notification_slot.to_le_bytes());
        output[32..36].copy_from_slice(&self.supervisor_wake_notification_slot.to_le_bytes());
        output[36..40].copy_from_slice(&self.fault_endpoint_slot.to_le_bytes());
        output[40..44].copy_from_slice(&self.child_cspace_slots.to_le_bytes());
        output[48..56].copy_from_slice(&self.root_wake_mask.to_le_bytes());
        output[56..64].copy_from_slice(&self.child_wake_mask.to_le_bytes());
        output[64..72].copy_from_slice(&self.ipc_buffer_vaddr.to_le_bytes());
        output[72..80].copy_from_slice(&self.packet_rx_vaddr.to_le_bytes());
        output[80..88].copy_from_slice(&self.packet_tx_vaddr.to_le_bytes());
        output[88..96].copy_from_slice(&self.command_vaddr.to_le_bytes());
        output[96..104].copy_from_slice(&self.event_vaddr.to_le_bytes());
        output[104..108].copy_from_slice(&self.shared_frame_bytes.to_le_bytes());
        output[108..110].copy_from_slice(&self.ethernet_frame_bytes.to_le_bytes());
        output[110..112].copy_from_slice(&self.listener_port.to_le_bytes());
        output[112..114].copy_from_slice(&self.max_packets_per_wake.to_le_bytes());
        output[114..116].copy_from_slice(&self.max_commands_per_wake.to_le_bytes());
        output[116] = self.max_control_inflight;
        output[117] = self.prefix_len;
        output[118] = self.auth_token_len;
        output[119] = self.reserved1;
        output[120..126].copy_from_slice(&self.mac);
        output[126..130].copy_from_slice(&self.ipv4);
        output[130..134].copy_from_slice(&self.gateway);
        output[134..136].copy_from_slice(&self.reserved2);
        output[136..140].copy_from_slice(&self.auth_timeout_ms.to_le_bytes());
        output[140..144].copy_from_slice(&self.idle_timeout_ms.to_le_bytes());
        output[144..152].copy_from_slice(&self.timer_clock_hz.to_le_bytes());
        output[152..216].copy_from_slice(&self.auth_token);
        output[216..224].copy_from_slice(&self.seal.to_le_bytes());
        Ok(())
    }

    /// Decode the canonical runtime-init wire form without native pointer casts.
    pub fn decode(input: &[u8]) -> Result<Self, AbiError> {
        if input.len() != RUNTIME_INIT_DESCRIPTOR_BYTES || !bytes_zero(&input[44..48]) {
            return Err(AbiError::InvalidLayout);
        }
        let mut mac = [0; 6];
        mac.copy_from_slice(&input[120..126]);
        let mut ipv4 = [0; 4];
        ipv4.copy_from_slice(&input[126..130]);
        let mut gateway = [0; 4];
        gateway.copy_from_slice(&input[130..134]);
        let mut reserved2 = [0; 2];
        reserved2.copy_from_slice(&input[134..136]);
        let mut auth_token = [0; AUTH_TOKEN_BYTES];
        auth_token.copy_from_slice(&input[152..216]);
        Ok(Self {
            magic: read_u32(input, 0),
            abi_version: read_u16(input, 4),
            descriptor_bytes: read_u16(input, 6),
            flags: read_u32(input, 8),
            reserved0: read_u32(input, 12),
            generation: read_u64(input, 16),
            child_wake_notification_slot: read_u32(input, 24),
            packet_tx_wake_notification_slot: read_u32(input, 28),
            supervisor_wake_notification_slot: read_u32(input, 32),
            fault_endpoint_slot: read_u32(input, 36),
            child_cspace_slots: read_u32(input, 40),
            root_wake_mask: read_u64(input, 48),
            child_wake_mask: read_u64(input, 56),
            ipc_buffer_vaddr: read_u64(input, 64),
            packet_rx_vaddr: read_u64(input, 72),
            packet_tx_vaddr: read_u64(input, 80),
            command_vaddr: read_u64(input, 88),
            event_vaddr: read_u64(input, 96),
            shared_frame_bytes: read_u32(input, 104),
            ethernet_frame_bytes: read_u16(input, 108),
            listener_port: read_u16(input, 110),
            max_packets_per_wake: read_u16(input, 112),
            max_commands_per_wake: read_u16(input, 114),
            max_control_inflight: input[116],
            prefix_len: input[117],
            auth_token_len: input[118],
            reserved1: input[119],
            mac,
            ipv4,
            gateway,
            reserved2,
            auth_timeout_ms: read_u32(input, 136),
            idle_timeout_ms: read_u32(input, 140),
            timer_clock_hz: read_u64(input, 144),
            auth_token,
            seal: read_u64(input, 216),
        })
    }

    const fn expected_seal(self) -> u64 {
        let mut hash = FNV64_OFFSET;
        hash = hash_u32(hash, self.magic);
        hash = hash_u16(hash, self.abi_version);
        hash = hash_u16(hash, self.descriptor_bytes);
        hash = hash_u32(hash, self.flags);
        hash = hash_u32(hash, self.reserved0);
        hash = hash_u64(hash, self.generation);
        hash = hash_u32(hash, self.child_wake_notification_slot);
        hash = hash_u32(hash, self.packet_tx_wake_notification_slot);
        hash = hash_u32(hash, self.supervisor_wake_notification_slot);
        hash = hash_u32(hash, self.fault_endpoint_slot);
        hash = hash_u32(hash, self.child_cspace_slots);
        hash = hash_u64(hash, self.root_wake_mask);
        hash = hash_u64(hash, self.child_wake_mask);
        hash = hash_u64(hash, self.ipc_buffer_vaddr);
        hash = hash_u64(hash, self.packet_rx_vaddr);
        hash = hash_u64(hash, self.packet_tx_vaddr);
        hash = hash_u64(hash, self.command_vaddr);
        hash = hash_u64(hash, self.event_vaddr);
        hash = hash_u32(hash, self.shared_frame_bytes);
        hash = hash_u16(hash, self.ethernet_frame_bytes);
        hash = hash_u16(hash, self.listener_port);
        hash = hash_u16(hash, self.max_packets_per_wake);
        hash = hash_u16(hash, self.max_commands_per_wake);
        hash = hash_byte(hash, self.max_control_inflight);
        hash = hash_byte(hash, self.prefix_len);
        hash = hash_byte(hash, self.auth_token_len);
        hash = hash_byte(hash, self.reserved1);
        hash = hash_bytes(hash, &self.mac);
        hash = hash_bytes(hash, &self.ipv4);
        hash = hash_bytes(hash, &self.gateway);
        hash = hash_bytes(hash, &self.reserved2);
        hash = hash_u32(hash, self.auth_timeout_ms);
        hash = hash_u32(hash, self.idle_timeout_ms);
        hash = hash_u64(hash, self.timer_clock_hz);
        hash_bytes(hash, &self.auth_token)
    }
}

/// One page carrying one copied Ethernet packet.
#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(C, align(4096))]
pub struct PacketPage {
    /// [`PACKET_PAGE_MAGIC`].
    pub magic: u32,
    /// [`ABI_VERSION`].
    pub abi_version: u16,
    /// Raw [`PacketDirection`].
    pub direction: u16,
    /// Nonzero child generation.
    pub generation: u64,
    /// Producer sequence.
    pub sequence: u64,
    /// Written last; must equal `sequence`.
    pub committed_sequence: u64,
    /// Initialized packet bytes.
    pub packet_len: u16,
    /// Reserved flags; zero.
    pub flags: u16,
    /// Reserved; zero.
    pub reserved0: u32,
    /// Copied Ethernet frame.
    pub packet: [u8; ETHERNET_FRAME_BYTES],
    /// Reserved page tail; zero.
    pub reserved: [u8; PACKET_RESERVED_BYTES],
}

impl core::fmt::Debug for PacketPage {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("PacketPage")
            .field("direction", &self.direction)
            .field("generation", &self.generation)
            .field("sequence", &self.sequence)
            .field("committed_sequence", &self.committed_sequence)
            .field("packet_len", &self.packet_len)
            .finish()
    }
}

impl PacketPage {
    /// Empty page for one exact direction and generation.
    #[must_use]
    pub const fn empty(direction: PacketDirection, generation: u64) -> Self {
        Self {
            magic: PACKET_PAGE_MAGIC,
            abi_version: ABI_VERSION,
            direction: direction as u16,
            generation,
            sequence: 0,
            committed_sequence: 0,
            packet_len: 0,
            flags: 0,
            reserved0: 0,
            packet: [0; ETHERNET_FRAME_BYTES],
            reserved: [0; PACKET_RESERVED_BYTES],
        }
    }

    /// Publish one packet directly into an already-zeroed shared page.
    ///
    /// Only the fixed header and initialized packet prefix are rewritten. The
    /// reserved tail is construction-zeroed, non-authoritative, and left
    /// untouched by this bounded publisher.
    /// The mapped commit word is cleared first and written last after a release
    /// fence, so a consumer can never accept a partially staged body.
    pub fn publish_into(
        output: &mut [u8],
        direction: PacketDirection,
        generation: u64,
        sequence: u64,
        packet: &[u8],
    ) -> Result<(), AbiError> {
        if output.len() != SHARED_PAGE_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        let header = PacketPageHeader::staged(direction, generation, sequence, packet.len())?;
        output[PACKET_COMMIT_OFFSET..PACKET_COMMIT_OFFSET + 8].copy_from_slice(&0u64.to_le_bytes());
        fence(Ordering::Release);
        output[0..4].copy_from_slice(&header.magic.to_le_bytes());
        output[4..6].copy_from_slice(&header.abi_version.to_le_bytes());
        output[6..8].copy_from_slice(&header.direction.to_le_bytes());
        output[8..16].copy_from_slice(&header.generation.to_le_bytes());
        output[16..24].copy_from_slice(&header.sequence.to_le_bytes());
        output[32..34].copy_from_slice(&header.packet_len.to_le_bytes());
        output[34..36].copy_from_slice(&header.flags.to_le_bytes());
        output[36..40].copy_from_slice(&header.reserved0.to_le_bytes());
        output[PACKET_PAYLOAD_OFFSET..PACKET_PAYLOAD_OFFSET + packet.len()].copy_from_slice(packet);
        fence(Ordering::Release);
        output[PACKET_COMMIT_OFFSET..PACKET_COMMIT_OFFSET + 8]
            .copy_from_slice(&sequence.to_le_bytes());
        Ok(())
    }

    /// Copy one stable packet publication without materializing a 4-KiB page.
    pub fn decode_bounded(
        input: &[u8],
        generation: u64,
        after_sequence: u64,
    ) -> Result<PacketRecord, AbiError> {
        if input.len() != SHARED_PAGE_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        let first = read_u64(input, PACKET_COMMIT_OFFSET);
        fence(Ordering::Acquire);
        let header = PacketPageHeader {
            magic: read_u32(input, 0),
            abi_version: read_u16(input, 4),
            direction: read_u16(input, 6),
            generation: read_u64(input, 8),
            sequence: read_u64(input, 16),
            committed_sequence: read_u64(input, PACKET_COMMIT_OFFSET),
            packet_len: read_u16(input, 32),
            flags: read_u16(input, 34),
            reserved0: read_u32(input, 36),
        };
        let (direction, packet_len) = header.validate(generation, after_sequence)?;
        if first != header.sequence {
            return Err(AbiError::InvalidSequence);
        }
        let mut packet = [0; ETHERNET_FRAME_BYTES];
        packet[..packet_len]
            .copy_from_slice(&input[PACKET_PAYLOAD_OFFSET..PACKET_PAYLOAD_OFFSET + packet_len]);
        fence(Ordering::Acquire);
        let second = read_u64(input, PACKET_COMMIT_OFFSET);
        if first != second {
            return Err(AbiError::InvalidSequence);
        }
        Ok(PacketRecord {
            direction,
            sequence: header.sequence,
            packet_len: header.packet_len,
            packet,
        })
    }

    /// Stage and commit one packet. Callers signal only after this returns.
    pub fn publish(&mut self, sequence: u64, packet: &[u8]) -> Result<(), AbiError> {
        if sequence == 0 || packet.is_empty() || packet.len() > self.packet.len() {
            return Err(AbiError::InvalidBound);
        }
        self.committed_sequence = 0;
        self.sequence = sequence;
        self.packet_len = packet.len() as u16;
        self.packet.fill(0);
        self.packet[..packet.len()].copy_from_slice(packet);
        self.committed_sequence = sequence;
        Ok(())
    }

    /// Validate one committed packet and return its direction and bytes.
    pub fn validate(
        &self,
        generation: u64,
        after_sequence: u64,
    ) -> Result<(PacketDirection, &[u8]), AbiError> {
        if self.magic != PACKET_PAGE_MAGIC || self.abi_version != ABI_VERSION {
            return Err(AbiError::InvalidIdentity);
        }
        if self.generation != generation {
            return Err(AbiError::StaleGeneration);
        }
        if self.sequence == 0
            || self.sequence <= after_sequence
            || self.committed_sequence != self.sequence
        {
            return Err(AbiError::InvalidSequence);
        }
        let len = self.packet_len as usize;
        if len == 0
            || len > self.packet.len()
            || self.flags != 0
            || self.reserved0 != 0
            || !bytes_zero(&self.reserved)
        {
            return Err(AbiError::InvalidBound);
        }
        Ok((
            PacketDirection::from_raw(self.direction)?,
            &self.packet[..len],
        ))
    }

    /// Encode this fixed record into one shared page without pointer casts.
    pub fn encode(&self, output: &mut [u8]) -> Result<(), AbiError> {
        if output.len() != SHARED_PAGE_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        output[PACKET_COMMIT_OFFSET..PACKET_COMMIT_OFFSET + 8].copy_from_slice(&0u64.to_le_bytes());
        fence(Ordering::Release);
        output.fill(0);
        output[0..4].copy_from_slice(&self.magic.to_le_bytes());
        output[4..6].copy_from_slice(&self.abi_version.to_le_bytes());
        output[6..8].copy_from_slice(&self.direction.to_le_bytes());
        output[8..16].copy_from_slice(&self.generation.to_le_bytes());
        output[16..24].copy_from_slice(&self.sequence.to_le_bytes());
        output[32..34].copy_from_slice(&self.packet_len.to_le_bytes());
        output[34..36].copy_from_slice(&self.flags.to_le_bytes());
        output[36..40].copy_from_slice(&self.reserved0.to_le_bytes());
        output[40..40 + ETHERNET_FRAME_BYTES].copy_from_slice(&self.packet);
        output[40 + ETHERNET_FRAME_BYTES..].copy_from_slice(&self.reserved);
        fence(Ordering::Release);
        output[24..32].copy_from_slice(&self.committed_sequence.to_le_bytes());
        Ok(())
    }

    /// Decode one shared page into a fixed record without pointer casts.
    pub fn decode(input: &[u8]) -> Result<Self, AbiError> {
        if input.len() != SHARED_PAGE_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        let mut packet = [0; ETHERNET_FRAME_BYTES];
        packet.copy_from_slice(&input[40..40 + ETHERNET_FRAME_BYTES]);
        let mut reserved = [0; PACKET_RESERVED_BYTES];
        reserved.copy_from_slice(&input[40 + ETHERNET_FRAME_BYTES..]);
        Ok(Self {
            magic: read_u32(input, 0),
            abi_version: read_u16(input, 4),
            direction: read_u16(input, 6),
            generation: read_u64(input, 8),
            sequence: read_u64(input, 16),
            committed_sequence: read_u64(input, 24),
            packet_len: read_u16(input, 32),
            flags: read_u16(input, 34),
            reserved0: read_u32(input, 36),
            packet,
            reserved,
        })
    }
}

/// One page carrying one bounded root control or child event.
#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(C, align(4096))]
pub struct ExchangePage {
    /// [`EXCHANGE_PAGE_MAGIC`].
    pub magic: u32,
    /// [`ABI_VERSION`].
    pub abi_version: u16,
    /// Raw [`ExchangeKind`].
    pub kind: u16,
    /// Exact fixed page size.
    pub record_bytes: u16,
    /// Initialized payload bytes.
    pub payload_len: u16,
    /// Reserved; zero.
    pub reserved0: u32,
    /// Nonzero child generation.
    pub generation: u64,
    /// Producer sequence.
    pub sequence: u64,
    /// Exact connection identity, or zero for READY.
    pub connection_id: u64,
    /// Monotonic observation timestamp supplied by root.
    pub now_ms: u64,
    /// Exact packet/control sequence acknowledged by completion kinds.
    pub related_sequence: u64,
    /// Written last; must equal `sequence`.
    pub committed_sequence: u64,
    /// Bounded payload validated according to the exchange kind.
    pub payload: [u8; CONSOLE_PAYLOAD_BYTES],
    /// Reserved page tail; zero.
    pub reserved: [u8; EXCHANGE_RESERVED_BYTES],
}

impl core::fmt::Debug for ExchangePage {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ExchangePage")
            .field("kind", &self.kind)
            .field("generation", &self.generation)
            .field("sequence", &self.sequence)
            .field("connection_id", &self.connection_id)
            .field("payload_len", &self.payload_len)
            .finish()
    }
}

fn classify_publication_commit(
    first: u64,
    second: u64,
    after_sequence: u64,
) -> Result<bool, AbiError> {
    if first != second {
        // Completion watermarks and semantic events deliberately share one
        // notification bit. The child may therefore begin replacing the
        // already-accepted event while root is handling an earlier watermark
        // hint. A transition from the empty marker or the exact accepted
        // commit is not yet observable; the publisher signals again only
        // after the new body is committed. Any transition from an unaccepted
        // commit would violate the one-credit publication invariant.
        return if first == 0 || first == after_sequence {
            Ok(false)
        } else {
            Err(AbiError::InvalidSequence)
        };
    }
    if first != 0 && first < after_sequence {
        return Err(AbiError::InvalidSequence);
    }
    Ok(first != 0 && first > after_sequence)
}

impl ExchangePage {
    /// Empty page for a nonzero generation.
    #[must_use]
    pub const fn empty(generation: u64) -> Self {
        Self {
            magic: EXCHANGE_PAGE_MAGIC,
            abi_version: ABI_VERSION,
            kind: 0,
            record_bytes: SHARED_PAGE_BYTES as u16,
            payload_len: 0,
            reserved0: 0,
            generation,
            sequence: 0,
            connection_id: 0,
            now_ms: 0,
            related_sequence: 0,
            committed_sequence: 0,
            payload: [0; CONSOLE_PAYLOAD_BYTES],
            reserved: [0; EXCHANGE_RESERVED_BYTES],
        }
    }

    /// Determine whether the event page contains a publication newer than the
    /// caller's last accepted sequence.
    ///
    /// Completion-watermark notifications share the event notification cap
    /// without replacing the event body. This readiness check lets root
    /// distinguish that bounded hint from a newly committed semantic event.
    pub fn publication_pending(
        input: &[u8],
        generation: u64,
        after_sequence: u64,
    ) -> Result<bool, AbiError> {
        if input.len() != SHARED_PAGE_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        let first = read_u64(input, EXCHANGE_COMMIT_OFFSET);
        fence(Ordering::Acquire);
        if read_u32(input, 0) != EXCHANGE_PAGE_MAGIC
            || read_u16(input, 4) != ABI_VERSION
            || read_u16(input, 8) as usize != SHARED_PAGE_BYTES
            || read_u32(input, 12) != 0
        {
            return Err(AbiError::InvalidIdentity);
        }
        if read_u64(input, 16) != generation {
            return Err(AbiError::StaleGeneration);
        }
        let second = read_u64(input, EXCHANGE_COMMIT_OFFSET);
        classify_publication_commit(first, second, after_sequence)
    }

    /// Read a stable pair of exact input-consumption watermarks.
    ///
    /// The sealed runtime descriptor requires
    /// [`INIT_FLAG_COMPLETION_WATERMARKS`]. Both words are independently
    /// monotonic; the bounded retry only prevents a torn pair from being
    /// interpreted as one observation.
    pub fn completion_watermarks(
        input: &[u8],
        generation: u64,
    ) -> Result<CompletionWatermarks, AbiError> {
        if input.len() != SHARED_PAGE_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        if read_u32(input, 0) != EXCHANGE_PAGE_MAGIC
            || read_u16(input, 4) != ABI_VERSION
            || read_u16(input, 8) as usize != SHARED_PAGE_BYTES
        {
            return Err(AbiError::InvalidIdentity);
        }
        if read_u64(input, 16) != generation {
            return Err(AbiError::StaleGeneration);
        }
        let mut attempt = 0;
        while attempt < 2 {
            let ingress_sequence = read_u64(input, INGRESS_CONSUMED_SEQUENCE_OFFSET);
            let control_sequence = read_u64(input, CONTROL_CONSUMED_SEQUENCE_OFFSET);
            fence(Ordering::Acquire);
            if ingress_sequence == read_u64(input, INGRESS_CONSUMED_SEQUENCE_OFFSET)
                && control_sequence == read_u64(input, CONTROL_CONSUMED_SEQUENCE_OFFSET)
            {
                return Ok(CompletionWatermarks {
                    ingress_sequence,
                    control_sequence,
                });
            }
            attempt += 1;
        }
        Err(AbiError::InvalidSequence)
    }

    /// Publish one exchange directly into an already-zeroed shared page.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_related_into(
        output: &mut [u8],
        kind: ExchangeKind,
        generation: u64,
        sequence: u64,
        connection_id: u64,
        now_ms: u64,
        related_sequence: u64,
        payload: &[u8],
    ) -> Result<(), AbiError> {
        if output.len() != SHARED_PAGE_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        validate_exchange_payload(kind, payload)?;
        let header = ExchangePageHeader::staged(
            kind,
            generation,
            sequence,
            connection_id,
            now_ms,
            related_sequence,
            payload.len(),
        )?;
        output[EXCHANGE_COMMIT_OFFSET..EXCHANGE_COMMIT_OFFSET + 8]
            .copy_from_slice(&0u64.to_le_bytes());
        fence(Ordering::Release);
        output[0..4].copy_from_slice(&header.magic.to_le_bytes());
        output[4..6].copy_from_slice(&header.abi_version.to_le_bytes());
        output[6..8].copy_from_slice(&header.kind.to_le_bytes());
        output[8..10].copy_from_slice(&header.record_bytes.to_le_bytes());
        output[10..12].copy_from_slice(&header.payload_len.to_le_bytes());
        output[12..16].copy_from_slice(&header.reserved0.to_le_bytes());
        output[16..24].copy_from_slice(&header.generation.to_le_bytes());
        output[24..32].copy_from_slice(&header.sequence.to_le_bytes());
        output[32..40].copy_from_slice(&header.connection_id.to_le_bytes());
        output[40..48].copy_from_slice(&header.now_ms.to_le_bytes());
        output[48..56].copy_from_slice(&header.related_sequence.to_le_bytes());
        output[EXCHANGE_PAYLOAD_OFFSET..EXCHANGE_PAYLOAD_OFFSET + payload.len()]
            .copy_from_slice(payload);
        fence(Ordering::Release);
        output[EXCHANGE_COMMIT_OFFSET..EXCHANGE_COMMIT_OFFSET + 8]
            .copy_from_slice(&sequence.to_le_bytes());
        Ok(())
    }

    /// Copy one stable exchange without materializing a 4-KiB page.
    pub fn decode_bounded(
        input: &[u8],
        generation: u64,
        after_sequence: u64,
        root_to_child: bool,
    ) -> Result<ExchangeRecord, AbiError> {
        if input.len() != SHARED_PAGE_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        let first = read_u64(input, EXCHANGE_COMMIT_OFFSET);
        fence(Ordering::Acquire);
        let header = ExchangePageHeader {
            magic: read_u32(input, 0),
            abi_version: read_u16(input, 4),
            kind: read_u16(input, 6),
            record_bytes: read_u16(input, 8),
            payload_len: read_u16(input, 10),
            reserved0: read_u32(input, 12),
            generation: read_u64(input, 16),
            sequence: read_u64(input, 24),
            connection_id: read_u64(input, 32),
            now_ms: read_u64(input, 40),
            related_sequence: read_u64(input, 48),
            committed_sequence: read_u64(input, EXCHANGE_COMMIT_OFFSET),
        };
        let (kind, payload_len) = header.validate(generation, after_sequence, root_to_child)?;
        if first != header.sequence {
            return Err(AbiError::InvalidSequence);
        }
        let mut payload = [0; CONSOLE_PAYLOAD_BYTES];
        payload[..payload_len].copy_from_slice(
            &input[EXCHANGE_PAYLOAD_OFFSET..EXCHANGE_PAYLOAD_OFFSET + payload_len],
        );
        validate_exchange_payload(kind, &payload[..payload_len])?;
        fence(Ordering::Acquire);
        let second = read_u64(input, EXCHANGE_COMMIT_OFFSET);
        if first != second {
            return Err(AbiError::InvalidSequence);
        }
        Ok(ExchangeRecord {
            kind,
            sequence: header.sequence,
            connection_id: header.connection_id,
            now_ms: header.now_ms,
            related_sequence: header.related_sequence,
            payload_len: header.payload_len,
            payload,
        })
    }

    /// Stage and commit one directional record.
    pub fn publish(
        &mut self,
        kind: ExchangeKind,
        sequence: u64,
        connection_id: u64,
        now_ms: u64,
        payload: &[u8],
    ) -> Result<(), AbiError> {
        self.publish_related(kind, sequence, connection_id, now_ms, 0, payload)
    }

    /// Stage and commit one record that durably acknowledges a related input.
    pub fn publish_related(
        &mut self,
        kind: ExchangeKind,
        sequence: u64,
        connection_id: u64,
        now_ms: u64,
        related_sequence: u64,
        payload: &[u8],
    ) -> Result<(), AbiError> {
        validate_exchange_payload(kind, payload)?;
        if sequence == 0 || payload.len() > self.payload.len() {
            return Err(AbiError::InvalidBound);
        }
        if matches!(
            kind,
            ExchangeKind::SendLine
                | ExchangeKind::SendBatch
                | ExchangeKind::Command
                | ExchangeKind::CommandBatch
        ) && payload.is_empty()
        {
            return Err(AbiError::InvalidBound);
        }
        if kind == ExchangeKind::Command && payload.len() > COMMAND_LINE_BYTES {
            return Err(AbiError::InvalidBound);
        }
        if matches!(
            kind,
            ExchangeKind::Connected
                | ExchangeKind::Authenticated
                | ExchangeKind::Command
                | ExchangeKind::CommandBatch
                | ExchangeKind::Disconnected
                | ExchangeKind::Backpressure
                | ExchangeKind::Rejected
                | ExchangeKind::OutputDrained
        ) && connection_id == 0
        {
            return Err(AbiError::InvalidBound);
        }
        let completion = matches!(
            kind,
            ExchangeKind::PacketConsumed
                | ExchangeKind::ControlCompleted
                | ExchangeKind::OutputDrained
        );
        if completion != (related_sequence != 0) {
            return Err(AbiError::InvalidSequence);
        }
        self.committed_sequence = 0;
        self.kind = kind as u16;
        self.sequence = sequence;
        self.connection_id = connection_id;
        self.now_ms = now_ms;
        self.related_sequence = related_sequence;
        self.payload_len = payload.len() as u16;
        self.payload.fill(0);
        self.payload[..payload.len()].copy_from_slice(payload);
        self.committed_sequence = sequence;
        Ok(())
    }

    /// Validate one committed record for a direction.
    pub fn validate(
        &self,
        generation: u64,
        after_sequence: u64,
        root_to_child: bool,
    ) -> Result<(ExchangeKind, &[u8]), AbiError> {
        if self.magic != EXCHANGE_PAGE_MAGIC || self.abi_version != ABI_VERSION {
            return Err(AbiError::InvalidIdentity);
        }
        if self.record_bytes as usize != SHARED_PAGE_BYTES
            || self.reserved0 != 0
            || !bytes_zero(&self.reserved)
        {
            return Err(AbiError::InvalidLayout);
        }
        if self.generation != generation {
            return Err(AbiError::StaleGeneration);
        }
        if self.sequence == 0
            || self.sequence <= after_sequence
            || self.committed_sequence != self.sequence
        {
            return Err(AbiError::InvalidSequence);
        }
        let kind = ExchangeKind::from_raw(self.kind)?;
        if kind.root_to_child() != root_to_child {
            return Err(AbiError::InvalidKind);
        }
        let len = self.payload_len as usize;
        if len > self.payload.len()
            || (matches!(
                kind,
                ExchangeKind::SendLine | ExchangeKind::Command | ExchangeKind::CommandBatch
            ) && len == 0)
            || (kind == ExchangeKind::Command && len > COMMAND_LINE_BYTES)
        {
            return Err(AbiError::InvalidBound);
        }
        validate_exchange_payload(kind, &self.payload[..len])?;
        let completion = matches!(
            kind,
            ExchangeKind::PacketConsumed
                | ExchangeKind::ControlCompleted
                | ExchangeKind::OutputDrained
        );
        if completion != (self.related_sequence != 0) {
            return Err(AbiError::InvalidSequence);
        }
        Ok((kind, &self.payload[..len]))
    }

    /// Encode this fixed record into one shared page without pointer casts.
    pub fn encode(&self, output: &mut [u8]) -> Result<(), AbiError> {
        if output.len() != SHARED_PAGE_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        output[EXCHANGE_COMMIT_OFFSET..EXCHANGE_COMMIT_OFFSET + 8]
            .copy_from_slice(&0u64.to_le_bytes());
        fence(Ordering::Release);
        output.fill(0);
        output[0..4].copy_from_slice(&self.magic.to_le_bytes());
        output[4..6].copy_from_slice(&self.abi_version.to_le_bytes());
        output[6..8].copy_from_slice(&self.kind.to_le_bytes());
        output[8..10].copy_from_slice(&self.record_bytes.to_le_bytes());
        output[10..12].copy_from_slice(&self.payload_len.to_le_bytes());
        output[12..16].copy_from_slice(&self.reserved0.to_le_bytes());
        output[16..24].copy_from_slice(&self.generation.to_le_bytes());
        output[24..32].copy_from_slice(&self.sequence.to_le_bytes());
        output[32..40].copy_from_slice(&self.connection_id.to_le_bytes());
        output[40..48].copy_from_slice(&self.now_ms.to_le_bytes());
        output[48..56].copy_from_slice(&self.related_sequence.to_le_bytes());
        output[64..64 + CONSOLE_PAYLOAD_BYTES].copy_from_slice(&self.payload);
        output[64 + CONSOLE_PAYLOAD_BYTES..].copy_from_slice(&self.reserved);
        fence(Ordering::Release);
        output[56..64].copy_from_slice(&self.committed_sequence.to_le_bytes());
        Ok(())
    }

    /// Decode one shared page into a fixed record without pointer casts.
    pub fn decode(input: &[u8]) -> Result<Self, AbiError> {
        if input.len() != SHARED_PAGE_BYTES {
            return Err(AbiError::InvalidLayout);
        }
        let mut payload = [0; CONSOLE_PAYLOAD_BYTES];
        payload.copy_from_slice(&input[64..64 + CONSOLE_PAYLOAD_BYTES]);
        let mut reserved = [0; EXCHANGE_RESERVED_BYTES];
        reserved.copy_from_slice(&input[64 + CONSOLE_PAYLOAD_BYTES..]);
        Ok(Self {
            magic: read_u32(input, 0),
            abi_version: read_u16(input, 4),
            kind: read_u16(input, 6),
            record_bytes: read_u16(input, 8),
            payload_len: read_u16(input, 10),
            reserved0: read_u32(input, 12),
            generation: read_u64(input, 16),
            sequence: read_u64(input, 24),
            connection_id: read_u64(input, 32),
            now_ms: read_u64(input, 40),
            related_sequence: read_u64(input, 48),
            committed_sequence: read_u64(input, 56),
            payload,
            reserved,
        })
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

fn encode_u64_array<const N: usize>(output: &mut [u8], offset: usize, values: &[u64; N]) {
    let mut index = 0usize;
    while index < N {
        let start = offset + index * size_of::<u64>();
        output[start..start + size_of::<u64>()].copy_from_slice(&values[index].to_le_bytes());
        index += 1;
    }
}

fn decode_u64_array<const N: usize>(input: &[u8], offset: usize) -> [u64; N] {
    let mut values = [0u64; N];
    let mut index = 0usize;
    while index < N {
        values[index] = read_u64(input, offset + index * size_of::<u64>());
        index += 1;
    }
    values
}

const fn page_aligned_nonzero(address: u64) -> bool {
    address != 0 && address & (DIRECT_VIRTIO_PAGE_BYTES as u64 - 1) == 0
}

const fn ranges_overlap(left_start: u64, left_end: u64, right_start: u64, right_end: u64) -> bool {
    left_start < right_end && right_start < left_end
}

fn validate_exchange_metadata(
    kind: ExchangeKind,
    generation: u64,
    sequence: u64,
    connection_id: u64,
    related_sequence: u64,
    payload_len: usize,
) -> Result<(), AbiError> {
    if generation == 0 || sequence == 0 || payload_len > CONSOLE_PAYLOAD_BYTES {
        return Err(AbiError::InvalidBound);
    }
    if matches!(
        kind,
        ExchangeKind::SendLine
            | ExchangeKind::SendBatch
            | ExchangeKind::Command
            | ExchangeKind::CommandBatch
    ) && payload_len == 0
    {
        return Err(AbiError::InvalidBound);
    }
    if kind == ExchangeKind::Command && payload_len > COMMAND_LINE_BYTES {
        return Err(AbiError::InvalidBound);
    }
    if matches!(
        kind,
        ExchangeKind::Connected
            | ExchangeKind::Authenticated
            | ExchangeKind::Command
            | ExchangeKind::CommandBatch
            | ExchangeKind::Disconnected
            | ExchangeKind::Backpressure
            | ExchangeKind::Rejected
            | ExchangeKind::OutputDrained
    ) && connection_id == 0
    {
        return Err(AbiError::InvalidBound);
    }
    let completion = matches!(
        kind,
        ExchangeKind::PacketConsumed | ExchangeKind::ControlCompleted | ExchangeKind::OutputDrained
    );
    if completion != (related_sequence != 0) {
        return Err(AbiError::InvalidSequence);
    }
    Ok(())
}

fn validate_exchange_payload(kind: ExchangeKind, payload: &[u8]) -> Result<(), AbiError> {
    match kind {
        ExchangeKind::SendBatch => SendBatchCursor::validate(payload).map(|_| ()),
        ExchangeKind::CommandBatch => CommandBatchCursor::validate(payload).map(|_| ()),
        _ if core::str::from_utf8(payload).is_err() => Err(AbiError::InvalidBound),
        _ => Ok(()),
    }
}

const fn hash_byte(hash: u64, byte: u8) -> u64 {
    (hash ^ byte as u64).wrapping_mul(FNV64_PRIME)
}

const fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    let mut index = 0usize;
    while index < bytes.len() {
        hash = hash_byte(hash, bytes[index]);
        index += 1;
    }
    hash
}

const fn hash_u16(mut hash: u64, value: u16) -> u64 {
    let mut shift = 0;
    while shift < 16 {
        hash = hash_byte(hash, ((value >> shift) & 0xff) as u8);
        shift += 8;
    }
    hash
}

const fn hash_u32(mut hash: u64, value: u32) -> u64 {
    let mut shift = 0;
    while shift < 32 {
        hash = hash_byte(hash, ((value >> shift) & 0xff) as u8);
        shift += 8;
    }
    hash
}

const fn hash_u64(mut hash: u64, value: u64) -> u64 {
    let mut shift = 0;
    while shift < 64 {
        hash = hash_byte(hash, ((value >> shift) & 0xff) as u8);
        shift += 8;
    }
    hash
}

const fn hash_u64_slice(mut hash: u64, values: &[u64]) -> u64 {
    let mut index = 0usize;
    while index < values.len() {
        hash = hash_u64(hash, values[index]);
        index += 1;
    }
    hash
}

const fn bytes_zero(bytes: &[u8]) -> bool {
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

const _: () = assert!(size_of::<PacketPage>() == SHARED_PAGE_BYTES);
const _: () = assert!(align_of::<PacketPage>() == SHARED_PAGE_BYTES);
const _: () = assert!(size_of::<ExchangePage>() == SHARED_PAGE_BYTES);
const _: () = assert!(size_of::<RuntimeInitDescriptor>() == RUNTIME_INIT_DESCRIPTOR_BYTES);
const _: () = assert!(size_of::<DirectVirtioLayout>() == DIRECT_VIRTIO_LAYOUT_BYTES);
const _: () = assert!(align_of::<DirectVirtioLayout>() == align_of::<u64>());
const _: () =
    assert!(DIRECT_VIRTIO_LAYOUT_OFFSET + DIRECT_VIRTIO_LAYOUT_BYTES <= SHARED_PAGE_BYTES);
const _: () = assert!(size_of::<DirectGenetLayout>() == DIRECT_GENET_LAYOUT_BYTES);
const _: () = assert!(align_of::<DirectGenetLayout>() == align_of::<u64>());
const _: () = assert!(size_of::<DirectGenetControlHeader>() == DIRECT_GENET_CONTROL_HEADER_BYTES);
const _: () = assert!(size_of::<DirectGenetCursorState>() == DIRECT_GENET_CURSOR_STATE_BYTES);
const _: () = assert!(align_of::<DirectGenetCursorState>() == DIRECT_GENET_CURSOR_STATE_BYTES);
const _: () = assert!(size_of::<DirectGenetSlotHeader>() == DIRECT_GENET_SLOT_HEADER_BYTES);
const _: () =
    assert!(size_of::<DirectGenetRuntimeDiagnostic>() == DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES);
const _: () = assert!(align_of::<DirectGenetRuntimeDiagnostic>() == 64);
const _: () = assert!(
    DIRECT_GENET_LAYOUT_OFFSET + DIRECT_GENET_LAYOUT_BYTES <= SHARED_PAGE_BYTES
        && DIRECT_VIRTIO_LAYOUT_OFFSET + DIRECT_VIRTIO_LAYOUT_BYTES <= DIRECT_GENET_LAYOUT_OFFSET
);
const _: () = assert!(
    DIRECT_GENET_CONTROL_PAGE_INDEX + 1 == DIRECT_GENET_RX_FIRST_PAGE_INDEX
        && DIRECT_GENET_RX_FIRST_PAGE_INDEX + DIRECT_GENET_RX_SLOT_COUNT
            == DIRECT_GENET_TX_FIRST_PAGE_INDEX
        && DIRECT_GENET_TX_FIRST_PAGE_INDEX + DIRECT_GENET_TX_SLOT_COUNT
            == DIRECT_GENET_SHARED_PAGE_COUNT
);
const _: () = assert!(
    DIRECT_GENET_TX_CONSUMER_STATE_OFFSET + DIRECT_GENET_CURSOR_STATE_BYTES
        == DIRECT_GENET_RUNTIME_DIAGNOSTIC_OFFSET
        && DIRECT_GENET_RUNTIME_DIAGNOSTIC_OFFSET + DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES
            <= SHARED_PAGE_BYTES
);
const _: () = assert!(
    core::mem::offset_of!(DirectGenetRuntimeDiagnostic, committed_sequence)
        == DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET
);
const _: () = assert!(
    core::mem::offset_of!(DirectGenetSlotHeader, committed_sequence)
        == DIRECT_GENET_SLOT_COMMIT_OFFSET
);
const _: () = assert!(
    core::mem::offset_of!(DirectGenetSlotHeader, frame_len) == DIRECT_GENET_SLOT_LENGTH_OFFSET
);
const _: () = assert!(
    core::mem::offset_of!(DirectGenetSlotHeader, generation) == DIRECT_GENET_SLOT_GENERATION_OFFSET
);
const _: () = assert!(
    core::mem::offset_of!(DirectGenetSlotHeader, sequence) == DIRECT_GENET_SLOT_SEQUENCE_OFFSET
);
const _: () = assert!(
    core::mem::offset_of!(DirectGenetCursorState, generation)
        == DIRECT_GENET_CURSOR_GENERATION_OFFSET
);
const _: () =
    assert!(core::mem::offset_of!(DirectGenetCursorState, cursor) == DIRECT_GENET_CURSOR_OFFSET);
const _: () = assert!(
    core::mem::offset_of!(DirectGenetCursorState, state_sequence)
        == DIRECT_GENET_CURSOR_STATE_SEQUENCE_OFFSET
);
const _: () = assert!(
    core::mem::offset_of!(DirectGenetCursorState, flags) == DIRECT_GENET_CURSOR_FLAGS_OFFSET
);
const _: () = assert!(
    core::mem::offset_of!(DirectGenetCursorState, poison_reason)
        == DIRECT_GENET_CURSOR_POISON_REASON_OFFSET
);
const _: () = assert!(
    core::mem::offset_of!(DirectGenetCursorState, committed_sequence)
        == DIRECT_GENET_CURSOR_COMMIT_OFFSET
);
const _: () = assert!(align_of::<ExchangePage>() == SHARED_PAGE_BYTES);
const _: () = assert!(size_of::<PacketPageHeader>() == PACKET_HEADER_BYTES);
const _: () = assert!(size_of::<ExchangePageHeader>() == EXCHANGE_HEADER_BYTES);
const _: () = assert!(
    SEND_BATCH_HEADER_BYTES
        + SEND_BATCH_MAX_RECORDS * (SEND_BATCH_RECORD_HEADER_BYTES + SEND_BATCH_LINE_BYTES)
        <= CONSOLE_PAYLOAD_BYTES
);
const _: () = assert!(SEND_BATCH_LINE_BYTES <= CONSOLE_OUTPUT_BYTES);
const _: () = assert!(core::mem::offset_of!(PacketPage, packet_len) == PACKET_LENGTH_OFFSET);
const _: () =
    assert!(core::mem::offset_of!(PacketPage, committed_sequence) == PACKET_COMMIT_OFFSET);
const _: () = assert!(core::mem::offset_of!(PacketPage, packet) == PACKET_PAYLOAD_OFFSET);
const _: () = assert!(core::mem::offset_of!(ExchangePage, payload_len) == EXCHANGE_LENGTH_OFFSET);
const _: () =
    assert!(core::mem::offset_of!(ExchangePage, committed_sequence) == EXCHANGE_COMMIT_OFFSET);
const _: () = assert!(core::mem::offset_of!(ExchangePage, payload) == EXCHANGE_PAYLOAD_OFFSET);

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> RuntimeInitDescriptor {
        RuntimeInitDescriptor {
            magic: RUNTIME_INIT_MAGIC,
            abi_version: ABI_VERSION,
            descriptor_bytes: size_of::<RuntimeInitDescriptor>() as u16,
            flags: REQUIRED_INIT_FLAGS,
            reserved0: 0,
            generation: 7,
            child_wake_notification_slot: CHILD_WAKE_NOTIFICATION_SLOT,
            packet_tx_wake_notification_slot: PACKET_TX_WAKE_NOTIFICATION_SLOT,
            supervisor_wake_notification_slot: SUPERVISOR_WAKE_NOTIFICATION_SLOT,
            fault_endpoint_slot: FAULT_ENDPOINT_SLOT,
            child_cspace_slots: CHILD_CSPACE_SLOTS,
            root_wake_mask: ROOT_WAKE_MASK,
            child_wake_mask: CHILD_WAKE_MASK,
            ipc_buffer_vaddr: 0x7200_0000,
            packet_rx_vaddr: 0x7202_0000,
            packet_tx_vaddr: 0x7202_1000,
            command_vaddr: 0x7202_2000,
            event_vaddr: 0x7202_3000,
            shared_frame_bytes: SHARED_PAGE_BYTES as u32,
            ethernet_frame_bytes: ETHERNET_FRAME_BYTES as u16,
            listener_port: 31_337,
            max_packets_per_wake: 1,
            max_commands_per_wake: 1,
            max_control_inflight: 1,
            prefix_len: 24,
            auth_token_len: 6,
            reserved1: 0,
            mac: [2, 0, 0, 0, 0, 1],
            ipv4: [10, 0, 2, 15],
            gateway: [10, 0, 2, 2],
            reserved2: [0; 2],
            auth_timeout_ms: 5000,
            idle_timeout_ms: 300_000,
            timer_clock_hz: 62_500_000,
            auth_token: {
                let mut token = [0; AUTH_TOKEN_BYTES];
                token[0] = b's';
                token[1] = b'e';
                token[2] = b'c';
                token[3] = b'r';
                token[4] = b'e';
                token[5] = b't';
                token
            },
            seal: 0,
        }
        .sealed()
    }

    fn direct_virtio_layout() -> DirectVirtioLayout {
        let mut rx_paddrs = [0u64; DIRECT_VIRTIO_BUFFER_COUNT];
        let mut tx_paddrs = [0u64; DIRECT_VIRTIO_BUFFER_COUNT];
        let mut index = 0usize;
        while index < DIRECT_VIRTIO_BUFFER_COUNT {
            rx_paddrs[index] = 0x4000_2000 + (index * DIRECT_VIRTIO_PAGE_BYTES) as u64;
            tx_paddrs[index] = 0x4001_2000 + (index * DIRECT_VIRTIO_PAGE_BYTES) as u64;
            index += 1;
        }
        DirectVirtioLayout {
            magic: DIRECT_VIRTIO_LAYOUT_MAGIC,
            version: DIRECT_VIRTIO_LAYOUT_VERSION,
            layout_bytes: DIRECT_VIRTIO_LAYOUT_BYTES as u16,
            flags: 0,
            queue_size: DIRECT_VIRTIO_QUEUE_SIZE as u16,
            buffer_count: DIRECT_VIRTIO_BUFFER_COUNT as u16,
            mmio_vaddr: 0x7204_0000,
            mmio_paddr: 0x0a00_0000,
            queue_vaddrs: [0x7205_0000, 0x7205_1000],
            queue_paddrs: [0x4000_0000, 0x4000_1000],
            rx_vaddr: 0x7206_0000,
            tx_vaddr: 0x7207_0000,
            rx_paddrs,
            tx_paddrs,
            seal: 0,
        }
        .sealed()
    }

    fn direct_genet_layout() -> DirectGenetLayout {
        let base = 0x7300_0000u64;
        let mut rx_vaddrs = [0u64; DIRECT_GENET_RX_SLOT_COUNT];
        let mut tx_vaddrs = [0u64; DIRECT_GENET_TX_SLOT_COUNT];
        let mut index = 0usize;
        while index < DIRECT_GENET_RX_SLOT_COUNT {
            rx_vaddrs[index] =
                base + ((DIRECT_GENET_RX_FIRST_PAGE_INDEX + index) * SHARED_PAGE_BYTES) as u64;
            index += 1;
        }
        index = 0;
        while index < DIRECT_GENET_TX_SLOT_COUNT {
            tx_vaddrs[index] =
                base + ((DIRECT_GENET_TX_FIRST_PAGE_INDEX + index) * SHARED_PAGE_BYTES) as u64;
            index += 1;
        }
        DirectGenetLayout {
            magic: DIRECT_GENET_LAYOUT_MAGIC,
            version: DIRECT_GENET_LAYOUT_VERSION,
            layout_bytes: DIRECT_GENET_LAYOUT_BYTES as u16,
            flags: DIRECT_GENET_LAYOUT_FLAGS,
            shared_page_bytes: SHARED_PAGE_BYTES as u16,
            rx_slot_count: DIRECT_GENET_RX_SLOT_COUNT as u8,
            tx_slot_count: DIRECT_GENET_TX_SLOT_COUNT as u8,
            generation: 7,
            peer_wake_notification_slot: DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT,
            reserved0: 0,
            control_vaddr: base,
            rx_vaddrs,
            tx_vaddrs,
            seal: 0,
        }
        .sealed()
    }

    #[test]
    fn pages_are_exact_and_sequence_last() {
        assert_eq!(size_of::<PacketPage>(), SHARED_PAGE_BYTES);
        assert_eq!(size_of::<ExchangePage>(), SHARED_PAGE_BYTES);
        let mut packet = PacketPage::empty(PacketDirection::Ingress, 7);
        packet.publish(1, &[1, 2, 3]).unwrap();
        assert_eq!(packet.validate(7, 0).unwrap().1, &[1, 2, 3]);
        packet.committed_sequence = 0;
        assert_eq!(packet.validate(7, 0), Err(AbiError::InvalidSequence));
    }

    #[test]
    fn bounded_packet_io_preserves_v3_layout_and_ignores_padding() {
        assert_eq!(PACKET_COMMIT_OFFSET, 24);
        assert_eq!(PACKET_LENGTH_OFFSET, 32);
        assert_eq!(PACKET_HEADER_BYTES, PACKET_PAYLOAD_OFFSET);

        let mut compact = [0xa5; SHARED_PAGE_BYTES];
        PacketPage::publish_into(&mut compact, PacketDirection::Ingress, 7, 1, &[1, 2, 3]).unwrap();
        assert!(compact[PACKET_PAYLOAD_OFFSET + 3..]
            .iter()
            .all(|byte| *byte == 0xa5));
        let decoded = PacketPage::decode_bounded(&compact, 7, 0).unwrap();
        assert_eq!(decoded.direction(), PacketDirection::Ingress);
        assert_eq!(decoded.sequence(), 1);
        assert_eq!(decoded.packet(), &[1, 2, 3]);

        compact[PACKET_COMMIT_OFFSET..PACKET_COMMIT_OFFSET + 8]
            .copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            PacketPage::decode_bounded(&compact, 7, 0),
            Err(AbiError::InvalidSequence)
        );
    }

    #[test]
    fn bounded_exchange_io_is_active_length_only_and_fail_closed() {
        assert_eq!(EXCHANGE_LENGTH_OFFSET, 10);
        assert_eq!(EXCHANGE_COMMIT_OFFSET, 56);
        assert_eq!(EXCHANGE_HEADER_BYTES, EXCHANGE_PAYLOAD_OFFSET);

        let mut compact = [0x5a; SHARED_PAGE_BYTES];
        ExchangePage::publish_related_into(
            &mut compact,
            ExchangeKind::Command,
            7,
            3,
            9,
            20,
            0,
            b"cat /proc/boot",
        )
        .unwrap();
        assert!(compact[EXCHANGE_PAYLOAD_OFFSET + b"cat /proc/boot".len()..]
            .iter()
            .all(|byte| *byte == 0x5a));
        let decoded = ExchangePage::decode_bounded(&compact, 7, 0, false).unwrap();
        assert_eq!(decoded.kind(), ExchangeKind::Command);
        assert_eq!(decoded.connection_id(), 9);
        assert_eq!(decoded.payload(), b"cat /proc/boot");

        compact[EXCHANGE_COMMIT_OFFSET..EXCHANGE_COMMIT_OFFSET + 8]
            .copy_from_slice(&4u64.to_le_bytes());
        assert_eq!(
            ExchangePage::decode_bounded(&compact, 7, 0, false),
            Err(AbiError::InvalidSequence)
        );
        assert_eq!(
            ExchangePage::publish_related_into(
                &mut compact,
                ExchangeKind::Command,
                7,
                4,
                9,
                21,
                0,
                &[b'x'; COMMAND_LINE_BYTES + 1],
            ),
            Err(AbiError::InvalidBound)
        );
    }

    #[test]
    fn send_batch_builder_and_cursor_preserve_exact_eight_record_bound() {
        let max_line_bytes = [b'x'; SEND_BATCH_LINE_BYTES];
        let max_line = core::str::from_utf8(&max_line_bytes).unwrap();
        let mut storage = [0x5au8; CONSOLE_PAYLOAD_BYTES];
        let active_len;
        {
            let mut builder = SendBatchBuilder::new(&mut storage);
            for _ in 0..SEND_BATCH_MAX_RECORDS {
                assert_eq!(builder.try_push_line(max_line), Ok(true));
            }
            assert_eq!(builder.record_count(), SEND_BATCH_MAX_RECORDS);
            assert_eq!(builder.try_push_line("ninth"), Ok(false));
            let payload = builder.finish().unwrap();
            active_len = payload.len();
            assert_eq!(active_len, 2_072);
            assert_eq!(read_u16(payload, 0), SEND_BATCH_ENCODING_VERSION);
            assert_eq!(usize::from(read_u16(payload, 2)), SEND_BATCH_MAX_RECORDS);
            assert_eq!(usize::from(read_u16(payload, 4)), active_len - 8);
            assert_eq!(read_u16(payload, 6), 0);

            let mut cursor = SendBatchCursor::validate(payload).unwrap();
            for remaining in (1..=SEND_BATCH_MAX_RECORDS).rev() {
                assert_eq!(cursor.remaining(), remaining);
                assert_eq!(cursor.next_line(payload).unwrap(), Some(max_line));
            }
            assert!(cursor.is_empty());
            assert_eq!(cursor.next_line(payload), Ok(None));
        }
        assert!(storage[active_len..].iter().all(|byte| *byte == 0x5a));
    }

    #[test]
    fn command_batch_preserves_order_timestamps_and_exact_eight_record_bound() {
        let mut storage = [0x5au8; CONSOLE_PAYLOAD_BYTES];
        let active_len;
        {
            let mut builder = CommandBatchBuilder::new(&mut storage);
            for index in 0..COMMAND_BATCH_MAX_RECORDS {
                assert_eq!(
                    builder.try_push_command(100 + index as u64, "x\n"),
                    Ok(true)
                );
            }
            assert_eq!(builder.record_count(), COMMAND_BATCH_MAX_RECORDS);
            assert_eq!(builder.try_push_command(999, "ninth"), Ok(false));
            let payload = builder.finish().unwrap();
            active_len = payload.len();
            assert_eq!(active_len, 104);

            let mut cursor = CommandBatchCursor::validate(payload).unwrap();
            for index in 0..COMMAND_BATCH_MAX_RECORDS {
                assert_eq!(cursor.remaining(), COMMAND_BATCH_MAX_RECORDS - index);
                assert_eq!(
                    cursor.next_command(payload).unwrap(),
                    Some((100 + index as u64, "x\n"))
                );
            }
            assert!(cursor.is_empty());
            assert_eq!(cursor.next_command(payload), Ok(None));
        }
        assert!(storage[active_len..].iter().all(|byte| *byte == 0x5a));
    }

    #[test]
    fn command_batch_rejects_inexact_records_and_respects_payload_capacity() {
        let max_command_bytes = [b'x'; COMMAND_LINE_BYTES];
        let max_command = core::str::from_utf8(&max_command_bytes).unwrap();
        let mut storage = [0u8; CONSOLE_PAYLOAD_BYTES];
        let active_len = {
            let mut builder = CommandBatchBuilder::new(&mut storage);
            assert_eq!(builder.try_push_command(7, max_command), Ok(true));
            let second_bytes = [b'y'; 40];
            let second = core::str::from_utf8(&second_bytes).unwrap();
            assert_eq!(builder.try_push_command(8, second), Ok(false));
            builder.finish().unwrap().len()
        };
        assert_eq!(
            active_len,
            COMMAND_BATCH_HEADER_BYTES + 10 + COMMAND_LINE_BYTES
        );
        assert!(CommandBatchCursor::validate(&storage[..active_len]).is_ok());

        let valid = storage;
        storage[2..4].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            CommandBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidBound)
        );
        storage = valid;
        storage[4..6].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            CommandBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidLayout)
        );
        storage = valid;
        storage[COMMAND_BATCH_HEADER_BYTES + 8..COMMAND_BATCH_HEADER_BYTES + 10]
            .copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            CommandBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidBound)
        );

        let mut page = ExchangePage::empty(7);
        page.publish(ExchangeKind::CommandBatch, 1, 9, 20, &valid[..active_len])
            .unwrap();
        assert_eq!(
            page.validate(7, 0, false).unwrap().0,
            ExchangeKind::CommandBatch
        );
    }

    #[test]
    fn send_batch_rejects_noncanonical_or_inexact_binary_records() {
        let mut storage = [0u8; CONSOLE_PAYLOAD_BYTES];
        let active_len = {
            let mut builder = SendBatchBuilder::new(&mut storage);
            assert_eq!(builder.try_push_line("\r\n"), Err(AbiError::InvalidBound));
            assert_eq!(
                builder.try_push_line("ACK\nforged"),
                Err(AbiError::InvalidBound)
            );
            assert_eq!(
                builder.try_push_line("ACK\rforged"),
                Err(AbiError::InvalidBound)
            );
            let oversized = [b'x'; SEND_BATCH_LINE_BYTES + 1];
            let oversized = core::str::from_utf8(&oversized).unwrap();
            assert_eq!(
                builder.try_push_line(oversized),
                Err(AbiError::InvalidBound)
            );
            assert_eq!(builder.try_push_line("ACK CAT"), Ok(true));
            assert_eq!(builder.try_push_line("END CAT\r\n"), Ok(true));
            builder.finish().unwrap().len()
        };
        assert!(SendBatchCursor::validate(&storage[..active_len]).is_ok());

        let valid = storage;
        storage[0..2].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidIdentity)
        );
        storage = valid;
        storage[2..4].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidBound)
        );
        storage = valid;
        storage[2..4].copy_from_slice(&((SEND_BATCH_MAX_RECORDS + 1) as u16).to_le_bytes());
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidBound)
        );
        storage = valid;
        storage[4..6].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidLayout)
        );
        storage = valid;
        storage[6..8].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidLayout)
        );
        storage = valid;
        storage[8..10].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidBound)
        );
        storage = valid;
        storage[8..10].copy_from_slice(&((SEND_BATCH_LINE_BYTES + 1) as u16).to_le_bytes());
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidBound)
        );
        storage = valid;
        storage[10] = 0xff;
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidBound)
        );
        storage = valid;
        storage[11] = b'\n';
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidBound)
        );
        storage = valid;
        storage[11] = b'\r';
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidBound)
        );
        storage = valid;
        storage[10 + usize::from(read_u16(&storage, 8)) - 1] = b'\n';
        assert_eq!(
            SendBatchCursor::validate(&storage[..active_len]),
            Err(AbiError::InvalidBound)
        );
        assert_eq!(
            SendBatchCursor::validate(&valid[..active_len - 1]),
            Err(AbiError::InvalidLayout)
        );
        let mut trailing = valid;
        trailing[active_len] = b'x';
        assert_eq!(
            SendBatchCursor::validate(&trailing[..active_len + 1]),
            Err(AbiError::InvalidLayout)
        );
    }

    #[test]
    fn exchange_page_validates_send_batch_as_binary_not_whole_payload_utf8() {
        let line_bytes = [b'x'; 200];
        let line = core::str::from_utf8(&line_bytes).unwrap();
        let mut batch_storage = [0u8; CONSOLE_PAYLOAD_BYTES];
        let batch_len = {
            let mut builder = SendBatchBuilder::new(&mut batch_storage);
            assert_eq!(builder.try_push_line(line), Ok(true));
            builder.finish().unwrap().len()
        };
        assert!(core::str::from_utf8(&batch_storage[..batch_len]).is_err());

        let mut page = [0u8; SHARED_PAGE_BYTES];
        ExchangePage::publish_related_into(
            &mut page,
            ExchangeKind::SendBatch,
            7,
            4,
            9,
            21,
            0,
            &batch_storage[..batch_len],
        )
        .unwrap();
        let record = ExchangePage::decode_bounded(&page, 7, 0, true).unwrap();
        assert_eq!(record.kind(), ExchangeKind::SendBatch);
        let mut cursor = SendBatchCursor::validate(record.payload()).unwrap();
        assert_eq!(cursor.next_line(record.payload()).unwrap(), Some(line));
        assert_eq!(cursor.next_line(record.payload()), Ok(None));

        assert_eq!(
            ExchangePage::publish_related_into(
                &mut page,
                ExchangeKind::SendLine,
                7,
                5,
                9,
                22,
                0,
                &[0xff],
            ),
            Err(AbiError::InvalidBound)
        );
    }

    #[test]
    fn directions_and_generations_fail_closed() {
        let mut exchange = ExchangePage::empty(7);
        exchange
            .publish(ExchangeKind::Command, 1, 9, 20, b"cat /proc/boot")
            .unwrap();
        assert_eq!(exchange.validate(7, 0, true), Err(AbiError::InvalidKind));
        assert_eq!(
            exchange.validate(8, 0, false),
            Err(AbiError::StaleGeneration)
        );
    }

    #[test]
    fn completion_watermarks_preserve_the_semantic_event_slot() {
        let generation = 7;
        let mut page = [0u8; SHARED_PAGE_BYTES];
        ExchangePage::empty(generation).encode(&mut page).unwrap();
        assert_eq!(
            ExchangePage::completion_watermarks(&page, generation),
            Ok(CompletionWatermarks::default())
        );
        assert_eq!(
            ExchangePage::publication_pending(&page, generation, 0),
            Ok(false)
        );

        page[INGRESS_CONSUMED_SEQUENCE_OFFSET..INGRESS_CONSUMED_SEQUENCE_OFFSET + 8]
            .copy_from_slice(&3u64.to_le_bytes());
        page[CONTROL_CONSUMED_SEQUENCE_OFFSET..CONTROL_CONSUMED_SEQUENCE_OFFSET + 8]
            .copy_from_slice(&5u64.to_le_bytes());
        ExchangePage::publish_related_into(
            &mut page,
            ExchangeKind::Ready,
            generation,
            1,
            0,
            20,
            0,
            CONSOLE_NETWORK_SERVICE_IDENTITY,
        )
        .unwrap();

        assert_eq!(
            ExchangePage::completion_watermarks(&page, generation),
            Ok(CompletionWatermarks {
                ingress_sequence: 3,
                control_sequence: 5,
            })
        );
        assert_eq!(
            ExchangePage::publication_pending(&page, generation, 0),
            Ok(true)
        );
        assert_eq!(
            ExchangePage::publication_pending(&page, generation, 1),
            Ok(false)
        );
        assert_eq!(
            ExchangePage::completion_watermarks(&page, generation + 1),
            Err(AbiError::StaleGeneration)
        );
    }

    #[test]
    fn event_readiness_admits_only_causal_in_progress_replacement() {
        assert_eq!(classify_publication_commit(0, 2, 1), Ok(false));
        assert_eq!(classify_publication_commit(1, 0, 1), Ok(false));
        assert_eq!(classify_publication_commit(1, 2, 1), Ok(false));
        assert_eq!(classify_publication_commit(2, 2, 1), Ok(true));
        assert_eq!(classify_publication_commit(1, 1, 1), Ok(false));
        assert_eq!(
            classify_publication_commit(2, 0, 1),
            Err(AbiError::InvalidSequence)
        );
        assert_eq!(
            classify_publication_commit(1, 1, 2),
            Err(AbiError::InvalidSequence)
        );
    }

    #[test]
    fn command_and_output_drain_records_are_exactly_bounded() {
        let mut command = ExchangePage::empty(7);
        let oversized = [b'x'; COMMAND_LINE_BYTES + 1];
        assert_eq!(
            command.publish(ExchangeKind::Command, 1, 9, 20, &oversized),
            Err(AbiError::InvalidBound)
        );

        let mut drained = ExchangePage::empty(7);
        drained
            .publish_related(ExchangeKind::OutputDrained, 2, 9, 21, 4, &[])
            .unwrap();
        assert_eq!(
            drained.validate(7, 1, false).unwrap().0,
            ExchangeKind::OutputDrained
        );
        assert_eq!(
            drained.publish_related(ExchangeKind::OutputDrained, 3, 0, 22, 4, &[]),
            Err(AbiError::InvalidBound)
        );
    }

    #[test]
    fn descriptor_seal_binds_authority_and_secret_tail() {
        let valid = descriptor();
        assert_eq!(valid.validate(), Ok(()));
        assert_eq!(ABI_VERSION, 5);
        assert_eq!(
            CONSOLE_NETWORK_SERVICE_IDENTITY,
            b"console-network-service/v5"
        );
        assert_eq!(ExchangeKind::SendBatch as u16, 3);
        assert_eq!(ExchangeKind::CommandBatch as u16, 27);
        assert_eq!(WAKE_PUBLICATION_ACK, 64);
        assert_eq!(WAKE_DIRECT_VIRTIO_IRQ, 128);
        assert_eq!(WAKE_DIRECT_GENET_LINK, 256);
        assert_eq!(ROOT_WAKE_MASK, 79);
        assert_eq!(REQUIRED_INIT_FLAGS & INIT_FLAG_PUBLICATION_ACK, 16);
        assert_eq!(REQUIRED_INIT_FLAGS & INIT_FLAG_COMPLETION_WATERMARKS, 32);
        let mut broad = valid;
        broad.child_cspace_slots = 32;
        assert_eq!(broad.validate(), Err(AbiError::InvalidAuthority));
        let mut stale = valid;
        stale.generation = 8;
        assert_eq!(stale.validate(), Err(AbiError::InvalidSeal));
        let mut trailing = valid;
        trailing.auth_token[63] = 1;
        trailing = trailing.sealed();
        assert_eq!(trailing.validate(), Err(AbiError::InvalidBound));
    }

    #[test]
    fn direct_virtio_extension_is_exact_sealed_and_disjoint() {
        let layout = direct_virtio_layout();
        assert_eq!(layout.validate(), Ok(()));
        assert_eq!(size_of::<DirectVirtioLayout>(), DIRECT_VIRTIO_LAYOUT_BYTES);
        let mut encoded = [0u8; DIRECT_VIRTIO_LAYOUT_BYTES];
        layout.encode(&mut encoded).unwrap();
        assert_eq!(DirectVirtioLayout::decode(&encoded), Ok(layout));

        let descriptor = descriptor().with_direct_virtio();
        assert!(descriptor.direct_virtio());
        assert_eq!(descriptor.root_wake_mask, 207);
        assert_eq!(descriptor.validate(), Ok(()));

        let mut overlapping = layout;
        overlapping.tx_paddrs[0] = overlapping.rx_paddrs[0];
        overlapping = overlapping.sealed();
        assert_eq!(overlapping.validate(), Err(AbiError::InvalidLayout));

        let mut unknown_flag = descriptor;
        unknown_flag.flags |= 1 << 31;
        unknown_flag = unknown_flag.sealed();
        assert_eq!(unknown_flag.validate(), Err(AbiError::InvalidAuthority));
    }

    #[test]
    fn direct_genet_extension_is_v5_exact_sealed_and_disjoint() {
        assert_eq!(DIRECT_GENET_SHARED_PAGE_COUNT, 32);
        assert_eq!(DIRECT_GENET_RX_SLOT_COUNT, 15);
        assert_eq!(DIRECT_GENET_TX_SLOT_COUNT, 16);

        let layout = direct_genet_layout();
        assert_eq!(layout.validate_for(7), Ok(()));
        assert_eq!(size_of::<DirectGenetLayout>(), DIRECT_GENET_LAYOUT_BYTES);
        let mut encoded = [0u8; DIRECT_GENET_LAYOUT_BYTES];
        layout.encode(&mut encoded).unwrap();
        assert_eq!(DirectGenetLayout::decode(&encoded), Ok(layout));

        let descriptor = descriptor().with_direct_genet();
        assert!(descriptor.direct_genet());
        assert!(!descriptor.direct_virtio());
        assert_eq!(
            descriptor.root_wake_mask,
            ROOT_WAKE_MASK | WAKE_DIRECT_GENET_LINK
        );
        assert_eq!(descriptor.validate(), Ok(()));
        assert_eq!(descriptor.validate_direct_genet_layout(layout), Ok(()));

        let both = descriptor.with_direct_virtio();
        assert_eq!(both.validate(), Err(AbiError::InvalidAuthority));

        let mut duplicate = layout;
        duplicate.rx_vaddrs[0] = duplicate.control_vaddr;
        duplicate = duplicate.sealed();
        assert_eq!(duplicate.validate(), Err(AbiError::InvalidLayout));

        let mut legacy_overlap = layout;
        legacy_overlap.control_vaddr = descriptor.packet_rx_vaddr;
        legacy_overlap = legacy_overlap.sealed();
        assert_eq!(legacy_overlap.validate(), Ok(()));
        assert_eq!(
            descriptor.validate_direct_genet_layout(legacy_overlap),
            Err(AbiError::InvalidLayout)
        );
        assert_eq!(layout.validate_for(8), Err(AbiError::StaleGeneration));
    }

    #[test]
    fn direct_genet_runtime_diagnostic_is_sequence_last_and_generation_exact() {
        let mut diagnostic = DirectGenetRuntimeDiagnostic::empty();
        diagnostic.flags = DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_INITIALIZED
            | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_ACTIVE
            | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_RX_RING_VALID
            | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_TX_RING_VALID
            | DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_MMIO_SAMPLED;
        diagnostic.generation = 7;
        diagnostic.publication_sequence = 3;
        diagnostic.irq_badge = 1 << 10;
        diagnostic.irq_wakes = 2;
        diagnostic.irq_acks = 3;
        diagnostic.dpc_turns = 4;
        diagnostic.irq_raw = 0x0001_2000;
        diagnostic.irq_mask = 0x0001_0000;
        diagnostic.irq_active = 0x0000_2000;
        diagnostic.rdma_producer = 11;
        diagnostic.rdma_consumer = 9;
        diagnostic.direct_rx_packets = 8;
        diagnostic.direct_tx_packets = 6;
        diagnostic.peer_wakes = 5;
        diagnostic.peer_signals = 4;
        diagnostic.rx_producer_cursor = 8;
        diagnostic.rx_consumer_cursor = 7;
        diagnostic.tx_producer_cursor = 6;
        diagnostic.tx_consumer_cursor = 6;
        diagnostic.committed_sequence = 3;
        assert!(diagnostic.valid_for(7));

        let mut encoded = [0u8; DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES];
        diagnostic.encode(&mut encoded).unwrap();
        assert_eq!(
            DirectGenetRuntimeDiagnostic::decode(&encoded, 7),
            Ok(diagnostic)
        );
        assert_eq!(
            DirectGenetRuntimeDiagnostic::decode(&encoded, 8),
            Err(DirectGenetError::InvalidLayout)
        );

        let exact = encoded;
        encoded[DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET
            ..DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET + 8]
            .copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            DirectGenetRuntimeDiagnostic::decode(&encoded, 7),
            Err(DirectGenetError::InvalidLayout)
        );
        encoded = exact;
        encoded[8..12].copy_from_slice(&(1u32 << 31).to_le_bytes());
        assert_eq!(
            DirectGenetRuntimeDiagnostic::decode(&encoded, 7),
            Err(DirectGenetError::InvalidLayout)
        );
    }

    #[test]
    fn direct_genet_ring_moves_one_exact_sequence_and_reconciles_commits() {
        let mut control = [0xa5u8; SHARED_PAGE_BYTES];
        DirectGenetControlPage::initialize_into(&mut control, 7).unwrap();
        assert!(control[320..].iter().all(|byte| *byte == 0));

        let initial =
            DirectGenetControlPage::snapshot(&control, 7, DirectGenetDirection::Rx).unwrap();
        assert_eq!(initial.occupancy(), 0);
        assert_eq!(initial.next_producer(), Ok((1, 0)));
        assert_eq!(initial.next_consumer(), Err(DirectGenetError::Empty));

        let mut slot = [0xa5u8; SHARED_PAGE_BYTES];
        DirectGenetSlotPage::initialize_into(&mut slot).unwrap();
        let frame = [0xabu8; 64];
        assert_eq!(
            DirectGenetSlotPage::publish_next_into(
                &mut slot,
                DirectGenetDirection::Rx,
                7,
                initial.producer_cursor,
                &frame,
            ),
            Ok(1)
        );
        let published = DirectGenetControlPage::commit_producer(&mut control, initial).unwrap();
        assert_eq!(published.sequence, 1);
        assert_eq!(published.slot_index, 0);
        assert!(published.data_notification_due);
        assert_eq!(
            DirectGenetControlPage::commit_producer(&mut control, initial),
            Ok(published),
            "retrying one ambiguous commit must not advance twice"
        );

        let ready =
            DirectGenetControlPage::snapshot(&control, 7, DirectGenetDirection::Rx).unwrap();
        assert_eq!(ready.occupancy(), 1);
        assert_eq!(ready.next_consumer(), Ok((1, 0)));
        let record = DirectGenetSlotPage::decode_next(
            &slot,
            DirectGenetDirection::Rx,
            7,
            ready.consumer_cursor,
        )
        .unwrap();
        assert_eq!(record.direction(), DirectGenetDirection::Rx);
        assert_eq!(record.sequence(), 1);
        assert_eq!(record.frame(), frame);

        let consumed = DirectGenetControlPage::commit_consumer(&mut control, ready).unwrap();
        assert_eq!(consumed.sequence, 1);
        assert!(!consumed.work_remaining);
        assert_eq!(
            DirectGenetControlPage::commit_consumer(&mut control, ready),
            Ok(consumed),
            "retrying one ambiguous consume must not advance twice"
        );
        assert_eq!(
            DirectGenetControlPage::snapshot(&control, 7, DirectGenetDirection::Rx)
                .unwrap()
                .occupancy(),
            0
        );
        assert_eq!(
            DirectGenetControlPage::snapshot(&control, 8, DirectGenetDirection::Rx),
            Err(DirectGenetError::StaleGeneration)
        );
        assert_eq!(direct_genet_slot_index(DirectGenetDirection::Rx, 16), Ok(0));
        assert_eq!(direct_genet_slot_index(DirectGenetDirection::Tx, 17), Ok(0));
    }

    #[test]
    fn direct_genet_cursors_fail_closed_on_full_torn_invalid_and_poisoned_state() {
        let mut control = [0u8; SHARED_PAGE_BYTES];
        DirectGenetControlPage::initialize_into(&mut control, 7).unwrap();
        for expected in 1..=DIRECT_GENET_RX_SLOT_COUNT as u64 {
            let snapshot =
                DirectGenetControlPage::snapshot(&control, 7, DirectGenetDirection::Rx).unwrap();
            assert_eq!(snapshot.next_producer().unwrap().0, expected);
            DirectGenetControlPage::commit_producer(&mut control, snapshot).unwrap();
        }
        let full = DirectGenetControlPage::snapshot(&control, 7, DirectGenetDirection::Rx).unwrap();
        assert_eq!(full.occupancy(), DIRECT_GENET_RX_SLOT_COUNT as u64);
        assert_eq!(full.next_producer(), Err(DirectGenetError::Backpressure));

        let first_consumer = full;
        let consumed =
            DirectGenetControlPage::commit_consumer(&mut control, first_consumer).unwrap();
        assert!(consumed.producer_rearm_due);
        assert!(consumed.work_remaining);

        let mut invalid = [0u8; SHARED_PAGE_BYTES];
        DirectGenetControlPage::initialize_into(&mut invalid, 7).unwrap();
        let producer_offset = DIRECT_GENET_RX_PRODUCER_STATE_OFFSET;
        invalid[producer_offset + 16..producer_offset + 24].copy_from_slice(&16u64.to_le_bytes());
        invalid[producer_offset + 24..producer_offset + 32].copy_from_slice(&17u64.to_le_bytes());
        invalid[producer_offset + 56..producer_offset + 64].copy_from_slice(&17u64.to_le_bytes());
        assert_eq!(
            DirectGenetControlPage::snapshot(&invalid, 7, DirectGenetDirection::Rx),
            Err(DirectGenetError::InvalidCursor)
        );

        let mut torn = [0u8; SHARED_PAGE_BYTES];
        DirectGenetControlPage::initialize_into(&mut torn, 7).unwrap();
        torn[producer_offset + 56..producer_offset + 64].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            DirectGenetControlPage::snapshot(&torn, 7, DirectGenetDirection::Rx),
            Err(DirectGenetError::StateChanged)
        );

        let mut poisoned = [0u8; SHARED_PAGE_BYTES];
        DirectGenetControlPage::initialize_into(&mut poisoned, 7).unwrap();
        assert_eq!(
            DirectGenetControlPage::poison_owner(
                &mut poisoned,
                7,
                DirectGenetCursorRole::RxProducer,
                1,
                DIRECT_GENET_POISON_INVALID_CURSOR,
            ),
            Ok(DirectGenetPoison {
                role: DirectGenetCursorRole::RxProducer,
                reason: DIRECT_GENET_POISON_INVALID_CURSOR,
            })
        );
        assert_eq!(
            DirectGenetControlPage::snapshot(&poisoned, 7, DirectGenetDirection::Rx),
            Err(DirectGenetError::Poisoned(DirectGenetPoison {
                role: DirectGenetCursorRole::RxProducer,
                reason: DIRECT_GENET_POISON_INVALID_CURSOR,
            }))
        );
    }

    #[test]
    fn direct_genet_slot_tampering_and_lost_wake_edges_are_exact() {
        let mut slot = [0u8; SHARED_PAGE_BYTES];
        DirectGenetSlotPage::initialize_into(&mut slot).unwrap();
        DirectGenetSlotPage::publish_next_into(
            &mut slot,
            DirectGenetDirection::Tx,
            7,
            0,
            &[1, 2, 3],
        )
        .unwrap();
        assert_eq!(
            DirectGenetSlotPage::decode_next(&slot, DirectGenetDirection::Tx, 7, 0)
                .unwrap()
                .frame(),
            &[1, 2, 3]
        );
        assert_eq!(
            DirectGenetSlotPage::decode_next(&slot, DirectGenetDirection::Rx, 7, 0),
            Err(DirectGenetError::InvalidIdentity)
        );
        assert_eq!(
            DirectGenetSlotPage::decode_next(&slot, DirectGenetDirection::Tx, 8, 0),
            Err(DirectGenetError::StaleGeneration)
        );
        slot[DIRECT_GENET_SLOT_COMMIT_OFFSET..DIRECT_GENET_SLOT_COMMIT_OFFSET + 8]
            .copy_from_slice(&2u64.to_le_bytes());
        assert_eq!(
            DirectGenetSlotPage::decode_next(&slot, DirectGenetDirection::Tx, 7, 0),
            Err(DirectGenetError::InvalidSequence)
        );
        assert_eq!(
            DirectGenetSlotPage::publish_next_into(
                &mut slot,
                DirectGenetDirection::Tx,
                7,
                u64::MAX,
                &[1],
            ),
            Err(DirectGenetError::InvalidSequence)
        );

        let mut control = [0u8; SHARED_PAGE_BYTES];
        DirectGenetControlPage::initialize_into(&mut control, 7).unwrap();
        let empty =
            DirectGenetControlPage::snapshot(&control, 7, DirectGenetDirection::Tx).unwrap();
        DirectGenetControlPage::commit_producer(&mut control, empty).unwrap();
        let occupied =
            DirectGenetControlPage::snapshot(&control, 7, DirectGenetDirection::Tx).unwrap();
        DirectGenetControlPage::commit_consumer(&mut control, occupied).unwrap();
        let raced = DirectGenetControlPage::commit_producer(&mut control, occupied).unwrap();
        assert!(
            raced.data_notification_due,
            "a consumer draining through the old producer cursor requires a hint"
        );
        let final_state =
            DirectGenetControlPage::snapshot(&control, 7, DirectGenetDirection::Tx).unwrap();
        assert_eq!(
            occupied.reconcile_producer_commit(final_state, raced.sequence),
            Ok(raced)
        );
    }

    #[test]
    fn descriptor_wire_form_is_exact_and_rejects_padding_data() {
        let descriptor = descriptor();
        let mut encoded = [0u8; RUNTIME_INIT_DESCRIPTOR_BYTES];
        descriptor.encode(&mut encoded).unwrap();
        assert_eq!(RuntimeInitDescriptor::decode(&encoded), Ok(descriptor));
        assert!(encoded[44..48].iter().all(|byte| *byte == 0));

        encoded[45] = 1;
        assert_eq!(
            RuntimeInitDescriptor::decode(&encoded),
            Err(AbiError::InvalidLayout)
        );
    }
}
