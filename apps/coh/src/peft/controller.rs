// Author: Lukas Bower
// Purpose: Persist one PEFT release ticket before dispatch and reuse the established signed workflow verifier after lost acknowledgements.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use super::{release, transaction::Request};
use crate::{
    operator::read_bounded,
    recipe,
    workflow::{self, Step},
    CohAccess,
};
use anyhow::{anyhow, ensure, Result};
use cohesix_evidence::{Kind, Outcome, Trust};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A controller holds references and intent; native authority remains on the host.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    /// Exact host composition grammar.
    pub schema: String,
    /// Private durable intent directory, shared with the recipe journal implementation.
    pub journal: PathBuf,
    /// Full immutable release request retained in the provider's configured CAS.
    pub request: Request,
    /// Original ticket and independently enrolled gateway/native/Worker evidence.
    pub execution: Step,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    deployment_sha256: String,
    submitted: bool,
    acknowledged: bool,
}

/// Validate ticket, native request and independent enrollment before any write.
pub fn load(path: &Path) -> Result<Deployment> {
    let d: Deployment = serde_json::from_slice(&read_bounded(path, 65536)?)?;
    ensure!(
        d.schema == "cohesix-peft-deployment/v1" && d.journal.is_absolute(),
        "invalid_request peft-deployment"
    );
    let caller = cohesix_evidence::ticket::caller_request(&d.execution.request, false)?;
    ensure!(
        caller["schema"] == "host-ticket/v2"
            && caller["action"] == "peft.release"
            && caller["receipt_mode"] == "worker"
            && caller["receipt_worker_role"] == "worker-lora"
            && caller["operation_id"] == d.request.operation_id
            && caller["subject_ref"] == d.request.model_id
            && caller["args"]["request_sha256"] == release::identity(&d.request)?
            && cohesix_authority::peft::validate_release_args(&caller["args"]),
        "EPERM peft-release-binding"
    );
    let trust: Trust = serde_json::from_slice(&read_bounded(&d.execution.trust, 65536)?)?;
    cohesix_evidence::ticket::require_binding(&trust.expected, &caller, false)?;
    Ok(d)
}

fn report(d: &Deployment, intent: &Intent) -> Result<Value> {
    let verified = workflow::verified(&d.execution)?;
    let result = if let Some(graph) = &verified {
        let node = graph
            .nodes()
            .iter()
            .find(|n| n.kind == Kind::Observation)
            .ok_or_else(|| anyhow!("invalid_evidence native-release-observation"))?;
        ensure!(
            node.artifacts.len() == 1,
            "invalid_evidence native-release-artifact"
        );
        let artifact: Value = serde_json::from_slice(&read_bounded(
            &d.execution.cas.join(&node.artifacts[0].sha256),
            65536,
        )?)?;
        let native = &artifact["observation"];
        let recovery = d.execution.request["args"]["recovery_only"] == true;
        let identity = if recovery {
            &native["recovery_authority"]
        } else {
            native
        };
        ensure!(
            native["schema"] == "cohesix-peft-recipe-journal/v1"
                && native["operation_id"] == d.request.operation_id
                && native["request_sha256"] == release::identity(&d.request)?
                && identity["ticket_id"] == d.execution.request["id"]
                && identity["idempotency_key"] == d.execution.request["idempotency_key"],
            "invalid_evidence release-substitution"
        );
        require_outcome(&native["state"], graph.outcome())?;
        Some(json!({"state":native["state"], "graph_sha256":graph.digest(), "native":native}))
    } else {
        None
    };
    Ok(
        json!({"schema":"cohesix-peft-report/v1", "authoritative":false, "production_use_case_accepted":false,
        "operation_id":d.request.operation_id, "request_sha256":release::identity(&d.request)?,
        "submitted":intent.submitted, "acknowledged":intent.acknowledged,
        "ambiguous":intent.submitted && result.is_none(), "result":result}),
    )
}

fn require_outcome(native_state: &Value, outcome: Outcome) -> Result<()> {
    ensure!(
        (native_state == "succeeded") == (outcome == Outcome::Succeeded),
        "invalid_evidence release-terminal-outcome"
    );
    Ok(())
}

/// Plan, submit at most once, or reconstruct signed native results without re-execution.
pub fn advance(
    d: &Deployment,
    mode: &str,
    access: Option<&mut dyn CohAccess>,
    now: u64,
) -> Result<Value> {
    ensure!(
        matches!(
            mode,
            "plan" | "apply" | "watch" | "explain" | "verify" | "recover"
        ),
        "invalid_request release-mode"
    );
    let _lock = recipe::lock_directory(&d.journal, mode == "plan")?;
    let path = d.journal.join("recipe.json");
    let digest = release::identity(d)?;
    let mut intent = if path.try_exists()? {
        let i: Intent = serde_json::from_slice(&read_bounded(&path, 8192)?)?;
        ensure!(
            i.deployment_sha256 == digest,
            "conflict release-controller-identity"
        );
        i
    } else {
        ensure!(mode == "plan", "EPERM release-plan-required");
        let i = Intent {
            deployment_sha256: digest,
            submitted: false,
            acknowledged: false,
        };
        recipe::save_record(&d.journal, "recipe.json", &i)?;
        i
    };
    if mode == "apply" && !intent.submitted {
        ensure!(
            d.execution.request["expires_unix_ms"]
                .as_u64()
                .is_some_and(|expiry| now < expiry),
            "EPERM release-expired-authority"
        );
        let access = access.ok_or_else(|| anyhow!("EPERM release-transport"))?;
        let mut bytes = serde_json::to_vec(&d.execution.request)?;
        bytes.push(b'\n');
        intent.submitted = true;
        recipe::save_record(&d.journal, "recipe.json", &intent)?;
        ensure!(
            access.write_append("/host/tickets/spec", &bytes)? == bytes.len(),
            "ambiguous release-submission-ack"
        );
        intent.acknowledged = true;
        recipe::save_record(&d.journal, "recipe.json", &intent)?;
    }
    let value = report(d, &intent)?;
    if mode == "verify" {
        ensure!(
            value["result"]["state"] == "succeeded",
            "unverified native-release"
        );
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_success_requires_successful_verified_terminal_evidence() {
        require_outcome(&json!("succeeded"), Outcome::Succeeded).unwrap();
        for outcome in [Outcome::Failed, Outcome::Cancelled, Outcome::Expired] {
            assert!(require_outcome(&json!("succeeded"), outcome).is_err());
        }
        for state in ["failed", "recovered_failure", "rollback_failed"] {
            require_outcome(&json!(state), Outcome::Failed).unwrap();
            assert!(require_outcome(&json!(state), Outcome::Succeeded).is_err());
        }
    }
}
