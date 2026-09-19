// Author: Lukas Bower
// Purpose: Project only current remote-ACK field-bus values from the private durable spool and preserve their original expiry.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{anyhow, ensure, Result};
use cohesix_authority::{
    bus::{Endpoint, Protocol},
    snapshot::Entry,
};
use sha2::{Digest, Sha256};
use sidecar_bus::live::{Spool, State};
use std::path::Path;

/// No native read is issued by a snapshot publisher. The ticket/sidecar owner
/// supplies fresh ACKs, and a failed latest read withdraws the complete family.
pub fn collect(provider: &str, root: &Path, now: u64) -> Result<(Vec<Entry>, u64)> {
    ensure!(root.join("owner").exists(), "not_enabled field-bus WAL");
    let endpoints = cohesix_authority::bus::endpoints()?;
    let spool = Spool::open(root)?;
    project(provider, &endpoints, spool.entries(), now)
}

fn project(
    provider: &str,
    endpoints: &[Endpoint],
    rows: &[sidecar_bus::live::Entry],
    now: u64,
) -> Result<(Vec<Entry>, u64)> {
    let protocol = match provider {
        "modbus" => Protocol::Modbus,
        "dnp3" => Protocol::Dnp3,
        _ => return Err(anyhow!("not_enabled field-bus provider")),
    };
    let registry = cohesix_authority::provider::registry()?;
    let mut entries = Vec::new();
    let mut remaining = 30000;
    for endpoint in endpoints
        .iter()
        .filter(|endpoint| endpoint.protocol == protocol)
    {
        endpoint.validate().map_err(|error| anyhow!(error))?;
        for point in endpoint
            .points
            .iter()
            .filter(|point| !point.operation.is_control())
        {
            let row = rows
                .iter()
                .rev()
                .find(|row| row.request.endpoint == endpoint.id && row.request.point == point.id)
                .ok_or_else(|| anyhow!("not_enabled field-bus point"))?;
            let fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&(endpoint, point))?));
            ensure!(
                row.state == State::Delivered
                    && !row.control
                    && row.operation_sha256 == fingerprint
                    && registry["graph_sha256"] == row.provider_graph_sha256,
                "unconfirmed field-bus point"
            );
            let observed = row
                .observed_unix_ms
                .ok_or_else(|| anyhow!("unconfirmed field-bus ACK"))?;
            let expiry = observed
                .checked_add(u64::from(endpoint.observation_ttl_ms))
                .ok_or_else(|| anyhow!("invalid field-bus expiry"))?;
            ensure!(observed <= now && expiry > now, "expired field-bus ACK");
            remaining = remaining.min(expiry - now);
            let prefix = format!("{}/{}", endpoint.id, point.id);
            let acknowledgement = hex::encode(Sha256::digest(serde_json::to_vec(
                &row.acknowledgements_hex,
            )?));
            entries.push(Entry { path: format!("{prefix}/status"), value: serde_json::json!({
                "schema":"cohesix-field-bus-observation/v1", "source":"remote-protocol-ack", "authoritative":false,
                "worker_proof":false, "sequence":row.sequence, "unit":endpoint.unit, "outstation":endpoint.outstation,
                "operation_sha256":fingerprint,"acknowledgement_sha256":acknowledgement,
                "observed_unix_ms":observed,"expires_unix_ms":expiry,
            }).to_string() });
            for (index, values) in row.values.chunks(16).enumerate() {
                entries.push(Entry {
                    path: format!("{prefix}/values-{index}"),
                    value: serde_json::to_string(values)?,
                });
            }
        }
    }
    ensure!(!entries.is_empty(), "not_enabled field-bus maps");
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    ensure!(entries.len() <= 64, "limit field-bus snapshot");
    Ok((entries, remaining))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cohesix_authority::bus::{Operation, Point, Transport};
    #[test]
    fn stale_failed_or_reconfigured_ack_cannot_refresh_snapshot() {
        let endpoint = Endpoint {
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
            points: vec![Point {
                id: "value".into(),
                operation: Operation::ModbusRead {
                    function: 3,
                    start: 0,
                    count: 1,
                },
                approval_required: false,
            }],
        };
        let mut row = sidecar_bus::live::Entry {
            sequence: 1,
            request: sidecar_bus::live::Request {
                schema: "cohesix-field-bus-request/v1".into(),
                id: "one".into(),
                idempotency_key: "once".into(),
                endpoint: "plc".into(),
                point: "value".into(),
            },
            operation_sha256: hex::encode(Sha256::digest(
                serde_json::to_vec(&(&endpoint, &endpoint.points[0])).unwrap(),
            )),
            provider_graph_sha256: cohesix_authority::provider::registry().unwrap()["graph_sha256"]
                .as_str()
                .unwrap()
                .into(),
            control: false,
            state: State::Delivered,
            attempts: 1,
            created_unix_ms: 1,
            observed_unix_ms: Some(1000),
            error: None,
            requests_hex: vec!["000100000006010300000001".into()],
            acknowledgements_hex: vec!["000100000005010302002a".into()],
            values: vec![],
        };
        let endpoints = vec![endpoint];
        assert_eq!(
            project("modbus", &endpoints, &[row.clone()], 1500)
                .unwrap()
                .1,
            1500
        );
        assert_eq!(
            project("modbus", &endpoints, &[row.clone()], 2500)
                .unwrap()
                .1,
            500
        );
        assert!(project("modbus", &endpoints, &[row.clone()], 3000).is_err());
        assert!(project("modbus", &endpoints, &[row.clone()], 999).is_err());
        row.state = State::Failed;
        assert!(project("modbus", &endpoints, &[row.clone()], 1500).is_err());
        row.state = State::Delivered;
        row.operation_sha256 = "0".repeat(64);
        assert!(project("modbus", &endpoints, &[row], 1500).is_err());
    }
}
