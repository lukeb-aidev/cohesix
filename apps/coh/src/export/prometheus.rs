// Author: Lukas Bower
// Purpose: Expose evidence gauges with bounded generated action labels and no ticket-level metric cardinality.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use cohesix_evidence::VerifiedGraph;

pub(super) fn render(graph: &VerifiedGraph) -> Result<String> {
    // Action ids passed registry validation; outcome comes from a closed enum.
    let outcome = super::label(graph.outcome())?;
    Ok(format!("# HELP cohesix_evidence_terminal_info Validated terminal evidence; derived and non-authoritative.\n# TYPE cohesix_evidence_terminal_info gauge\ncohesix_evidence_terminal_info{{action=\"{}\",outcome=\"{}\"}} 1\n# HELP cohesix_evidence_records Number of validated causal records.\n# TYPE cohesix_evidence_records gauge\ncohesix_evidence_records{{action=\"{}\"}} {}\n", graph.binding().action, outcome, graph.binding().action, graph.nodes().len()))
}
