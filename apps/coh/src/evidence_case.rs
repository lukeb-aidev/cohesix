// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Link retained timeline observations without asserting execution or target proof.
// Author: Lukas Bower

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde::Serialize;

use super::TimelineEvent;
use crate::operator::{self, Availability};

/// Review framing only; scenario selection cannot alter evidence or its authority.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Scenario {
    /// General evidence review.
    #[default]
    Generic,
    /// Incident reconstruction.
    Incident,
    /// Configuration or policy change review.
    Change,
    /// Maintenance and lifecycle review.
    Maintenance,
    /// Model or software rollout review.
    Rollout,
    /// Cross-hive relay review.
    Federation,
}

#[derive(Debug, Serialize)]
struct SourceLink {
    path: String,
    timeline_event: usize,
    event_sha256: String,
    sequence: Option<u64>,
}

#[derive(Debug, Serialize)]
struct Stage {
    name: &'static str,
    status: &'static str,
    sources: Vec<SourceLink>,
}

#[derive(Debug, Serialize)]
struct Chain {
    // A structured tuple avoids collisions between delimiter-bearing identifiers.
    correlation: Vec<Option<String>>,
    outcome: &'static str,
    stages: Vec<Stage>,
}

#[derive(Debug, Serialize)]
struct Case {
    schema: &'static str,
    scenario: Scenario,
    evidence_class: &'static str,
    proof: &'static str,
    inventory: BTreeMap<String, Availability>,
    chains: Vec<Chain>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recipe: Option<serde_json::Value>,
}

const STAGES: &[(&str, &[&str])] = &[
    ("request", &["host/tickets/spec", "audit/journal"]),
    ("decision", &["audit/decisions", "audit/journal"]),
    (
        "state-lease-lifecycle",
        &["proc/lease/active", "proc/lifecycle", "proc/root"],
    ),
    ("host-result", &["host/tickets/status"]),
    ("receipt-deadletter", &["host/tickets/deadletter"]),
    (
        "federation-relay",
        &[
            "host/tickets/spec",
            "host/tickets/status",
            "host/tickets/deadletter",
        ],
    ),
];

pub(super) fn build(
    root: &Path,
    events: &[TimelineEvent],
    scenario: Scenario,
) -> Result<(Vec<u8>, String)> {
    let inventory = inventory(root)?;
    let mut groups: BTreeMap<Vec<Option<String>>, Vec<(usize, &TimelineEvent)>> = BTreeMap::new();
    for (index, event) in events.iter().enumerate() {
        let key = if event.id.is_some() && event.idempotency_key.is_some() {
            vec![
                event.id.clone(),
                event.idempotency_key.clone(),
                event.source_hive.clone(),
                event.target_hive.clone(),
            ]
        } else {
            // An id-only decision is not enough to bind a particular retry, lease,
            // resource generation, or hive. Preserve it as a separate unknown chain.
            vec![
                event.id.clone(),
                None,
                None,
                None,
                Some(format!("unlinked:{index}")),
            ]
        };
        groups.entry(key).or_default().push((index, event));
    }
    let mut chains = Vec::new();
    for (correlation, records) in groups {
        let mut stages = Vec::new();
        for (name, paths) in STAGES {
            let selected: Vec<_> = records
                .iter()
                .filter(|(_, event)| belongs(name, event))
                .collect();
            let values: BTreeSet<_> = selected
                .iter()
                .filter_map(|(_, e)| e.outcome.as_deref().or(e.state.as_deref()))
                .collect();
            let status = if selected.iter().any(|(_, e)| e.error.is_some()) {
                "error"
            } else if values.len() > 1
                && matches!(*name, "host-result" | "receipt-deadletter" | "decision")
            {
                "ambiguous"
            } else if !selected.is_empty() {
                "observed"
            } else if paths
                .iter()
                .any(|p| inventory.get(*p) == Some(&Availability::Error))
            {
                "error"
            } else if paths
                .iter()
                .all(|p| inventory.get(*p) == Some(&Availability::Missing))
            {
                "missing"
            } else {
                "unknown"
            };
            let mut sources = Vec::new();
            for (index, event) in selected {
                sources.push(SourceLink {
                    path: event.source.clone(),
                    timeline_event: *index,
                    event_sha256: operator::digest(&serde_json::to_vec(event)?),
                    sequence: event.seq.or(event.lease_seq),
                });
            }
            stages.push(Stage {
                name,
                status,
                sources,
            });
        }
        let has = |stage: &str| {
            stages
                .iter()
                .any(|s| s.name == stage && s.status == "observed")
        };
        let outcome = if stages.iter().any(|s| s.status == "ambiguous") {
            "ambiguous"
        } else if records
            .iter()
            .any(|(_, e)| e.source == "host/tickets/deadletter")
        {
            "deadlettered"
        } else if records
            .iter()
            .any(|(_, e)| matches!(e.outcome.as_deref(), Some("denied" | "refused" | "deny")))
        {
            "refused"
        } else if has("request")
            && has("host-result")
            && records.iter().any(|(_, e)| {
                e.source == "host/tickets/status"
                    && matches!(
                        e.state.as_deref(),
                        Some("done" | "succeeded" | "failed" | "error" | "completed")
                    )
            })
        {
            "recorded-terminal"
        } else {
            "incomplete"
        };
        chains.push(Chain {
            correlation,
            outcome,
            stages,
        });
    }
    let case = Case {
        schema: "cohesix-evidence-pack/case-v1",
        scenario,
        evidence_class: "offline-review",
        proof: "none",
        inventory,
        chains,
        recipe: crate::recipe::case_diagnostic(root)?,
    };
    let json = serde_json::to_vec_pretty(&case)?;
    ensure!(json.len() <= operator::MAX_BYTES, "case-output-bound");
    let mut markdown = format!("# Evidence case\n\nScenario: {}\n\nOffline review; no target, hardware, health, external-execution, or authoritative-receipt proof.\n\n", serde_json::to_string(&scenario)?);
    if let Some(recipe) = &case.recipe {
        markdown.push_str(&format!(
            "Recipe operation `{}` (local observation, proof none).\n\nCause: {}.\n\nRemaining uncertainty: {}.\n\nRecovery: {}.\n\nSource: `attachments/recipe.json`, sha256:{}.\n\n",
            markdown_escape(recipe["operation_id"].as_str().unwrap_or("unknown")),
            markdown_escape(recipe["cause"].as_str().unwrap_or("unknown")),
            markdown_escape(recipe["uncertainty"].as_str().unwrap_or("unknown")),
            markdown_escape(recipe["recovery"].as_str().unwrap_or("unknown")),
            markdown_escape(recipe["source_sha256"].as_str().unwrap_or("unknown")),
        ));
    }
    for chain in &case.chains {
        markdown.push_str(&format!(
            "- Correlation `{}`: **{}**\n",
            markdown_escape(&serde_json::to_string(&chain.correlation)?),
            chain.outcome
        ));
        for stage in &chain.stages {
            markdown.push_str(&format!("  - {}: {}", stage.name, stage.status));
            for link in &stage.sources {
                markdown.push_str(&format!(
                    "; {} / timeline event {} / sha256:{}",
                    link.path, link.timeline_event, link.event_sha256
                ));
            }
            markdown.push('\n');
        }
    }
    markdown.push_str("\nCapture inventory:\n\n");
    for (path, status) in &case.inventory {
        markdown.push_str(&format!(
            "- `{}`: {}\n",
            markdown_escape(path),
            serde_json::to_string(status)?
        ));
    }
    ensure!(markdown.len() <= operator::MAX_BYTES, "case-output-bound");
    Ok((json, markdown))
}

fn belongs(stage: &str, event: &TimelineEvent) -> bool {
    match stage {
        "request" => {
            event.source == "host/tickets/spec"
                || (event.source == "audit/journal"
                    && event.path.as_deref() == Some("/host/tickets/spec"))
        }
        "decision" => {
            event.source == "audit/decisions"
                || (event.source == "audit/journal" && event.outcome.is_some())
        }
        "state-lease-lifecycle" => {
            event.source.starts_with("proc/lease/")
                || event.source.starts_with("proc/lifecycle/")
                || event.source.starts_with("proc/root/")
        }
        "host-result" => event.source == "host/tickets/status",
        "receipt-deadletter" => event.source == "host/tickets/deadletter",
        "federation-relay" => {
            event.source_hive.is_some()
                && event.target_hive.is_some()
                && event.relay_correlation_id.is_some()
        }
        _ => false,
    }
}

pub(super) fn inventory(root: &Path) -> Result<BTreeMap<String, Availability>> {
    let path = operator::confined_path(root, "summary.json")?;
    if !path.exists() {
        return Ok(BTreeMap::from([(
            "summary.json".to_owned(),
            Availability::Unknown,
        )]));
    }
    let value: serde_json::Value =
        serde_json::from_slice(&operator::read_bounded(&path, operator::MAX_BYTES)?)
            .context("case-summary-json")?;
    ensure!(
        value.get("schema").and_then(|v| v.as_str()) == Some("cohesix-evidence-pack/summary-v1"),
        "case-summary-schema"
    );
    let items = value
        .get("items")
        .and_then(|v| v.as_array())
        .context("case-summary-items")?;
    ensure!(items.len() <= operator::MAX_FILES, "case-inventory-bound");
    let mut inventory = BTreeMap::new();
    for item in items {
        let path = item
            .get("saved_as")
            .and_then(|v| v.as_str())
            .context("case-inventory-path")?;
        operator::confined_path(root, path)?;
        let status = match item.get("status").and_then(|v| v.as_str()) {
            Some("captured") => {
                if root.join(path).is_file() {
                    Availability::Observed
                } else {
                    Availability::Error
                }
            }
            Some("missing") => Availability::Missing,
            Some("error") => Availability::Error,
            _ => Availability::Unknown,
        };
        ensure!(
            inventory.insert(path.to_owned(), status).is_none(),
            "case-duplicate-inventory"
        );
    }
    Ok(inventory)
}

fn markdown_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('`', "&#96;")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
