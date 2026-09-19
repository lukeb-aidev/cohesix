// Author: Lukas Bower
// Purpose: Execute MAC-authenticated host-ticket GPU jobs through a private bounded Unix socket with lease heartbeats and durable non-replay recovery.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
#![allow(missing_docs)]

use crate::reference::{self, ReferenceRequest};
use anyhow::{anyhow, bail, ensure, Context, Result};
use fs2::FileExt;
use ring::hmac;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::{
    fs::{DirBuilderExt, PermissionsExt},
    net::{UnixListener, UnixStream},
};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_FRAME: usize = 16_384;
const MAX_WAL: usize = 4 * 1024 * 1024;
const MAX_JOBS: usize = 64;
const GRANT_MS: u64 = 1000;
const IO_MS: u64 = 100;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub socket: PathBuf,
    pub state_root: PathBuf,
    pub helper: PathBuf,
    pub helper_sha256: String,
    pub credential_ref: String,
    pub writer_epoch: u64,
    pub gpu_id: String,
    pub device_uuid: String,
    pub provider_graph_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mig: Option<crate::mig::Selection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_lane: Option<crate::enforcement::Lane>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let result: Self = serde_json::from_slice(&read_file(path, MAX_FRAME)?)?;
        result.validate()?;
        Ok(result)
    }

    /// Validate a configuration supplied by either a file or a Rust caller.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == "cohesix-gpu-executor-config/v1"
                && self.socket.is_absolute()
                && self.state_root.is_absolute()
                && self.helper.is_absolute()
                && self.writer_epoch > 0
                && is_hash(&self.helper_sha256)
                && self.device_uuid.len() == 32
                && self.device_uuid.bytes().all(hex_digit)
                && self.provider_graph_sha256 == graph()?,
            "invalid_executor_configuration"
        );
        cohesix_authority::validate_id(&self.gpu_id).map_err(|_| anyhow!("invalid_gpu_id"))?;
        ensure!(
            cohesix_authority::secret::resolve_reference(&self.credential_ref)?.len() >= 32,
            "executor_credential_too_short"
        );
        let contract: cohesix_authority::gpu::ExecutorContract = serde_json::from_value(
            cohesix_authority::provider::registry()?["contract"]["gpu_executor"].clone(),
        )?;
        ensure!(contract.is_supported(), "executor_registry_abi_mismatch");
        match (contract.profile.as_str(), self.mig.as_ref()) {
            ("jetson-orin-nano-jp7", None) => {}
            ("nvidia-mig-cuda13", Some(selection)) => {
                ensure!(
                    selection.cuda_uuid()? == self.device_uuid,
                    "invalid_mig_executor_identity"
                );
            }
            _ => bail!("profile_mismatch executor_selection"),
        }
        Ok(())
    }
}

/// Immutable binding copied only from the authenticated root admission snapshot.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub ticket_id: String,
    pub idempotency_key: String,
    pub action: String,
    pub operation_id: String,
    pub gpu_id: String,
    pub worker_id: String,
    pub worker_slot: u16,
    pub lease_epoch: u64,
    pub supervisor_generation: u64,
    pub cap_generation: u64,
    pub admission_sequence: u64,
    pub writer_epoch: u64,
    pub expires_unix_ms: u64,
    pub provider_graph_sha256: String,
}

impl Binding {
    fn validate(&self, config: &Config, now: u64) -> Result<()> {
        for id in [
            &self.ticket_id,
            &self.idempotency_key,
            &self.operation_id,
            &self.gpu_id,
            &self.worker_id,
        ] {
            cohesix_authority::validate_id(id).map_err(|_| anyhow!("invalid_binding_id"))?;
        }
        ensure!(
            matches!(
                self.action.as_str(),
                "gpu.workload.submit" | "gpu.workload.cancel" | "gpu.workload.observe"
            ) && self.worker_id.len() <= 32
                && self.lease_epoch > 0
                && self.supervisor_generation > 0
                && self.cap_generation > 0
                && self.admission_sequence > 0
                && self.writer_epoch == config.writer_epoch
                && self.gpu_id == config.gpu_id
                && self.provider_graph_sha256 == config.provider_graph_sha256
                && self.expires_unix_ms > now,
            "stale_or_invalid_admission"
        );
        Ok(())
    }
    fn same_resource(&self, other: &Self) -> bool {
        self.gpu_id == other.gpu_id
            && self.worker_id == other.worker_id
            && self.worker_slot == other.worker_slot
            && self.lease_epoch == other.lease_epoch
            && self.supervisor_generation == other.supervisor_generation
            && self.cap_generation == other.cap_generation
            && self.writer_epoch == other.writer_epoch
            && self.provider_graph_sha256 == other.provider_graph_sha256
    }
}

/// CAS input permits only compiled references and exact physical inventory identity.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema: String,
    pub artifact_sha256: String,
    pub topology_sha256: String,
    pub expected_output_sha256: String,
    pub request: ReferenceRequest,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Submit {
        binding: Binding,
        lease_id: String,
        lease_sequence: u64,
        request_sha256: String,
        input: Box<Input>,
    },
    Renew {
        binding: Binding,
        lease_id: String,
        lease_sequence: u64,
        serial: u64,
    },
    Observe {
        binding: Binding,
        job_id: String,
    },
    Cancel {
        binding: Binding,
        job_id: String,
    },
    /// Reconcile an admitted control request without dispatching its effect again.
    ControlStatus {
        binding: Binding,
        job_id: String,
    },
    Revoke {
        binding: Binding,
    },
    Status {
        binding: Binding,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    schema: String,
    issued_unix_ms: u64,
    command: Command,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: Payload,
    mac: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub binding: Binding,
    pub lease_id: String,
    pub lease_sequence: u64,
    pub input_sha256: String,
    pub state: String,
    pub detail: String,
    pub observation: Option<Value>,
    pub terminal_unix_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Wal {
    schema: String,
    config_sha256: String,
    jobs: BTreeMap<String, Job>,
}
struct Active {
    id: String,
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<Result<Value>>,
    worker: thread::JoinHandle<()>,
    grant: Instant,
    serial: u64,
}

pub fn now_ms() -> Result<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn hex_digit(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
}
fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(hex_digit)
}
fn graph() -> Result<String> {
    Ok(cohesix_authority::provider::registry()?["graph_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid_registry"))?
        .to_owned())
}

pub fn read_file(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let info = fs::symlink_metadata(path)?;
    ensure!(
        info.is_file() && info.len() <= maximum as u64,
        "invalid_file_kind_or_bound"
    );
    let mut bytes = Vec::new();
    File::open(path)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= maximum, "file_limit");
    Ok(bytes)
}

fn private_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::DirBuilder::new().mode(0o700).create(path)?;
    }
    let info = fs::symlink_metadata(path)?;
    ensure!(
        info.is_dir() && !info.file_type().is_symlink() && info.permissions().mode() & 0o077 == 0,
        "private_directory_required"
    );
    Ok(())
}
fn persist(path: &Path, wal: &Wal) -> Result<()> {
    let bytes = serde_json::to_vec(wal)?;
    ensure!(bytes.len() <= MAX_WAL, "wal_capacity");
    let parent = path.parent().ok_or_else(|| anyhow!("state_parent"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

/// Stable topology identity includes the exact helper and native immutable device attributes.
pub fn topology_hash(observation: &Value, helper_sha256: &str) -> Result<String> {
    let n = &observation["native"];
    let mut fields = BTreeMap::new();
    for name in [
        "device_uuid",
        "device_ordinal",
        "total_bytes",
        "sm_count",
        "compute_major",
        "compute_minor",
        "runtime_version",
        "driver_version",
        "integrated",
    ] {
        let value = n
            .get(name)
            .ok_or_else(|| anyhow!("native_inventory_missing_field {name}"))?;
        fields.insert(name, value.clone());
    }
    fields.insert("helper_sha256", Value::String(helper_sha256.to_owned()));
    if let Some(mig) = observation.get("mig") {
        fields.insert("mig", mig.clone());
    }
    // A host boot fences device-enumeration changes even if its ordinal is reused.
    fields.insert(
        "boot_id",
        Value::String(
            String::from_utf8(read_file(Path::new("/proc/sys/kernel/random/boot_id"), 64)?)?
                .trim()
                .to_owned(),
        ),
    );
    Ok(digest(&serde_json::to_vec(&fields)?))
}

fn encode(command: Command, key: &[u8], now: u64) -> Result<Vec<u8>> {
    let payload = Payload {
        schema: "cohesix-gpu-local/v1".into(),
        issued_unix_ms: now,
        command,
    };
    let mac = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, key),
        &serde_json::to_vec(&payload)?,
    );
    let bytes = serde_json::to_vec(&Envelope {
        payload,
        mac: hex::encode(mac.as_ref()),
    })?;
    ensure!(bytes.len() <= MAX_FRAME, "request_limit");
    Ok(bytes)
}
fn decode(bytes: &[u8], key: &[u8], now: u64) -> Result<Command> {
    ensure!(bytes.len() <= MAX_FRAME, "request_limit");
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    ensure!(
        envelope.payload.schema == "cohesix-gpu-local/v1"
            && envelope.payload.issued_unix_ms <= now
            && now - envelope.payload.issued_unix_ms < GRANT_MS,
        "expired_local_message"
    );
    hmac::verify(
        &hmac::Key::new(hmac::HMAC_SHA256, key),
        &serde_json::to_vec(&envelope.payload)?,
        &hex::decode(&envelope.mac).map_err(|_| anyhow!("invalid_mac"))?,
    )
    .map_err(|_| anyhow!("unauthorized_local_message"))?;
    Ok(envelope.payload.command)
}

fn read_exact_deadline(stream: &mut UnixStream, bytes: &mut [u8], deadline: Instant) -> Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| anyhow!("local_deadline"))?;
        stream.set_read_timeout(Some(remaining))?;
        let n = stream.read(&mut bytes[offset..])?;
        ensure!(n > 0, "local_connection_closed");
        offset += n;
    }
    Ok(())
}
fn read_frame(stream: &mut UnixStream, deadline: Instant) -> Result<Vec<u8>> {
    let mut prefix = [0u8; 4];
    read_exact_deadline(stream, &mut prefix, deadline)?;
    let length = u32::from_be_bytes(prefix) as usize;
    ensure!(length > 0 && length <= MAX_FRAME, "local_frame_limit");
    let mut bytes = vec![0; length];
    read_exact_deadline(stream, &mut bytes, deadline)?;
    Ok(bytes)
}
fn write_frame(stream: &mut UnixStream, bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() <= MAX_FRAME, "local_frame_limit");
    stream.set_write_timeout(Some(Duration::from_millis(IO_MS)))?;
    // One finite frame; progress never refreshes the deadline.
    let mut frame = (bytes.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(bytes);
    let deadline = Instant::now() + Duration::from_millis(IO_MS);
    let mut offset = 0;
    while offset < frame.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| anyhow!("local_deadline"))?;
        stream.set_write_timeout(Some(remaining))?;
        let count = stream.write(&frame[offset..])?;
        ensure!(count > 0, "local_connection_closed");
        offset += count;
    }
    Ok(())
}

/// Only the ticket-agent credential custodian can send accepted workload commands.
pub fn call(socket: &Path, credential_ref: &str, command: Command) -> Result<Job> {
    let key = cohesix_authority::secret::resolve_reference(credential_ref)?;
    let bytes = encode(command, key.as_bytes(), now_ms()?)?;
    let request_sha256 = digest(&bytes);
    let mut stream = UnixStream::connect(socket).context("not_enabled GPU executor socket")?;
    write_frame(&mut stream, &bytes)?;
    let bytes = read_frame(&mut stream, Instant::now() + Duration::from_millis(500))?;
    let signed: Value = serde_json::from_slice(&bytes)?;
    let response = &signed["response"];
    ensure!(
        response["request_sha256"] == request_sha256,
        "response_request_mismatch"
    );
    let mac = hex::decode(
        signed["mac"]
            .as_str()
            .ok_or_else(|| anyhow!("response_mac_missing"))?,
    )
    .map_err(|_| anyhow!("response_mac_invalid"))?;
    hmac::verify(
        &hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
        &serde_json::to_vec(response)?,
        &mac,
    )
    .map_err(|_| anyhow!("response_unauthenticated"))?;
    if response["ok"] != true {
        bail!(
            "GPU executor refusal: {}",
            response["code"].as_str().unwrap_or("invalid_response")
        );
    }
    Ok(serde_json::from_value(response["job"].clone())?)
}

/// Own one CUDA context at a time. Pending jobs are never evicted or replayed.
pub fn serve(config: Config) -> Result<()> {
    config.validate()?;
    private_directory(&config.state_root)?;
    let socket_parent = config
        .socket
        .parent()
        .ok_or_else(|| anyhow!("socket_parent"))?;
    private_directory(socket_parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(config.state_root.join("owner.lock"))?;
    lock.try_lock_exclusive()
        .context("GPU executor already owned")?;
    let key = cohesix_authority::secret::resolve_reference(&config.credential_ref)?;
    ensure!(
        digest(&read_file(&config.helper, 16 * 1024 * 1024)?) == config.helper_sha256,
        "helper_digest_mismatch"
    );
    let wal_path = config.state_root.join("jobs.json");
    let config_hash = digest(&serde_json::to_vec(&config)?);
    let mut wal = if wal_path.exists() {
        serde_json::from_slice::<Wal>(&read_file(&wal_path, MAX_WAL)?)?
    } else {
        Wal {
            schema: "cohesix-gpu-journal/v1".into(),
            config_sha256: config_hash.clone(),
            jobs: BTreeMap::new(),
        }
    };
    ensure!(
        wal.schema == "cohesix-gpu-journal/v1"
            && wal.config_sha256 == config_hash
            && wal.jobs.len() <= MAX_JOBS,
        "executor_state_binding"
    );
    for (id, job) in &mut wal.jobs {
        job.binding
            .validate(&config, job.binding.expires_unix_ms.saturating_sub(1))?;
        ensure!(
            id == &job.binding.ticket_id
                && is_hash(&job.input_sha256)
                && matches!(
                    job.state.as_str(),
                    "running" | "succeeded" | "failed" | "cancelled" | "revoked" | "interrupted"
                ),
            "invalid_job_journal"
        );
        let input = read_file(&config.state_root.join(id).join("input.json"), 8192)?;
        ensure!(
            digest(&input) == job.input_sha256,
            "retained_input_mismatch"
        );
        if job.terminal_unix_ms.is_none() {
            job.state = "interrupted".into();
            job.detail = "bridge_restart_no_replay".into();
            job.terminal_unix_ms = Some(now_ms()?);
        }
        if job.state == "succeeded" {
            verify_retained_output(&config, job)?;
        }
    }
    persist(&wal_path, &wal)?;
    if config.socket.exists() {
        use std::os::unix::fs::FileTypeExt;
        ensure!(
            fs::symlink_metadata(&config.socket)?
                .file_type()
                .is_socket(),
            "socket_kind"
        );
        fs::remove_file(&config.socket)?;
    }
    let listener = UnixListener::bind(&config.socket)?;
    fs::set_permissions(&config.socket, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let mut active: Option<Active> = None;
    loop {
        if let Some(current) = &active {
            if Instant::now() >= current.grant
                || now_ms()? >= wal.jobs[&current.id].binding.expires_unix_ms
            {
                current.cancel.store(true, Ordering::Release);
                let job = wal
                    .jobs
                    .get_mut(&current.id)
                    .ok_or_else(|| anyhow!("active_job_missing"))?;
                if job.state == "running" {
                    job.state = "revoked".into();
                    job.detail = "lease_heartbeat_expired".into();
                    persist(&wal_path, &wal)?;
                }
            }
            match current.result.try_recv() {
                Ok(result) => {
                    let current = active.take().ok_or_else(|| anyhow!("active_job_missing"))?;
                    current
                        .worker
                        .join()
                        .map_err(|_| anyhow!("executor_owner_failed"))?;
                    let job = wal
                        .jobs
                        .get_mut(&current.id)
                        .ok_or_else(|| anyhow!("active_job_missing"))?;
                    if job.state == "running" {
                        match result {
                            Ok(value) => {
                                job.state = "succeeded".into();
                                job.detail = "cuda_output_verified".into();
                                job.observation = Some(value);
                            }
                            Err(error) => {
                                job.state = "failed".into();
                                job.detail = failure_code(&error);
                            }
                        }
                    }
                    job.terminal_unix_ms = Some(now_ms()?);
                    persist(&wal_path, &wal)?;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => bail!("executor_owner_disconnected"),
            }
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut request_sha256 = String::new();
                let result = (|| {
                    let bytes =
                        read_frame(&mut stream, Instant::now() + Duration::from_millis(IO_MS))?;
                    request_sha256 = digest(&bytes);
                    let command = decode(&bytes, key.as_bytes(), now_ms()?)?;
                    handle(command, &config, &mut wal, &wal_path, &mut active)
                })();
                let response = match result {
                    Ok(job) => {
                        serde_json::json!({"ok":true,"job":job,"request_sha256":request_sha256})
                    }
                    Err(error) => {
                        serde_json::json!({"ok":false,"code":failure_code(&error),"request_sha256":request_sha256})
                    }
                };
                // A lost client ACK cannot change durable execution or trigger a replay.
                let mac = hmac::sign(
                    &hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
                    &serde_json::to_vec(&response)?,
                );
                let _ = write_frame(
                    &mut stream,
                    &serde_json::to_vec(
                        &serde_json::json!({"response":response,"mac":hex::encode(mac.as_ref())}),
                    )?,
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn failure_code(error: &anyhow::Error) -> String {
    let text = error.to_string();
    let code = text.split_whitespace().next().unwrap_or("provider_failure");
    if code.len() <= 96
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        code.into()
    } else {
        "provider_failure".into()
    }
}
fn verify_retained_output(config: &Config, job: &Job) -> Result<()> {
    let output = job
        .observation
        .as_ref()
        .ok_or_else(|| anyhow!("terminal_observation_missing"))?;
    let bytes = read_file(
        &config
            .state_root
            .join(&job.binding.ticket_id)
            .join("execution/output.bin"),
        262_144,
    )?;
    ensure!(
        output["output"]["sha256"] == digest(&bytes) && output["output"]["bytes"] == bytes.len(),
        "retained_output_mismatch"
    );
    Ok(())
}

fn validate_control_action(command: &Command) -> Result<()> {
    let selected = match command {
        Command::Observe { binding, .. } => Some((binding, "gpu.workload.observe")),
        Command::Cancel { binding, .. } => Some((binding, "gpu.workload.cancel")),
        Command::ControlStatus { binding, .. } => {
            ensure!(
                matches!(
                    binding.action.as_str(),
                    "gpu.workload.observe" | "gpu.workload.cancel"
                ),
                "action_mismatch"
            );
            None
        }
        _ => None,
    };
    if let Some((binding, action)) = selected {
        ensure!(binding.action == action, "action_mismatch");
    }
    Ok(())
}

fn handle(
    command: Command,
    config: &Config,
    wal: &mut Wal,
    wal_path: &Path,
    active: &mut Option<Active>,
) -> Result<Job> {
    validate_control_action(&command)?;
    let dispatch_cancel = matches!(&command, Command::Cancel { .. });
    match command {
        Command::Submit {
            binding,
            lease_id,
            lease_sequence,
            request_sha256,
            input,
        } => {
            binding.validate(config, now_ms()?)?;
            ensure!(binding.action == "gpu.workload.submit", "action_mismatch");
            ensure!(
                cohesix_authority::validate_id(&lease_id).is_ok()
                    && lease_id.len() <= 32
                    && lease_sequence > 0,
                "invalid_lease"
            );
            let input_bytes = serde_json::to_vec(&input)?;
            ensure!(
                is_hash(&request_sha256)
                    && digest(&input_bytes) == request_sha256
                    && input_bytes.len() <= 8192,
                "request_cas_mismatch"
            );
            if let Some(job) = wal.jobs.get(&binding.ticket_id) {
                ensure!(
                    job.binding == binding
                        && job.input_sha256 == request_sha256
                        && job.lease_id == lease_id
                        && job.lease_sequence == lease_sequence,
                    "idempotency_conflict"
                );
                if job.state == "succeeded" {
                    verify_retained_output(config, job)?;
                }
                return Ok(job.clone());
            }
            ensure!(active.is_none(), "device_busy");
            ensure!(wal.jobs.len() < MAX_JOBS, "job_retention_backpressure");
            ensure!(
                input.schema == "cohesix-gpu-workload-input/v1"
                    && input.artifact_sha256 == config.helper_sha256
                    && is_hash(&input.topology_sha256)
                    && is_hash(&input.expected_output_sha256)
                    && input.request.ticket_id == binding.ticket_id
                    && input.request.device_uuid == config.device_uuid
                    && input.request.device_ordinal == 0,
                "workload_identity_mismatch"
            );
            input
                .request
                .validate(now_ms()?, &config.provider_graph_sha256)?;
            ensure!(
                now_ms()?
                    .checked_add(u64::from(input.request.deadline_ms))
                    .is_some_and(|end| end < binding.expires_unix_ms),
                "ticket_ttl_too_short"
            );
            let job = Job {
                binding: binding.clone(),
                lease_id,
                lease_sequence,
                input_sha256: request_sha256,
                state: "running".into(),
                detail: "admitted_before_dispatch".into(),
                observation: None,
                terminal_unix_ms: None,
            };
            let state = config.state_root.join(&binding.ticket_id);
            // Fresh directory and WAL precede every native call. A crash here is interrupted, never replayed.
            private_directory(&state)?;
            ensure!(fs::read_dir(&state)?.next().is_none(), "job_state_conflict");
            let mut input_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(state.join("input.json"))?;
            input_file.write_all(&input_bytes)?;
            input_file.sync_all()?;
            File::open(&state)?.sync_all()?;
            wal.jobs.insert(binding.ticket_id.clone(), job.clone());
            persist(wal_path, wal)?;
            let cancel = Arc::new(AtomicBool::new(false));
            let flag = cancel.clone();
            let config = config.clone();
            let (sender, result) = mpsc::sync_channel(1);
            let worker = thread::spawn(move || {
                let result = (|| {
                    let enforcement = crate::enforcement::observe(config.execution_lane.as_ref())?;
                    let observation = reference::inventory_selected(
                        &config.helper,
                        &config.helper_sha256,
                        &state.join("inventory"),
                        0,
                        config.mig.as_ref(),
                    )?;
                    ensure!(
                        topology_hash(&observation, &config.helper_sha256)?
                            == input.topology_sha256,
                        "stale_topology"
                    );
                    ensure!(!flag.load(Ordering::Acquire), "revoked_before_dispatch");
                    let mut observation = reference::execute_selected(
                        &config.helper,
                        &config.helper_sha256,
                        &state.join("execution"),
                        &input.request,
                        &flag,
                        config.mig.as_ref(),
                    )?;
                    ensure!(
                        observation["output"]["sha256"] == input.expected_output_sha256,
                        "output_hash_mismatch"
                    );
                    ensure!(
                        enforcement == crate::enforcement::observe(config.execution_lane.as_ref())?,
                        "native_enforcement_changed"
                    );
                    observation["native_enforcement"] = enforcement;
                    Ok(observation)
                })();
                let _ = sender.send(result);
            });
            *active = Some(Active {
                id: binding.ticket_id,
                cancel,
                result,
                worker,
                grant: Instant::now() + Duration::from_millis(GRANT_MS),
                serial: 0,
            });
            Ok(job)
        }
        Command::Renew {
            binding,
            lease_id,
            lease_sequence,
            serial,
        } => {
            binding.validate(config, now_ms()?)?;
            let job = wal
                .jobs
                .get(&binding.ticket_id)
                .ok_or_else(|| anyhow!("job_not_found"))?;
            ensure!(
                job.binding == binding
                    && job.lease_id == lease_id
                    && job.lease_sequence == lease_sequence,
                "lease_or_admission_changed"
            );
            if let Some(current) = active.as_mut().filter(|a| a.id == binding.ticket_id) {
                ensure!(
                    job.state == "running" && serial > current.serial,
                    "stale_lease_heartbeat"
                );
                current.serial = serial;
                current.grant = Instant::now() + Duration::from_millis(GRANT_MS);
            }
            Ok(job.clone())
        }
        Command::Observe { binding, job_id }
        | Command::Cancel { binding, job_id }
        | Command::ControlStatus { binding, job_id } => {
            binding.validate(config, now_ms()?)?;
            ensure!(
                matches!(
                    binding.action.as_str(),
                    "gpu.workload.observe" | "gpu.workload.cancel"
                ),
                "action_mismatch"
            );
            let job = wal
                .jobs
                .get_mut(&job_id)
                .ok_or_else(|| anyhow!("job_not_found"))?;
            ensure!(binding.same_resource(&job.binding), "job_binding_mismatch");
            if dispatch_cancel && job.state == "running" {
                let current = active
                    .as_ref()
                    .filter(|a| a.id == job_id)
                    .ok_or_else(|| anyhow!("job_owner_missing"))?;
                current.cancel.store(true, Ordering::Release);
                job.state = "cancelled".into();
                job.detail = "authorized_cancel_waiting_for_reap".into();
            }
            if job.state == "succeeded" {
                verify_retained_output(config, job)?;
            }
            let result = job.clone();
            if dispatch_cancel {
                persist(wal_path, wal)?;
            }
            Ok(result)
        }
        Command::Status { binding } => {
            let job = wal
                .jobs
                .get(&binding.ticket_id)
                .ok_or_else(|| anyhow!("job_not_found"))?;
            ensure!(job.binding == binding, "status_binding_mismatch");
            if job.state == "succeeded" {
                verify_retained_output(config, job)?;
            }
            Ok(job.clone())
        }
        Command::Revoke { binding } => {
            // A custodian can stop a job even after its ticket expires, but cannot change its identity.
            let job = wal
                .jobs
                .get_mut(&binding.ticket_id)
                .ok_or_else(|| anyhow!("job_not_found"))?;
            ensure!(job.binding == binding, "revoke_binding_mismatch");
            if let Some(current) = active.as_ref().filter(|a| a.id == binding.ticket_id) {
                current.cancel.store(true, Ordering::Release);
                if job.state == "running" {
                    job.state = "revoked".into();
                    job.detail = "root_lease_or_worker_revoked".into();
                }
            }
            let result = job.clone();
            persist(wal_path, wal)?;
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> Binding {
        Binding {
            ticket_id: "job".into(),
            idempotency_key: "once".into(),
            action: "gpu.workload.submit".into(),
            operation_id: "work".into(),
            gpu_id: "GPU-0".into(),
            worker_id: "gpu-worker-1".into(),
            worker_slot: 0,
            lease_epoch: 1,
            supervisor_generation: 2,
            cap_generation: 3,
            admission_sequence: 4,
            writer_epoch: 1,
            expires_unix_ms: 9000,
            provider_graph_sha256: "a".repeat(64),
        }
    }
    #[test]
    fn authenticated_local_frames_bind_every_byte_and_expire() {
        let bytes = encode(
            Command::Revoke { binding: binding() },
            b"fixture-key-for-unit-test",
            1000,
        )
        .unwrap();
        assert!(decode(&bytes, b"fixture-key-for-unit-test", 1001).is_ok());
        assert!(decode(&bytes, b"wrong-key", 1001).is_err());
        assert!(decode(&bytes, b"fixture-key-for-unit-test", 2000).is_err());
        assert!(decode(&bytes, b"fixture-key-for-unit-test", 999).is_err());
        let bytes = String::from_utf8(bytes).unwrap().replace("GPU-0", "GPU-1");
        assert!(decode(bytes.as_bytes(), b"fixture-key-for-unit-test", 1001).is_err());
        let mut changed = binding();
        changed.lease_epoch += 1;
        assert!(!binding().same_resource(&changed));
    }

    #[test]
    fn observe_envelope_cannot_dispatch_an_admitted_cancel() {
        let mut admission = binding();
        admission.action = "gpu.workload.cancel".into();
        assert!(validate_control_action(&Command::Cancel {
            binding: admission.clone(),
            job_id: "job".into(),
        })
        .is_ok());
        assert!(validate_control_action(&Command::Observe {
            binding: admission,
            job_id: "job".into(),
        })
        .is_err());
    }

    #[test]
    fn cancel_reconciliation_is_read_only_and_preserves_worker_fence() {
        let mut original = binding();
        original.expires_unix_ms = u64::MAX;
        let job = Job {
            binding: original.clone(),
            lease_id: "lease".into(),
            lease_sequence: 1,
            input_sha256: "b".repeat(64),
            state: "running".into(),
            detail: "native child active".into(),
            observation: None,
            terminal_unix_ms: None,
        };
        let config = Config {
            schema: "cohesix-gpu-executor-config/v1".into(),
            socket: "/unused".into(),
            state_root: "/unused".into(),
            helper: "/unused".into(),
            helper_sha256: "c".repeat(64),
            credential_ref: "unused".into(),
            writer_epoch: 1,
            gpu_id: "GPU-0".into(),
            device_uuid: "d".repeat(32),
            provider_graph_sha256: original.provider_graph_sha256.clone(),
            mig: None,
            execution_lane: None,
        };
        let mut wal = Wal {
            schema: "cohesix-gpu-workload-wal/v1".into(),
            config_sha256: "e".repeat(64),
            jobs: BTreeMap::from([("job".into(), job)]),
        };
        let before = serde_json::to_vec(&wal).unwrap();
        original.ticket_id = "cancel-ticket".into();
        original.action = "gpu.workload.cancel".into();
        let mut active = None;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wal.json");
        let observed = handle(
            Command::ControlStatus {
                binding: original.clone(),
                job_id: "job".into(),
            },
            &config,
            &mut wal,
            &path,
            &mut active,
        )
        .unwrap();
        assert_eq!(observed.state, "running");
        assert_eq!(serde_json::to_vec(&wal).unwrap(), before);
        assert!(!path.exists());
        original.cap_generation += 1;
        assert!(handle(
            Command::ControlStatus {
                binding: original,
                job_id: "job".into()
            },
            &config,
            &mut wal,
            &path,
            &mut active,
        )
        .is_err());
        assert_eq!(serde_json::to_vec(&wal).unwrap(), before);
    }
}
