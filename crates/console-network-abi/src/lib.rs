// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define the pointer-free fixed-page ABI for the isolated console-network service.
// Author: Lukas Bower

#![no_std]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! Fixed-layout `console-network-service/v1` records.
//!
//! The root remains the sole owner of operator policy and command execution.
//! The child owns Ethernet/IP/TCP processing, transport authentication, and
//! framing. Four single-producer/single-consumer pages carry one durable packet,
//! control request, or event at a time. A producer stages a complete body and
//! writes `committed_sequence` last before signaling the peer. The consumer
//! validates the generation and identical sequence fields before use. Page
//! tails are construction-zeroed, containment-scrubbed, and carry no record
//! authority. Bounded service turns never copy or scan those padding bytes.

use core::mem::{align_of, size_of};
use core::sync::atomic::{fence, Ordering};

/// Runtime-init magic (`CNI1`).
pub const RUNTIME_INIT_MAGIC: u32 = 0x434e_4931;
/// Ethernet page magic (`CNP1`).
pub const PACKET_PAGE_MAGIC: u32 = 0x434e_5031;
/// Console exchange page magic (`CNE1`).
pub const EXCHANGE_PAGE_MAGIC: u32 = 0x434e_4531;
/// ABI version.
pub const ABI_VERSION: u16 = 1;
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
/// Maximum authenticated command bytes admitted to root's parser.
pub const COMMAND_LINE_BYTES: usize = 2304;
/// Maximum authentication token bytes passed only to the restricted child.
pub const AUTH_TOKEN_BYTES: usize = 64;
/// Exact serialized runtime-init descriptor bytes.
pub const RUNTIME_INIT_DESCRIPTOR_BYTES: usize = 224;
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
/// All allowed root-to-child notification bits.
pub const ROOT_WAKE_MASK: u64 = WAKE_PACKET_RX | WAKE_CONTROL | WAKE_SHUTDOWN | WAKE_REVOKE;
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
/// Exact required runtime flags.
pub const REQUIRED_INIT_FLAGS: u32 = INIT_FLAG_POINTER_FREE
    | INIT_FLAG_SEQUENCE_LAST
    | INIT_FLAG_SINGLE_LISTENER
    | INIT_FLAG_CHILD_AUTH_FRAMING;

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

    /// Initialized and UTF-8-validated payload bytes.
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
            _ => Err(AbiError::InvalidKind),
        }
    }

    /// Whether this kind may be produced by root.
    #[must_use]
    pub const fn root_to_child(self) -> bool {
        matches!(self, Self::SendLine | Self::Disconnect)
    }

    /// Whether this kind may be produced by the child.
    #[must_use]
    pub const fn child_to_root(self) -> bool {
        !self.root_to_child()
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
    /// Exact [`REQUIRED_INIT_FLAGS`].
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
    /// Maximum control records serviced per notification turn.
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
        if self.flags != REQUIRED_INIT_FLAGS
            || self.child_cspace_slots != CHILD_CSPACE_SLOTS
            || self.child_wake_notification_slot != CHILD_WAKE_NOTIFICATION_SLOT
            || self.packet_tx_wake_notification_slot != PACKET_TX_WAKE_NOTIFICATION_SLOT
            || self.supervisor_wake_notification_slot != SUPERVISOR_WAKE_NOTIFICATION_SLOT
            || self.fault_endpoint_slot != FAULT_ENDPOINT_SLOT
            || self.root_wake_mask != ROOT_WAKE_MASK
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
            || self.max_commands_per_wake > 16
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
    /// Bounded UTF-8 payload when required by the kind.
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
        if core::str::from_utf8(&payload[..payload_len]).is_err() {
            return Err(AbiError::InvalidBound);
        }
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
        if sequence == 0 || payload.len() > self.payload.len() {
            return Err(AbiError::InvalidBound);
        }
        if matches!(kind, ExchangeKind::SendLine | ExchangeKind::Command) && payload.is_empty() {
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
            || (matches!(kind, ExchangeKind::SendLine | ExchangeKind::Command) && len == 0)
            || (kind == ExchangeKind::Command && len > COMMAND_LINE_BYTES)
        {
            return Err(AbiError::InvalidBound);
        }
        if core::str::from_utf8(&self.payload[..len]).is_err() {
            return Err(AbiError::InvalidBound);
        }
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
    if matches!(kind, ExchangeKind::SendLine | ExchangeKind::Command) && payload_len == 0 {
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
const _: () = assert!(align_of::<ExchangePage>() == SHARED_PAGE_BYTES);
const _: () = assert!(size_of::<PacketPageHeader>() == PACKET_HEADER_BYTES);
const _: () = assert!(size_of::<ExchangePageHeader>() == EXCHANGE_HEADER_BYTES);
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
    fn bounded_packet_io_preserves_v1_layout_and_ignores_padding() {
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
