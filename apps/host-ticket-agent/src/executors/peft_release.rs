// Author: Lukas Bower
// Purpose: Drive admitted PEFT release phases through the existing native systemd executor, retaining original identity and rechecking Root authority.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use super::{observation, ExecutorConfig, ReconcileOutcome};
use crate::{HostTicketSpec, HOST_TICKET_V2_SCHEMA};
use anyhow::{anyhow, ensure, Result};
use coh::peft::{
    release::DeploymentState,
    transaction::{self, Native, Observation, Phase, Request, State},
};
use cohesix_authority::peft::ReleaseArgs;
use cohsh::{Session, Transport};
use fs2::FileExt;
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    python: PathBuf,
    helper: PathBuf,
    helper_sha256: String,
    native_config: PathBuf,
}

fn read(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    ensure!(path.is_absolute(), "EPERM release-absolute-path");
    for ancestor in path.ancestors() {
        ensure!(
            !fs::symlink_metadata(ancestor)?.file_type().is_symlink(),
            "EPERM release-symlink"
        );
    }
    gpu_bridge_host::workload::read_file(path, maximum)
}

fn quiesce(operation: &Path, ticket_id: &str, include_rollback: bool) -> Result<()> {
    // A systemd-run client may outlive its crashed parent and submit late. The
    // durable fence prevents any delayed forward helper from mutating runtime.
    crate::wal::durable_atomic_write(
        &operation.join("forward-cancelled.json"),
        br#"{"cancelled":true}"#,
        8192,
        "native forward cancellation",
    )?;
    for phase in [
        "validate", "train", "evaluate", "scan", "stage", "load", "canary", "promote", "rollback",
    ] {
        if phase == "rollback" && !include_rollback {
            continue;
        }
        let unit = format!(
            "cohesix-lora-phase-{}-{phase}.service",
            &cohesix_evidence::digest(ticket_id.as_bytes())[..16]
        );
        let state = Command::new("systemctl")
            .args(["--user", "show", &unit, "--property=LoadState", "--value"])
            .output()?;
        if state.stdout == b"not-found\n" {
            continue;
        }
        ensure!(state.status.success(), "ambiguous original-native-unit");
        ensure!(
            Command::new("systemctl")
                .args(["--user", "stop", &unit])
                .status()?
                .success(),
            "ambiguous original-native-stop"
        );
        let stopped = Command::new("systemctl")
            .args(["--user", "show", &unit, "--property=ActiveState", "--value"])
            .output()?;
        ensure!(
            stopped.stdout == b"inactive\n" || stopped.stdout == b"failed\n",
            "ambiguous original-native-not-quiescent"
        );
    }
    Ok(())
}

struct Adapter<'a> {
    config: Config,
    root: PathBuf,
    operation: PathBuf,
    observation_directory: PathBuf,
    recovery_id: Option<String>,
    request: PathBuf,
    spec: &'a HostTicketSpec,
    transport: &'a mut dyn Transport,
    session: &'a Session,
}

impl Native for Adapter<'_> {
    fn now_ms(&self) -> Result<u64> {
        Ok(crate::unix_time_ms_now())
    }
    fn accepted(&mut self) -> Result<DeploymentState> {
        Ok(serde_json::from_slice(&read(
            &self.root.join("accepted.json"),
            8192,
        )?)?)
    }
    fn authorize(&mut self, _phase: Phase) -> Result<()> {
        ensure!(
            self.spec
                .expires_unix_ms
                .is_some_and(|expiry| self.now_ms().is_ok_and(|now| now < expiry)),
            "EPERM release-authority-expired"
        );
        crate::validate_ready_worker_binding(self.transport, self.session, self.spec)
    }
    fn reconcile(&mut self, phase: Phase) -> Result<Option<Observation>> {
        let path = self
            .observation_directory
            .join(format!("{}.json", phase.name()));
        if !path.try_exists()? {
            return Ok(None);
        }
        Ok(Some(serde_json::from_slice(&read(&path, 16384)?)?))
    }
    fn execute(&mut self, phase: Phase) -> Result<Observation> {
        self.authorize(phase)?;
        if phase == Phase::Rollback {
            quiesce(&self.operation, &self.spec.id, false)?;
            self.authorize(phase)?;
        }
        ensure!(
            cohesix_evidence::digest(&read(&self.config.helper, 262144)?)
                == self.config.helper_sha256,
            "EPERM native-helper-digest"
        );
        let unit = format!(
            "cohesix-lora-phase-{}-{}",
            &cohesix_evidence::digest(self.spec.id.as_bytes())[..16],
            phase.name()
        );
        let log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(
                self.observation_directory
                    .join(format!("{}.native.log", phase.name())),
            )?;
        let mut command = Command::new("systemd-run");
        let expiry = self
            .spec
            .expires_unix_ms
            .ok_or_else(|| anyhow!("EPERM release-expiry"))?;
        let remaining_ms = expiry
            .checked_sub(self.now_ms()?)
            .filter(|remaining| *remaining > 0)
            .ok_or_else(|| anyhow!("EPERM release-authority-expired"))?
            .min(300_000);
        command
            .args([
                "--user",
                "--wait",
                "--collect",
                "--quiet",
                "--service-type=exec",
                "--unit",
                &unit,
            ])
            .args([
                "--property=MemoryMax=3221225472",
                "--property=TasksMax=128",
                "--property=CPUQuota=200%",
                "--property=LimitFSIZE=67108864",
                "--property=NoNewPrivileges=yes",
            ])
            .arg(format!("--property=RuntimeMaxSec={remaining_ms}ms"))
            .arg(&self.config.python)
            .arg("-I")
            .arg(&self.config.helper)
            .arg("--config")
            .arg(&self.config.native_config)
            .arg("--request")
            .arg(&self.request)
            .arg("--phase")
            .arg(phase.name())
            .arg("--authority-expires-unix-ms")
            .arg(expiry.to_string())
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        if let Some(id) = &self.recovery_id {
            command.arg("--recovery-id").arg(id);
        }
        let mut child = command.spawn()?;
        let deadline = Instant::now() + Duration::from_millis(remaining_ms);
        loop {
            if let Some(status) = child.try_wait()? {
                ensure!(status.success(), "failed native-phase-process");
                return self
                    .reconcile(phase)?
                    .ok_or_else(|| anyhow!("ambiguous missing-native-completion"));
            }
            let authority = self.authorize(phase);
            if authority.is_err() || Instant::now() >= deadline {
                let stopped = Command::new("systemctl")
                    .args(["--user", "stop", &unit])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()?;
                ensure!(stopped.success(), "ambiguous native-phase-stop");
                let _status = child.wait()?;
                authority?;
                return Err(anyhow!("expired native-phase-deadline"));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

fn run(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    executor: &ExecutorConfig,
    recovery: bool,
) -> Result<transaction::Journal> {
    ensure!(
        cfg!(target_os = "linux"),
        "not_supported native-HF-requires-Linux-CUDA"
    );
    ensure!(
        spec.schema == HOST_TICKET_V2_SCHEMA && spec.action == "peft.release",
        "EPERM release-admission"
    );
    ensure!(
        executor.evidence_enrollment_dir.is_some()
            && executor.worker_evidence_enrollment_dir.is_some(),
        "not_enabled independent-native-and-Worker-evidence"
    );
    crate::claim::validate_spec(spec, crate::claim::SpecSource::AdmittedSnapshot)?;
    let path = executor
        .peft_release_config
        .as_deref()
        .ok_or_else(|| anyhow!("not_enabled native-PEFT-profile"))?;
    let config: Config = serde_json::from_slice(&read(path, 8192)?)?;
    ensure!(
        config.python.is_absolute() && config.helper.is_absolute(),
        "EPERM native-executable-path"
    );
    let native: Value = serde_json::from_slice(&read(&config.native_config, 8192)?)?;
    let root = PathBuf::from(
        native["root"]
            .as_str()
            .ok_or_else(|| anyhow!("invalid_config root"))?,
    );
    ensure!(
        root.is_absolute()
            && root.canonicalize()? == root
            && fs::metadata(&root)?.permissions().mode() & 0o077 == 0,
        "EPERM private-native-root"
    );
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(root.join("lock"))?;
    lock.try_lock_exclusive()
        .map_err(|_| anyhow!("EBUSY release-registry"))?;
    let args: ReleaseArgs = serde_json::from_value(spec.args.clone())?;
    let request_path = root.join("objects").join(&args.request_sha256);
    let bytes = read(&request_path, 8192)?;
    ensure!(
        cohesix_evidence::digest(&bytes) == args.request_sha256,
        "EPERM release-input-digest"
    );
    let request: Request = serde_json::from_slice(&bytes)?;
    ensure!(
        serde_json::to_vec(&request)? == bytes
            && spec.operation_id.as_ref() == Some(&request.operation_id)
            && spec.subject_ref.as_ref() == Some(&request.model_id),
        "EPERM release-input-binding"
    );
    cohesix_authority::validate_id(&request.operation_id)
        .map_err(|_| anyhow!("invalid_request operation-id"))?;
    let operation = root.join("operations").join(&request.operation_id);
    ensure!(
        operation.try_exists()? || fs::read_dir(root.join("operations"))?.count() < 256,
        "ELIMIT release-operation-capacity"
    );
    if operation.join("recipe.json").try_exists()? {
        let previous = transaction::inspect(&operation)?;
        ensure!(
            previous.request_sha256 == args.request_sha256,
            "conflict release-journal-input"
        );
    }
    let active = root.join("active-release.json");
    if active.try_exists()? {
        let owner: Value = serde_json::from_slice(&read(&active, 8192)?)?;
        let operation_id = owner["operation_id"]
            .as_str()
            .ok_or_else(|| anyhow!("invalid_state release-owner"))?;
        cohesix_authority::validate_id(operation_id)
            .map_err(|_| anyhow!("invalid_state release-owner"))?;
        if owner["request_sha256"] != args.request_sha256 {
            let previous_path = root.join("operations").join(operation_id);
            // No native dispatch can precede its durable recipe intent. A crash
            // between registry ownership and journal creation is known unexecuted.
            if previous_path.join("recipe.json").try_exists()? {
                let previous = transaction::inspect(&previous_path)?;
                ensure!(
                    owner["request_sha256"] == previous.request_sha256
                        && matches!(
                            previous.state,
                            State::Succeeded | State::Failed | State::RecoveredFailure
                        ),
                    "EBUSY unresolved-native-release"
                );
            }
            fs::remove_file(&active)?;
            fs::File::open(&root)?.sync_all()?;
        }
    }
    let blocked = root.join("release-blocked.json");
    if blocked.try_exists()? {
        let old: transaction::Journal = serde_json::from_slice(&read(&blocked, 65536)?)?;
        ensure!(
            args.recovery_only && old.request_sha256 == args.request_sha256,
            "EPERM unresolved-runtime-recovery"
        );
    }
    let observation_directory = if args.recovery_only {
        operation.join(format!("recovery-{}", spec.id))
    } else {
        operation.clone()
    };
    if args.recovery_only {
        let original = transaction::inspect(&operation)?;
        ensure!(
            original.ticket_id != spec.id,
            "EPERM fresh-recovery-authority-required"
        );
        ensure!(
            spec.expires_unix_ms
                .is_some_and(|expiry| crate::unix_time_ms_now() < expiry),
            "EPERM recovery-authority-expired"
        );
        crate::validate_ready_worker_binding(transport, session, spec)?;
        original.require_recovery_scope(&request)?;
        quiesce(&operation, &original.ticket_id, true)?;
    }
    let mut adapter = Adapter {
        config,
        root: root.clone(),
        operation: operation.clone(),
        observation_directory,
        recovery_id: args.recovery_only.then(|| spec.id.clone()),
        request: request_path,
        spec,
        transport,
        session,
    };
    crate::wal::durable_atomic_write(
        &active,
        &serde_json::to_vec(&serde_json::json!({
            "operation_id": request.operation_id, "request_sha256": args.request_sha256
        }))?,
        8192,
        "active native release",
    )?;
    let journal = if args.recovery_only {
        transaction::compensate(
            &operation,
            &request,
            transaction::RecoveryAuthority {
                ticket_id: spec.id.clone(),
                idempotency_key: spec.idempotency_key.clone(),
                writer_epoch: spec
                    .writer_epoch
                    .ok_or_else(|| anyhow!("EPERM writer-epoch"))?,
            },
            &mut adapter,
        )?
    } else {
        transaction::run(
            &operation,
            &request,
            &spec.id,
            &spec.idempotency_key,
            spec.writer_epoch
                .ok_or_else(|| anyhow!("EPERM writer-epoch"))?,
            &mut adapter,
            recovery,
        )?
    };
    if journal.state == State::RollbackFailed {
        crate::wal::durable_atomic_write(
            &root.join("release-blocked.json"),
            &serde_json::to_vec(&journal)?,
            262144,
            "native recovery blocker",
        )?;
    }
    if args.recovery_only && journal.state == State::RecoveredFailure && blocked.try_exists()? {
        fs::remove_file(&blocked)?;
        fs::File::open(&root)?.sync_all()?;
    }
    if matches!(
        journal.state,
        State::Succeeded | State::Failed | State::RecoveredFailure
    ) {
        fs::remove_file(&active)?;
        fs::File::open(&root)?.sync_all()?;
    }
    Ok(journal)
}

fn retain(
    executor: &ExecutorConfig,
    spec: &HostTicketSpec,
    journal: &transaction::Journal,
) -> Result<String> {
    ensure!(
        !matches!(journal.state, State::Running | State::Ambiguous),
        "ambiguous native-release"
    );
    let terminal = journal.terminal_observation()?;
    observation::retain_native_outcome_at(
        executor,
        spec,
        journal,
        &terminal.native_identity,
        terminal.completed_unix_ms,
        if journal.state == State::Succeeded {
            cohesix_evidence::Outcome::Verified
        } else {
            cohesix_evidence::Outcome::Failed
        },
    )
}

/// Root admission, native observations and the existing Worker receipt form one chain.
pub fn execute(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    observation::prepare(config)?;
    let journal = run(transport, session, spec, config, false)?;
    let reference = retain(config, spec, &journal)?;
    ensure!(
        journal.state == State::Succeeded,
        "failed native-release {reference}"
    );
    Ok(reference)
}

/// Reconcile durable native phase outcomes before considering any new dispatch.
pub fn reconcile(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<ReconcileOutcome> {
    let journal = run(transport, session, spec, config, true)?;
    if matches!(journal.state, State::Running | State::Ambiguous) {
        return Ok(ReconcileOutcome::Ambiguous);
    }
    let reference = retain(config, spec, &journal)?;
    Ok(if journal.state == State::Succeeded {
        ReconcileOutcome::Committed(reference)
    } else {
        ReconcileOutcome::Rejected(reference)
    })
}
