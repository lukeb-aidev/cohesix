// Author: Lukas Bower
// Purpose: Admit selected shared jobs once under delegated authority and durable host-local scope custody.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, ensure, Context, Result};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use coh::peft::{release::DeploymentState, transaction::Request};
use cohesix_authority::gpu::PublishedDevice;
use cohesix_authority::peft::ReleaseArgs;
use cohesix_authority::standing::{
    AdmissionFacts, JobBinding, StandingControls, StandingScope, StandingScopeFile,
    JOB_BINDING_SCHEMA,
};
use cohesix_authority::standing_ledger::{JobRecord, ReserveOutcome, StandingLedger};
use cohesix_authority::AdmissionCorrelation;
use host_ticket_agent::claim::{validate_spec, SpecSource};
use host_ticket_agent::{HostTicketSpec, HOST_TICKET_V1_SCHEMA, HOST_TICKET_V2_SCHEMA};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    auth, authority_now_ms, authorize_delegated, evidence, validate_request_auth, AppState,
    GatewayConfig,
};

const SPEC_PATH: &str = "/host/tickets/spec";
const STATUS_PATH: &str = "/host/tickets/status";
const ADMIN_PATH: &str = "/host/standing/admin";
const MAX_SCOPE_FILE_BYTES: u64 = 65_536;
const MAX_SUBMIT_BYTES: usize = 4096;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SubmitRequest {
    binding: JobBinding,
    ticket: HostTicketSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreflightRequest {
    scope_id: String,
    ticket: HostTicketSpec,
}

/// A Shortcut carries an explicit stable request identity, not a caller-made
/// ticket, action, target or stale fact snapshot.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ApprovedStartRequest {
    request_id: String,
}

#[derive(Debug, Serialize)]
struct JobResponse {
    schema: &'static str,
    record: JobRecord,
    submission: &'static str,
}

fn fail(status: StatusCode, message: impl ToString) -> Response {
    (status, Json(json!({"error": message.to_string()}))).into_response()
}

fn ledger(state: &AppState) -> Result<&StandingLedger> {
    state
        .inner
        .standing_ledger
        .as_deref()
        .ok_or_else(|| anyhow!("EPERM standing-authority-disabled"))
}

pub(super) fn selected_ledger(config: &GatewayConfig) -> Result<Option<Arc<StandingLedger>>> {
    let controls = StandingControls::from_resolved_manifest(include_bytes!(
        "../../../configs/generated/root_task_resolved.json"
    ))
    .map_err(|error| anyhow!("{error}"))?;
    if !controls.enabled {
        ensure!(
            config.standing_ledger.is_none()
                && config.standing_scopes.is_none()
                && config.peft_release_config.is_none(),
            "EPERM standing authority disabled by compiled manifest"
        );
        return Ok(None);
    }
    if config.standing_ledger.is_none() && config.standing_scopes.is_none() {
        ensure!(
            config.peft_release_config.is_none(),
            "EPERM release profile without standing authority"
        );
        return Ok(None);
    }
    ensure!(
        controls
            .actions
            .iter()
            .any(|action| action == "peft.release")
            == config.peft_release_config.is_some(),
        "EPERM selected release profile required only for release authority"
    );
    let path = config
        .standing_ledger
        .as_ref()
        .context("enabled standing authority requires --standing-ledger")?;
    let scopes_path = config
        .standing_scopes
        .as_ref()
        .context("enabled standing authority requires --standing-scopes")?;
    ensure!(
        scopes_path.is_absolute() && !scopes_path.is_symlink(),
        "EPERM standing scope file path"
    );
    let file = File::open(scopes_path)?;
    #[cfg(unix)]
    ensure!(
        file.metadata()?.permissions().mode() & 0o077 == 0,
        "EPERM standing scope file permissions"
    );
    let mut bytes = Vec::new();
    file.take(MAX_SCOPE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_SCOPE_FILE_BYTES as usize,
        "ELIMIT standing scope file"
    );
    let selection: StandingScopeFile = serde_json::from_slice(&bytes)?;
    selection
        .validate(&controls)
        .map_err(|error| anyhow!("{error}"))?;
    let graph = cohesix_authority::provider::registry()?["graph_sha256"]
        .as_str()
        .context("generated provider graph hash missing")?
        .to_owned();
    let ledger = StandingLedger::new(path.clone(), graph, controls, selection.scopes)?;
    ledger.status("startup-check")?;
    Ok(Some(Arc::new(ledger)))
}

pub(super) fn delegated_token<'a>(headers: &'a HeaderMap) -> Option<&'a str> {
    if headers.get_all(auth::TICKET_HEADER).iter().count() != 1 {
        return None;
    }
    headers.get(auth::TICKET_HEADER)?.to_str().ok()
}

pub(super) fn authorize_status_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<auth::DelegatedPrincipal> {
    validate_request_auth(headers, state.request_auth_token())
        .map_err(|error| anyhow!("{error}"))?;
    state
        .inner
        .delegation
        .lock()
        .map_err(|_| anyhow!("EPERM authority-state-unavailable"))?
        .authorize_read_principal(
            delegated_token(headers),
            STATUS_PATH,
            4096,
            authority_now_ms()?,
        )
        .map_err(|error| anyhow!("{error}"))
}

pub(super) fn authorize_status(state: &AppState, headers: &HeaderMap) -> Result<String> {
    authorize_status_principal(state, headers).map(|principal| principal.subject)
}

pub(super) fn available_actions(state: &AppState, subject: &str) -> Result<Vec<String>> {
    let ledger = ledger(state)?;
    let controls = StandingControls::from_resolved_manifest(include_bytes!(
        "../../../configs/generated/root_task_resolved.json"
    ))
    .map_err(|error| anyhow!("{error}"))?;
    let now = authority_now_ms()?;
    let mut actions = Vec::new();
    for action in controls.actions {
        if !ledger.available_scopes(subject, &action, now)?.is_empty() {
            actions.push(action);
        }
    }
    Ok(actions)
}

/// Return only currently usable CUDA/PEFT standing choices for this subject.
/// The native facts in a later preflight and the final submit are fresh checks.
pub(super) async fn available_selected(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let subject = match authorize_status(&state, &headers) {
        Ok(subject) => subject,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    let selected = match ledger(&state) {
        Ok(ledger) => ledger,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    let now = match authority_now_ms() {
        Ok(now) => now,
        Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    let mut scopes = Vec::new();
    for action in ["gpu.workload.submit", "peft.release"] {
        let choices = match selected.available_scopes(&subject, action, now) {
            Ok(choices) => choices,
            Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
        };
        for scope in choices {
            let status = match selected.scope_status(&scope.id) {
                Ok(status) => status,
                Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
            };
            scopes.push(json!({
                "scope_id":scope.id,
                "action":scope.action,
                "target":scope.target,
                "expires_unix_ms":scope.expires_unix_ms,
                "remaining_units":scope.max_total_units
                    .saturating_sub(status.budget.settled_units)
                    .saturating_sub(status.budget.reserved_units),
                "active":status.budget.active,
                "max_concurrent":scope.max_concurrent,
            }));
        }
    }
    let registry = match cohesix_authority::provider::registry() {
        Ok(registry) => registry,
        Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    Json(json!({
        "schema":"cohesix-available-selected-jobs/v1",
        "scopes":scopes,
        "provider_graph_sha256":registry["graph_sha256"],
    }))
    .into_response()
}

fn authorize_cancel_subject(
    delegation: &mut auth::Delegation,
    token: Option<&str>,
    control: &str,
    now: u64,
) -> Result<String, &'static str> {
    delegation
        .authorize_write_principal(token, SPEC_PATH, &[control], now)
        .map(|principal| principal.subject)
}

fn selected_record(state: &AppState, admission_id: &str, subject: &str) -> Result<JobRecord> {
    cohesix_authority::validate_id(admission_id).map_err(|_| anyhow!("EPERM job id"))?;
    let record = ledger(state)?
        .status(admission_id)?
        .ok_or_else(|| anyhow!("ENOENT job"))?;
    ensure!(record.binding.subject == subject, "EPERM job subject");
    Ok(record)
}

fn matching_target_results(
    lines: Vec<String>,
    binding: &JobBinding,
) -> Result<(Vec<Value>, Vec<String>)> {
    let mut values = Vec::new();
    let mut hashes = Vec::new();
    for line in lines {
        let value: Value = serde_json::from_str(&line)?;
        if value["id"] == binding.ticket_id
            && value["idempotency_key"] == binding.idempotency_key
            && value["admission"]["admission_id"] == binding.admission_id
        {
            hashes.push(hex::encode(Sha256::digest(line.as_bytes())));
            values.push(value);
        }
    }
    Ok((values, hashes))
}

fn observe_service(ticket: &HostTicketSpec, now: u64) -> Result<AdmissionFacts> {
    let unit = ticket
        .args
        .get("unit")
        .and_then(Value::as_str)
        .context("systemd unit required")?;
    host_sidecar_bridge::native::validate_native_id(unit)?;
    ensure!(
        ticket.target.as_deref() == Some(format!("/host/systemd/{unit}/restart").as_str()),
        "EPERM standing service target"
    );
    let native = host_sidecar_bridge::native::observe_systemd_before(
        unit,
        Instant::now() + Duration::from_secs(3),
    )?;
    let (state_epoch, resource_generation) =
        host_ticket_agent::standing::service_generations(&serde_json::to_value(native)?)?;
    Ok(AdmissionFacts {
        observed_unix_ms: now,
        state_epoch,
        resource_generation,
        policy_sha256: String::new(),
    })
}

fn gpu_execution_identity(encoded: &str, gpu_id: &str) -> Result<PublishedDevice> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Projection {
        id: String,
        #[serde(rename = "name")]
        _name: String,
        #[serde(rename = "memory_mb")]
        _memory_mb: u32,
        #[serde(rename = "sm_count")]
        _sm_count: u32,
        #[serde(rename = "driver_version")]
        _driver_version: String,
        #[serde(rename = "runtime_version")]
        _runtime_version: String,
        execution_identity: PublishedDevice,
    }
    let projection: Projection = serde_json::from_str(encoded)?;
    ensure!(
        projection.id == gpu_id
            && projection.execution_identity.schema == "cohesix-gpu-device/v1"
            && projection.execution_identity.source_id == "gpu-bridge-host/cuda-reference"
            && projection.execution_identity.source_mode == "production"
            && projection.execution_identity.source_epoch > 0,
        "EPERM GPU inventory identity"
    );
    Ok(projection.execution_identity)
}

fn observe_gpu(state: &AppState, ticket: &HostTicketSpec, now: u64) -> Result<AdmissionFacts> {
    let gpu_id = ticket.subject_ref.as_deref().context("GPU id required")?;
    let worker_id = ticket
        .receipt_worker_id
        .as_deref()
        .context("Worker id required")?;
    let lease_id = ticket
        .args
        .get("lease_id")
        .and_then(Value::as_str)
        .context("lease id required")?;
    for value in [gpu_id, worker_id, lease_id] {
        cohesix_authority::validate_id(value).map_err(|_| anyhow!("EPERM GPU identity"))?;
    }
    let lease = state.read_uncached(&format!("/proc/lease/by-id/{lease_id}"))?;
    ensure!(
        lease.len() == 1 && lease[0].len() <= 1024,
        "EPERM lease unavailable or ambiguous"
    );
    let mut fields = std::collections::BTreeMap::new();
    for token in lease[0].split_whitespace() {
        let (key, value) = token.split_once('=').context("invalid lease field")?;
        ensure!(
            fields.insert(key, value).is_none(),
            "EPERM duplicate lease field"
        );
    }
    ensure!(
        fields.get("id") == Some(&lease_id)
            && fields.get("resource") == Some(&gpu_id)
            && fields.get("subject") == Some(&worker_id)
            && fields.get("state") == Some(&"ACTIVE"),
        "EPERM lease binding changed"
    );
    let state_epoch = fields
        .get("seq")
        .context("lease sequence missing")?
        .parse::<u64>()?;
    ensure!(state_epoch > 0, "EPERM lease sequence");
    let lines = state.read_uncached(&format!("/gpu/{gpu_id}/info"))?;
    let encoded = lines.join("\n");
    ensure!(encoded.len() <= 8192, "ELIMIT GPU inventory");
    let identity = gpu_execution_identity(&encoded, gpu_id)?;
    Ok(AdmissionFacts {
        observed_unix_ms: now,
        state_epoch,
        resource_generation: identity.source_epoch,
        policy_sha256: String::new(),
    })
}

fn read_private(path: &std::path::Path, maximum: u64) -> Result<Vec<u8>> {
    ensure!(path.is_absolute(), "EPERM release-absolute-path");
    let mut directory = rustix::fs::open(
        "/",
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    let mut components = path.components().peekable();
    ensure!(
        components.next() == Some(std::path::Component::RootDir),
        "EPERM release-absolute-path"
    );
    let mut file = None;
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            return Err(anyhow!("EPERM release-path-component"));
        };
        if components.peek().is_some() {
            directory = rustix::fs::openat(
                &directory,
                name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )?;
        } else {
            file = Some(std::fs::File::from(rustix::fs::openat(
                &directory,
                name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::NONBLOCK
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )?));
        }
    }
    let file = file.context("EPERM release-file-path")?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= maximum,
        "ELIMIT release-file"
    );
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= maximum, "ELIMIT release-file");
    Ok(bytes)
}

fn observe_release_profile(
    profile_path: &std::path::Path,
    ticket: &HostTicketSpec,
    now: u64,
) -> Result<AdmissionFacts> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ReleaseProfile {
        python: std::path::PathBuf,
        helper: std::path::PathBuf,
        helper_sha256: String,
        native_config: std::path::PathBuf,
    }
    ensure!(
        std::fs::metadata(profile_path)?.permissions().mode() & 0o077 == 0,
        "EPERM release-profile-permissions"
    );
    let profile: ReleaseProfile = serde_json::from_slice(&read_private(profile_path, 8192)?)?;
    ensure!(
        profile.python.is_absolute()
            && profile.helper.is_absolute()
            && profile.native_config.is_absolute()
            && profile.helper_sha256.len() == 64
            && profile
                .helper_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && cohesix_evidence::digest(&read_private(&profile.helper, 262144)?)
                == profile.helper_sha256,
        "EPERM release-profile-identity"
    );
    ensure!(
        std::fs::metadata(&profile.native_config)?
            .permissions()
            .mode()
            & 0o077
            == 0,
        "EPERM release-native-config-permissions"
    );
    let native: Value = serde_json::from_slice(&read_private(&profile.native_config, 8192)?)?;
    let root = native["root"]
        .as_str()
        .map(std::path::PathBuf::from)
        .context("EPERM release-private-root")?;
    ensure!(
        root.is_absolute()
            && root.canonicalize()? == root
            && std::fs::metadata(&root)?.permissions().mode() & 0o077 == 0,
        "EPERM release-private-root"
    );
    ensure!(
        cohesix_authority::peft::validate_release_args(&ticket.args),
        "EPERM release-args"
    );
    let selected: ReleaseArgs = serde_json::from_value(ticket.args.clone())?;
    let request_path = root.join("objects").join(&selected.request_sha256);
    let request_bytes = read_private(&request_path, 8192)?;
    ensure!(
        cohesix_evidence::digest(&request_bytes) == selected.request_sha256,
        "EPERM release-input-digest"
    );
    let request: Request = serde_json::from_slice(&request_bytes)?;
    request.validate()?;
    ensure!(
        serde_json::to_vec(&request)? == request_bytes
            && native["profile_sha256"] == request.profile_sha256
            && ticket.operation_id.as_deref() == Some(request.operation_id.as_str())
            && ticket.subject_ref.as_deref() == Some(request.model_id.as_str())
            && ticket.target.is_none(),
        "EPERM release-ticket-input-binding"
    );
    let accepted_value: Value =
        serde_json::from_slice(&read_private(&root.join("accepted.json"), 8192)?)?;
    let accepted: DeploymentState = serde_json::from_value(accepted_value.clone())?;
    ensure!(
        accepted == request.baseline,
        "EPERM release-baseline-changed"
    );
    let (state_epoch, resource_generation) =
        host_ticket_agent::standing::release_generations(&accepted_value)?;
    Ok(AdmissionFacts {
        observed_unix_ms: now,
        state_epoch,
        resource_generation,
        policy_sha256: String::new(),
    })
}

fn observe_release(state: &AppState, ticket: &HostTicketSpec, now: u64) -> Result<AdmissionFacts> {
    let profile = state
        .inner
        .peft_release_config
        .as_deref()
        .context("EPERM release-profile-not-selected")?;
    observe_release_profile(profile, ticket, now)
}

fn observe_facts(state: &AppState, ticket: &HostTicketSpec) -> Result<AdmissionFacts> {
    let now = authority_now_ms()?;
    let mut facts = match ticket.action.as_str() {
        "systemd.restart" => observe_service(ticket, now)?,
        "gpu.workload.submit" => observe_gpu(state, ticket, now)?,
        "peft.release" => observe_release(state, ticket, now)?,
        _ => return Err(anyhow!("EPERM unsupported standing action")),
    };
    facts.policy_sha256 = cohesix_authority::provider::registry()?["graph_sha256"]
        .as_str()
        .context("generated provider graph hash missing")?
        .to_owned();
    Ok(facts)
}

fn build_selected_ticket(
    binding: &JobBinding,
    mut ticket: HostTicketSpec,
    expiry: u64,
    scope_generation: u64,
) -> Result<(HostTicketSpec, String)> {
    ensure!(
        ticket.admission.is_none(),
        "EPERM caller-supplied admission"
    );
    ensure!(
        binding.units == 1,
        "EPERM selected actions cost one budget unit"
    );
    ticket.admission = Some(AdmissionCorrelation {
        admission_id: binding.admission_id.clone(),
        intent_hash: binding
            .intent_sha256()
            .map_err(|error| anyhow!("{error}"))?,
        policy_hash: binding.policy_sha256.clone(),
        state_epoch: binding.state_epoch,
        resource_generation: binding.resource_generation,
        decision_expiry: expiry,
        standing_scope_id: Some(binding.scope_id.clone()),
    });
    let record = JobRecord {
        binding: binding.clone(),
        intent_sha256: binding
            .intent_sha256()
            .map_err(|error| anyhow!("{error}"))?,
        scope_generation,
        decision_expires_unix_ms: expiry,
        reserved_unix_ms: expiry.saturating_sub(1),
        dispatched_unix_ms: None,
        execution: cohesix_authority::standing_ledger::JobExecution::Reserved,
        delivery: cohesix_authority::standing_ledger::JobDelivery::Pending,
        result_sha256: None,
        cancel_requested: false,
    };
    host_ticket_agent::standing::verify_ticket(&ticket, &record)?;
    let line = serde_json::to_string(&ticket)?;
    ensure!(
        line.len() <= MAX_SUBMIT_BYTES && !line.contains('\n'),
        "ELIMIT selected ticket bytes"
    );
    Ok((ticket, line))
}

fn approved_service_request(
    scope: &StandingScope,
    request_id: &str,
    facts: &AdmissionFacts,
) -> Result<SubmitRequest> {
    cohesix_authority::validate_id(request_id).map_err(|_| anyhow!("EPERM request id"))?;
    ensure!(
        request_id.len() <= 96 && scope.action == "systemd.restart",
        "EPERM unsupported approved recipe"
    );
    let unit = scope
        .target
        .strip_prefix("/host/systemd/")
        .and_then(|value| value.strip_suffix("/restart"))
        .context("EPERM approved service target")?;
    host_sidecar_bridge::native::validate_native_id(unit)?;
    let args = json!({"unit":unit});
    let deadline = facts
        .observed_unix_ms
        .checked_add(300_000)
        .context("ELIMIT approved deadline")?
        .min(scope.expires_unix_ms);
    ensure!(
        deadline > facts.observed_unix_ms,
        "EPERM expired approved scope"
    );
    let binding = JobBinding {
        schema: JOB_BINDING_SCHEMA.to_owned(),
        scope_id: scope.id.clone(),
        admission_id: format!("mac-{request_id}"),
        ticket_id: format!("mac-{request_id}"),
        idempotency_key: format!("mac-{request_id}"),
        subject: scope.subject.clone(),
        action: scope.action.clone(),
        target: scope.target.clone(),
        input_sha256: hex::encode(Sha256::digest(serde_json::to_vec(&args)?)),
        policy_sha256: facts.policy_sha256.clone(),
        state_epoch: facts.state_epoch,
        resource_generation: facts.resource_generation,
        deadline_unix_ms: deadline,
        units: 1,
        attempt: 1,
    };
    let ticket = HostTicketSpec {
        schema: HOST_TICKET_V1_SCHEMA.to_owned(),
        id: binding.ticket_id.clone(),
        idempotency_key: binding.idempotency_key.clone(),
        action: binding.action.clone(),
        target: Some(binding.target.clone()),
        args,
        expires_unix_ms: Some(deadline),
        ..HostTicketSpec::default()
    };
    validate_spec(&ticket, SpecSource::RawRequest)?;
    binding
        .intent_sha256()
        .map_err(|error| anyhow!("{error}"))?;
    Ok(SubmitRequest { binding, ticket })
}

fn prepare_selected_request(
    scope: &StandingScope,
    subject: &str,
    facts: &AdmissionFacts,
    mut ticket: HostTicketSpec,
) -> Result<SubmitRequest> {
    ensure!(
        scope.subject == subject
            && scope.action == ticket.action
            && ticket.admission.is_none()
            && matches!(
                ticket.action.as_str(),
                "gpu.workload.submit" | "peft.release"
            ),
        "EPERM selected preflight identity"
    );
    let deadline = match facts.observed_unix_ms.checked_add(scope.decision_ttl_ms) {
        Some(deadline) => deadline.min(scope.expires_unix_ms),
        None => return Err(anyhow!("ELIMIT selected deadline")),
    };
    ensure!(
        deadline > facts.observed_unix_ms,
        "EPERM selected scope expired"
    );
    ticket.expires_unix_ms = Some(deadline);
    validate_spec(&ticket, SpecSource::RawRequest)?;
    let input_sha256 = ticket
        .args
        .get("request_sha256")
        .and_then(Value::as_str)
        .context("EPERM selected input digest")?;
    let binding = JobBinding {
        schema: JOB_BINDING_SCHEMA.to_owned(),
        scope_id: scope.id.clone(),
        admission_id: ticket.id.clone(),
        ticket_id: ticket.id.clone(),
        idempotency_key: ticket.idempotency_key.clone(),
        subject: subject.to_owned(),
        action: ticket.action.clone(),
        target: scope.target.clone(),
        input_sha256: input_sha256.to_owned(),
        policy_sha256: facts.policy_sha256.clone(),
        state_epoch: facts.state_epoch,
        resource_generation: facts.resource_generation,
        deadline_unix_ms: deadline,
        units: 1,
        attempt: 1,
    };
    binding
        .intent_sha256()
        .map_err(|error| anyhow!("{error}"))?;
    build_selected_ticket(&binding, ticket.clone(), deadline, scope.generation)?;
    Ok(SubmitRequest { binding, ticket })
}

/// Prepare one exact selected request from fresh native and target facts. This
/// returns no admission or execution claim; submit re-observes before reserve.
pub(super) async fn preflight(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PreflightRequest>,
) -> Response {
    let subject = match authorize_status(&state, &headers) {
        Ok(subject) => subject,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    if payload.scope_id.len() > 96
        || cohesix_authority::validate_id(&payload.scope_id).is_err()
        || payload.ticket.admission.is_some()
        || !matches!(
            payload.ticket.action.as_str(),
            "gpu.workload.submit" | "peft.release"
        )
    {
        return fail(StatusCode::BAD_REQUEST, "EPERM selected preflight input");
    }
    let ledger = match ledger(&state) {
        Ok(ledger) => ledger,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    let now = match authority_now_ms() {
        Ok(now) => now,
        Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    let scopes = match ledger.available_scopes(&subject, &payload.ticket.action, now) {
        Ok(scopes) => scopes,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    let Some(scope) = scopes
        .into_iter()
        .find(|scope| scope.id == payload.scope_id)
    else {
        return fail(StatusCode::FORBIDDEN, "EPERM selected scope unavailable");
    };
    let state_for_facts = state.clone();
    let ticket_for_facts = payload.ticket.clone();
    let facts = match tokio::task::spawn_blocking(move || {
        observe_facts(&state_for_facts, &ticket_for_facts)
    })
    .await
    {
        Ok(Ok(facts)) => facts,
        Ok(Err(error)) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
        Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    let request = match prepare_selected_request(&scope, &subject, &facts, payload.ticket) {
        Ok(request) => request,
        Err(error) => return fail(StatusCode::BAD_REQUEST, error),
    };
    Json(json!({
        "schema":"cohesix-selected-job-preflight/v1",
        "request":request,
        "admission":"not_submitted",
        "facts_observed_unix_ms":facts.observed_unix_ms,
    }))
    .into_response()
}

/// Start only a configured service recipe using a stable caller identity.
/// An uncertain prior write is returned for reconciliation, never retried.
pub(super) async fn start_approved(
    State(state): State<AppState>,
    Path(scope_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ApprovedStartRequest>,
) -> Response {
    let subject = match authorize_status(&state, &headers) {
        Ok(subject) => subject,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    let ledger = match ledger(&state) {
        Ok(ledger) => ledger,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    if cohesix_authority::validate_id(&request.request_id).is_err()
        || request.request_id.len() > 96
        || cohesix_authority::validate_id(&scope_id).is_err()
    {
        return fail(StatusCode::BAD_REQUEST, "EPERM approved request identity");
    }
    let scope = match ledger.scope(&scope_id) {
        Some(scope) if scope.subject == subject && scope.action == "systemd.restart" => scope,
        _ => return fail(StatusCode::FORBIDDEN, "EPERM approved scope"),
    };
    let status = match ledger.scope_status(&scope_id) {
        Ok(status) => status,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    if status.budget.revoked {
        return fail(StatusCode::FORBIDDEN, "EPERM revoked approved scope");
    }
    let admission_id = format!("mac-{}", request.request_id);
    match ledger.status(&admission_id) {
        Ok(Some(record))
            if record.binding.scope_id == scope_id && record.binding.subject == subject =>
        {
            return (
                StatusCode::OK,
                Json(JobResponse {
                    schema: "cohesix-selected-job-response/v1",
                    record,
                    submission: "existing",
                }),
            )
                .into_response();
        }
        Ok(Some(_)) => return fail(StatusCode::CONFLICT, "EPERM request identity conflict"),
        Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
        Ok(None) => {}
    }
    let selected = scope.clone();
    let state_for_facts = state.clone();
    let facts = match tokio::task::spawn_blocking(move || {
        let unit = selected
            .target
            .strip_prefix("/host/systemd/")
            .and_then(|value| value.strip_suffix("/restart"))
            .context("EPERM approved service target")?;
        let ticket = HostTicketSpec {
            action: "systemd.restart".into(),
            target: Some(selected.target.clone()),
            args: json!({"unit":unit}),
            ..HostTicketSpec::default()
        };
        observe_facts(&state_for_facts, &ticket)
    })
    .await
    {
        Ok(Ok(facts)) => facts,
        Ok(Err(error)) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
        Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    let payload = match approved_service_request(scope, &request.request_id, &facts) {
        Ok(payload) => payload,
        Err(error) => return fail(StatusCode::BAD_REQUEST, error),
    };
    submit(State(state), headers, Json(payload)).await
}

pub(super) async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SubmitRequest>,
) -> Response {
    if let Err(error) = validate_request_auth(&headers, state.request_auth_token()) {
        return fail(StatusCode::UNAUTHORIZED, error);
    }
    let ledger = match ledger(&state) {
        Ok(ledger) => ledger,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    if let Err(error) = validate_spec(&payload.ticket, SpecSource::RawRequest) {
        return fail(StatusCode::BAD_REQUEST, error);
    }
    if !matches!(
        (
            payload.ticket.schema.as_str(),
            payload.ticket.action.as_str()
        ),
        (HOST_TICKET_V1_SCHEMA, "systemd.restart")
            | (
                HOST_TICKET_V2_SCHEMA,
                "gpu.workload.submit" | "peft.release"
            )
    ) {
        return fail(StatusCode::FORBIDDEN, "EPERM unsupported selected action");
    }
    if payload.binding.units != 1 {
        return fail(
            StatusCode::FORBIDDEN,
            "EPERM selected actions cost one budget unit",
        );
    }
    if delegated_token(&headers).is_none() {
        return fail(StatusCode::FORBIDDEN, "EPERM delegated ticket required");
    }
    let scope = match ledger.scope(&payload.binding.scope_id) {
        Some(scope) => scope,
        None => return fail(StatusCode::FORBIDDEN, "EPERM standing scope unknown"),
    };
    // Direct native/target observations are outside the caller's request body.
    let state_for_facts = state.clone();
    let ticket_for_facts = payload.ticket.clone();
    let facts = match tokio::task::spawn_blocking(move || {
        observe_facts(&state_for_facts, &ticket_for_facts)
    })
    .await
    {
        Ok(Ok(facts)) => facts,
        Ok(Err(error)) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
        Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    if payload.binding.state_epoch != facts.state_epoch
        || payload.binding.resource_generation != facts.resource_generation
        || payload.binding.policy_sha256 != facts.policy_sha256
    {
        return fail(StatusCode::FORBIDDEN, "EPERM standing facts changed");
    }
    let expiry = facts
        .observed_unix_ms
        .saturating_add(scope.decision_ttl_ms)
        .min(scope.expires_unix_ms)
        .min(payload.binding.deadline_unix_ms);
    let (_ticket, line) =
        match build_selected_ticket(&payload.binding, payload.ticket, expiry, scope.generation) {
            Ok(value) => value,
            Err(error) => return fail(StatusCode::BAD_REQUEST, error),
        };
    let principal = match state
        .inner
        .delegation
        .lock()
        .map_err(|_| "EPERM authority-state-unavailable")
        .and_then(|mut delegation| {
            delegation.authorize_write_principal(
                delegated_token(&headers),
                SPEC_PATH,
                &[&line],
                authority_now_ms().map_err(|_| "EPERM authority-clock-unavailable")?,
            )
        }) {
        Ok(principal) => principal,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    if payload.binding.subject != principal.subject {
        return fail(StatusCode::FORBIDDEN, "EPERM standing subject mismatch");
    }
    let now = match authority_now_ms() {
        Ok(now) => now,
        Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    let admission_id = payload.binding.admission_id.clone();
    let reserved = match ledger.reserve_with_expiry(payload.binding, &facts, now, Some(expiry)) {
        Ok(value) => value,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    if let ReserveOutcome::Existing(record) = reserved {
        return (
            StatusCode::OK,
            Json(JobResponse {
                schema: "cohesix-selected-job-response/v1",
                record,
                submission: "existing",
            }),
        )
            .into_response();
    }
    let state_for_write = state.clone();
    let identity_for_write = principal.ticket_hash;
    let result = tokio::task::spawn_blocking(move || {
        evidence::write(
            &state_for_write,
            &identity_for_write,
            SPEC_PATH,
            line.as_bytes(),
        )
    })
    .await;
    let record = match ledger.status(&admission_id) {
        Ok(Some(record)) => record,
        Ok(None) | Err(_) => {
            return fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "EPERM standing ledger unavailable",
            )
        }
    };
    match result {
        Ok(Ok(_)) => (
            StatusCode::ACCEPTED,
            Json(JobResponse {
                schema: "cohesix-selected-job-response/v1",
                record,
                submission: "target_write_ack",
            }),
        )
            .into_response(),
        Ok(Err(error)) => fail(
            StatusCode::CONFLICT,
            format!("uncertain root write for {admission_id}: {error}"),
        ),
        Err(error) => fail(
            StatusCode::CONFLICT,
            format!("uncertain root write for {admission_id}: {error}"),
        ),
    }
}

pub(super) async fn status(
    State(state): State<AppState>,
    Path(admission_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let subject = match authorize_status(&state, &headers) {
        Ok(subject) => subject,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    match selected_record(&state, &admission_id, &subject) {
        Ok(record) => Json(record).into_response(),
        Err(error) => fail(StatusCode::NOT_FOUND, error),
    }
}

pub(super) async fn cancel(
    State(state): State<AppState>,
    Path(admission_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = validate_request_auth(&headers, state.request_auth_token()) {
        return fail(StatusCode::UNAUTHORIZED, error);
    }
    let control =
        json!({"schema":"cohesix-job-cancel/v1", "admission_id":admission_id}).to_string();
    let subject = match state
        .inner
        .delegation
        .lock()
        .map_err(|_| "EPERM authority-state-unavailable")
        .and_then(|mut delegation| {
            authorize_cancel_subject(
                &mut delegation,
                delegated_token(&headers),
                &control,
                authority_now_ms().map_err(|_| "EPERM authority-clock-unavailable")?,
            )
        }) {
        Ok(subject) => subject,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    if let Err(error) = selected_record(&state, &admission_id, &subject) {
        return fail(StatusCode::NOT_FOUND, error);
    }
    match ledger(&state).and_then(|ledger| ledger.request_cancel(&admission_id).map_err(Into::into))
    {
        Ok(record) => (StatusCode::ACCEPTED, Json(record)).into_response(),
        Err(error) => fail(StatusCode::CONFLICT, error),
    }
}

pub(super) async fn reconcile(
    State(state): State<AppState>,
    Path(admission_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let subject = match authorize_status(&state, &headers) {
        Ok(subject) => subject,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    let record = match selected_record(&state, &admission_id, &subject) {
        Ok(record) => record,
        Err(error) => return fail(StatusCode::NOT_FOUND, error),
    };
    let state_for_read = state.clone();
    let target =
        tokio::task::spawn_blocking(move || state_for_read.read_uncached(STATUS_PATH)).await;
    let lines = match target {
        Ok(Ok(lines)) => lines,
        Ok(Err(error)) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
        Err(error) => return fail(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    let (target_result, target_result_sha256) =
        match matching_target_results(lines, &record.binding) {
            Ok(results) => results,
            Err(error) => return fail(StatusCode::BAD_GATEWAY, error),
        };
    Json(json!({
        "schema":"cohesix-selected-job-reconciliation/v1",
        "record":record,
        "target_results":target_result,
        "target_result_sha256":target_result_sha256,
        "effect_replay_allowed":false,
    }))
    .into_response()
}

fn authorize_admin(
    state: &AppState,
    headers: &HeaderMap,
    scope_id: &str,
    verb: &str,
) -> Result<()> {
    validate_request_auth(headers, state.request_auth_token())
        .map_err(|error| anyhow!("{error}"))?;
    cohesix_authority::validate_id(scope_id).map_err(|_| anyhow!("EPERM scope id"))?;
    let control = json!({
        "schema":"cohesix-standing-administration/v1",
        "scope_id":scope_id,
        "verb":verb,
    })
    .to_string();
    authorize_delegated(state, headers, ADMIN_PATH, &[&control])
        .map_err(|error| anyhow!("{error}"))?;
    Ok(())
}

pub(super) async fn inspect_scope(
    State(state): State<AppState>,
    Path(scope_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = authorize_admin(&state, &headers, &scope_id, "inspect") {
        return fail(StatusCode::FORBIDDEN, error);
    }
    match ledger(&state).and_then(|ledger| ledger.scope_status(&scope_id).map_err(Into::into)) {
        Ok(status) => Json(status).into_response(),
        Err(error) => fail(StatusCode::NOT_FOUND, error),
    }
}

/// Supply current subject-filtered choices for native App Entities. A stale
/// cached entity never grants an effect because submission rechecks the ledger.
pub(super) async fn available_scopes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let subject = match authorize_status(&state, &headers) {
        Ok(subject) => subject,
        Err(error) => return fail(StatusCode::FORBIDDEN, error),
    };
    let result = ledger(&state).and_then(|selected| {
        selected
            .available_scopes(&subject, "systemd.restart", authority_now_ms()?)
            .map_err(Into::into)
    });
    match result {
        Ok(scopes) => Json(json!({
            "schema": "cohesix-available-scopes/v1",
            "scopes": scopes.iter().map(|scope| json!({
                "id": scope.id,
                "action": scope.action,
                "target": scope.target,
            })).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => fail(StatusCode::SERVICE_UNAVAILABLE, error),
    }
}

pub(super) async fn revoke_scope(
    State(state): State<AppState>,
    Path(scope_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = authorize_admin(&state, &headers, &scope_id, "revoke") {
        return fail(StatusCode::FORBIDDEN, error);
    }
    match ledger(&state).and_then(|ledger| {
        ledger.revoke(&scope_id)?;
        ledger.scope_status(&scope_id).map_err(Into::into)
    }) {
        Ok(status) => Json(status).into_response(),
        Err(error) => fail(StatusCode::CONFLICT, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cohesix_authority::policy::AuthorityPolicy;
    use cohesix_authority::standing::{JOB_BINDING_SCHEMA, STANDING_SCOPE_SCHEMA};
    use cohesix_ticket::{
        BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer, TicketKey, TicketScope, TicketVerb,
    };
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn release_admission_observes_exact_private_baseline_and_helper() {
        let temporary = tempfile::tempdir().expect("private test directory");
        let root = temporary.path().canonicalize().unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir(root.join("objects")).unwrap();
        let baseline = json!({
            "generation": 0,
            "adapter_sha256": null,
            "served_artifact_sha256": "a".repeat(64),
            "runtime_sha256": "b".repeat(64),
            "healthy": true,
            "rollback_verified": true,
        });
        let request: Request = serde_json::from_value(json!({
            "schema": "cohesix-peft-release/v1",
            "operation_id": "op-1",
            "model_id": "local-mlx",
            "entry": "train",
            "profile_sha256": "d".repeat(64),
            "input_sha256": "e".repeat(64),
            "evaluation_policy": {
                "minimum_samples": 4,
                "maximum_age_ms": 60_000,
                "metrics": {"eval_loss": {
                    "direction": "lower", "absolute_bound": 1.0,
                    "maximum_regression": 0.0
                }}
            },
            "baseline": baseline,
        }))
        .unwrap();
        let request_bytes = serde_json::to_vec(&request).unwrap();
        let request_hash = cohesix_evidence::digest(&request_bytes);
        std::fs::write(root.join("objects").join(&request_hash), &request_bytes).unwrap();
        let accepted = serde_json::to_vec(&request.baseline).unwrap();
        std::fs::write(root.join("accepted.json"), &accepted).unwrap();
        let helper = root.join("helper.py");
        std::fs::write(&helper, b"# selected helper\n").unwrap();
        let native_config = root.join("native.json");
        std::fs::write(
            &native_config,
            serde_json::to_vec(&json!({
                "root": root,
                "profile_sha256": request.profile_sha256,
            }))
            .unwrap(),
        )
        .unwrap();
        let profile = root.join("release.json");
        std::fs::write(
            &profile,
            serde_json::to_vec(&json!({
                "python": "/usr/bin/python3",
                "helper": helper,
                "helper_sha256": cohesix_evidence::digest(b"# selected helper\n"),
                "native_config": native_config,
            }))
            .unwrap(),
        )
        .unwrap();
        for path in [&profile, &native_config] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut ticket = HostTicketSpec {
            schema: HOST_TICKET_V2_SCHEMA.into(),
            action: "peft.release".into(),
            operation_id: Some(request.operation_id.clone()),
            subject_ref: Some(request.model_id.clone()),
            args: json!({"request_sha256":request_hash}),
            ..HostTicketSpec::default()
        };
        let facts = observe_release_profile(&profile, &ticket, 1000).unwrap();
        assert!(facts.state_epoch > 0 && facts.resource_generation > 0);
        let alias = root.join("profile-alias.json");
        std::os::unix::fs::symlink(&profile, &alias).unwrap();
        assert!(observe_release_profile(&alias, &ticket, 1000).is_err());
        let parent_alias = root.join("release-root-alias");
        std::os::unix::fs::symlink(&root, &parent_alias).unwrap();
        assert!(
            observe_release_profile(&parent_alias.join("release.json"), &ticket, 1000).is_err()
        );
        ticket.subject_ref = Some("other-model".into());
        assert!(observe_release_profile(&profile, &ticket, 1000).is_err());
        ticket.subject_ref = Some(request.model_id);
        std::fs::write(root.join("accepted.json"), b"{}").unwrap();
        assert!(observe_release_profile(&profile, &ticket, 1000).is_err());
        std::fs::write(root.join("accepted.json"), accepted).unwrap();
        std::fs::write(&helper, b"# changed helper\n").unwrap();
        assert!(observe_release_profile(&profile, &ticket, 1000).is_err());
    }

    #[test]
    fn cancellation_checks_the_verified_subject_and_write_scope() {
        let mut delegation = auth::Delegation::new(
            Some(TicketKey::from_secret("cancel-test-issuer")),
            AuthorityPolicy::default(),
            Role::Queen,
            None,
            1000,
        )
        .expect("delegation fixture");
        let issue = |subject: &str, verb| {
            TicketIssuer::new("cancel-test-issuer")
                .issue(
                    TicketClaims::new(
                        Role::Queen,
                        BudgetSpec::unbounded().with_ops(Some(2)).with_ttl(Some(10)),
                        Some(subject.into()),
                        MountSpec::empty(),
                        1000,
                    )
                    .with_scopes(vec![TicketScope::new(SPEC_PATH, verb, 2)]),
                )
                .expect("issue")
                .encode()
                .expect("encode")
        };
        let operator = issue("operator-1", TicketVerb::Write);
        let other = issue("operator-2", TicketVerb::Write);
        let read_only = issue("operator-1", TicketVerb::Read);
        let control = r#"{"schema":"cohesix-job-cancel/v1","admission_id":"admit-1"}"#;
        assert_eq!(
            authorize_cancel_subject(&mut delegation, Some(&operator), control, 1000),
            Ok("operator-1".into())
        );
        assert_eq!(
            authorize_cancel_subject(&mut delegation, Some(&other), control, 1000),
            Ok("operator-2".into())
        );
        assert!(
            authorize_cancel_subject(&mut delegation, Some(&read_only), control, 1000).is_err()
        );
    }

    #[test]
    fn approved_start_derives_only_the_selected_service_and_fresh_facts() {
        let scope = StandingScope {
            schema: STANDING_SCOPE_SCHEMA.into(),
            id: "service-1".into(),
            subject: "operator-1".into(),
            action: "systemd.restart".into(),
            target: "/host/systemd/cohesix-agent.service/restart".into(),
            policy_sha256: "a".repeat(64),
            generation: 1,
            expires_unix_ms: 500_000,
            max_job_units: 1,
            max_total_units: 4,
            max_concurrent: 1,
            max_retries: 1,
            cooldown_ms: 0,
            fact_max_age_ms: 5_000,
            decision_ttl_ms: 5_000,
        };
        let facts = AdmissionFacts {
            observed_unix_ms: 100_000,
            state_epoch: 2,
            resource_generation: 3,
            policy_sha256: scope.policy_sha256.clone(),
        };
        let request = approved_service_request(&scope, "req-123", &facts).unwrap();
        assert_eq!(request.binding.admission_id, "mac-req-123");
        assert_eq!(request.binding.subject, scope.subject);
        assert_eq!(request.binding.state_epoch, 2);
        assert_eq!(request.binding.resource_generation, 3);
        assert_eq!(request.binding.deadline_unix_ms, 400_000);
        assert_eq!(request.ticket.args, json!({"unit":"cohesix-agent.service"}));
        assert_eq!(
            request.ticket.target.as_deref(),
            Some(scope.target.as_str())
        );
        assert!(approved_service_request(&scope, "../escape", &facts).is_err());
        assert!(approved_service_request(&scope, &"a".repeat(97), &facts).is_err());
        let mut changed = scope.clone();
        changed.action = "peft.release".into();
        assert!(approved_service_request(&changed, "req-124", &facts).is_err());
    }

    #[test]
    fn selected_release_preflight_prepares_only_an_unadmitted_original_identity() {
        let scope = StandingScope {
            schema: STANDING_SCOPE_SCHEMA.into(),
            id: "release-scope".into(),
            subject: "operator-1".into(),
            action: "peft.release".into(),
            target: "/models/model-a/release".into(),
            policy_sha256: "a".repeat(64),
            generation: 2,
            expires_unix_ms: 200_000,
            max_job_units: 1,
            max_total_units: 4,
            max_concurrent: 1,
            max_retries: 1,
            cooldown_ms: 0,
            fact_max_age_ms: 5_000,
            decision_ttl_ms: 5_000,
        };
        let facts = AdmissionFacts {
            observed_unix_ms: 100_000,
            state_epoch: 7,
            resource_generation: 8,
            policy_sha256: scope.policy_sha256.clone(),
        };
        let ticket = HostTicketSpec {
            schema: HOST_TICKET_V2_SCHEMA.into(),
            id: "release-ticket-1".into(),
            idempotency_key: "release-once-1".into(),
            action: "peft.release".into(),
            args: json!({"request_sha256":"b".repeat(64)}),
            receipt_mode: Some(host_ticket_agent::ReceiptMode::Worker),
            operation_id: Some("release-op-1".into()),
            subject_ref: Some("model-a".into()),
            receipt_worker_role: Some("worker-lora".into()),
            receipt_worker_id: Some("worker-1".into()),
            receipt_supervisor_generation: Some(1),
            receipt_cap_generation: Some(2),
            ..HostTicketSpec::default()
        };
        let request = prepare_selected_request(&scope, "operator-1", &facts, ticket.clone())
            .expect("fresh selected release request");
        assert_eq!(request.binding.admission_id, "release-ticket-1");
        assert_eq!(request.binding.input_sha256, "b".repeat(64));
        assert_eq!(request.binding.deadline_unix_ms, 105_000);
        assert_eq!(request.ticket.expires_unix_ms, Some(105_000));
        assert!(request.ticket.admission.is_none());
        assert!(prepare_selected_request(&scope, "other", &facts, ticket.clone()).is_err());
        let mut supplied_grant = ticket;
        supplied_grant.admission = build_selected_ticket(
            &request.binding,
            request.ticket.clone(),
            105_000,
            scope.generation,
        )
        .unwrap()
        .0
        .admission;
        assert!(prepare_selected_request(&scope, "operator-1", &facts, supplied_grant).is_err());
    }

    fn service() -> (JobBinding, HostTicketSpec) {
        let args = json!({"unit":"cohesix-agent.service"});
        let input_sha256 = hex::encode(Sha256::digest(serde_json::to_vec(&args).unwrap()));
        let binding = JobBinding {
            schema: JOB_BINDING_SCHEMA.into(),
            scope_id: "scope-1".into(),
            admission_id: "admit-1".into(),
            ticket_id: "ticket-1".into(),
            idempotency_key: "once-1".into(),
            subject: "operator-1".into(),
            action: "systemd.restart".into(),
            target: "/host/systemd/cohesix-agent.service/restart".into(),
            input_sha256,
            policy_sha256: "a".repeat(64),
            state_epoch: 1,
            resource_generation: 2,
            deadline_unix_ms: 2000,
            units: 1,
            attempt: 1,
        };
        let ticket = HostTicketSpec {
            schema: HOST_TICKET_V1_SCHEMA.into(),
            id: binding.ticket_id.clone(),
            idempotency_key: binding.idempotency_key.clone(),
            action: binding.action.clone(),
            target: Some(binding.target.clone()),
            args,
            expires_unix_ms: Some(binding.deadline_unix_ms),
            ..HostTicketSpec::default()
        };
        (binding, ticket)
    }

    #[test]
    fn exact_authenticated_ticket_correlates_with_same_job_identity() {
        let (binding, ticket) = service();
        let (selected, line) =
            build_selected_ticket(&binding, ticket, 1900, 1).expect("selected service");
        assert_eq!(
            selected.admission.unwrap().standing_scope_id.as_deref(),
            Some("scope-1")
        );
        assert!(line.contains("\"decision_expiry\":1900"));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn changed_target_input_or_caller_supplied_grant_refuses() {
        let (binding, ticket) = service();
        let mut changed = binding.clone();
        changed.input_sha256 = "f".repeat(64);
        assert!(build_selected_ticket(&changed, ticket.clone(), 1900, 1).is_err());
        changed = binding.clone();
        changed.target = "/host/systemd/other.service/restart".into();
        assert!(build_selected_ticket(&changed, ticket.clone(), 1900, 1).is_err());
        let (selected, _) = build_selected_ticket(&binding, ticket, 1900, 1).unwrap();
        assert!(build_selected_ticket(&binding, selected, 1900, 1).is_err());
        let mut changed = binding.clone();
        changed.units = 2;
        assert!(build_selected_ticket(&changed, service().1, 1900, 1).is_err());
    }

    #[test]
    fn reconciliation_preserves_exact_target_line_digest_and_rejects_malformed_receipts() {
        let (binding, _) = service();
        let exact = json!({
            "id": binding.ticket_id,
            "idempotency_key": binding.idempotency_key,
            "admission": {"admission_id": binding.admission_id},
            "state": "succeeded",
        })
        .to_string();
        let (results, hashes) = matching_target_results(vec![exact.clone()], &binding).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(hashes, vec![hex::encode(Sha256::digest(exact.as_bytes()))]);
        assert!(matching_target_results(vec!["not-json".into()], &binding).is_err());
    }

    #[test]
    fn native_gpu_info_accepts_full_descriptor_and_rejects_rebinding() {
        let descriptor = json!({
            "id": "GPU-0",
            "name": "CUDA device",
            "memory_mb": 7485,
            "sm_count": 8,
            "driver_version": "13020",
            "runtime_version": "13020",
            "execution_identity": {
                "schema": "cohesix-gpu-device/v1",
                "device_uuid": "a".repeat(32),
                "device_ordinal": 0,
                "topology_sha256": "b".repeat(64),
                "helper_sha256": "c".repeat(64),
                "provider_graph_sha256": "d".repeat(64),
                "source_id": "gpu-bridge-host/cuda-reference",
                "source_epoch": 7,
                "source_mode": "production",
            },
        });
        let encoded = descriptor.to_string();
        assert_eq!(
            gpu_execution_identity(&encoded, "GPU-0")
                .expect("full native descriptor")
                .source_epoch,
            7
        );
        assert!(gpu_execution_identity(&encoded, "GPU-1").is_err());
        let mut fixture = descriptor.clone();
        fixture["execution_identity"]["source_mode"] = json!("fixture");
        assert!(gpu_execution_identity(&fixture.to_string(), "GPU-0").is_err());
        assert!(gpu_execution_identity(
            &encoded.replacen("\"name\":", "\"unexpected\":0,\"name\":", 1),
            "GPU-0"
        )
        .is_err());
    }
}
