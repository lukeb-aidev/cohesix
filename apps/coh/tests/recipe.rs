// Author: Lukas Bower
// Purpose: Protect durable CUDA intent, selective dependency reuse and evidence-bound resource release across failures.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#[path = "../../../crates/cohesix-evidence/tests/support/mod.rs"]
mod support;

use anyhow::{bail, Result};
use coh::{
    recipe::{self, Deployment, Runtime, Stage},
    workflow::Step,
    CohAccess,
};
use cohesix_evidence::{digest, Artifact, Kind, Outcome, Trust, WorkerIdentity};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

struct Access {
    writes: Vec<Value>,
    lose_ack: bool,
    deny_reads: bool,
    info: Value,
}
impl CohAccess for Access {
    fn list_dir(&mut self, _: &str, _: usize) -> Result<Vec<String>> {
        Ok(vec![])
    }
    fn read_file(&mut self, path: &str, _: usize) -> Result<Vec<u8>> {
        if self.deny_reads {
            bail!("EPERM scoped read");
        }
        if path.starts_with("/gpu/") {
            return Ok(serde_json::to_vec(&self.info)?);
        }
        Ok(b"schema=host-ticket-current/v1 state=confirmed".to_vec())
    }
    fn write_append(&mut self, _: &str, payload: &[u8]) -> Result<usize> {
        self.writes.push(serde_json::from_slice(payload)?);
        if self.lose_ack {
            bail!("connection_lost");
        }
        Ok(payload.len())
    }
}
fn access() -> Access {
    Access {
        writes: vec![],
        lose_ack: false,
        deny_reads: false,
        info: json!({"id":"GPU-0", "driver_version":"13020", "runtime_version":"13020",
        "execution_identity":{"source_mode":"production","device_uuid":"11".repeat(16),"helper_sha256":"22".repeat(32),
            "topology_sha256":"33".repeat(32),"provider_graph_sha256":"44".repeat(32)}}),
    }
}
fn stage(root: &Path, name: &str, dimension: u32, after: &[&str]) -> Stage {
    let output: Vec<u8> = (0..dimension)
        .flat_map(|i| (3.0 * (i % 1024) as f32).to_le_bytes())
        .collect();
    let input = json!({"schema":"cohesix-gpu-workload-input/v1", "artifact_sha256":"22".repeat(32), "topology_sha256":"33".repeat(32),
        "expected_output_sha256":digest(&output), "request":{"schema":"cohesix-cuda-reference-request/v1", "ticket_id":name,
        "entrypoint":"vadd", "dimension":dimension,"iterations":1,"device_ordinal":0,"device_uuid":"11".repeat(16),
        "inventory_observed_unix_ms":1000,"provider_graph_sha256":"44".repeat(32),"memory_budget_bytes":1048576,"deadline_ms":1000}});
    let parsed: gpu_bridge_host::workload::Input = serde_json::from_value(input).unwrap();
    let bytes = serde_json::to_vec(&parsed).unwrap();
    let input_path = root.join(format!("{name}-input.json"));
    fs::write(&input_path, &bytes).unwrap();
    let request = json!({"schema":"host-ticket/v2","id":name,"idempotency_key":format!("{name}-once"),"action":"gpu.workload.submit",
        "writer_epoch":3,"expires_unix_ms":10000,"receipt_mode":"worker","operation_id":name,"subject_ref":"GPU-0",
        "receipt_worker_role":"worker-gpu","receipt_worker_id":"worker-1","receipt_supervisor_generation":1,"receipt_cap_generation":1,
        "args":{"lease_id":"lease-1","request_sha256":digest(&bytes)}});
    let (_, mut trust) = support::fixture();
    trust.expected.ticket_id = name.into();
    trust.expected.action = "gpu.workload.submit".into();
    trust.expected.idempotency_key = format!("{name}-once");
    trust.expected.provider_graph_sha256 = "44".repeat(32);
    trust.expected.worker = Some(WorkerIdentity {
        id: "worker-1".into(),
        role: "worker-gpu".into(),
        generation: 1,
        image_sha256: "55".repeat(32),
    });
    trust.keys[0].kinds.insert(Kind::Worker);
    let trust_path = root.join(format!("{name}-trust.json"));
    fs::write(&trust_path, serde_json::to_vec(&trust).unwrap()).unwrap();
    let cas = root.join("cas");
    fs::create_dir_all(&cas).unwrap();
    Stage {
        id: name.into(),
        after: after.iter().map(|s| (*s).into()).collect(),
        input: input_path,
        runtime: Runtime {
            driver_version: 13020,
            runtime_version: 13020,
        },
        execution: Step {
            request,
            graph: root.join(format!("{name}-graph.json")),
            trust: trust_path,
            cas,
        },
    }
}
fn deployment(root: &Path, stages: Vec<Stage>) -> Deployment {
    Deployment {
        schema: "cohesix-cuda-recipe/v1".into(),
        operation_id: "operation-1".into(),
        contract_sha256: recipe::contract().unwrap()["contract_sha256"]
            .as_str()
            .unwrap()
            .into(),
        topology: BTreeMap::from([
            ("controller".into(), "mac".into()),
            ("target-hive".into(), "qemu".into()),
            ("provider-host".into(), "cuda".into()),
        ]),
        journal: root.join("journal"),
        stages,
        recovery: vec![],
    }
}
fn artifact(cas: &Path, v: &Value) -> Artifact {
    let bytes = serde_json::to_vec(v).unwrap();
    let hash = digest(&bytes);
    fs::write(cas.join(&hash), &bytes).unwrap();
    Artifact {
        sha256: hash,
        bytes: bytes.len() as u64,
        media_type: "application/json".into(),
    }
}
fn complete(stage: &Stage, state: &str) {
    let input: Value = serde_json::from_slice(&fs::read(&stage.input).unwrap()).unwrap();
    let native = json!({"entrypoint":"vadd","dimension":input["request"]["dimension"],"iterations":1,"device_ordinal":0,
        "device_uuid":"11".repeat(16),"driver_version":13020,"runtime_version":13020,"allocation_bytes":input["request"]["dimension"].as_u64().unwrap()*12});
    let output: Vec<u8> = (0..input["request"]["dimension"].as_u64().unwrap())
        .flat_map(|i| (3.0 * (i % 1024) as f32).to_le_bytes())
        .collect();
    fs::write(stage.execution.cas.join(digest(&output)), &output).unwrap();
    let job_id = if stage.execution.request["action"] == "gpu.workload.cancel" {
        stage.execution.request["args"]["job_id"].clone()
    } else {
        stage.execution.request["id"].clone()
    };
    let input_hash = digest(&fs::read(&stage.input).unwrap());
    let native_identity = format!("gpu-job:{}:{}", job_id.as_str().unwrap(), input_hash);
    let result = json!({"schema":"cohesix-native-provider-evidence/v1","ticket_id":stage.execution.request["id"],
        "observation":{"binding":{"ticket_id":job_id,"gpu_id":"GPU-0"},"input_sha256":input_hash,
        "terminal_unix_ms":1008,"state":state,"observation":{"outcome":"verified","helper_sha256":"22".repeat(32),"native":native,
            "output":{"sha256":digest(&output),"bytes":output.len()},"native_enforcement":{"deadline_mechanism":"owned_child_kill_and_reap","concurrency_limit":1}}}});
    let intent = artifact(&stage.execution.cas, &stage.execution.request);
    let observation = artifact(&stage.execution.cas, &result);
    fs::write(
        stage
            .execution
            .cas
            .join(digest(b"observed immutable bytes")),
        b"observed immutable bytes",
    )
    .unwrap();
    let (mut graph, _) = support::fixture();
    let trust: Trust = serde_json::from_slice(&fs::read(&stage.execution.trust).unwrap()).unwrap();
    graph.binding = trust.expected;
    let mut worker = graph.records.last().unwrap().clone();
    worker.record.kind = Kind::Worker;
    graph.records.insert(7, worker);
    let key = SigningKey::from_bytes(&[71; 32]);
    let mut parents = vec![];
    for (i, r) in graph.records.iter_mut().enumerate() {
        r.record.binding = graph.binding.clone();
        r.record.parents = parents.clone();
        r.record.sequence = i as u64 + 1;
        r.record.observed_unix_ms = 1000 + i as u64;
        r.record.native_identity = (i >= 4).then(|| native_identity.clone());
        if r.record.kind == Kind::Intent {
            r.record.artifacts = vec![intent.clone()];
        }
        if matches!(
            r.record.kind,
            Kind::Execution | Kind::Observation | Kind::Verification
        ) {
            r.record.artifacts = vec![observation.clone()];
        }
        if matches!(r.record.kind, Kind::Worker | Kind::Terminal) {
            r.record.outcome = if state == "succeeded"
                || stage.execution.request["action"] == "gpu.workload.cancel"
            {
                Outcome::Succeeded
            } else {
                Outcome::Cancelled
            };
        }
        let bytes = serde_json::to_vec(&r.record).unwrap();
        r.sha256 = digest(&bytes);
        r.signature = hex::encode(key.sign(&bytes).to_bytes());
        parents.push(r.sha256.clone());
    }
    fs::write(&stage.execution.graph, serde_json::to_vec(&graph).unwrap()).unwrap();
}
fn validate(d: &Deployment) -> Result<Deployment> {
    let p = d.journal.parent().unwrap().join("deployment.json");
    fs::write(&p, serde_json::to_vec(d)?)?;
    recipe::load(&p)
}

#[test]
fn lost_ack_and_restart_never_repeat_ambiguous_work_or_release_capacity() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let d = validate(&deployment(
        tmp.path(),
        vec![stage(tmp.path(), "a", 4, &[])],
    ))?;
    recipe::plan(&d)?;
    let mut client = access();
    client.lose_ack = true;
    assert!(recipe::advance(&mut client, &d, 2000, false, None).is_err());
    let report = recipe::inspect(&d, 2000)?;
    assert_eq!(report["attempts"][0]["state"], "ambiguous");
    assert_eq!(report["accounting"]["unresolved_reserved_bytes"], 1048576);
    assert_eq!(report["attempts"][0]["idempotency_key"], "a-once");
    client.lose_ack = false;
    for recovery in [false, true, false] {
        recipe::advance(&mut client, &d, 2000, recovery, None)?;
    }
    assert_eq!(client.writes.len(), 1);
    complete(&d.stages[0], "succeeded");
    let result = recipe::advance(&mut client, &d, 2000, true, None)?;
    assert_eq!(result["all_steps_verified"], true);
    assert_eq!(result["accounting"]["unresolved_reserved_bytes"], 0);
    assert_eq!(result["accounting"]["confirmed_released_bytes"], 1048576);
    assert_eq!(client.writes.len(), 1);
    Ok(())
}

#[test]
fn changed_inputs_invalidate_descendants_and_reuse_independent_verified_stage() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let mut d = validate(&deployment(
        tmp.path(),
        vec![
            stage(tmp.path(), "a", 4, &[]),
            stage(tmp.path(), "b", 4, &[]),
            stage(tmp.path(), "c", 4, &["a"]),
        ],
    ))?;
    recipe::plan(&d)?;
    let mut client = access();
    for s in &d.stages {
        recipe::advance(&mut client, &d, 2000, false, None)?;
        complete(s, "succeeded");
    }
    assert_eq!(recipe::inspect(&d, 2000)?["all_steps_verified"], true);
    let mut replacement = stage(tmp.path(), "a2", 8, &[]);
    replacement.id = "a".into();
    d.stages[0] = replacement;
    let mut descendant = stage(tmp.path(), "c2", 4, &["a"]);
    descendant.id = "c".into();
    d.stages[2] = descendant;
    d = validate(&d)?;
    recipe::plan(&d)?;
    recipe::advance(&mut client, &d, 2000, false, None)?;
    complete(&d.stages[0], "succeeded");
    let state = recipe::advance(&mut client, &d, 2000, false, None)?;
    assert_eq!(state["stages"][1]["reused"], true);
    assert_eq!(client.writes.last().unwrap()["id"], "c2");
    complete(&d.stages[2], "succeeded");
    let state = recipe::inspect(&d, 2000)?;
    assert_eq!(state["all_steps_verified"], true);
    assert_eq!(client.writes.len(), 5);
    assert_eq!(
        state["accounting"]["cumulative_requested_bytes"],
        5 * 1048576
    );
    assert_eq!(state["accounting"]["confirmed_released_bytes"], 5 * 1048576);
    Ok(())
}

#[test]
fn authority_resource_and_visibility_refusals_precede_submission() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let d = validate(&deployment(
        tmp.path(),
        vec![stage(tmp.path(), "a", 4, &[])],
    ))?;
    recipe::plan(&d)?;
    let mut client = access();
    assert!(recipe::advance(&mut client, &d, 10000, false, None).is_err());
    assert!(client.writes.is_empty());
    client.info["runtime_version"] = "13010".into();
    assert!(recipe::advance(&mut client, &d, 2000, false, None).is_err());
    client = access();
    recipe::advance(&mut client, &d, 2000, false, None)?;
    complete(&d.stages[0], "succeeded");
    client.deny_reads = true;
    assert!(recipe::advance(&mut client, &d, 2000, false, None).is_err());
    assert_eq!(client.writes.len(), 1);
    let mut bad = stage(tmp.path(), "resource", 4, &[]);
    let mut v: Value = serde_json::from_slice(&fs::read(&bad.input)?)?;
    v["request"]["memory_budget_bytes"] = 1.into();
    let bytes = serde_json::to_vec(&v)?;
    fs::write(&bad.input, &bytes)?;
    bad.execution.request["args"]["request_sha256"] = digest(&bytes).into();
    assert!(validate(&deployment(tmp.path(), vec![bad])).is_err());
    Ok(())
}

#[test]
fn corrupted_output_or_graph_cannot_advance_or_release_an_unverified_reservation() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let d = validate(&deployment(
        tmp.path(),
        vec![stage(tmp.path(), "a", 4, &[])],
    ))?;
    recipe::plan(&d)?;
    recipe::advance(&mut access(), &d, 2000, false, None)?;
    complete(&d.stages[0], "succeeded");
    let v: Value = serde_json::from_slice(&fs::read(&d.stages[0].input)?)?;
    fs::write(
        d.stages[0]
            .execution
            .cas
            .join(v["expected_output_sha256"].as_str().unwrap()),
        b"corrupt",
    )?;
    assert!(recipe::inspect(&d, 2000).is_err());
    complete(&d.stages[0], "succeeded");
    let mut graph: Value = serde_json::from_slice(&fs::read(&d.stages[0].execution.graph)?)?;
    graph["records"][5]["signature"] = "00".repeat(64).into();
    fs::write(&d.stages[0].execution.graph, serde_json::to_vec(&graph)?)?;
    assert!(recipe::inspect(&d, 2000).is_err());
    Ok(())
}

#[test]
fn cancelling_keeps_reservation_until_independent_native_termination_evidence() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let mut d = deployment(tmp.path(), vec![stage(tmp.path(), "a", 4, &[])]);
    let mut cancel = stage(tmp.path(), "cancel", 4, &[]);
    cancel.execution.request["action"] = "gpu.workload.cancel".into();
    cancel.execution.request["args"] = json!({"job_id":"a"});
    let mut trust: Trust = serde_json::from_slice(&fs::read(&cancel.execution.trust)?)?;
    trust.expected.action = "gpu.workload.cancel".into();
    fs::write(&cancel.execution.trust, serde_json::to_vec(&trust)?)?;
    d = validate(&d)?;
    recipe::plan(&d)?;
    let mut client = access();
    recipe::advance(&mut client, &d, 2000, false, None)?;
    d.recovery.push(recipe::Recovery {
        stage: "a".into(),
        execution: cancel.execution.clone(),
    });
    d = validate(&d)?;
    let state = recipe::advance(&mut client, &d, 2000, true, Some("a"))?;
    assert_eq!(state["attempts"][1]["state"], "cancellation_requested");
    assert_eq!(state["accounting"]["unresolved_reserved_bytes"], 1048576);
    cancel.input = d.stages[0].input.clone();
    complete(&cancel, "cancelled");
    let result = recipe::advance(&mut client, &d, 2000, true, None)?;
    assert_eq!(result["accounting"]["unresolved_reserved_bytes"], 0);
    assert_eq!(result["all_steps_verified"], false);
    assert_eq!(client.writes.len(), 2);
    Ok(())
}

#[test]
fn invalid_dag_input_binding_and_conflicting_journal_are_refused() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let mut d = deployment(tmp.path(), vec![stage(tmp.path(), "a", 4, &[])]);
    d.stages[0].after = vec!["a".into()];
    assert!(validate(&d).is_err());
    d.stages[0].after.clear();
    recipe::plan(&validate(&d)?)?;
    d.operation_id = "replacement".into();
    assert!(recipe::plan(&validate(&d)?).is_err());
    d.operation_id = "operation-1".into();
    fs::write(&d.stages[0].input, b"{}")?;
    assert!(validate(&d).is_err());
    Ok(())
}

#[test]
fn diagnostic_uses_canonical_case_and_withholds_unstructured_details() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let report = json!({"schema":"cohesix-recipe-operation-report/v1","authoritative":false,"operation_id":"operation-1",
        "secret":"DO_NOT_EXPORT","attempts":[{"stage":"vadd","ticket_id":"t1","idempotency_key":"k1","state":"ambiguous","native_message":"DO_NOT_EXPORT"}]});
    let source = tmp.path().join("report.json");
    fs::write(&source, serde_json::to_vec(&report)?)?;
    let pack = tmp.path().join("pack");
    fs::create_dir(&pack)?;
    fs::write(
        pack.join("summary.json"),
        r#"{"schema":"cohesix-evidence-pack/summary-v1","items":[]}"#,
    )?;
    coh::recipe::attach_diagnostic(&pack, &source)?;
    coh::evidence_timeline::write_timeline_with_scenario(
        &pack,
        coh::evidence_timeline::Scenario::Incident,
    )?;
    let case: Value = serde_json::from_slice(&fs::read(pack.join("case.json"))?)?;
    assert_eq!(case["schema"], "cohesix-evidence-pack/case-v1");
    assert_eq!(case["proof"], "none");
    assert_eq!(case["recipe"]["attempts"][0]["state"], "ambiguous");
    assert!(fs::read_to_string(pack.join("case.md"))?.contains("never blindly redispatch"));
    assert!(!fs::read_to_string(pack.join("attachments/recipe.json"))?.contains("DO_NOT_EXPORT"));
    assert!(coh::evidence::verify_pack_integrity(&pack)?);
    Ok(())
}

#[test]
fn reuse_freshness_and_journal_integrity_do_not_depend_on_cached_labels() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let mut d = validate(&deployment(
        tmp.path(),
        vec![stage(tmp.path(), "a", 4, &[])],
    ))?;
    recipe::plan(&d)?;
    recipe::advance(&mut access(), &d, 2000, false, None)?;
    complete(&d.stages[0], "succeeded");
    let mut changed = stage(tmp.path(), "a2", 4, &[]);
    changed.id = "a".into();
    d.stages[0] = changed;
    d = validate(&d)?;
    recipe::plan(&d)?;
    let fresh = recipe::inspect(&d, 2000)?;
    assert_eq!(fresh["stages"][0]["reused"], true);
    let stale = recipe::inspect(&d, 3601009)?;
    assert_eq!(stale["all_steps_verified"], false);
    let path = d.journal.join("recipe.json");
    let original = fs::read(&path)?;
    let mut journal: Value = serde_json::from_slice(&original)?;
    journal["attempts"][0]["reservation_bytes"] = 0.into();
    fs::write(&path, serde_json::to_vec(&journal)?)?;
    assert!(recipe::inspect(&d, 2000).is_err());
    journal = serde_json::from_slice(&original)?;
    journal["attempts"][0]["key"] = "00".repeat(32).into();
    fs::write(&path, serde_json::to_vec(&journal)?)?;
    assert!(recipe::inspect(&d, 2000).is_err());
    Ok(())
}
