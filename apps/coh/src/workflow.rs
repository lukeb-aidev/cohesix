// Author: Lukas Bower
// Purpose: Advance generated host workflows only through admitted tickets and verified causal terminal records.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use crate::{operator::read_bounded, CohAccess};
use anyhow::{anyhow, ensure, Result};
use cohesix_evidence::{Kind, Outcome, Trust, VerifiedGraph};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// Operator-owned deployment inputs. They cannot override generated action order.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    /// Exact version of this non-authoritative orchestration input.
    pub schema: String,
    /// Generated playbook id.
    pub workflow_id: String,
    /// Exact provider/integration graph selection.
    pub provider_graph_sha256: String,
    /// Explicit controller, target hive and provider host identities.
    pub topology: BTreeMap<String, String>,
    /// Installed exact host package and its separately enrolled trust policy.
    pub package: Package,
    /// One request per generated execution action, in its declared order.
    pub steps: Vec<Step>,
    /// Separately admitted compensation requests, never inferred from failure.
    pub recovery: Vec<Step>,
}
/// Package verification never proves native execution.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    /// Exact installed directory.
    pub input: PathBuf,
    /// Independently enrolled trust policy outside that directory.
    pub trust: PathBuf,
}
/// One request and its provider-owned causal graph destination.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Root-admitted request grammar, including stable idempotency and writer fence.
    pub request: Value,
    /// Native/gateway/Worker signed graph, never created by this orchestrator.
    pub graph: PathBuf,
    /// Independent phase custodian enrollment.
    pub trust: PathBuf,
    /// Hash-addressed producer artifact store.
    pub cas: PathBuf,
}

/// Inspect the generated DAG and unavailable external owners without a transport.
pub fn plan(id: &str) -> Result<Value> {
    let registry = cohesix_authority::provider::registry()?;
    let row = registry["playbooks"]
        .as_array()
        .and_then(|rows| rows.iter().find(|p| p["id"] == id))
        .ok_or_else(|| anyhow!("not_registered workflow"))?;
    Ok(
        json!({"schema":"cohesix-workflow-plan/v1","authoritative":false,
        "provider_graph_sha256":registry["graph_sha256"],"playbook":row,
        "production_use_case_accepted":false}),
    )
}

/// Validate a bounded deployment without dispatching any action.
pub fn load(path: &Path, id: &str) -> Result<Deployment> {
    let deployment: Deployment = serde_json::from_slice(&read_bounded(path, 65_536)?)?;
    let generated = plan(id)?;
    validate(&deployment, &generated)?;
    Ok(deployment)
}

fn validate(d: &Deployment, generated: &Value) -> Result<()> {
    ensure!(
        d.schema == "cohesix-workflow-deployment/v1"
            && d.workflow_id == generated["playbook"]["id"]
            && d.provider_graph_sha256 == generated["provider_graph_sha256"],
        "EPERM workflow-selection"
    );
    let workflow = &generated["playbook"]["workflow"];
    let nodes = workflow["topology"]
        .as_array()
        .ok_or_else(|| anyhow!("EPERM workflow-topology"))?;
    ensure!(
        d.topology.len() == nodes.len(),
        "EPERM workflow-topology-count"
    );
    for node in nodes {
        let name = node["id"]
            .as_str()
            .ok_or_else(|| anyhow!("EPERM workflow-node"))?;
        cohesix_authority::validate_id(
            d.topology
                .get(name)
                .ok_or_else(|| anyhow!("EPERM missing-workflow-node"))?,
        )
        .map_err(|_| anyhow!("EPERM workflow-native-node"))?;
    }
    let mut identities = BTreeSet::new();
    for (steps, key) in [
        (&d.steps, "execute_actions"),
        (&d.recovery, "recovery_actions"),
    ] {
        let actions = workflow[key]
            .as_array()
            .ok_or_else(|| anyhow!("EPERM workflow-actions"))?;
        ensure!(
            steps.len() == actions.len() && steps.len() <= 16,
            "EPERM workflow-step-count"
        );
        for (step, action) in steps.iter().zip(actions) {
            let request = cohesix_evidence::ticket::caller_request(&step.request, false)?;
            ensure!(request["action"] == *action, "EPERM workflow-action-order");
            let id = request["id"]
                .as_str()
                .ok_or_else(|| anyhow!("EPERM workflow-ticket"))?;
            let key = request["idempotency_key"]
                .as_str()
                .ok_or_else(|| anyhow!("EPERM workflow-idempotency"))?;
            for value in [id, key] {
                cohesix_authority::validate_id(value).map_err(|_| anyhow!("EPERM workflow-id"))?;
            }
            ensure!(
                identities.insert((id.to_owned(), key.to_owned())),
                "EPERM workflow-duplicate-ticket"
            );
            let action = action
                .as_str()
                .ok_or_else(|| anyhow!("EPERM workflow-action"))?;
            cohesix_authority::provider::validate_request_size(
                action,
                serde_json::to_vec(&request)?.len(),
            )?;
            let trust = enrollment(step)?;
            cohesix_evidence::ticket::require_binding(&trust.expected, &request, false)?;
            ensure!(
                trust.expected.provider_graph_sha256 == d.provider_graph_sha256,
                "EPERM workflow-evidence-graph"
            );
            ensure!(
                [&step.graph, &step.trust, &step.cas]
                    .iter()
                    .all(|p| p.is_absolute()),
                "EPERM workflow-evidence-path"
            );
        }
    }
    Ok(())
}
fn enrollment(step: &Step) -> Result<Trust> {
    Ok(serde_json::from_slice(&read_bounded(&step.trust, 65_536)?)?)
}
fn preflight(d: &Deployment) -> Result<()> {
    let generated = plan(&d.workflow_id)?;
    validate(d, &generated)?;
    let required = generated["playbook"]["workflow"]["external_dependencies"]
        .as_array()
        .ok_or_else(|| anyhow!("EPERM workflow-dependencies"))?;
    // The shipped domain applications and 27d transactions have no enrolled
    // deployment yet. The generated owner remains visible; a client file or
    // generic control operation cannot impersonate their signed deployment.
    ensure!(
        required.is_empty(),
        "not_enabled workflow-external-deployment {}",
        required
            .iter()
            .filter_map(|r| r["id"].as_str())
            .collect::<Vec<_>>()
            .join(",")
    );
    let package = crate::package::verify(
        &d.package.input,
        &crate::package::load_external_trust(&d.package.input, &d.package.trust)?,
    )?;
    let package = serde_json::to_value(package)?;
    ensure!(
        package["profile_id"] == generated["playbook"]["workflow"]["package_profile"]
            && package["provider_graph_sha256"] == d.provider_graph_sha256,
        "EPERM workflow-package-profile"
    );
    Ok(())
}

fn verified(step: &Step) -> Result<Option<VerifiedGraph>> {
    if !step.graph.try_exists()? {
        return Ok(None);
    }
    let bytes = read_bounded(&step.graph, cohesix_evidence::MAX_GRAPH_BYTES)?;
    let graph: cohesix_evidence::Graph = serde_json::from_slice(&bytes)?;
    if !graph
        .records
        .iter()
        .any(|r| r.record.kind == Kind::Terminal)
    {
        return Ok(None);
    }
    let trust = enrollment(step)?;
    let verified = cohesix_evidence::verify(&bytes, &trust, |artifact| {
        cohesix_evidence::verify_cas(&step.cas, artifact)
    })?;
    cohesix_evidence::ticket::require_binding(verified.binding(), &step.request, false)?;
    // Identity alone is insufficient: compare the complete signed caller intent.
    let intent = verified
        .nodes()
        .iter()
        .find(|n| n.kind == Kind::Intent)
        .and_then(|n| n.artifacts.first())
        .ok_or_else(|| anyhow!("EPERM workflow-intent"))?;
    let artifact: Value =
        serde_json::from_slice(&read_bounded(&step.cas.join(&intent.sha256), 8192)?)?;
    ensure!(
        cohesix_evidence::ticket::caller_request(&artifact, false)?
            == cohesix_evidence::ticket::caller_request(&step.request, false)?,
        "EPERM workflow-intent-substitution"
    );
    Ok(Some(verified))
}

/// Reconstruct authoritative terminal results; pending/local status never advances a stage.
pub fn inspect(d: &Deployment) -> Result<Value> {
    validate(d, &plan(&d.workflow_id)?)?;
    let mut rows = Vec::new();
    let mut complete = true;
    for step in &d.steps {
        let result = verified(step)?;
        complete &= result
            .as_ref()
            .is_some_and(|v| v.outcome() == Outcome::Succeeded);
        rows.push(json!({"ticket_id":step.request["id"],"action":step.request["action"],
            "state":if result.is_some() {"verified_terminal"} else {"pending_or_unverified"},"evidence":result}));
    }
    Ok(
        json!({"schema":"cohesix-workflow-operation-report/v1","authoritative":false,
        "workflow_id":d.workflow_id,"provider_graph_sha256":d.provider_graph_sha256,
        "topology":d.topology,"steps":rows,"all_steps_verified":complete,
        "production_use_case_accepted":false}),
    )
}

/// Submit at most one bounded action. Root idempotency and native WAL own retries.
pub fn apply(access: &mut dyn CohAccess, d: &Deployment) -> Result<Value> {
    preflight(d)?;
    for step in &d.steps {
        if let Some(result) = verified(step)? {
            ensure!(
                result.outcome() == Outcome::Succeeded,
                "refused workflow-terminal-requires-recovery"
            );
            continue;
        }
        return submit(access, step, "admit_execute_observe_pending");
    }
    inspect(d)
}

/// Read bounded current status as a non-authoritative observation alongside signed evidence.
pub fn watch(access: &mut dyn CohAccess, d: &Deployment) -> Result<Value> {
    let bytes = access.read_file("/host/tickets/status", 65_536)?;
    ensure!(bytes.len() <= 65_536, "ELIMIT workflow-status");
    let mut result = inspect(d)?;
    // Only known identity and state labels are exported; native messages can contain paths.
    let ids: BTreeSet<_> = d
        .steps
        .iter()
        .filter_map(|s| s.request["id"].as_str())
        .collect();
    let mut rows = Vec::new();
    for line in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let row: Value = serde_json::from_slice(line)?;
        if row["id"].as_str().is_some_and(|id| ids.contains(id)) {
            rows.push(json!({"id":row["id"],"action":row["action"],"state":row["state"],"authoritative":false}));
        }
    }
    result["root_status_observations"] = rows.into();
    Ok(result)
}

/// Recovery requires a new enrolled ticket linked to an exact original terminal graph.
pub fn recover(access: &mut dyn CohAccess, d: &Deployment) -> Result<Value> {
    preflight(d)?;
    for step in &d.recovery {
        if let Some(result) = verified(step)? {
            ensure!(
                result.outcome() == Outcome::Succeeded,
                "refused recovery-terminal"
            );
            continue;
        }
        let trust = enrollment(step)?;
        let link = trust
            .expected
            .recovery_of
            .as_ref()
            .ok_or_else(|| anyhow!("EPERM recovery-link"))?;
        let original = d
            .steps
            .iter()
            .find(|s| s.request["id"] == link.ticket_id)
            .ok_or_else(|| anyhow!("EPERM recovery-original-ticket"))?;
        let original =
            verified(original)?.ok_or_else(|| anyhow!("unverified recovery-original"))?;
        let view = serde_json::to_value(&original)?;
        ensure!(
            original.digest() == link.graph_sha256
                && view["terminal_sha256"] == link.terminal_sha256,
            "EPERM recovery-original-graph"
        );
        return submit(access, step, "recovery_admission_pending");
    }
    Ok(
        json!({"schema":"cohesix-workflow-operation-report/v1","authoritative":false,
        "workflow_id":d.workflow_id,"recovery":"verified","production_use_case_accepted":false}),
    )
}
fn submit(access: &mut dyn CohAccess, step: &Step, phase: &str) -> Result<Value> {
    let mut bytes = serde_json::to_vec(&step.request)?;
    bytes.push(b'\n');
    let count = access.write_append("/host/tickets/spec", &bytes)?;
    ensure!(count == bytes.len(), "ambiguous workflow-ticket-write");
    Ok(
        json!({"schema":"cohesix-workflow-operation-report/v1","authoritative":false,
        "ticket_id":step.request["id"],"action":step.request["action"],"phase":phase,
        "terminal_verified":false,"production_use_case_accepted":false}),
    )
}
