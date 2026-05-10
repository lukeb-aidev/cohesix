// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Deterministic no_std DHCPv4 state machine and packet codec for root-task networking.
// Author: Lukas Bower

//! Deterministic DHCPv4 client core for the root-task control-plane interface.

use super::NetDhcpConfig;

pub const DHCP_CLIENT_PORT: u16 = 68;
pub const DHCP_SERVER_PORT: u16 = 67;

const BOOTP_HEADER_LEN: usize = 236;
const DHCP_COOKIE_OFFSET: usize = BOOTP_HEADER_LEN;
const DHCP_FIXED_LEN: usize = BOOTP_HEADER_LEN + 4;
const DHCP_MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
const DHCP_OP_BOOTREQUEST: u8 = 1;
const DHCP_OP_BOOTREPLY: u8 = 2;
const DHCP_HTYPE_ETHERNET: u8 = 1;
const DHCP_HLEN_ETHERNET: u8 = 6;
const DHCP_FLAGS_BROADCAST: u16 = 0x8000;
const OPT_SUBNET_MASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_LEASE_TIME: u8 = 51;
const OPT_MESSAGE_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_PARAMETER_REQUEST_LIST: u8 = 55;
const OPT_CLIENT_ID: u8 = 61;
const OPT_END: u8 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DhcpLease {
    pub ip: [u8; 4],
    pub prefix_len: u8,
    pub gateway: Option<[u8; 4]>,
    pub server_id: [u8; 4],
    pub lease_seconds: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DhcpFailureReason {
    TimeoutExhausted,
    Nak,
    BufferTooSmall,
}

impl DhcpFailureReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TimeoutExhausted => "timeout-exhausted",
            Self::Nak => "nak",
            Self::BufferTooSmall => "buffer-too-small",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DhcpMetrics {
    pub tx_packets: u32,
    pub rx_packets: u32,
    pub invalid_packets: u32,
    pub timeouts: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DhcpPhase {
    Disabled,
    Selecting,
    Requesting,
    Bound,
    Failed,
}

impl DhcpPhase {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Selecting => "selecting",
            Self::Requesting => "requesting",
            Self::Bound => "bound",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DhcpStatus {
    pub phase: DhcpPhase,
    pub lease: Option<DhcpLease>,
    pub failure: Option<DhcpFailureReason>,
    pub metrics: DhcpMetrics,
    pub attempts: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DhcpEvent {
    None,
    SendQueued,
    LeaseAcquired(DhcpLease),
    Failed(DhcpFailureReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingOffer {
    lease: DhcpLease,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DhcpState {
    Disabled,
    Selecting {
        next_tx_ms: u64,
        deadline_ms: u64,
        attempts: u8,
    },
    Requesting {
        offer: PendingOffer,
        next_tx_ms: u64,
        deadline_ms: u64,
        attempts: u8,
    },
    Bound {
        lease: DhcpLease,
    },
    Failed {
        reason: DhcpFailureReason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DhcpMessageType {
    Discover,
    Offer,
    Request,
    Ack,
    Nak,
}

impl DhcpMessageType {
    #[must_use]
    const fn code(self) -> u8 {
        match self {
            Self::Discover => 1,
            Self::Offer => 2,
            Self::Request => 3,
            Self::Ack => 5,
            Self::Nak => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ParsedPacket {
    message_type: DhcpMessageType,
    yiaddr: [u8; 4],
    server_id: Option<[u8; 4]>,
    subnet_mask: Option<[u8; 4]>,
    gateway: Option<[u8; 4]>,
    lease_seconds: Option<u32>,
}

pub struct DhcpClient {
    config: NetDhcpConfig,
    xid: u32,
    state: DhcpState,
    metrics: DhcpMetrics,
}

impl DhcpClient {
    #[must_use]
    pub const fn new(config: NetDhcpConfig) -> Self {
        Self {
            config,
            xid: 0,
            state: DhcpState::Disabled,
            metrics: DhcpMetrics {
                tx_packets: 0,
                rx_packets: 0,
                invalid_packets: 0,
                timeouts: 0,
            },
        }
    }

    pub fn start(&mut self, mac: [u8; 6], now_ms: u64) {
        self.xid = make_xid(mac);
        self.metrics = DhcpMetrics::default();
        self.state = DhcpState::Selecting {
            next_tx_ms: now_ms,
            deadline_ms: now_ms.saturating_add(u64::from(self.config.discover_timeout_ms)),
            attempts: 0,
        };
    }

    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        !matches!(self.state, DhcpState::Disabled)
    }

    #[must_use]
    pub fn status(&self) -> DhcpStatus {
        match self.state {
            DhcpState::Disabled => DhcpStatus {
                phase: DhcpPhase::Disabled,
                lease: None,
                failure: None,
                metrics: self.metrics,
                attempts: 0,
            },
            DhcpState::Selecting { attempts, .. } => DhcpStatus {
                phase: DhcpPhase::Selecting,
                lease: None,
                failure: None,
                metrics: self.metrics,
                attempts,
            },
            DhcpState::Requesting {
                offer, attempts, ..
            } => DhcpStatus {
                phase: DhcpPhase::Requesting,
                lease: Some(offer.lease),
                failure: None,
                metrics: self.metrics,
                attempts,
            },
            DhcpState::Bound { lease } => DhcpStatus {
                phase: DhcpPhase::Bound,
                lease: Some(lease),
                failure: None,
                metrics: self.metrics,
                attempts: 0,
            },
            DhcpState::Failed { reason } => DhcpStatus {
                phase: DhcpPhase::Failed,
                lease: None,
                failure: Some(reason),
                metrics: self.metrics,
                attempts: 0,
            },
        }
    }

    pub fn on_timer(&mut self, now_ms: u64) -> DhcpEvent {
        match self.state {
            DhcpState::Selecting {
                attempts,
                deadline_ms,
                ..
            } if now_ms >= deadline_ms => self.timeout_selecting(now_ms, attempts),
            DhcpState::Requesting {
                offer,
                attempts,
                deadline_ms,
                ..
            } if now_ms >= deadline_ms => self.timeout_requesting(now_ms, offer, attempts),
            _ => DhcpEvent::None,
        }
    }

    pub fn build_outbound(
        &mut self,
        mac: [u8; 6],
        buffer: &mut [u8],
        now_ms: u64,
    ) -> Result<Option<usize>, DhcpFailureReason> {
        match self.state {
            DhcpState::Selecting {
                next_tx_ms,
                deadline_ms,
                attempts,
            } if now_ms >= next_tx_ms => {
                let len = encode_discover(self.xid, mac, buffer)?;
                self.metrics.tx_packets = self.metrics.tx_packets.saturating_add(1);
                self.state = DhcpState::Selecting {
                    next_tx_ms: u64::MAX,
                    deadline_ms,
                    attempts: attempts.saturating_add(1),
                };
                Ok(Some(len))
            }
            DhcpState::Requesting {
                offer,
                next_tx_ms,
                deadline_ms,
                attempts,
            } if now_ms >= next_tx_ms => {
                let len = encode_request(self.xid, mac, offer.lease, buffer)?;
                self.metrics.tx_packets = self.metrics.tx_packets.saturating_add(1);
                self.state = DhcpState::Requesting {
                    offer,
                    next_tx_ms: u64::MAX,
                    deadline_ms,
                    attempts: attempts.saturating_add(1),
                };
                Ok(Some(len))
            }
            _ => Ok(None),
        }
    }

    pub fn handle_packet(&mut self, mac: [u8; 6], packet: &[u8], now_ms: u64) -> DhcpEvent {
        let parsed = match parse_packet(packet, self.xid, mac) {
            Some(parsed) => parsed,
            None => {
                self.metrics.invalid_packets = self.metrics.invalid_packets.saturating_add(1);
                return DhcpEvent::None;
            }
        };
        self.metrics.rx_packets = self.metrics.rx_packets.saturating_add(1);

        match self.state {
            DhcpState::Selecting { .. } if parsed.message_type == DhcpMessageType::Offer => {
                let lease = match packet_to_lease(parsed) {
                    Some(lease) => lease,
                    None => {
                        self.metrics.invalid_packets =
                            self.metrics.invalid_packets.saturating_add(1);
                        return DhcpEvent::None;
                    }
                };
                self.state = DhcpState::Requesting {
                    offer: PendingOffer { lease },
                    next_tx_ms: now_ms,
                    deadline_ms: now_ms.saturating_add(u64::from(self.config.request_timeout_ms)),
                    attempts: 0,
                };
                DhcpEvent::SendQueued
            }
            DhcpState::Requesting { offer, .. } if parsed.message_type == DhcpMessageType::Ack => {
                let lease = match packet_to_lease(parsed) {
                    Some(lease) if lease.server_id == offer.lease.server_id => lease,
                    _ => {
                        self.metrics.invalid_packets =
                            self.metrics.invalid_packets.saturating_add(1);
                        return DhcpEvent::None;
                    }
                };
                self.state = DhcpState::Bound { lease };
                DhcpEvent::LeaseAcquired(lease)
            }
            DhcpState::Requesting { .. } if parsed.message_type == DhcpMessageType::Nak => {
                self.state = DhcpState::Failed {
                    reason: DhcpFailureReason::Nak,
                };
                DhcpEvent::Failed(DhcpFailureReason::Nak)
            }
            _ => DhcpEvent::None,
        }
    }

    fn timeout_selecting(&mut self, now_ms: u64, attempts: u8) -> DhcpEvent {
        self.metrics.timeouts = self.metrics.timeouts.saturating_add(1);
        if attempts >= self.config.max_retries {
            self.state = DhcpState::Failed {
                reason: DhcpFailureReason::TimeoutExhausted,
            };
            DhcpEvent::Failed(DhcpFailureReason::TimeoutExhausted)
        } else {
            self.state = DhcpState::Selecting {
                next_tx_ms: now_ms,
                deadline_ms: now_ms.saturating_add(u64::from(self.config.discover_timeout_ms)),
                attempts,
            };
            DhcpEvent::SendQueued
        }
    }

    fn timeout_requesting(&mut self, now_ms: u64, offer: PendingOffer, attempts: u8) -> DhcpEvent {
        self.metrics.timeouts = self.metrics.timeouts.saturating_add(1);
        if attempts >= self.config.max_retries {
            self.state = DhcpState::Failed {
                reason: DhcpFailureReason::TimeoutExhausted,
            };
            DhcpEvent::Failed(DhcpFailureReason::TimeoutExhausted)
        } else {
            self.state = DhcpState::Requesting {
                offer,
                next_tx_ms: now_ms,
                deadline_ms: now_ms.saturating_add(u64::from(self.config.request_timeout_ms)),
                attempts,
            };
            DhcpEvent::SendQueued
        }
    }
}

fn packet_to_lease(packet: ParsedPacket) -> Option<DhcpLease> {
    let server_id = packet.server_id?;
    let prefix_len = packet
        .subnet_mask
        .and_then(prefix_len_from_mask)
        .filter(|prefix| *prefix <= 32)?;
    Some(DhcpLease {
        ip: packet.yiaddr,
        prefix_len,
        gateway: packet.gateway,
        server_id,
        lease_seconds: packet.lease_seconds.unwrap_or(0),
    })
}

fn make_xid(mac: [u8; 6]) -> u32 {
    0x434f_4800
        ^ u32::from(mac[0]) << 24
        ^ u32::from(mac[1]) << 16
        ^ u32::from(mac[2]) << 8
        ^ u32::from(mac[3])
        ^ u32::from(mac[4]) << 4
        ^ u32::from(mac[5])
}

fn encode_discover(xid: u32, mac: [u8; 6], buffer: &mut [u8]) -> Result<usize, DhcpFailureReason> {
    let mut cursor = DhcpWriter::new(buffer)?;
    cursor.write_header(xid, mac);
    cursor.push_option_u8(OPT_MESSAGE_TYPE, DhcpMessageType::Discover.code())?;
    cursor.push_option_bytes(
        OPT_CLIENT_ID,
        &[
            DHCP_HTYPE_ETHERNET,
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5],
        ],
    )?;
    cursor.push_option_bytes(
        OPT_PARAMETER_REQUEST_LIST,
        &[OPT_SUBNET_MASK, OPT_ROUTER, OPT_LEASE_TIME, OPT_SERVER_ID],
    )?;
    cursor.finish()
}

fn encode_request(
    xid: u32,
    mac: [u8; 6],
    lease: DhcpLease,
    buffer: &mut [u8],
) -> Result<usize, DhcpFailureReason> {
    let mut cursor = DhcpWriter::new(buffer)?;
    cursor.write_header(xid, mac);
    cursor.push_option_u8(OPT_MESSAGE_TYPE, DhcpMessageType::Request.code())?;
    cursor.push_option_bytes(
        OPT_CLIENT_ID,
        &[
            DHCP_HTYPE_ETHERNET,
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5],
        ],
    )?;
    cursor.push_option_bytes(OPT_REQUESTED_IP, &lease.ip)?;
    cursor.push_option_bytes(OPT_SERVER_ID, &lease.server_id)?;
    cursor.push_option_bytes(
        OPT_PARAMETER_REQUEST_LIST,
        &[OPT_SUBNET_MASK, OPT_ROUTER, OPT_LEASE_TIME],
    )?;
    cursor.finish()
}

fn parse_packet(packet: &[u8], xid: u32, mac: [u8; 6]) -> Option<ParsedPacket> {
    if packet.len() < DHCP_FIXED_LEN || packet[0] != DHCP_OP_BOOTREPLY {
        return None;
    }
    if packet[1] != DHCP_HTYPE_ETHERNET || packet[2] != DHCP_HLEN_ETHERNET {
        return None;
    }
    if read_u32(packet, 4)? != xid {
        return None;
    }
    if packet.get(28..34)? != mac {
        return None;
    }
    if packet.get(DHCP_COOKIE_OFFSET..DHCP_FIXED_LEN)? != DHCP_MAGIC_COOKIE {
        return None;
    }

    let yiaddr = [packet[16], packet[17], packet[18], packet[19]];
    if yiaddr == [0, 0, 0, 0] {
        return None;
    }

    let mut message_type = None;
    let mut server_id = None;
    let mut subnet_mask = None;
    let mut gateway = None;
    let mut lease_seconds = None;
    let mut cursor = DHCP_FIXED_LEN;

    while cursor < packet.len() {
        let code = packet[cursor];
        cursor = cursor.saturating_add(1);
        if code == OPT_END {
            break;
        }
        if code == 0 {
            continue;
        }
        let len = usize::from(*packet.get(cursor)?);
        cursor = cursor.saturating_add(1);
        let end = cursor.checked_add(len)?;
        let value = packet.get(cursor..end)?;
        cursor = end;

        match code {
            OPT_MESSAGE_TYPE if len == 1 => {
                message_type = match value[0] {
                    2 => Some(DhcpMessageType::Offer),
                    5 => Some(DhcpMessageType::Ack),
                    6 => Some(DhcpMessageType::Nak),
                    _ => None,
                };
            }
            OPT_SERVER_ID if len == 4 => {
                server_id = Some([value[0], value[1], value[2], value[3]]);
            }
            OPT_SUBNET_MASK if len == 4 => {
                subnet_mask = Some([value[0], value[1], value[2], value[3]]);
            }
            OPT_ROUTER if len >= 4 => {
                gateway = Some([value[0], value[1], value[2], value[3]]);
            }
            OPT_LEASE_TIME if len == 4 => {
                lease_seconds = Some(read_u32(value, 0)?);
            }
            _ => {}
        }
    }

    Some(ParsedPacket {
        message_type: message_type?,
        yiaddr,
        server_id,
        subnet_mask,
        gateway,
        lease_seconds,
    })
}

fn prefix_len_from_mask(mask: [u8; 4]) -> Option<u8> {
    let bits = u32::from_be_bytes(mask);
    let leading = bits.leading_ones() as u8;
    let trailing_mask = if leading == 32 {
        0
    } else {
        (1u32 << (32 - u32::from(leading))) - 1
    };
    if bits & trailing_mask == 0 {
        Some(leading)
    } else {
        None
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let chunk = bytes.get(offset..offset.saturating_add(4))?;
    Some(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

struct DhcpWriter<'a> {
    buffer: &'a mut [u8],
    cursor: usize,
}

impl<'a> DhcpWriter<'a> {
    fn new(buffer: &'a mut [u8]) -> Result<Self, DhcpFailureReason> {
        if buffer.len() < DHCP_FIXED_LEN {
            return Err(DhcpFailureReason::BufferTooSmall);
        }
        buffer.fill(0);
        buffer[DHCP_COOKIE_OFFSET..DHCP_FIXED_LEN].copy_from_slice(&DHCP_MAGIC_COOKIE);
        Ok(Self {
            buffer,
            cursor: DHCP_FIXED_LEN,
        })
    }

    fn write_header(&mut self, xid: u32, mac: [u8; 6]) {
        self.buffer[0] = DHCP_OP_BOOTREQUEST;
        self.buffer[1] = DHCP_HTYPE_ETHERNET;
        self.buffer[2] = DHCP_HLEN_ETHERNET;
        self.buffer[3] = 0;
        self.buffer[4..8].copy_from_slice(&xid.to_be_bytes());
        self.buffer[10..12].copy_from_slice(&DHCP_FLAGS_BROADCAST.to_be_bytes());
        self.buffer[28..34].copy_from_slice(&mac);
    }

    fn push_option_u8(&mut self, code: u8, value: u8) -> Result<(), DhcpFailureReason> {
        self.push_option_bytes(code, &[value])
    }

    fn push_option_bytes(&mut self, code: u8, value: &[u8]) -> Result<(), DhcpFailureReason> {
        let end = self
            .cursor
            .checked_add(2)
            .and_then(|cursor| cursor.checked_add(value.len()))
            .ok_or(DhcpFailureReason::BufferTooSmall)?;
        if end > self.buffer.len() {
            return Err(DhcpFailureReason::BufferTooSmall);
        }
        self.buffer[self.cursor] = code;
        self.buffer[self.cursor + 1] = value.len() as u8;
        self.buffer[self.cursor + 2..end].copy_from_slice(value);
        self.cursor = end;
        Ok(())
    }

    fn finish(mut self) -> Result<usize, DhcpFailureReason> {
        if self.cursor >= self.buffer.len() {
            return Err(DhcpFailureReason::BufferTooSmall);
        }
        self.buffer[self.cursor] = OPT_END;
        self.cursor += 1;
        Ok(self.cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];

    fn config() -> NetDhcpConfig {
        NetDhcpConfig {
            discover_timeout_ms: 10,
            request_timeout_ms: 10,
            max_retries: 2,
        }
    }

    fn offer_packet(xid: u32) -> [u8; 300] {
        let mut packet = [0u8; 300];
        let mut writer = DhcpWriter::new(&mut packet).expect("writer");
        writer.write_header(xid, MAC);
        writer.buffer[0] = DHCP_OP_BOOTREPLY;
        writer.buffer[16..20].copy_from_slice(&[192, 168, 10, 42]);
        writer
            .push_option_u8(OPT_MESSAGE_TYPE, DhcpMessageType::Offer.code())
            .unwrap();
        writer
            .push_option_bytes(OPT_SERVER_ID, &[192, 168, 10, 1])
            .unwrap();
        writer
            .push_option_bytes(OPT_SUBNET_MASK, &[255, 255, 255, 0])
            .unwrap();
        writer
            .push_option_bytes(OPT_ROUTER, &[192, 168, 10, 1])
            .unwrap();
        writer
            .push_option_bytes(OPT_LEASE_TIME, &300u32.to_be_bytes())
            .unwrap();
        let len = writer.finish().unwrap();
        packet[len..].fill(0);
        packet
    }

    fn ack_packet(xid: u32) -> [u8; 300] {
        let mut packet = offer_packet(xid);
        packet[DHCP_FIXED_LEN + 2] = DhcpMessageType::Ack.code();
        packet
    }

    fn option_value<'a>(packet: &'a [u8], code: u8) -> Option<&'a [u8]> {
        let mut cursor = DHCP_FIXED_LEN;
        while cursor < packet.len() {
            let found = packet[cursor];
            cursor = cursor.saturating_add(1);
            if found == OPT_END {
                break;
            }
            if found == 0 {
                continue;
            }
            let len = usize::from(*packet.get(cursor)?);
            cursor = cursor.saturating_add(1);
            let end = cursor.checked_add(len)?;
            let value = packet.get(cursor..end)?;
            if found == code {
                return Some(value);
            }
            cursor = end;
        }
        None
    }

    #[test]
    fn dhcp_discover_encodes_message_type() {
        let mut packet = [0u8; 300];
        let len = encode_discover(0x1234_5678, MAC, &mut packet).expect("discover");
        assert!(len > DHCP_FIXED_LEN);
        assert_eq!(packet[0], DHCP_OP_BOOTREQUEST);
        assert_eq!(
            packet[DHCP_COOKIE_OFFSET..DHCP_FIXED_LEN],
            DHCP_MAGIC_COOKIE
        );
        assert_eq!(
            option_value(&packet[..len], OPT_MESSAGE_TYPE),
            Some(&[DhcpMessageType::Discover.code()][..])
        );
        assert_eq!(
            option_value(&packet[..len], OPT_CLIENT_ID),
            Some(&[DHCP_HTYPE_ETHERNET, 0x02, 0, 0, 0, 0, 1][..])
        );
        assert_eq!(
            option_value(&packet[..len], OPT_PARAMETER_REQUEST_LIST),
            Some(&[OPT_SUBNET_MASK, OPT_ROUTER, OPT_LEASE_TIME, OPT_SERVER_ID][..])
        );
    }

    #[test]
    fn dhcp_request_encodes_selected_offer() {
        let lease = DhcpLease {
            ip: [192, 168, 10, 42],
            prefix_len: 24,
            gateway: Some([192, 168, 10, 1]),
            server_id: [192, 168, 10, 1],
            lease_seconds: 300,
        };
        let mut packet = [0u8; 300];
        let len = encode_request(0x1234_5678, MAC, lease, &mut packet).expect("request");
        assert_eq!(
            option_value(&packet[..len], OPT_MESSAGE_TYPE),
            Some(&[DhcpMessageType::Request.code()][..])
        );
        assert_eq!(
            option_value(&packet[..len], OPT_REQUESTED_IP),
            Some(&lease.ip[..])
        );
        assert_eq!(
            option_value(&packet[..len], OPT_SERVER_ID),
            Some(&lease.server_id[..])
        );
    }

    #[test]
    fn dhcp_offer_moves_client_to_requesting() {
        let mut client = DhcpClient::new(config());
        client.start(MAC, 0);
        let offer = offer_packet(client.xid);
        let event = client.handle_packet(MAC, &offer, 5);
        assert_eq!(event, DhcpEvent::SendQueued);
        assert_eq!(client.status().phase, DhcpPhase::Requesting);
    }

    #[test]
    fn dhcp_ack_yields_bound_lease() {
        let mut client = DhcpClient::new(config());
        client.start(MAC, 0);
        let offer = offer_packet(client.xid);
        assert_eq!(client.handle_packet(MAC, &offer, 5), DhcpEvent::SendQueued);
        let ack = ack_packet(client.xid);
        let event = client.handle_packet(MAC, &ack, 6);
        let lease = match event {
            DhcpEvent::LeaseAcquired(lease) => lease,
            other => panic!("unexpected event: {other:?}"),
        };
        assert_eq!(lease.ip, [192, 168, 10, 42]);
        assert_eq!(lease.prefix_len, 24);
        assert_eq!(lease.gateway, Some([192, 168, 10, 1]));
        assert_eq!(client.status().phase, DhcpPhase::Bound);
    }

    #[test]
    fn dhcp_nak_fails_requesting_client() {
        let mut client = DhcpClient::new(config());
        client.start(MAC, 0);
        let offer = offer_packet(client.xid);
        assert_eq!(client.handle_packet(MAC, &offer, 5), DhcpEvent::SendQueued);
        let mut nak = offer_packet(client.xid);
        nak[DHCP_FIXED_LEN + 2] = DhcpMessageType::Nak.code();
        assert_eq!(
            client.handle_packet(MAC, &nak, 6),
            DhcpEvent::Failed(DhcpFailureReason::Nak)
        );
        assert_eq!(client.status().phase, DhcpPhase::Failed);
        assert_eq!(client.status().failure, Some(DhcpFailureReason::Nak));
    }

    #[test]
    fn dhcp_ack_server_mismatch_is_rejected() {
        let mut client = DhcpClient::new(config());
        client.start(MAC, 0);
        let offer = offer_packet(client.xid);
        assert_eq!(client.handle_packet(MAC, &offer, 5), DhcpEvent::SendQueued);
        let mut ack = ack_packet(client.xid);
        ack[DHCP_FIXED_LEN + 5..DHCP_FIXED_LEN + 9].copy_from_slice(&[10, 0, 0, 1]);
        assert_eq!(client.handle_packet(MAC, &ack, 6), DhcpEvent::None);
        assert_eq!(client.status().metrics.invalid_packets, 1);
        assert_eq!(client.status().phase, DhcpPhase::Requesting);
    }

    #[test]
    fn dhcp_rejects_bad_cookie() {
        let mut client = DhcpClient::new(config());
        client.start(MAC, 0);
        let mut offer = offer_packet(client.xid);
        offer[DHCP_COOKIE_OFFSET] = 0;
        assert_eq!(client.handle_packet(MAC, &offer, 5), DhcpEvent::None);
        assert_eq!(client.status().metrics.invalid_packets, 1);
        assert_eq!(client.status().phase, DhcpPhase::Selecting);
    }

    #[test]
    fn dhcp_timeouts_are_bounded() {
        let mut client = DhcpClient::new(config());
        client.start(MAC, 0);

        let mut frame = [0u8; 300];
        assert!(client.build_outbound(MAC, &mut frame, 0).unwrap().is_some());
        assert_eq!(client.on_timer(10), DhcpEvent::SendQueued);
        assert!(client
            .build_outbound(MAC, &mut frame, 10)
            .unwrap()
            .is_some());
        assert_eq!(
            client.on_timer(20),
            DhcpEvent::Failed(DhcpFailureReason::TimeoutExhausted)
        );
        assert_eq!(client.status().phase, DhcpPhase::Failed);
        assert_eq!(
            client.status().failure,
            Some(DhcpFailureReason::TimeoutExhausted)
        );
    }
}
