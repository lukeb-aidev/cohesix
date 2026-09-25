// Author: Lukas Bower
// Purpose: Derive the selected MCP tool and evidence catalogue from compiler-owned authority inputs.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

const REVISION: &str = "2025-11-25";

fn digest_schema() -> Value {
    json!({"type":"string","pattern":"^[0-9a-f]{64}$"})
}

fn selected_ticket_schema(actions: &[&str]) -> Value {
    let mut conditions = Vec::new();
    if actions.contains(&"gpu.workload.submit") {
        conditions.push(json!({
            "if":{"properties":{"action":{"const":"gpu.workload.submit"}}},
            "then":{"properties":{
                "receipt_worker_role":{"const":"worker-gpu"},
                "args":{"type":"object","additionalProperties":false,
                    "required":["lease_id","request_sha256"],
                    "properties":{"lease_id":{"type":"string","minLength":1,"maxLength":32},
                        "request_sha256":digest_schema()}}
            }}
        }));
    }
    if actions.contains(&"peft.release") {
        conditions.push(json!({
            "if":{"properties":{"action":{"const":"peft.release"}}},
            "then":{"properties":{
                "receipt_worker_role":{"const":"worker-lora"},
                "args":{"type":"object","additionalProperties":false,
                    "required":["request_sha256"],
                    "properties":{"request_sha256":digest_schema(),"recovery_only":{"type":"boolean"}}}
            }}
        }));
    }
    json!({
        "type":"object","additionalProperties":false,
        "required":["schema","id","idempotency_key","action","args","receipt_mode",
            "operation_id","subject_ref","receipt_worker_role","receipt_worker_id",
            "receipt_supervisor_generation","receipt_cap_generation"],
        "properties":{
            "writer_epoch":{"type":"integer","minimum":1},
            "schema":{"const":"host-ticket/v2"},
            "id":{"type":"string","minLength":1,"maxLength":96},
            "idempotency_key":{"type":"string","minLength":1,"maxLength":96},
            "action":{"type":"string","enum":actions},
            "args":{"type":"object"},
            "expires_unix_ms":{"type":"integer","minimum":1},
            "receipt_mode":{"const":"worker"},
            "operation_id":{"type":"string","minLength":1,"maxLength":96},
            "subject_ref":{"type":"string","minLength":1,"maxLength":96},
            "receipt_worker_role":{"type":"string"},
            "receipt_worker_id":{"type":"string","minLength":1,"maxLength":96},
            "receipt_supervisor_generation":{"type":"integer","minimum":1},
            "receipt_cap_generation":{"type":"integer","minimum":1}
        },
        "allOf":conditions
    })
}

fn job_binding_schema(actions: &[&str]) -> Value {
    json!({
        "type":"object","additionalProperties":false,
        "required":["schema","scope_id","admission_id","ticket_id","idempotency_key",
            "subject","action","target","input_sha256","policy_sha256","state_epoch",
            "resource_generation","deadline_unix_ms","units","attempt"],
        "properties":{
            "schema":{"const":cohesix_authority::standing::JOB_BINDING_SCHEMA},
            "scope_id":{"type":"string","minLength":1,"maxLength":96},
            "admission_id":{"type":"string","minLength":1,"maxLength":96},
            "ticket_id":{"type":"string","minLength":1,"maxLength":96},
            "idempotency_key":{"type":"string","minLength":1,"maxLength":96},
            "subject":{"type":"string","minLength":1,"maxLength":96},
            "action":{"type":"string","enum":actions},
            "target":{"type":"string","minLength":1,"maxLength":96},
            "input_sha256":digest_schema(),
            "policy_sha256":digest_schema(),
            "state_epoch":{"type":"integer","minimum":1},
            "resource_generation":{"type":"integer","minimum":1},
            "deadline_unix_ms":{"type":"integer","minimum":1},
            "units":{"const":1},
            "attempt":{"type":"integer","minimum":1,"maximum":9}
        }
    })
}

fn output_schema(reference: &str) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert("schema".into(), json!({"const":reference}));
    match reference {
        "cohesix-selected-job-preflight/v1" => {
            properties.insert("request".into(), json!({"type":"object"}));
            properties.insert("admission".into(), json!({"const":"not_submitted"}));
        }
        "cohesix-selected-job-response/v1" | "cohesix-selected-job-reconciliation/v1" => {
            properties.insert("record".into(), json!({"type":"object"}));
        }
        "cohesix-available-scopes/v1" | "cohesix-available-selected-jobs/v1" => {
            properties.insert("scopes".into(), json!({"type":"array"}));
        }
        _ => {}
    }
    json!({"type":"object","properties":properties})
}

fn tool(
    name: &str,
    description: &str,
    properties: Value,
    required: &[&str],
    actions: &[&str],
    authority_path: &str,
    read_only: bool,
    output_schema_ref: &str,
) -> Value {
    json!({
        "name":name,
        "description":description,
        "input_schema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},
        "output_schema_ref":output_schema_ref,
        "output_schema":output_schema(output_schema_ref),
        "selected_actions":actions,
        "authority_path":authority_path,
        "read_only":read_only,
        "lifecycle":"admission-or-observation-only; inspect original identity for terminal result",
        "evidence":"shared verifier and native provider receipt only; MCP result is not proof"
    })
}

/// Emit the bounded selected catalogue only from the resolved manifest and
/// provider graph. The client sees no arbitrary namespace or raw payload tool.
pub fn emit(resolved: &Path, registry: &Path, output: &Path) -> Result<()> {
    let resolved_bytes = fs::read(resolved).context("read selected resolved manifest")?;
    let manifest: Value = serde_json::from_slice(&resolved_bytes)?;
    let registry: Value = serde_json::from_slice(&fs::read(registry)?)?;
    let gateway = &manifest["gateway"];
    let selected = manifest["standing_authority"]["actions"]
        .as_array()
        .context("selected standing actions missing")?;
    if gateway["schema"] != "cohesix-agent-protocol-controls/v1" {
        bail!("unsupported selected gateway controls");
    }
    let enabled =
        gateway["agent_protocols"]["enabled"] == true && gateway["mcp"]["enabled"] == true;
    let mut actions = Vec::new();
    for action in selected {
        let id = action.as_str().context("selected standing action id")?;
        let row = registry["contract"]["families"]
            .as_array()
            .context("generated provider families missing")?
            .iter()
            .flat_map(|family| family["actions"].as_array().into_iter().flatten())
            .find(|candidate| candidate["id"] == id)
            .context("selected action absent from generated provider registry")?;
        actions.push(json!({
            "id":id,
            "intent_schema_ref":row["intent_schema_ref"],
            "argument_schema_ref":row["argument_schema_ref"],
            "receipt_schema_ref":row["receipt_schema_ref"],
            "authority_custodian":row["authority_custodian"],
            "effect":row["effect"],
            "correlation_fields":row["correlation_fields"],
            "evidence_profile":row["evidence_profile"],
        }));
    }
    let id = json!({"admission_id":{"type":"string","minLength":1,"maxLength":96}});
    let mut tools = vec![
        tool("cohesix.inspect_job", "Read original execution and result-delivery state for this subject.", id.clone(), &["admission_id"], &[], "/host/tickets/status", true, "cohesix-selected-job-record/v1"),
        tool("cohesix.recover_job", "Reconcile the original identity with target result evidence without replaying its effect.", id.clone(), &["admission_id"], &[], "/host/tickets/status", true, "cohesix-selected-job-reconciliation/v1"),
        tool("cohesix.request_cancel", "Record cancellation for the original identity; this does not prove native termination.", id, &["admission_id"], &[], "/host/tickets/spec", false, "cohesix-selected-job-record/v1"),
    ];
    if selected.iter().any(|row| row == "systemd.restart") {
        tools.push(tool(
            "cohesix.available_services",
            "List currently usable approved service scopes for this subject.",
            json!({}),
            &[],
            &["systemd.restart"],
            "/host/tickets/status",
            true,
            "cohesix-available-scopes/v1",
        ));
        tools.push(tool("cohesix.start_approved_service", "Submit one approved service restart with a stable request ID.", json!({"scope_id":{"type":"string","minLength":1,"maxLength":96},"request_id":{"type":"string","minLength":1,"maxLength":96}}), &["scope_id","request_id"], &["systemd.restart"], "/host/tickets/spec", false, "cohesix-selected-job-response/v1"));
    }
    let submitted: Vec<_> = selected
        .iter()
        .filter_map(Value::as_str)
        .filter(|id| matches!(*id, "gpu.workload.submit" | "peft.release"))
        .collect();
    if !submitted.is_empty() {
        let ticket_schema = selected_ticket_schema(&submitted);
        tools.push(tool("cohesix.available_selected_jobs", "List currently usable CUDA and PEFT standing scopes, finite capacity and exact targets for this subject.", json!({}), &[], &submitted, "/host/tickets/status", true, "cohesix-available-selected-jobs/v1"));
        tools.push(tool("cohesix.preflight_selected_job", "Observe the selected CUDA or PEFT host and prepare one short-lived exact job request. No effect is submitted; submit rechecks facts and authority.", json!({"scope_id":{"type":"string","minLength":1,"maxLength":96},"ticket":ticket_schema}), &["scope_id","ticket"], &submitted, "/host/tickets/status", true, "cohesix-selected-job-preflight/v1"));
        tools.push(tool("cohesix.submit_selected_job", "Submit one exact selected CUDA or PEFT host ticket and standing binding. An ACK is admission only.", json!({"binding":job_binding_schema(&submitted),"ticket":selected_ticket_schema(&submitted)}), &["binding","ticket"], &submitted, "/host/tickets/spec", false, "cohesix-selected-job-response/v1"));
    }
    let catalogue = json!({
        "schema":"cohesix-mcp-catalogue/v1",
        "enabled":enabled,
        "revision":REVISION,
        "transports":["streamable-http","stdio"],
        "http_path":"/mcp",
        "bounds":{"request_bytes":8192,"response_bytes":16384,"concurrent_requests":16,"call_timeout_ms":30000,"session_mode":"stateless"},
        "resolved_manifest_sha256":hex::encode(Sha256::digest(&resolved_bytes)),
        "provider_graph_sha256":registry["graph_sha256"],
        "actions":actions,
        "tools": if enabled { tools } else { Vec::new() },
        "resources":[{"uri_template":"cohesix://jobs/{admission_id}","authority_path":"/host/tickets/status","output_schema_ref":"cohesix-selected-job-record/v1","effect":"read_only"}],
    });
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        output,
        format!("{}\n", serde_json::to_string_pretty(&catalogue)?),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_actions_and_master_switch_determine_the_mcp_catalogue() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../configs/generated");
        let registry = root.join("provider_registry.json");
        let temporary = tempfile::tempdir().unwrap();
        let manifest_path = temporary.path().join("resolved.json");
        let catalogue_path = temporary.path().join("catalogue.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(root.join("root_task_resolved.json")).unwrap())
                .unwrap();
        manifest["standing_authority"]["actions"] = json!(["peft.release"]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        emit(&manifest_path, &registry, &catalogue_path).unwrap();
        let catalogue: Value = serde_json::from_slice(&fs::read(&catalogue_path).unwrap()).unwrap();
        assert_eq!(catalogue["actions"][0]["id"], "peft.release");
        assert!(catalogue["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| { tool["name"] == "cohesix.preflight_selected_job" }));
        assert!(!catalogue["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| { tool["name"] == "cohesix.start_approved_service" }));

        manifest["gateway"]["agent_protocols"]["enabled"] = false.into();
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        emit(&manifest_path, &registry, &catalogue_path).unwrap();
        let closed: Value = serde_json::from_slice(&fs::read(&catalogue_path).unwrap()).unwrap();
        assert_eq!(closed["enabled"], false);
        assert!(closed["tools"].as_array().unwrap().is_empty());
    }
}
