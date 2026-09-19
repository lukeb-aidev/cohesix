// Author: Lukas Bower
// Purpose: Render bounded derived projections only from the shared verifier's accepted graph type.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

//! Exported telemetry and attestations are never accepted as Cohesix receipts.

use anyhow::{bail, ensure, Result};
use cohesix_evidence::VerifiedGraph;
use serde_json::{json, Value};

/// Durable acknowledgement-aware delivery of validator-derived SIEM projections.
pub mod delivery;
mod otel;
mod prometheus;

fn label(value: impl serde::Serialize) -> Result<String> {
    Ok(serde_json::to_value(value)?
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("invalid export label"))?
        .to_owned())
}

fn projection(graph: &VerifiedGraph) -> Result<Value> {
    let nodes: Vec<_> = graph.nodes().iter().map(|node| json!({
        "record_sha256": node.sha256, "kind": node.kind, "outcome": node.outcome,
        "source": node.source,
        "native_identity_sha256": node.native_identity.as_ref().map(|id| cohesix_evidence::digest(id.as_bytes())),
        "artifact_sha256": node.artifacts.iter().map(|artifact| &artifact.sha256).collect::<Vec<_>>()
    })).collect();
    Ok(
        json!({"schema":"cohesix-derived-export/v1", "authoritative":false,
        "ticket_id":graph.binding().ticket_id, "action":graph.binding().action,
        "graph_sha256":graph.digest(), "provider_graph_sha256":graph.binding().provider_graph_sha256,
        "outcome":graph.outcome(), "nodes":nodes}),
    )
}

/// Render a registered format without accepting raw records or imported projections.
pub fn render(graph: &VerifiedGraph, format: &str) -> Result<Vec<u8>> {
    let policy = &cohesix_authority::provider::registry()?["contract"]["export"];
    ensure!(
        policy["formats"]
            .as_array()
            .is_some_and(|formats| formats.iter().any(|value| value == format)),
        "not_registered export format"
    );
    let value = match format {
        "prometheus" => return bounded(prometheus::render(graph)?.into_bytes(), policy),
        "otel" => otel::render(graph)?,
        "cloudevents" => json!({"specversion":"1.0", "id":graph.digest(),
            "source":"urn:cohesix:evidence", "type":"io.cohesix.evidence.verified.v1",
            "subject":graph.binding().ticket_id, "datacontenttype":"application/json",
            "data":projection(graph)?}),
        "in_toto" => json!({"_type":"https://in-toto.io/Statement/v1",
            "subject":[{"name":"cohesix-causal-evidence","digest":{"sha256":graph.digest()}}],
            "predicateType":"urn:cohesix:derived-evidence:v1", "predicate":projection(graph)?}),
        "siem" => projection(graph)?,
        "slsa" => bail!("not_implemented registered build provenance action required"),
        _ => bail!("not_registered export format"),
    };
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    bounded(bytes, policy)
}

fn bounded(bytes: Vec<u8>, policy: &Value) -> Result<Vec<u8>> {
    let maximum = policy["maximum_bytes"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("invalid export bound"))?;
    ensure!(bytes.len() as u64 <= maximum, "ELIMIT exporter projection");
    Ok(bytes)
}
