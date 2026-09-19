// Author: Lukas Bower
// Purpose: Encode bounded mapped MODBUS and DNP3 operations and validate exact remote protocol responses without arbitrary function passthrough.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
#![allow(missing_docs)]

use crate::live::{BusError, Result};
use cohesix_authority::bus::{Endpoint, Operation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Value {
    pub index: u16,
    pub value: i64,
    pub flags: u8,
}

pub fn modbus_crc(bytes: &[u8]) -> u16 {
    let mut crc = 0xffff_u16;
    for byte in bytes {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xa001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

pub fn modbus_pdu(operation: &Operation) -> Result<Vec<u8>> {
    if !operation.valid() {
        return Err(BusError::Policy);
    }
    let (function, address, value) = match *operation {
        Operation::ModbusRead {
            function,
            start,
            count,
        } => (function, start, count),
        Operation::ModbusWrite {
            function,
            address,
            value,
        } => (function, address, value),
        _ => return Err(BusError::Policy),
    };
    let mut pdu = vec![function];
    pdu.extend(address.to_be_bytes());
    pdu.extend(value.to_be_bytes());
    Ok(pdu)
}

pub fn modbus_request(
    unit: u8,
    transaction: u16,
    operation: &Operation,
    tcp: bool,
) -> Result<Vec<u8>> {
    if !(1..=247).contains(&unit) {
        return Err(BusError::Policy);
    }
    let pdu = modbus_pdu(operation)?;
    if tcp {
        let mut frame = transaction.to_be_bytes().to_vec();
        frame.extend([0, 0, 0, 6, unit]);
        frame.extend(pdu);
        Ok(frame)
    } else {
        let mut frame = vec![unit];
        frame.extend(pdu);
        frame.extend(modbus_crc(&frame).to_le_bytes());
        Ok(frame)
    }
}

pub fn modbus_response(
    unit: u8,
    transaction: u16,
    operation: &Operation,
    tcp: bool,
    frame: &[u8],
) -> Result<Vec<Value>> {
    let request = modbus_pdu(operation)?;
    let pdu = if tcp {
        if frame.len() < 9
            || frame.len() > 260
            || frame[..2] != transaction.to_be_bytes()
            || frame[2..4] != [0, 0]
            || usize::from(u16::from_be_bytes([frame[4], frame[5]])) + 6 != frame.len()
            || frame[6] != unit
        {
            return Err(BusError::Correlation);
        }
        &frame[7..]
    } else {
        if frame.len() < 5 || frame.len() > 256 || frame[0] != unit {
            return Err(BusError::Correlation);
        }
        let end = frame.len() - 2;
        if frame[end..] != modbus_crc(&frame[..end]).to_le_bytes() {
            return Err(BusError::Crc);
        }
        &frame[1..end]
    };
    if pdu[0] == request[0] | 0x80 {
        return if pdu.len() == 2 {
            Err(BusError::ModbusException(pdu[1]))
        } else {
            Err(BusError::Frame)
        };
    }
    if pdu[0] != request[0] {
        return Err(BusError::Correlation);
    }
    match *operation {
        Operation::ModbusWrite { address, value, .. } => {
            if pdu != request {
                return Err(BusError::Correlation);
            }
            Ok(vec![Value {
                index: address,
                value: i64::from(value),
                flags: 0,
            }])
        }
        Operation::ModbusRead {
            function,
            start,
            count,
        } => {
            let length = if function <= 2 {
                usize::from(count).div_ceil(8)
            } else {
                usize::from(count) * 2
            };
            if pdu.len() != length + 2 || usize::from(pdu[1]) != length {
                return Err(BusError::Frame);
            }
            if function <= 2 && count % 8 != 0 && pdu[pdu.len() - 1] >> (count % 8) != 0 {
                return Err(BusError::Frame);
            }
            (0..count)
                .map(|offset| {
                    let value = if function <= 2 {
                        i64::from((pdu[2 + usize::from(offset / 8)] >> (offset % 8)) & 1)
                    } else {
                        i64::from(u16::from_be_bytes([
                            pdu[2 + usize::from(offset) * 2],
                            pdu[3 + usize::from(offset) * 2],
                        ]))
                    };
                    Ok(Value {
                        index: start + offset,
                        value,
                        flags: 0,
                    })
                })
                .collect()
        }
        _ => Err(BusError::Policy),
    }
}

pub fn dnp3_crc(bytes: &[u8]) -> u16 {
    let mut crc = 0_u16;
    for byte in bytes {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xa6bc
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

pub fn dnp3_frame(master: u16, outstation: u16, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.is_empty()
        || payload.len() > 250
        || master == outstation
        || master >= 65520
        || outstation >= 65520
    {
        return Err(BusError::Policy);
    }
    let mut frame = vec![0x05, 0x64, (payload.len() + 5) as u8, 0xc4];
    frame.extend(outstation.to_le_bytes());
    frame.extend(master.to_le_bytes());
    frame.extend(dnp3_crc(&frame).to_le_bytes());
    for block in payload.chunks(16) {
        frame.extend(block);
        frame.extend(dnp3_crc(block).to_le_bytes());
    }
    Ok(frame)
}

pub fn dnp3_payload(master: u16, outstation: u16, frame: &[u8]) -> Result<Vec<u8>> {
    if frame.len() < 10
        || frame.len() > 292
        || frame[..2] != [0x05, 0x64]
        || frame[2] < 6
        || frame[3] != 0x44
        || frame[4..6] != master.to_le_bytes()
        || frame[6..8] != outstation.to_le_bytes()
    {
        return Err(BusError::Correlation);
    }
    if frame[8..10] != dnp3_crc(&frame[..8]).to_le_bytes() {
        return Err(BusError::Crc);
    }
    let length = usize::from(frame[2] - 5);
    if frame.len() != 10 + length + 2 * length.div_ceil(16) {
        return Err(BusError::Frame);
    }
    let mut payload = Vec::with_capacity(length);
    let mut cursor = 10;
    while payload.len() < length {
        let count = (length - payload.len()).min(16);
        let block = &frame[cursor..cursor + count];
        if frame[cursor + count..cursor + count + 2] != dnp3_crc(block).to_le_bytes() {
            return Err(BusError::Crc);
        }
        payload.extend(block);
        cursor += count + 2;
    }
    if payload[0] & 0xc0 != 0xc0 {
        return Err(BusError::Unsupported);
    }
    Ok(payload)
}

pub fn dnp3_request(
    endpoint: &Endpoint,
    operation: &Operation,
    sequence: u8,
    operate: bool,
) -> Result<Vec<u8>> {
    dnp3_request_sequences(endpoint, operation, sequence, sequence, operate)
}

pub fn dnp3_request_sequences(
    endpoint: &Endpoint,
    operation: &Operation,
    sequence: u8,
    transport_sequence: u8,
    operate: bool,
) -> Result<Vec<u8>> {
    if !operation.valid() {
        return Err(BusError::Policy);
    }
    let mut data = vec![0xc0 | (transport_sequence & 0x3f), 0xc0 | (sequence & 0x0f)];
    match *operation {
        Operation::Dnp3Read {
            group,
            variation,
            start,
            count,
        } => {
            if operate {
                return Err(BusError::Policy);
            }
            data.extend([1, group, variation, 1]);
            data.extend(start.to_le_bytes());
            data.extend((start + (count - 1)).to_le_bytes());
        }
        Operation::Dnp3Control {
            index,
            code,
            on_ms,
            off_ms,
        } => {
            data.extend([if operate { 4 } else { 3 }, 12, 1, 0x17, 1, index, code, 1]);
            data.extend(on_ms.to_le_bytes());
            data.extend(off_ms.to_le_bytes());
            data.push(0);
        }
        _ => return Err(BusError::Policy),
    }
    dnp3_frame(endpoint.master, endpoint.outstation, &data)
}

/// Response data is accepted only for the requested static range or exact CROB.
/// Event scans, unsolicited responses, secure authentication and fragmentation
/// need separately implemented profiles; none are silently treated as success.
pub fn dnp3_response(
    endpoint: &Endpoint,
    operation: &Operation,
    sequence: u8,
    operate: bool,
    frame: &[u8],
) -> Result<(Vec<Value>, bool)> {
    if !operation.valid() || operation.protocol() != cohesix_authority::bus::Protocol::Dnp3 {
        return Err(BusError::Policy);
    }
    let data = dnp3_payload(endpoint.master, endpoint.outstation, frame)?;
    if data.len() < 5 || data[1] & 0xdf != 0xc0 | (sequence & 0xf) || data[2] != 0x81 {
        return Err(BusError::Correlation);
    }
    let iin = u16::from_le_bytes([data[3], data[4]]);
    // Class-data-available bits are observations, not failures. Restart, device
    // trouble, local control, need-time and request error bits fail this profile.
    if iin & !0x000e != 0 {
        return Err(BusError::Dnp3Iin(iin));
    }
    let objects = &data[5..];
    let values = match *operation {
        Operation::Dnp3Control { index, .. } => {
            let request = dnp3_request(endpoint, operation, sequence, operate)?;
            // Decode our outgoing link framing with its reversed direction; the
            // independently specified CROB body must be echoed with status zero.
            let mut reversed = request.clone();
            reversed[3] = 0x44;
            reversed[4..6].copy_from_slice(&endpoint.master.to_le_bytes());
            reversed[6..8].copy_from_slice(&endpoint.outstation.to_le_bytes());
            let crc = dnp3_crc(&reversed[..8]);
            reversed[8..10].copy_from_slice(&crc.to_le_bytes());
            let expected = dnp3_payload(endpoint.master, endpoint.outstation, &reversed)?;
            if objects.len() != expected[3..].len() || objects.is_empty() {
                return Err(BusError::Frame);
            }
            if objects[..objects.len() - 1] != expected[3..expected.len() - 1] {
                return Err(BusError::Correlation);
            }
            if objects[objects.len() - 1] != 0 {
                return Err(BusError::Dnp3Control(objects[objects.len() - 1]));
            }
            vec![Value {
                index: u16::from(index),
                value: 0,
                flags: 0,
            }]
        }
        Operation::Dnp3Read {
            group,
            variation,
            start,
            count,
        } => {
            if objects.len() < 5 || objects[..2] != [group, variation] {
                return Err(BusError::Correlation);
            }
            let (first, last, cursor) = match objects[2] {
                0 => (u16::from(objects[3]), u16::from(objects[4]), 5),
                1 if objects.len() >= 7 => (
                    u16::from_le_bytes([objects[3], objects[4]]),
                    u16::from_le_bytes([objects[5], objects[6]]),
                    7,
                ),
                _ => return Err(BusError::Unsupported),
            };
            if first != start || last != start + (count - 1) {
                return Err(BusError::Correlation);
            }
            let width = if group == 1 { 1 } else { 5 };
            if objects.len() != cursor + usize::from(count) * width {
                return Err(BusError::Frame);
            }
            let mut rows = Vec::with_capacity(usize::from(count));
            for (offset, raw) in objects[cursor..].chunks_exact(width).enumerate() {
                let flags = raw[0];
                // A received point with OFFLINE/RESTART/COMM_LOST quality is
                // never projected as a healthy read, even though its CRC passed.
                if flags & 1 == 0 || flags & 0x06 != 0 {
                    return Err(BusError::Quality);
                }
                let value = if group == 1 {
                    i64::from(flags >> 7)
                } else {
                    let bytes: [u8; 4] = raw[1..5].try_into().map_err(|_| BusError::Frame)?;
                    if group == 30 {
                        i64::from(i32::from_le_bytes(bytes))
                    } else {
                        i64::from(u32::from_le_bytes(bytes))
                    }
                };
                rows.push(Value {
                    index: start + offset as u16,
                    value,
                    flags,
                });
            }
            rows
        }
        _ => return Err(BusError::Policy),
    };
    Ok((values, data[1] & 0x20 != 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cohesix_authority::bus::{Protocol, Transport};

    fn endpoint() -> Endpoint {
        Endpoint {
            id: "test".into(),
            protocol: Protocol::Dnp3,
            transport: Transport::Tcp {
                address: "127.0.0.1:20000".into(),
            },
            unit: 0,
            master: 1,
            outstation: 1024,
            timeout_ms: 1000,
            poll_interval_ms: 1000,
            observation_ttl_ms: 2000,
            points: Vec::new(),
        }
    }
    fn response(endpoint: &Endpoint, payload: &[u8]) -> Vec<u8> {
        let mut frame = dnp3_frame(endpoint.outstation, endpoint.master, payload).unwrap();
        frame[3] = 0x44;
        let crc = dnp3_crc(&frame[..8]);
        frame[8..10].copy_from_slice(&crc.to_le_bytes());
        frame
    }

    #[test]
    fn modbus_spec_request_crc_response_exception_and_echo_binding() {
        // MODBUS V1.1b3 FC03: unit 1, register 0, quantity 1; RTU CRC wire order is low byte first.
        let op = Operation::ModbusRead {
            function: 3,
            start: 0,
            count: 1,
        };
        assert_eq!(
            modbus_request(1, 0, &op, false).unwrap(),
            [1, 3, 0, 0, 0, 1, 0x84, 0x0a]
        );
        assert_eq!(modbus_crc(b"123456789"), 0x4b37);
        let good = [0, 7, 0, 0, 0, 5, 1, 3, 2, 0x12, 0x34];
        assert_eq!(
            modbus_response(1, 7, &op, true, &good).unwrap(),
            [Value {
                index: 0,
                value: 4660,
                flags: 0
            }]
        );
        assert!(modbus_response(1, 8, &op, true, &good).is_err());
        assert!(matches!(
            modbus_response(1, 7, &op, true, &[0, 7, 0, 0, 0, 3, 1, 0x83, 2]),
            Err(BusError::ModbusException(2))
        ));
        let write = Operation::ModbusWrite {
            function: 6,
            address: 10,
            value: 42,
        };
        let echo = [0, 8, 0, 0, 0, 6, 1, 6, 0, 10, 0, 42];
        assert_eq!(
            modbus_response(1, 8, &write, true, &echo).unwrap()[0].value,
            42
        );
        let mut wrong = echo;
        wrong[11] = 41;
        assert!(modbus_response(1, 8, &write, true, &wrong).is_err());
    }

    #[test]
    fn dnp3_static_points_require_crc_range_application_sequence_and_quality() {
        // OpenDNP3 3.1.2 TestCRC.cpp independently fixes this link-header check.
        assert_eq!(dnp3_crc(&[5, 0x64, 5, 0xc0, 1, 0, 0, 4]), 0x21e9);
        assert_eq!(dnp3_crc(b"123456789"), 0xea82);
        let endpoint = endpoint();
        let op = Operation::Dnp3Read {
            group: 30,
            variation: 1,
            start: 0,
            count: 1,
        };
        // Transport FIR/FIN, application FIR/FIN/sequence 7, response 0x81,
        // IIN zero, g30v1, 16-bit range 0..0, ONLINE quality, signed analog -2.
        let payload = [
            0xc0, 0xc7, 0x81, 0, 0, 30, 1, 1, 0, 0, 0, 0, 1, 0xfe, 0xff, 0xff, 0xff,
        ];
        let good = response(&endpoint, &payload);
        assert!(matches!(
            dnp3_response(
                &endpoint,
                &Operation::Dnp3Read {
                    group: 30,
                    variation: 1,
                    start: 0,
                    count: 0
                },
                7,
                false,
                &good
            ),
            Err(BusError::Policy)
        ));
        assert_eq!(
            dnp3_response(&endpoint, &op, 7, false, &good).unwrap(),
            (
                vec![Value {
                    index: 0,
                    value: -2,
                    flags: 1
                }],
                false
            )
        );
        assert!(dnp3_response(&endpoint, &op, 6, false, &good).is_err());
        let mut bad = good.clone();
        bad[10] ^= 1;
        assert!(matches!(
            dnp3_response(&endpoint, &op, 7, false, &bad),
            Err(BusError::Crc)
        ));
        let mut bad = payload;
        bad[12] = 0;
        assert!(matches!(
            dnp3_response(&endpoint, &op, 7, false, &response(&endpoint, &bad)),
            Err(BusError::Quality)
        ));
        let mut bad = payload;
        bad[3] = 0x80;
        assert!(matches!(
            dnp3_response(&endpoint, &op, 7, false, &response(&endpoint, &bad)),
            Err(BusError::Dnp3Iin(128))
        ));
        let mut bad = payload;
        bad[1] |= 0x20;
        assert!(
            dnp3_response(&endpoint, &op, 7, false, &response(&endpoint, &bad))
                .unwrap()
                .1
        );
        for len in 0..good.len() {
            assert!(dnp3_response(&endpoint, &op, 7, false, &good[..len]).is_err());
        }
    }

    #[test]
    fn dnp3_control_requires_exact_echo_and_success_status() {
        let endpoint = endpoint();
        let op = Operation::Dnp3Control {
            index: 3,
            code: 3,
            on_ms: 0,
            off_ms: 0,
        };
        let payload = [
            0xc0, 0xc2, 0x81, 0, 0, 12, 1, 0x17, 1, 3, 3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert!(dnp3_response(&endpoint, &op, 2, false, &response(&endpoint, &payload)).is_ok());
        let mut bad = payload;
        bad[20] = 7;
        assert!(matches!(
            dnp3_response(&endpoint, &op, 2, false, &response(&endpoint, &bad)),
            Err(BusError::Dnp3Control(7))
        ));
        let mut bad = payload;
        bad[9] = 4;
        assert!(dnp3_response(&endpoint, &op, 2, false, &response(&endpoint, &bad)).is_err());
    }
}
