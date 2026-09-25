// Author: Lukas Bower
// Purpose: Generate the bounded A2A skill catalogue from selected authority and provider truth.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// NeMo Agent Toolkit 1.9.0 uses a2a-sdk 0.3.26 and the A2A 0.3 JSON-RPC binding.
pub const REVISION: &str = "0.3.0";

/// The compiler, not a runtime flag or peer, decides which effects are visible.
pub fn emit(resolved: &Path, registry: &Path, output: &Path) -> Result<()> {
    let resolved_bytes = fs::read(resolved).context("read selected resolved manifest")?;
    let manifest: Value = serde_json::from_slice(&resolved_bytes)?;
    let registry: Value = serde_json::from_slice(&fs::read(registry)?)?;
    let gateway = &manifest["gateway"];
    if gateway["schema"] != "cohesix-agent-protocol-controls/v1" {
        bail!("unsupported selected gateway controls");
    }
    let selected = manifest["standing_authority"]["actions"]
        .as_array()
        .context("selected standing actions missing")?;
    let enabled =
        gateway["agent_protocols"]["enabled"] == true && gateway["a2a"]["enabled"] == true;
    let provider_actions: Vec<&Value> = registry["contract"]["families"]
        .as_array()
        .context("generated provider families missing")?
        .iter()
        .flat_map(|family| family["actions"].as_array().into_iter().flatten())
        .collect();
    let mut skills = Vec::new();
    for (action, name) in [
        ("gpu.workload.submit", "CUDA selected job"),
        ("peft.release", "PEFT release job"),
    ] {
        if !selected.iter().any(|row| row == action) {
            continue;
        }
        let provider = provider_actions
            .iter()
            .find(|row| row["id"] == action)
            .context("selected A2A action absent from provider registry")?;
        skills.push(json!({
            "id":action,
            "name":name,
            "description":"Submit one selected standing job; inspect the original admission for native outcome and evidence. A task state is not a provider verification receipt.",
            "tags":["cohesix","durable-job"],
            "action":action,
            "authority_path":"/host/tickets/spec",
            "status_path":"/host/tickets/status",
            "intent_schema_ref":provider["intent_schema_ref"],
            "receipt_schema_ref":provider["receipt_schema_ref"],
            "correlation_fields":provider["correlation_fields"],
        }));
    }
    let catalogue = json!({
        "schema":"cohesix-a2a-catalogue/v1",
        "enabled":enabled,
        "revision":REVISION,
        "sdk_client":"a2a-sdk==0.3.26",
        "binding":"JSONRPC",
        "http_path":"/a2a",
        "card_path":"/.well-known/agent-card.json",
        "bounds":{"request_bytes":8192,"response_bytes":16384,"concurrent_requests":16,
            "stream_events":64,"stream_interval_ms":500,"stream_seconds":30},
        "resolved_manifest_sha256":hex::encode(Sha256::digest(&resolved_bytes)),
        "provider_graph_sha256":registry["graph_sha256"],
        "skills":if enabled {skills} else {Vec::new()},
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
    fn selected_skills_follow_manifest_ceiling() {
        let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../configs/generated");
        let temporary = tempfile::tempdir().unwrap();
        let manifest_path = temporary.path().join("resolved.json");
        let output = temporary.path().join("a2a.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(generated.join("root_task_resolved.json")).unwrap())
                .unwrap();
        manifest["standing_authority"]["actions"] = json!(["peft.release"]);
        manifest["gateway"]["agent_protocols"]["enabled"] = true.into();
        manifest["gateway"]["a2a"]["enabled"] = true.into();
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        emit(
            &manifest_path,
            &generated.join("provider_registry.json"),
            &output,
        )
        .unwrap();
        let selected: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(selected["skills"].as_array().unwrap().len(), 1);
        assert_eq!(selected["skills"][0]["id"], "peft.release");
        manifest["gateway"]["agent_protocols"]["enabled"] = false.into();
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        emit(
            &manifest_path,
            &generated.join("provider_registry.json"),
            &output,
        )
        .unwrap();
        let disabled: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(disabled["enabled"], false);
        assert!(disabled["skills"].as_array().unwrap().is_empty());
    }
}
