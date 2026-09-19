// Author: Lukas Bower
// Purpose: Preserve source graph links as derived OTLP JSON spans without inventing native timing or authority.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use cohesix_evidence::VerifiedGraph;
use serde_json::{json, Value};

pub(super) fn render(graph: &VerifiedGraph) -> Result<Value> {
    let trace = &graph.digest()[..32];
    let mut spans = Vec::new();
    for node in graph.nodes() {
        let time = (node.observed_unix_ms * 1_000_000).to_string();
        spans.push(json!({"traceId":trace,"spanId":&node.sha256[..16],
            "name":format!("{}.{}", graph.binding().action, super::label(node.kind)?),
            "kind":1, "startTimeUnixNano":time,"endTimeUnixNano":time,
            "attributes":[
                {"key":"cohesix.authoritative","value":{"boolValue":false}},
                {"key":"cohesix.ticket_id","value":{"stringValue":graph.binding().ticket_id}},
                {"key":"cohesix.graph_sha256","value":{"stringValue":graph.digest()}},
                {"key":"cohesix.record_sha256","value":{"stringValue":node.sha256}},
                {"key":"cohesix.outcome","value":{"stringValue":super::label(node.outcome)?}}],
            "links":node.parents.iter().map(|parent| json!({"traceId":trace,"spanId":&parent[..16]})).collect::<Vec<_>>()
        }));
    }
    Ok(
        json!({"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"cohesix-evidence-export"}}]},
        "scopeSpans":[{"scope":{"name":"cohesix-derived-evidence","version":"1"},"spans":spans}]}]}),
    )
}
