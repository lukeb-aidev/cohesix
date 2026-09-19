// Author: Lukas Bower
// Purpose: Restrict host field-bus requests to compiler-owned endpoints and exact read or separately approved control maps.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum Transport {
    Tcp {
        address: String,
    },
    Serial {
        path: String,
        baud: u32,
        parity: Parity,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Parity {
    None,
    Even,
    Odd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Modbus,
    Dnp3,
}

/// No request supplies an address, register, function, control value or native path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub id: String,
    pub protocol: Protocol,
    pub transport: Transport,
    pub unit: u8,
    pub master: u16,
    pub outstation: u16,
    pub timeout_ms: u32,
    pub poll_interval_ms: u32,
    pub observation_ttl_ms: u32,
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub id: String,
    pub operation: Operation,
    pub approval_required: bool,
}

/// Bounded protocol subset. Unsupported functions/variations have no passthrough.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    ModbusRead {
        function: u8,
        start: u16,
        count: u16,
    },
    ModbusWrite {
        function: u8,
        address: u16,
        value: u16,
    },
    Dnp3Read {
        group: u8,
        variation: u8,
        start: u16,
        count: u16,
    },
    Dnp3Control {
        index: u8,
        code: u8,
        on_ms: u32,
        off_ms: u32,
    },
}

impl Operation {
    pub fn is_control(&self) -> bool {
        matches!(self, Self::ModbusWrite { .. } | Self::Dnp3Control { .. })
    }
    pub fn protocol(&self) -> Protocol {
        match self {
            Self::ModbusRead { .. } | Self::ModbusWrite { .. } => Protocol::Modbus,
            _ => Protocol::Dnp3,
        }
    }
    pub fn valid(&self) -> bool {
        match *self {
            Self::ModbusRead {
                function,
                start,
                count,
            } => {
                matches!(function, 1..=4)
                    && count > 0
                    && count <= if function <= 2 { 256 } else { 64 }
                    && u32::from(start) + u32::from(count) <= 65536
            }
            Self::ModbusWrite {
                function, value, ..
            } => function == 6 || (function == 5 && matches!(value, 0 | 0xff00)),
            Self::Dnp3Read {
                group,
                variation,
                start,
                count,
            } => {
                matches!((group, variation), (1, 2) | (20, 1) | (30, 1))
                    && count > 0
                    && count <= 16
                    && u32::from(start) + u32::from(count) <= 65536
            }
            Self::Dnp3Control {
                code,
                on_ms,
                off_ms,
                ..
            } => {
                matches!(code, 1 | 3 | 4)
                    && on_ms <= 1000
                    && off_ms <= 1000
                    && (code != 1 || on_ms > 0)
            }
        }
    }
}

impl Endpoint {
    pub fn validate(&self) -> Result<(), &'static str> {
        if crate::validate_id(&self.id).is_err()
            || self.id.len() > 32
            || !(1..=5000).contains(&self.timeout_ms)
            || !(100..=30000).contains(&self.poll_interval_ms)
            || self.observation_ttl_ms < self.poll_interval_ms
            || self.observation_ttl_ms > 30000
            || self.points.is_empty()
            || self.points.len() > 64
            || (self.protocol == Protocol::Modbus
                && (!(1..=247).contains(&self.unit) || self.master != 0 || self.outstation != 0))
            || (self.protocol == Protocol::Dnp3
                && (self.unit != 0
                    || self.master >= 65520
                    || self.outstation >= 65520
                    || self.master == self.outstation))
        {
            return Err("invalid_field_bus_endpoint");
        }
        match &self.transport {
            Transport::Tcp { address } => {
                let native: std::net::SocketAddr = address
                    .parse()
                    .map_err(|_| "literal_field_bus_endpoint_required")?;
                if native.port() == 0
                    || native.ip().is_unspecified()
                    || native.ip().is_multicast()
                    || address.len() > 64
                {
                    return Err("invalid_field_bus_tcp_endpoint");
                }
            }
            Transport::Serial { path, baud, .. } => {
                if !path.starts_with("/dev/")
                    || path.len() > 128
                    || path.split('/').any(|part| part == ".." || part == ".")
                    || path.bytes().any(|b| b.is_ascii_control())
                    || !matches!(baud, 9600 | 19200 | 38400 | 57600 | 115200)
                {
                    return Err("invalid_field_bus_serial_endpoint");
                }
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for point in &self.points {
            if crate::validate_id(&point.id).is_err()
                || point.id.len() > 32
                || !ids.insert(&point.id)
                || !point.operation.valid()
                || point.operation.protocol() != self.protocol
                || point.approval_required != point.operation.is_control()
            {
                return Err("invalid_field_bus_point_map");
            }
        }
        Ok(())
    }
}

/// The independent compiler registry is the only source of native mappings.
pub fn endpoints() -> Result<Vec<Endpoint>, crate::provider::ProviderError> {
    let registry = crate::provider::registry()?;
    let value = registry["contract"]
        .get("field_bus")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let rows: Vec<Endpoint> = serde_json::from_value(value)
        .map_err(|_| crate::provider::ProviderError::InvalidRegistry)?;
    if rows.len() > 64 || rows.iter().any(|row| row.validate().is_err()) {
        return Err(crate::provider::ProviderError::InvalidRegistry);
    }
    Ok(rows)
}

/// Resolve one exact compiled map. Ticket operands never become wire addresses.
pub fn resolve(
    endpoint_id: &str,
    point_id: &str,
) -> Result<(Endpoint, Point), crate::provider::ProviderError> {
    let endpoint = endpoints()?
        .into_iter()
        .find(|row| row.id == endpoint_id)
        .ok_or(crate::provider::ProviderError::InvalidTarget)?;
    let point = endpoint
        .points
        .iter()
        .find(|row| row.id == point_id)
        .cloned()
        .ok_or(crate::provider::ProviderError::InvalidTarget)?;
    Ok((endpoint, point))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    #[test]
    fn exact_maps_refuse_broadcast_unbounded_reads_and_unapproved_controls() {
        let point = Point {
            id: "temperature".into(),
            approval_required: false,
            operation: Operation::ModbusRead {
                function: 3,
                start: 65535,
                count: 1,
            },
        };
        let mut endpoint = Endpoint {
            id: "plc".into(),
            protocol: Protocol::Modbus,
            transport: Transport::Tcp {
                address: "127.0.0.1:502".into(),
            },
            unit: 1,
            master: 0,
            outstation: 0,
            timeout_ms: 1000,
            poll_interval_ms: 1000,
            observation_ttl_ms: 2000,
            points: vec![point],
        };
        endpoint.validate().unwrap();
        endpoint.unit = 0;
        assert!(endpoint.validate().is_err());
        endpoint.unit = 1;
        endpoint.points[0].operation = Operation::ModbusRead {
            function: 3,
            start: 65535,
            count: 2,
        };
        assert!(endpoint.validate().is_err());
        endpoint.points[0].operation = Operation::ModbusWrite {
            function: 5,
            address: 0,
            value: 1,
        };
        endpoint.points[0].approval_required = true;
        assert!(endpoint.validate().is_err());
        endpoint.points[0].operation = Operation::ModbusWrite {
            function: 5,
            address: 0,
            value: 0xff00,
        };
        endpoint.validate().unwrap();
        endpoint.points[0].approval_required = false;
        assert!(endpoint.validate().is_err());
        endpoint.points[0].approval_required = true;
        endpoint.transport = Transport::Tcp {
            address: "dynamic.example:502".into(),
        };
        assert!(endpoint.validate().is_err());
        assert!(!Operation::Dnp3Read {
            group: 30,
            variation: 0,
            start: 0,
            count: 1
        }
        .valid());
        assert!(!Operation::Dnp3Control {
            index: 0,
            code: 2,
            on_ms: 1,
            off_ms: 0
        }
        .valid());
    }
}
