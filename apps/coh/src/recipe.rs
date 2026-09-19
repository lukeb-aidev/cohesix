// Author: Lukas Bower
// Purpose: Persist bounded recipe intent before admission and reconcile signed native termination before reuse or releasing reservations.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use crate::{
    operator::{read_bounded, write_atomic},
    workflow::{self, Step},
    CohAccess,
};
use anyhow::{anyhow, ensure, Result};
use cohesix_evidence::{digest, Artifact, Kind, Outcome, Trust, VerifiedGraph};
use fs2::FileExt;
use gpu_bridge_host::workload::Input;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const CONTRACT: &str = include_str!("../../../configs/generated/cuda_recipe.json");

/// The recipe is a host composition of immutable workload inputs and existing tickets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    /// Versioned host deployment grammar.
    pub schema: String,
    /// Logical identity retained across revisions and attempts.
    pub operation_id: String,
    /// Digest returned by the installed compiler-owned recipe plan.
    pub contract_sha256: String,
    /// Controller, target hive and provider identities; immutable across recovery.
    pub topology: BTreeMap<String, String>,
    /// Owner-only durable state directory.
    pub journal: PathBuf,
    /// Dependency-ordered bounded work, never arbitrary commands.
    pub stages: Vec<Stage>,
    /// Explicit separately admitted cancellation requests.
    pub recovery: Vec<Recovery>,
}

/// An immutable allowlisted workload and its dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    /// Stable stage identity within this operation.
    pub id: String,
    /// Earlier stages whose verified output digests enter the reuse key.
    pub after: Vec<String>,
    /// Exact native request bytes addressed by the execution ticket.
    pub input: PathBuf,
    /// Required CUDA driver and runtime versions.
    pub runtime: Runtime,
    /// Existing ticket and independently enrolled evidence locations.
    pub execution: Step,
}

/// Exact compatibility is intentionally conservative; a runtime upgrade invalidates reuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    /// Native CUDA driver version integer.
    pub driver_version: u64,
    /// Native CUDA runtime version integer.
    pub runtime_version: u64,
}

/// Cancellation is an explicit new ticket, never implied by a local failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    /// Original stage whose native job may be cancelled.
    pub stage: String,
    /// Existing ticket and independently enrolled evidence locations.
    pub execution: Step,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: String,
    operation_id: String,
    contract_sha256: String,
    topology: BTreeMap<String, String>,
    configuration_sha256: String,
    revision: u32,
    attempts: Vec<Attempt>,
    blocker: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Attempt {
    stage: String,
    key: String,
    predecessors: Vec<(String, String)>,
    configuration_sha256: String,
    number: u32,
    cancel: bool,
    execution: Step,
    input: Value,
    runtime: Runtime,
    reservation_bytes: u64,
    // This is only a local cursor. Every result is independently reverified on read.
    acknowledged: bool,
}

#[derive(Clone, Serialize)]
struct Terminal {
    graph_sha256: String,
    native_identity: String,
    state: String,
    terminal_unix_ms: u64,
    requested_bytes: u64,
    allocation_bytes: Option<u64>,
    enforcement: Value,
    output: Option<Artifact>,
}

fn limit(name: &str) -> Result<u64> {
    let c: Value = serde_json::from_str(CONTRACT)?;
    c[name]
        .as_u64()
        .ok_or_else(|| anyhow!("EPERM recipe-contract"))
}
fn id(value: &str) -> Result<()> {
    cohesix_authority::validate_id(value).map_err(|_| anyhow!("EPERM recipe-identity"))
}
fn string<'a>(v: &'a Value, name: &str) -> Result<&'a str> {
    v[name]
        .as_str()
        .ok_or_else(|| anyhow!("EPERM recipe-{name}"))
}
fn hash(v: &impl Serialize) -> Result<String> {
    Ok(digest(&serde_json::to_vec(v)?))
}

fn configuration_hash(d: &Deployment) -> Result<String> {
    let mut value = serde_json::to_value(d)?;
    // Recovery is separately admitted current authority, not reusable workload configuration.
    value
        .as_object_mut()
        .ok_or_else(|| anyhow!("EPERM recipe-configuration"))?
        .remove("recovery");
    hash(&value)
}

/// Plan without a deployment only describes compiler-owned stages and finite bounds.
pub fn contract() -> Result<Value> {
    Ok(
        json!({"schema":"cohesix-recipe-plan/v1", "authoritative":false,
        "contract":serde_json::from_str::<Value>(CONTRACT)?,
        "contract_sha256":digest(CONTRACT.as_bytes()), "production_use_case_accepted":false}),
    )
}

fn validate_step(step: &Step, action: &str) -> Result<()> {
    let request = cohesix_evidence::ticket::caller_request(&step.request, false)?;
    ensure!(
        request["schema"] == "host-ticket/v2"
            && request["action"] == action
            && request["receipt_mode"] == "worker"
            && request["receipt_worker_role"] == "worker-gpu",
        "EPERM recipe-action"
    );
    for name in [
        "id",
        "idempotency_key",
        "operation_id",
        "subject_ref",
        "receipt_worker_id",
    ] {
        id(string(&request, name)?)?;
    }
    ensure!(
        request["expires_unix_ms"].as_u64().is_some_and(|x| x > 0),
        "EPERM recipe-expiry"
    );
    ensure!(
        [&step.graph, &step.trust, &step.cas]
            .iter()
            .all(|p| p.is_absolute()),
        "EPERM recipe-evidence-path"
    );
    let trust: Trust = serde_json::from_slice(&read_bounded(&step.trust, 65536)?)?;
    cohesix_evidence::ticket::require_binding(&trust.expected, &request, false)?;
    cohesix_authority::provider::validate_request_size(
        action,
        serde_json::to_vec(&request)?.len(),
    )?;
    Ok(())
}

fn input(stage: &Stage) -> Result<Value> {
    ensure!(stage.input.is_absolute(), "EPERM recipe-input-path");
    let bytes = read_bounded(&stage.input, 8192)?;
    ensure!(
        stage.execution.request["args"]["request_sha256"] == digest(&bytes),
        "EPERM recipe-input-digest"
    );
    let parsed: Input = serde_json::from_slice(&bytes)?;
    ensure!(
        serde_json::to_vec(&parsed)? == bytes,
        "EPERM recipe-input-canonical"
    );
    parsed.request.validate(
        parsed.request.inventory_observed_unix_ms,
        &parsed.request.provider_graph_sha256,
    )?;
    ensure!(
        parsed.schema == "cohesix-gpu-workload-input/v1"
            && parsed.request.ticket_id == stage.execution.request["id"],
        "EPERM recipe-input-identity"
    );
    for h in [
        &parsed.artifact_sha256,
        &parsed.topology_sha256,
        &parsed.expected_output_sha256,
    ] {
        ensure!(
            h.len() == 64
                && h.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "EPERM recipe-input-hash"
        );
    }
    Ok(serde_json::from_slice(&bytes)?)
}

/// Load and validate the dependency order and exact input bytes without opening a transport.
pub fn load(path: &Path) -> Result<Deployment> {
    let d: Deployment = serde_json::from_slice(&read_bounded(path, 65536)?)?;
    ensure!(
        d.schema == "cohesix-cuda-recipe/v1" && d.contract_sha256 == digest(CONTRACT.as_bytes()),
        "EPERM recipe-schema-or-contract"
    );
    id(&d.operation_id)?;
    ensure!(
        d.journal.is_absolute() && d.topology.len() == 3,
        "EPERM recipe-topology-or-journal"
    );
    for name in ["controller", "target-hive", "provider-host"] {
        id(d.topology
            .get(name)
            .ok_or_else(|| anyhow!("EPERM recipe-topology"))?)?;
    }
    ensure!(
        !d.stages.is_empty()
            && d.stages.len() <= limit("max_stages")? as usize
            && d.recovery.len() <= d.stages.len(),
        "ELIMIT recipe-stages"
    );
    let mut seen = BTreeSet::new();
    let mut tickets = BTreeSet::new();
    for stage in &d.stages {
        id(&stage.id)?;
        ensure!(
            !seen.contains(&stage.id)
                && stage.after.len() <= seen.len()
                && stage.after.iter().collect::<BTreeSet<_>>().len() == stage.after.len()
                && stage.after.iter().all(|p| seen.contains(p)),
            "EPERM recipe-dependencies"
        );
        seen.insert(stage.id.clone());
        ensure!(
            stage.runtime.driver_version > 0 && stage.runtime.runtime_version > 0,
            "EPERM recipe-runtime"
        );
        validate_step(&stage.execution, "gpu.workload.submit")?;
        ensure!(
            tickets.insert(hash(&json!([
                stage.execution.request["id"],
                stage.execution.request["idempotency_key"]
            ]))?),
            "EPERM recipe-duplicate-ticket"
        );
        input(stage)?;
    }
    let mut recovery_stages = BTreeSet::new();
    for recovery in &d.recovery {
        ensure!(
            seen.contains(&recovery.stage) && recovery_stages.insert(&recovery.stage),
            "EPERM recipe-recovery-stage"
        );
        validate_step(&recovery.execution, "gpu.workload.cancel")?;
        ensure!(
            tickets.insert(hash(&json!([
                recovery.execution.request["id"],
                recovery.execution.request["idempotency_key"]
            ]))?),
            "EPERM recipe-duplicate-ticket"
        );
    }
    Ok(d)
}

fn lock(d: &Deployment, create: bool) -> Result<File> {
    lock_directory(&d.journal, create)
}

/// CUDA and PEFT phases share the same private exclusive journal ownership.
pub(crate) fn lock_directory(directory: &Path, create: bool) -> Result<File> {
    if create && !directory.try_exists()? {
        fs::DirBuilder::new().mode(0o700).create(directory)?;
    }
    let meta = fs::symlink_metadata(directory)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink() && meta.permissions().mode() & 0o077 == 0,
        "EPERM recipe-journal-private-directory"
    );
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join("lock"))?;
    file.try_lock_exclusive()
        .map_err(|_| anyhow!("EBUSY recipe-owner"))?;
    Ok(file)
}
fn save(d: &Deployment, journal: &Journal) -> Result<()> {
    save_record(&d.journal, "recipe.json", journal)
}

/// A native phase journal has the same atomic durability and finite bound as CUDA.
pub(crate) fn save_record(directory: &Path, name: &str, record: &impl Serialize) -> Result<()> {
    crate::validate_component(name)?;
    let bytes = serde_json::to_vec(record)?;
    ensure!(
        bytes.len() <= limit("max_journal_bytes")? as usize,
        "ELIMIT recipe-journal"
    );
    write_atomic(&directory.join(name), &bytes)?;
    File::open(directory)?.sync_all()?;
    Ok(())
}
fn journal(d: &Deployment) -> Result<Journal> {
    let j: Journal = serde_json::from_slice(&read_bounded(
        &d.journal.join("recipe.json"),
        limit("max_journal_bytes")? as usize,
    )?)?;
    ensure!(
        j.schema == "cohesix-recipe-journal/v1"
            && j.operation_id == d.operation_id
            && j.contract_sha256 == d.contract_sha256
            && j.topology == d.topology
            && j.attempts.len() <= limit("max_attempts")? as usize,
        "EPERM recipe-journal-binding"
    );
    let mut ids = BTreeSet::new();
    for (i, a) in j.attempts.iter().enumerate() {
        ensure!(
            a.number as usize == i + 1
                && ids.insert(hash(&json!([
                    a.execution.request["id"],
                    a.execution.request["idempotency_key"]
                ]))?),
            "EPERM recipe-attempt-identity"
        );
        ensure!(
            a.predecessors.len() <= limit("max_stages")? as usize
                && a.key == input_key(&a.input, &a.runtime, &a.predecessors, &j.topology)?,
            "EPERM recipe-journal-cache-key"
        );
        let parsed: Input = serde_json::from_value(a.input.clone())?;
        ensure!(
            a.reservation_bytes
                == if a.cancel {
                    0
                } else {
                    parsed.request.memory_budget_bytes
                },
            "EPERM recipe-journal-accounting"
        );
        if !a.cancel {
            ensure!(
                digest(&serde_json::to_vec(&parsed)?)
                    == a.execution.request["args"]["request_sha256"],
                "EPERM recipe-journal-input"
            );
        }
        validate_step(
            &a.execution,
            if a.cancel {
                "gpu.workload.cancel"
            } else {
                "gpu.workload.submit"
            },
        )?;
    }
    Ok(j)
}

/// Identity and configuration are durable before any ticket can be submitted.
pub fn plan(d: &Deployment) -> Result<Value> {
    let _lock = lock(d, true)?;
    let configuration = configuration_hash(d)?;
    let mut j = if d.journal.join("recipe.json").try_exists()? {
        journal(d)?
    } else {
        Journal {
            schema: "cohesix-recipe-journal/v1".into(),
            operation_id: d.operation_id.clone(),
            contract_sha256: d.contract_sha256.clone(),
            topology: d.topology.clone(),
            configuration_sha256: configuration.clone(),
            revision: 1,
            attempts: vec![],
            blocker: None,
        }
    };
    if j.configuration_sha256 != configuration {
        for t in reconciled(&j)? {
            ensure!(t.is_some(), "ambiguous recipe-replan-reconcile-first");
        }
        ensure!(
            j.revision < limit("max_attempts")? as u32,
            "ELIMIT recipe-revisions"
        );
        j.revision += 1;
        j.configuration_sha256 = configuration;
        j.blocker = None;
    }
    save(d, &j)?;
    report(d, &j, 0)
}

fn terminal(a: &Attempt) -> Result<Option<Terminal>> {
    let Some(graph) = workflow::verified(&a.execution)? else {
        return Ok(None);
    };
    native_terminal(a, &graph).map(Some)
}
fn native_terminal(a: &Attempt, graph: &VerifiedGraph) -> Result<Terminal> {
    let node = graph
        .nodes()
        .iter()
        .find(|n| n.kind == Kind::Observation)
        .ok_or_else(|| anyhow!("EPERM recipe-native-observation"))?;
    ensure!(node.artifacts.len() == 1, "EPERM recipe-native-artifact");
    let bytes = read_bounded(&a.execution.cas.join(&node.artifacts[0].sha256), 65536)?;
    let artifact: Value = serde_json::from_slice(&bytes)?;
    let job = &artifact["observation"];
    let saved_input: Input = serde_json::from_value(a.input.clone())?;
    ensure!(
        job["input_sha256"] == digest(&serde_json::to_vec(&saved_input)?),
        "EPERM recipe-native-input"
    );
    let native_identity = node
        .native_identity
        .clone()
        .ok_or_else(|| anyhow!("EPERM recipe-native-identity"))?;
    let expected_job = if a.cancel {
        &a.execution.request["args"]["job_id"]
    } else {
        &a.execution.request["id"]
    };
    ensure!(
        artifact["schema"] == "cohesix-native-provider-evidence/v1"
            && artifact["ticket_id"] == a.execution.request["id"]
            && job["binding"]["ticket_id"] == *expected_job
            && native_identity
                == format!(
                    "gpu-job:{}:{}",
                    string(&job["binding"], "ticket_id")?,
                    string(job, "input_sha256")?
                )
            && job["binding"]["gpu_id"] == a.execution.request["subject_ref"]
            && (a.cancel || job["input_sha256"] == a.execution.request["args"]["request_sha256"])
            && job["terminal_unix_ms"].as_u64().is_some_and(|n| n > 0)
            && matches!(
                job["state"].as_str(),
                Some("succeeded" | "failed" | "cancelled" | "revoked" | "interrupted")
            ),
        "EPERM recipe-native-terminal"
    );
    let observed = &job["observation"];
    let output = if job["state"] == "succeeded" {
        ensure!(
            graph.outcome() == Outcome::Succeeded
                && observed["outcome"] == "verified"
                && observed["helper_sha256"] == a.input["artifact_sha256"]
                && observed["native"]["device_uuid"] == a.input["request"]["device_uuid"]
                && observed["native"]["driver_version"] == a.runtime.driver_version
                && observed["native"]["runtime_version"] == a.runtime.runtime_version
                && observed["output"]["sha256"] == a.input["expected_output_sha256"],
            "EPERM recipe-output-compatibility"
        );
        for field in ["entrypoint", "dimension", "iterations", "device_ordinal"] {
            ensure!(
                observed["native"][field] == a.input["request"][field],
                "EPERM recipe-output-parameters"
            );
        }
        let bytes = observed["output"]["bytes"]
            .as_u64()
            .ok_or_else(|| anyhow!("EPERM recipe-output-size"))?;
        ensure!(
            bytes > 0 && bytes <= limit("max_artifact_bytes")?,
            "ELIMIT recipe-output"
        );
        let output = Artifact {
            sha256: string(&observed["output"], "sha256")?.into(),
            bytes,
            media_type: "application/octet-stream".into(),
        };
        cohesix_evidence::verify_cas(&a.execution.cas, &output)?;
        Some(output)
    } else {
        None
    };
    Ok(Terminal {
        graph_sha256: graph.digest().into(),
        native_identity,
        state: string(job, "state")?.into(),
        terminal_unix_ms: job["terminal_unix_ms"]
            .as_u64()
            .ok_or_else(|| anyhow!("EPERM recipe-terminal-time"))?,
        requested_bytes: a.reservation_bytes,
        allocation_bytes: observed["native"]["allocation_bytes"].as_u64(),
        enforcement: observed["native_enforcement"].clone(),
        output,
    })
}

fn key(
    stage: &Stage,
    value: &Value,
    predecessors: &BTreeMap<String, (String, String)>,
    topology: &BTreeMap<String, String>,
) -> Result<String> {
    let deps = stage
        .after
        .iter()
        .map(|id| {
            predecessors
                .get(id)
                .cloned()
                .ok_or_else(|| anyhow!("pending recipe-predecessor"))
        })
        .collect::<Result<Vec<_>>>()?;
    input_key(value, &stage.runtime, &deps, topology)
}

fn input_key(
    value: &Value,
    runtime: &Runtime,
    predecessors: &[(String, String)],
    topology: &BTreeMap<String, String>,
) -> Result<String> {
    let mut normalized = value.clone();
    let request = normalized["request"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("EPERM recipe-input"))?;
    request.remove("ticket_id");
    request.remove("inventory_observed_unix_ms");
    hash(
        &json!({"contract":digest(CONTRACT.as_bytes()),"input":normalized,
        "runtime":runtime,"topology":topology,"predecessors":predecessors}),
    )
}

fn visible(access: &mut dyn CohAccess, step: &Step) -> Result<()> {
    let path = cohesix_evidence::ticket::current_path(
        string(&step.request, "id")?,
        string(&step.request, "idempotency_key")?,
    )?;
    let bytes = access.read_file(&path, 1024)?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= 1024,
        "EPERM recipe-read-visibility"
    );
    Ok(())
}
fn current(access: &mut dyn CohAccess, a: &Attempt, now: u64, dispatch: bool) -> Result<()> {
    let gpu = string(&a.execution.request, "subject_ref")?;
    let bytes = access.read_file(&format!("/gpu/{gpu}/info"), 8192)?;
    ensure!(bytes.len() <= 8192, "ELIMIT recipe-inventory");
    let info: Value = serde_json::from_slice(&bytes)?;
    let identity = &info["execution_identity"];
    ensure!(
        info["id"] == gpu
            && identity["source_mode"] == "production"
            && identity["device_uuid"] == a.input["request"]["device_uuid"]
            && identity["topology_sha256"] == a.input["topology_sha256"]
            && identity["helper_sha256"] == a.input["artifact_sha256"]
            && identity["provider_graph_sha256"] == a.input["request"]["provider_graph_sha256"],
        "EPERM recipe-current-compatibility"
    );
    for (name, expected) in [
        ("driver_version", a.runtime.driver_version),
        ("runtime_version", a.runtime.runtime_version),
    ] {
        let observed = info[name]
            .as_u64()
            .or_else(|| info[name].as_str().and_then(|s| s.parse().ok()));
        ensure!(observed == Some(expected), "EPERM recipe-current-runtime");
    }
    if dispatch {
        ensure!(
            a.execution.request["expires_unix_ms"]
                .as_u64()
                .is_some_and(|t| now < t),
            "EPERM recipe-expired-authority"
        );
        if !a.cancel {
            let parsed: Input = serde_json::from_value(a.input.clone())?;
            parsed
                .request
                .validate(now, &parsed.request.provider_graph_sha256)?;
        }
    }
    Ok(())
}

fn reconciled(j: &Journal) -> Result<Vec<Option<Terminal>>> {
    let mut results: Vec<_> = j.attempts.iter().map(terminal).collect::<Result<_>>()?;
    for (index, original) in j.attempts.iter().enumerate() {
        if original.cancel || results[index].is_some() {
            continue;
        }
        for (control_index, control) in j.attempts.iter().enumerate() {
            if control.cancel
                && control.execution.request["args"]["job_id"] == original.execution.request["id"]
                && control.input == original.input
                && control.runtime == original.runtime
            {
                if let Some(t) = &results[control_index] {
                    let mut t = t.clone();
                    t.requested_bytes = original.reservation_bytes;
                    results[index] = Some(t);
                    break;
                }
            }
        }
    }
    Ok(results)
}

fn report(d: &Deployment, j: &Journal, now: u64) -> Result<Value> {
    let mut reserved = 0u64;
    let mut released = 0u64;
    let mut requested = 0u64;
    let mut observed = 0u64;
    let mut retained = 0u64;
    let mut rows = Vec::new();
    let mut accepted = BTreeMap::new();
    let mut stages = Vec::new();
    let terminals = reconciled(j)?;
    let reuse_age = limit("max_reuse_age_ms")?;
    for (a, terminal) in j.attempts.iter().zip(&terminals) {
        if !a.cancel {
            requested += a.reservation_bytes;
            if terminal.is_some() {
                released += a.reservation_bytes;
            } else {
                reserved += a.reservation_bytes;
            }
        }
        if let Some(t) = terminal.as_ref().filter(|_| !a.cancel) {
            observed += t.allocation_bytes.unwrap_or(0);
            retained += t.output.as_ref().map_or(0, |o| o.bytes);
        }
        rows.push(json!({"stage":a.stage,"attempt":a.number,"ticket_id":a.execution.request["id"],
            "idempotency_key":a.execution.request["idempotency_key"],"cancel":a.cancel,
            "state":if terminal.is_some() {"verified_terminal"} else if a.cancel {"cancellation_requested"} else {"ambiguous"},
            "acknowledged":a.acknowledged,"evidence":terminal,"graph":a.execution.graph,
            "requested_limits":{"memory_budget_bytes":a.reservation_bytes,"deadline_ms":a.input["request"]["deadline_ms"],"streams":1},
            "cache_key":a.key,"input_sha256":a.execution.request["args"]["request_sha256"]}));
    }
    ensure!(
        retained <= limit("max_retained_bytes")?,
        "ELIMIT recipe-retention"
    );
    for s in &d.stages {
        let value = input(s)?;
        let cache_key = if s.after.iter().all(|p| accepted.contains_key(p)) {
            Some(key(s, &value, &accepted, &d.topology)?)
        } else {
            None
        };
        let mut reused = false;
        let found = j.attempts.iter().zip(&terminals).rev().find(|(a, t)| {
            !a.cancel
                && a.stage == s.id
                && Some(&a.key) == cache_key.as_ref()
                && t.as_ref().is_some_and(|t| {
                    t.output.is_some()
                        && (a.configuration_sha256 == j.configuration_sha256
                            || (now >= t.terminal_unix_ms && now - t.terminal_unix_ms <= reuse_age))
                })
        });
        if let Some((a, Some(t))) = found {
            if let (Some(k), Some(o)) = (&cache_key, &t.output) {
                accepted.insert(s.id.clone(), (k.clone(), o.sha256.clone()));
            }
            reused = a.configuration_sha256 != j.configuration_sha256;
        }
        stages.push(json!({"id":s.id,"after":s.after,"key":cache_key,"verified":found.is_some(),"reused":reused,
            "output":found.and_then(|(_,t)|t.as_ref()).and_then(|t|t.output.as_ref())}));
    }
    Ok(
        json!({"schema":"cohesix-recipe-operation-report/v1","authoritative":false,
        "operation_id":j.operation_id,"configuration_sha256":j.configuration_sha256,"revision":j.revision,
        "topology":d.topology,"stages":stages,"attempts":rows,"blocker":j.blocker,
        "all_steps_verified":accepted.len()==d.stages.len(),
        "accounting":{"cumulative_requested_bytes":requested,"cumulative_observed_allocation_bytes":observed,
            "unresolved_reserved_bytes":reserved,"confirmed_released_bytes":released,"retained_output_bytes":retained,
            "unobserved_attempts":j.attempts.iter().zip(&terminals).filter(|(a,t)| !a.cancel && t.as_ref().is_none_or(|t|t.allocation_bytes.is_none())).count(),
            "native_allocation_hard_partition":false,"parallel_limit":1,"automatic_retries":0},
        "recovery":"reconcile existing host-ticket and native journals; provide a fresh exact cancellation ticket for unresolved work; never blindly resubmit",
        "admission_mode":"operator_approved","machine_checked_admission":"unavailable","production_use_case_accepted":false}),
    )
}

/// Read-only offline status rechecks original signed evidence and every output hash.
pub fn inspect(d: &Deployment, now: u64) -> Result<Value> {
    let _lock = lock(d, false)?;
    let j = journal(d)?;
    ensure!(
        j.configuration_sha256 == configuration_hash(d)?,
        "EPERM recipe-plan-required"
    );
    report(d, &j, now)
}

fn dispatch(
    access: &mut dyn CohAccess,
    d: &Deployment,
    j: &mut Journal,
    a: Attempt,
    now: u64,
) -> Result<()> {
    ensure!(
        j.attempts.len() < limit("max_attempts")? as usize,
        "ELIMIT recipe-attempts"
    );
    ensure!(
        !j.attempts.iter().any(
            |old| old.execution.request["id"] == a.execution.request["id"]
                || old.execution.request["idempotency_key"]
                    == a.execution.request["idempotency_key"]
        ),
        "EPERM recipe-new-attempt-needs-new-ticket"
    );
    current(access, &a, now, true)?;
    let mut bytes = serde_json::to_vec(&a.execution.request)?;
    bytes.push(b'\n');
    j.attempts.push(a);
    save(d, j)?;
    // A short write, lost ACK or process death leaves the durable intent ambiguous.
    ensure!(
        access.write_append("/host/tickets/spec", &bytes)? == bytes.len(),
        "ambiguous recipe-submission-ack"
    );
    if let Some(a) = j.attempts.last_mut() {
        a.acknowledged = true;
    }
    save(d, j)
}

/// Advance one action at most; recovery without a selected cancellation is reconciliation only.
pub fn advance(
    access: &mut dyn CohAccess,
    d: &Deployment,
    now: u64,
    recover: bool,
    cancel_stage: Option<&str>,
) -> Result<Value> {
    let _lock = lock(d, false)?;
    let mut j = journal(d)?;
    ensure!(
        j.configuration_sha256 == configuration_hash(d)?,
        "EPERM recipe-plan-required"
    );
    let result = advance_inner(access, d, &mut j, now, recover, cancel_stage);
    j.blocker = result.as_ref().err().map(|_| "refused_or_unverified; reconcile signed evidence and current exact authority before recovery".into());
    save(d, &j)?;
    result?;
    report(d, &j, now)
}
fn advance_inner(
    access: &mut dyn CohAccess,
    d: &Deployment,
    j: &mut Journal,
    now: u64,
    recover: bool,
    cancel_stage: Option<&str>,
) -> Result<()> {
    let state = report(d, j, now)?;
    if let Some(stage) = cancel_stage {
        ensure!(recover, "EPERM recipe-cancel-requires-recover");
        let original = j
            .attempts
            .iter()
            .rev()
            .find(|a| !a.cancel && a.stage == stage)
            .ok_or_else(|| anyhow!("EPERM recipe-cancel-original"))?;
        if terminal(original)?.is_some() {
            return Ok(());
        }
        let recovery = d
            .recovery
            .iter()
            .find(|r| r.stage == stage)
            .ok_or_else(|| anyhow!("EPERM recipe-cancel-current-ticket-required"))?;
        ensure!(
            recovery.execution.request["args"]["job_id"] == original.execution.request["id"]
                && recovery.execution.request["subject_ref"]
                    == original.execution.request["subject_ref"],
            "EPERM recipe-cancel-binding"
        );
        if j.attempts
            .iter()
            .any(|a| a.cancel && a.execution.request["id"] == recovery.execution.request["id"])
        {
            return Ok(());
        }
        let a = Attempt {
            stage: stage.into(),
            key: original.key.clone(),
            predecessors: original.predecessors.clone(),
            configuration_sha256: j.configuration_sha256.clone(),
            number: j.attempts.len() as u32 + 1,
            cancel: true,
            execution: recovery.execution.clone(),
            input: original.input.clone(),
            runtime: original.runtime.clone(),
            reservation_bytes: 0,
            acknowledged: false,
        };
        return dispatch(access, d, j, a, now);
    }
    for a in &j.attempts {
        visible(access, &a.execution)?;
    }
    if recover {
        return Ok(());
    }
    if state["accounting"]["unresolved_reserved_bytes"].as_u64() != Some(0) {
        return Ok(());
    }
    for (s, status) in d.stages.iter().zip(
        state["stages"]
            .as_array()
            .ok_or_else(|| anyhow!("EPERM recipe-report"))?,
    ) {
        let value = input(s)?;
        let cache_key = status["key"]
            .as_str()
            .ok_or_else(|| anyhow!("pending recipe-predecessor"))?;
        if status["verified"] == true {
            let a = j
                .attempts
                .iter()
                .rev()
                .find(|a| !a.cancel && a.stage == s.id && a.key == cache_key)
                .ok_or_else(|| anyhow!("EPERM recipe-cache"))?;
            current(access, a, now, false)?;
            continue;
        }
        let a = Attempt {
            stage: s.id.clone(),
            key: cache_key.into(),
            predecessors: s
                .after
                .iter()
                .map(|id| {
                    let row = state["stages"]
                        .as_array()
                        .and_then(|rows| rows.iter().find(|r| r["id"] == *id))
                        .ok_or_else(|| anyhow!("EPERM recipe-predecessor"))?;
                    Ok((
                        string(row, "key")?.into(),
                        string(&row["output"], "sha256")?.into(),
                    ))
                })
                .collect::<Result<_>>()?,
            configuration_sha256: j.configuration_sha256.clone(),
            number: j.attempts.len() as u32 + 1,
            cancel: false,
            execution: s.execution.clone(),
            reservation_bytes: value["request"]["memory_budget_bytes"]
                .as_u64()
                .ok_or_else(|| anyhow!("EPERM recipe-budget"))?,
            input: value,
            runtime: s.runtime.clone(),
            acknowledged: false,
        };
        return dispatch(access, d, j, a, now);
    }
    Ok(())
}

/// Attach a sanitized local report to an existing canonical pack, then reseal it.
/// Its digest proves content identity only; it cannot confer receipt authority.
pub fn attach_diagnostic(root: &Path, input: &Path) -> Result<()> {
    crate::evidence::verify_pack_integrity(root)?;
    let bytes = read_bounded(input, limit("max_journal_bytes")? as usize)?;
    let report: Value = serde_json::from_slice(&bytes)?;
    let diagnostic = diagnostic(&report, &digest(&bytes))?;
    let directory = root.join("attachments");
    if !directory.exists() {
        fs::create_dir(&directory)?;
    }
    let path = crate::operator::confined_path(root, "attachments/recipe.json")?;
    write_atomic(&path, &serde_json::to_vec(&diagnostic)?)?;
    crate::evidence::seal_pack(root)
}

fn diagnostic(report: &Value, source: &str) -> Result<Value> {
    ensure!(
        report["schema"] == "cohesix-recipe-operation-report/v1"
            && report["authoritative"] == false,
        "EPERM recipe-diagnostic-schema"
    );
    let operation = string(report, "operation_id")?;
    id(operation)?;
    let rows = report["attempts"]
        .as_array()
        .ok_or_else(|| anyhow!("EPERM recipe-diagnostic-attempts"))?;
    ensure!(
        rows.len() <= limit("max_attempts")? as usize,
        "ELIMIT recipe-diagnostic"
    );
    let mut attempts = Vec::new();
    let mut ambiguous = false;
    for row in rows {
        for field in ["stage", "ticket_id", "idempotency_key"] {
            id(string(row, field)?)?;
        }
        let state = string(row, "state")?;
        ensure!(
            matches!(
                state,
                "verified_terminal" | "ambiguous" | "cancellation_requested"
            ),
            "EPERM recipe-diagnostic-state"
        );
        ambiguous |= state != "verified_terminal";
        attempts.push(json!({"stage":row["stage"],"ticket_id":row["ticket_id"],"idempotency_key":row["idempotency_key"],"state":state}));
    }
    Ok(
        json!({"schema":"cohesix-recipe-operation-report/v1", "authoritative":false,"proof":"none",
        "operation_id":operation,"source_sha256":source,"attempts":attempts,
        "cause":if ambiguous {"submission or cancellation has no verified native terminal acknowledgement"} else {"retained local recipe observation"},
        "uncertainty":if ambiguous {"native execution may have occurred; retain reservations and original idempotency"} else {"historical verification does not refresh current authority"},
        "recovery":"reconcile original host-ticket journal, native job and signed evidence; cancel only with a current exact ticket; never blindly redispatch"}),
    )
}

/// Read only the bounded, sanitized attachment; legacy cases remain byte-identical.
pub(crate) fn case_diagnostic(root: &Path) -> Result<Option<Value>> {
    let path = crate::operator::confined_path(root, "attachments/recipe.json")?;
    if !path.try_exists()? {
        return Ok(None);
    }
    let bytes = read_bounded(&path, 65536)?;
    let report: Value = serde_json::from_slice(&bytes)?;
    Ok(Some(diagnostic(&report, &digest(&bytes))?))
}
