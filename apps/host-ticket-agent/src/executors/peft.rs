// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Execute confined PEFT receipt actions through existing coh helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use coh::peft::{
    activate_model, export_job, import_adapter, rollback_model, PeftActivateSpec, PeftExportSpec,
    PeftImportSpec, PeftRollbackSpec,
};
use coh::policy::CohPolicy;
use coh::CohAudit;
use cohsh::{Session, Transport};
use fs2::FileExt;
use sha2::{Digest, Sha256};

use super::{
    arg_bool, arg_str, provider_pending, ExecutorConfig, ReconcileOutcome, TransportAccess,
};
use crate::{HostTicketSpec, HOST_TICKET_V2_SCHEMA};

const ADAPTER_FILE: &str = "adapter.safetensors";
const LORA_FILE: &str = "lora.json";
const METRICS_FILE: &str = "metrics.json";
const EXPORT_FILES: [&str; 3] = ["telemetry.cbor", "base_model.ref", "policy.toml"];

/// Execute PEFT ticket actions.
pub fn execute(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    if spec.schema == HOST_TICKET_V2_SCHEMA {
        return execute_v2(transport, session, spec, config);
    }
    match spec.action.as_str() {
        "peft.export" => execute_compat_export(transport, session, spec, config),
        "peft.import" => execute_compat_import(spec, config),
        "peft.activate" => execute_compat_activate(transport, session, spec, config),
        "peft.rollback" => execute_compat_rollback(transport, session, spec, config),
        other => Err(anyhow!("unsupported peft action {other}")),
    }
}

/// Reconcile a possibly committed version-2 PEFT action without re-execution.
pub fn reconcile(
    _transport: &mut dyn Transport,
    _session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<ReconcileOutcome> {
    if spec.schema != HOST_TICKET_V2_SCHEMA {
        return Ok(ReconcileOutcome::Ambiguous);
    }
    let subject = required_v2(spec.subject_ref.as_deref(), "subject_ref")?;
    match spec.action.as_str() {
        "peft.export" => {
            if let Some(export) = completed_export(&config.export_root, subject)? {
                Ok(ReconcileOutcome::Committed(format!(
                    "peft.export job={subject} observed=complete files={} bytes={} digest={}",
                    export.files, export.bytes, export.digest
                )))
            } else {
                Ok(ReconcileOutcome::Ambiguous)
            }
        }
        "peft.import" => {
            let manifest = confined_registry_model(&config.registry_root, subject, true)?
                .join("manifest.toml");
            if manifest.is_file() {
                Ok(ReconcileOutcome::Committed(format!(
                    "peft.import model={subject} observed=manifest sha256={}",
                    hash_file(&manifest, 8192)?
                )))
            } else {
                Ok(ReconcileOutcome::Ambiguous)
            }
        }
        "peft.activate" | "peft.rollback" => {
            let registry_root = confined_root(&config.registry_root)?;
            validate_registry_control_files(&registry_root)?;
            let manifest =
                confined_registry_model(&registry_root, subject, true)?.join("manifest.toml");
            hash_file(&manifest, 8192)?;
            let active = registry_root.join("active");
            let current = fs::read_to_string(&active).ok().and_then(|value| {
                value
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .map(str::trim)
                    .map(str::to_owned)
            });
            if current.as_deref() == Some(subject) {
                Ok(ReconcileOutcome::Committed(format!(
                    "{} model={subject} observed=active",
                    spec.action
                )))
            } else {
                Ok(ReconcileOutcome::Ambiguous)
            }
        }
        _ => Ok(ReconcileOutcome::Ambiguous),
    }
}

fn execute_v2(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    crate::claim::validate_v2_action_args(spec)?;
    let subject = required_v2(spec.subject_ref.as_deref(), "subject_ref")?.to_owned();
    match spec.action.as_str() {
        "peft.export" => {
            if let Some(export) = completed_export(&config.export_root, &subject)? {
                return Ok(format!(
                    "peft.export job={subject} observed=complete files={} bytes={} digest={}",
                    export.files, export.bytes, export.digest
                ));
            }
            let Some(_lock) = PeftRootLock::try_acquire(&config.export_root, "export")? else {
                return Err(provider_pending(format!(
                    "peft.export job={subject} is waiting for the elected producer"
                )));
            };
            if let Some(export) = completed_export(&config.export_root, &subject)? {
                return Ok(format!(
                    "peft.export job={subject} observed=complete files={} bytes={} digest={}",
                    export.files, export.bytes, export.digest
                ));
            }
            let export_root = confined_root(&config.export_root)?;
            let job_dir = prepare_confined_child(&export_root, &subject)?;
            let mut access = TransportAccess::new(transport, session);
            let policy = CohPolicy::from_generated();
            let mut audit = CohAudit::new();
            let summary = export_job(
                &mut access,
                &policy,
                &PeftExportSpec {
                    job_id: subject.clone(),
                    out_dir: export_root.clone(),
                },
                &mut audit,
            )?;
            let digest = hash_directory_files(&job_dir, &EXPORT_FILES)?;
            Ok(format!(
                "peft.export job={subject} files={} bytes={} digest={digest}",
                summary.files, summary.bytes
            ))
        }
        "peft.import" => {
            let _registry_lock = PeftRootLock::acquire(&config.registry_root, "registry")?;
            let _adapter_lock = PeftRootLock::acquire(&config.adapter_root, "adapter")?;
            let _export_lock = PeftRootLock::acquire(&config.export_root, "export")?;
            let adapter_ref = arg_str(spec, "adapter_ref")
                .ok_or_else(|| anyhow!("peft.import requires args.adapter_ref"))?;
            let job_id = arg_str(spec, "job_id")
                .ok_or_else(|| anyhow!("peft.import requires args.job_id"))?;
            let adapter_dir = confined_child(&config.adapter_root, adapter_ref, true)?;
            let export_root = confined_root(&config.export_root)?;
            let registry_root = confined_root(&config.registry_root)?;
            let registry_model = confined_registry_model(&registry_root, &subject, false)?;
            if registry_model.join("manifest.toml").exists() {
                return Err(anyhow!("model {subject} is already imported"));
            }
            hash_file(&adapter_dir.join(ADAPTER_FILE), u64::MAX)?;
            hash_file(&adapter_dir.join(LORA_FILE), u64::MAX)?;
            if adapter_dir.join(METRICS_FILE).exists() {
                hash_file(&adapter_dir.join(METRICS_FILE), u64::MAX)?;
            }
            verify_optional_hash(spec, "adapter_sha256", &adapter_dir.join(ADAPTER_FILE))?;
            verify_optional_hash(spec, "lora_sha256", &adapter_dir.join(LORA_FILE))?;
            if let Some(expected) = arg_str(spec, "metrics_sha256") {
                verify_hash(&adapter_dir.join(METRICS_FILE), expected)?;
            }
            let mut audit = CohAudit::new();
            let summary = import_adapter(
                &CohPolicy::from_generated(),
                &PeftImportSpec {
                    model_id: subject.clone(),
                    adapter_dir,
                    export_root,
                    job_id: job_id.to_owned(),
                    registry_root,
                },
                &mut audit,
            )?;
            Ok(format!(
                "peft.import model={subject} job={job_id} manifest_sha256={} adapter_bytes={} lora_bytes={}",
                hash_file(&summary.manifest_path, 8192)?,
                summary.adapter_bytes,
                summary.lora_bytes
            ))
        }
        "peft.activate" => {
            let _lock = PeftRootLock::acquire(&config.registry_root, "registry")?;
            let registry_root = confined_root(&config.registry_root)?;
            validate_registry_control_files(&registry_root)?;
            let manifest =
                confined_registry_model(&registry_root, &subject, true)?.join("manifest.toml");
            hash_file(&manifest, 8192)?;
            let mut access = TransportAccess::new(transport, session);
            let mut audit = CohAudit::new();
            activate_model(
                &mut access,
                &CohPolicy::from_generated(),
                &PeftActivateSpec {
                    model_id: subject.clone(),
                    registry_root,
                },
                &mut audit,
            )?;
            Ok(format!(
                "peft.activate model={subject} ack_lines={}",
                audit.lines().len()
            ))
        }
        "peft.rollback" => {
            let _lock = PeftRootLock::acquire(&config.registry_root, "registry")?;
            let registry_root = confined_root(&config.registry_root)?;
            validate_registry_control_files(&registry_root)?;
            let manifest =
                confined_registry_model(&registry_root, &subject, true)?.join("manifest.toml");
            hash_file(&manifest, 8192)?;
            let mut access = TransportAccess::new(transport, session);
            let mut audit = CohAudit::new();
            rollback_model(
                &mut access,
                &CohPolicy::from_generated(),
                &PeftRollbackSpec {
                    registry_root: registry_root.clone(),
                },
                &mut audit,
            )?;
            let active = fs::read_to_string(registry_root.join("active"))?;
            if active.trim() != subject {
                return Err(anyhow!(
                    "peft.rollback expected subject_ref={subject}, observed active={}",
                    active.trim()
                ));
            }
            Ok(format!(
                "peft.rollback model={subject} ack_lines={}",
                audit.lines().len()
            ))
        }
        other => Err(anyhow!("unsupported peft action {other}")),
    }
}

fn execute_compat_export(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    let job_id = arg_str(spec, "job_id")
        .or_else(|| arg_str(spec, "job"))
        .ok_or_else(|| anyhow!("peft.export requires args.job_id"))?;
    let out_dir = path_arg(spec, "out_dir")
        .or_else(|| path_arg(spec, "out"))
        .unwrap_or_else(|| config.export_root.clone());
    let mut access = TransportAccess::new(transport, session);
    let mut audit = CohAudit::new();
    let summary = export_job(
        &mut access,
        &CohPolicy::from_generated(),
        &PeftExportSpec {
            job_id: job_id.to_owned(),
            out_dir,
        },
        &mut audit,
    )?;
    Ok(format!(
        "peft export job={job_id} files={} bytes={}",
        summary.files, summary.bytes
    ))
}

fn execute_compat_import(spec: &HostTicketSpec, config: &ExecutorConfig) -> Result<String> {
    let model_id = arg_str(spec, "model_id")
        .or_else(|| arg_str(spec, "model"))
        .ok_or_else(|| anyhow!("peft.import requires args.model_id"))?
        .to_owned();
    let job_id = arg_str(spec, "job_id")
        .or_else(|| arg_str(spec, "job"))
        .ok_or_else(|| anyhow!("peft.import requires args.job_id"))?
        .to_owned();
    let adapter_dir = path_arg(spec, "adapter_dir")
        .or_else(|| path_arg(spec, "from"))
        .ok_or_else(|| anyhow!("peft.import requires args.adapter_dir"))?;
    let export_root = path_arg(spec, "export_root")
        .or_else(|| path_arg(spec, "export"))
        .unwrap_or_else(|| config.export_root.clone());
    let registry_root = path_arg(spec, "registry_root")
        .or_else(|| path_arg(spec, "registry"))
        .unwrap_or_else(|| config.registry_root.clone());

    let policy = CohPolicy::from_generated();
    let mut audit = CohAudit::new();
    let summary = import_adapter(
        &policy,
        &PeftImportSpec {
            model_id: model_id.clone(),
            adapter_dir,
            export_root,
            job_id: job_id.clone(),
            registry_root,
        },
        &mut audit,
    )?;
    let publish = arg_bool(spec, "publish").unwrap_or(false);
    Ok(format!(
        "peft import model={model_id} job={job_id} manifest={} adapter_bytes={} publish_requested={publish}",
        summary.manifest_path.display(),
        summary.adapter_bytes
    ))
}

fn execute_compat_activate(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    let model_id = arg_str(spec, "model_id")
        .or_else(|| arg_str(spec, "model"))
        .ok_or_else(|| anyhow!("peft.activate requires args.model_id"))?
        .to_owned();
    let registry_root = path_arg(spec, "registry_root")
        .or_else(|| path_arg(spec, "registry"))
        .unwrap_or_else(|| config.registry_root.clone());
    let mut access = TransportAccess::new(transport, session);
    let mut audit = CohAudit::new();
    activate_model(
        &mut access,
        &CohPolicy::from_generated(),
        &PeftActivateSpec {
            model_id: model_id.clone(),
            registry_root,
        },
        &mut audit,
    )?;
    Ok(format!(
        "peft activate model={model_id} ack_lines={}",
        audit.lines().len()
    ))
}

fn execute_compat_rollback(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    let registry_root = path_arg(spec, "registry_root")
        .or_else(|| path_arg(spec, "registry"))
        .unwrap_or_else(|| config.registry_root.clone());
    let mut access = TransportAccess::new(transport, session);
    let mut audit = CohAudit::new();
    rollback_model(
        &mut access,
        &CohPolicy::from_generated(),
        &PeftRollbackSpec { registry_root },
        &mut audit,
    )?;
    Ok(format!("peft rollback ack_lines={}", audit.lines().len()))
}

fn path_arg(spec: &HostTicketSpec, key: &str) -> Option<PathBuf> {
    arg_str(spec, key).map(PathBuf::from)
}

fn required_v2<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str> {
    value.ok_or_else(|| anyhow!("host-ticket/v2 requires {field}"))
}

fn confined_root(root: &Path) -> Result<PathBuf> {
    reject_non_normal_components(root)?;
    if let Ok(meta) = fs::symlink_metadata(root) {
        if meta.file_type().is_symlink() {
            return Err(anyhow!(
                "configured PEFT root {} is a symlink",
                root.display()
            ));
        }
    }
    fs::create_dir_all(root).with_context(|| format!("create PEFT root {}", root.display()))?;
    root.canonicalize()
        .with_context(|| format!("canonicalize PEFT root {}", root.display()))
}

fn confined_child(root: &Path, component: &str, require_existing: bool) -> Result<PathBuf> {
    validate_component(component)?;
    let canonical_root = confined_root(root)?;
    reject_symlink_walk(&canonical_root)?;
    let child = canonical_root.join(component);
    if let Ok(meta) = fs::symlink_metadata(&child) {
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(anyhow!(
                "PEFT child {} is not a regular directory",
                child.display()
            ));
        }
        let canonical_child = child
            .canonicalize()
            .with_context(|| format!("canonicalize PEFT child {}", child.display()))?;
        if !canonical_child.starts_with(&canonical_root) {
            return Err(anyhow!("PEFT child escapes configured root"));
        }
        return Ok(canonical_child);
    }
    if require_existing {
        return Err(anyhow!("PEFT child {} does not exist", child.display()));
    }
    Ok(child)
}

fn prepare_confined_child(root: &Path, component: &str) -> Result<PathBuf> {
    let child = confined_child(root, component, false)?;
    if !child.exists() {
        fs::create_dir(&child).with_context(|| format!("create PEFT child {}", child.display()))?;
        File::open(root)
            .with_context(|| format!("open PEFT root {}", root.display()))?
            .sync_all()
            .with_context(|| format!("sync PEFT root {}", root.display()))?;
    }
    confined_child(root, component, true)
}

fn confined_registry_model(root: &Path, model: &str, require_existing: bool) -> Result<PathBuf> {
    validate_component(model)?;
    let canonical_root = confined_root(root)?;
    let available = canonical_root.join("available");
    let available = confined_root(&available)?;
    confined_child(&available, model, require_existing)
}

fn validate_registry_control_files(root: &Path) -> Result<()> {
    for name in ["active", "active_state.toml"] {
        let path = root.join(name);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(anyhow!(
                    "PEFT registry control path {} is not a regular file",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn reject_non_normal_components(path: &Path) -> Result<()> {
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            return Err(anyhow!(
                "configured PEFT path {} contains traversal components",
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_component(value: &str) -> Result<()> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.len() > 128
        || value.starts_with('-')
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
    {
        return Err(anyhow!("invalid PEFT path component {value:?}"));
    }
    Ok(())
}

fn reject_symlink_walk(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if let Ok(meta) = fs::symlink_metadata(&current) {
            if meta.file_type().is_symlink() {
                return Err(anyhow!("PEFT path {} contains a symlink", path.display()));
            }
        }
    }
    Ok(())
}

fn verify_optional_hash(spec: &HostTicketSpec, key: &str, path: &Path) -> Result<()> {
    if let Some(expected) = arg_str(spec, key) {
        verify_hash(path, expected)?;
    }
    Ok(())
}

fn verify_hash(path: &Path, expected: &str) -> Result<()> {
    let actual = hash_file(path, u64::MAX)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(anyhow!(
            "{} SHA-256 mismatch: expected {} got {}",
            path.display(),
            expected,
            actual
        ));
    }
    Ok(())
}

fn hash_directory_files(root: &Path, names: &[&str]) -> Result<String> {
    let mut aggregate = Sha256::new();
    for name in names {
        let digest = hash_file(&root.join(name), u64::MAX)?;
        aggregate.update(name.as_bytes());
        aggregate.update([0]);
        aggregate.update(digest.as_bytes());
        aggregate.update(b"\n");
    }
    Ok(hex::encode(aggregate.finalize()))
}

#[derive(Debug, PartialEq, Eq)]
struct CompletedExport {
    files: usize,
    bytes: u64,
    digest: String,
}

fn completed_export(root: &Path, subject: &str) -> Result<Option<CompletedExport>> {
    validate_component(subject)?;
    let root = confined_root(root)?;
    let candidate = root.join(subject);
    match fs::symlink_metadata(&candidate) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(anyhow!(
                "PEFT child {} is not a regular directory",
                candidate.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("stat PEFT child {}", candidate.display()))
        }
    }
    let job_dir = confined_child(&root, subject, true)?;
    let mut bytes = 0u64;
    for name in EXPORT_FILES {
        let path = job_dir.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("stat PEFT file {}", path.display()))
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
            return Ok(None);
        }
        bytes = bytes
            .checked_add(metadata.len())
            .ok_or_else(|| anyhow!("completed PEFT export byte count overflow"))?;
    }
    Ok(Some(CompletedExport {
        files: EXPORT_FILES.len(),
        bytes,
        digest: hash_directory_files(&job_dir, &EXPORT_FILES)?,
    }))
}

fn hash_file(path: &Path, max_bytes: u64) -> Result<String> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("stat PEFT file {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(anyhow!(
            "PEFT file {} is not a regular file",
            path.display()
        ));
    }
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > max_bytes {
            return Err(anyhow!("{} exceeds {} bytes", path.display(), max_bytes));
        }
        hasher.update(&buffer[..read]);
    }
    if total == 0 {
        return Err(anyhow!("{} is empty", path.display()));
    }
    Ok(hex::encode(hasher.finalize()))
}

#[derive(Debug)]
struct PeftRootLock {
    file: File,
}

impl PeftRootLock {
    fn acquire(root: &Path, label: &str) -> Result<Self> {
        Self::try_acquire(root, label)?.ok_or_else(|| {
            anyhow!(
                "PEFT root {} already has an active {label} transaction",
                root.display()
            )
        })
    }

    fn try_acquire(root: &Path, label: &str) -> Result<Option<Self>> {
        let root = confined_root(root)?;
        let path = root.join(format!(".cohesix-{label}.lock"));
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(anyhow!("PEFT lock {} is a symlink", path.display()));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("open PEFT lock {}", path.display()))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error).with_context(|| format!("lock PEFT root {}", root.display())),
        }
    }
}

impl Drop for PeftRootLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_child_rejects_traversal_and_symlink() {
        let temp = tempfile::TempDir::new().expect("temp");
        assert!(confined_child(temp.path(), "../escape", false).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(temp.path(), temp.path().join("linked"))
                .expect("create symlink");
            assert!(confined_child(temp.path(), "linked", true).is_err());
        }
    }

    #[test]
    fn hash_validation_detects_mismatch() {
        let temp = tempfile::TempDir::new().expect("temp");
        let path = temp.path().join(ADAPTER_FILE);
        fs::write(&path, b"adapter").expect("write");
        assert!(verify_hash(
            &path,
            "0000000000000000000000000000000000000000000000000000000000000000"
        )
        .is_err());
    }

    #[test]
    fn completed_export_requires_all_nonempty_regular_files() {
        let temp = tempfile::TempDir::new().expect("temp");
        let job = temp.path().join("job-1");
        fs::create_dir(&job).expect("job directory");
        fs::write(job.join(EXPORT_FILES[0]), b"telemetry").expect("telemetry");
        assert_eq!(
            completed_export(temp.path(), "job-1").expect("partial export"),
            None
        );

        fs::write(job.join(EXPORT_FILES[1]), b"model").expect("model");
        fs::write(job.join(EXPORT_FILES[2]), b"policy").expect("policy");
        let completed = completed_export(temp.path(), "job-1")
            .expect("complete export")
            .expect("completion");
        assert_eq!(completed.files, EXPORT_FILES.len());
        assert_eq!(completed.bytes, 20);
        assert_eq!(completed.digest.len(), 64);
    }

    #[test]
    fn export_lock_elects_one_producer_without_blocking_contenders() {
        let temp = tempfile::TempDir::new().expect("temp");
        let first = PeftRootLock::try_acquire(temp.path(), "export")
            .expect("first lock attempt")
            .expect("first producer");
        assert!(
            PeftRootLock::try_acquire(temp.path(), "export")
                .expect("contending lock attempt")
                .is_none(),
            "a contending export must become provider-pending",
        );
        drop(first);
        assert!(
            PeftRootLock::try_acquire(temp.path(), "export")
                .expect("post-release lock attempt")
                .is_some(),
            "producer lock must be reusable after deterministic release",
        );
    }

    #[cfg(unix)]
    #[test]
    fn completed_export_does_not_follow_file_symlinks() {
        let temp = tempfile::TempDir::new().expect("temp");
        let job = temp.path().join("job-1");
        fs::create_dir(&job).expect("job directory");
        let outside = temp.path().join("outside");
        fs::write(&outside, b"outside").expect("outside file");
        std::os::unix::fs::symlink(&outside, job.join(EXPORT_FILES[0]))
            .expect("create file symlink");
        fs::write(job.join(EXPORT_FILES[1]), b"model").expect("model");
        fs::write(job.join(EXPORT_FILES[2]), b"policy").expect("policy");

        assert_eq!(
            completed_export(temp.path(), "job-1").expect("symlink is incomplete"),
            None
        );
    }
}
