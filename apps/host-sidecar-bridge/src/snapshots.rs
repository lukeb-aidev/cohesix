// Author: Lukas Bower
// Purpose: Reserve durable source sequences before publishing bounded native observations through the authenticated target interface.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use cohesix_authority::snapshot::{Entry, Limits, Snapshot, UnavailableReason, SCHEMA};
use cohsh::{Session, Transport};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// Compiler-owned enrollment copied into the binary, never supplied by an upload.
#[derive(Debug, Clone, Deserialize)]
pub struct Enrollment {
    source_id: String,
    providers: Vec<String>,
    epoch: u64,
    mount: String,
    max_bytes: usize,
    max_entries: usize,
    max_value_bytes: usize,
    max_ttl_ms: u64,
}

impl Enrollment {
    /// Select an exact enrolled source from the compiled resolved manifest.
    pub fn compiled(source_id: &str) -> Result<Self> {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../configs/generated/root_task_resolved.json"
        ))?;
        let host = &manifest["ecosystem"]["host"];
        let config = &host["snapshots"];
        if host["enable"] != true || config["enable"] != true {
            bail!("not_enabled host-snapshots");
        }
        let publisher = config["publishers"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["source_id"] == source_id))
            .ok_or_else(|| anyhow!("not_enabled snapshot-source"))?;
        let mut value = config.clone();
        let object = value
            .as_object_mut()
            .ok_or_else(|| anyhow!("invalid snapshot-config"))?;
        object.remove("enable");
        object.remove("publishers");
        object.insert("source_id".into(), publisher["source_id"].clone());
        object.insert("providers".into(), publisher["providers"].clone());
        object.insert(
            "epoch".into(),
            manifest["authority"]["writer_epoch"].clone(),
        );
        object.insert("mount".into(), host["mount_at"].clone());
        let selected: Self = serde_json::from_value(value)?;
        selected
            .with_limits(|limits| limits.validate())
            .map_err(|_| anyhow!("invalid generated-snapshot-limits"))?;
        Ok(selected)
    }

    fn with_limits<T>(&self, operation: impl FnOnce(Limits<'_>) -> T) -> T {
        let providers: Vec<_> = self.providers.iter().map(String::as_str).collect();
        operation(Limits {
            source_id: &self.source_id,
            providers: &providers,
            epoch: self.epoch,
            max_bytes: self.max_bytes,
            max_entries: self.max_entries,
            max_value_bytes: self.max_value_bytes,
            max_ttl_ms: self.max_ttl_ms,
        })
    }

    /// Provider selection belongs to generated source enrollment.
    pub fn providers(&self) -> &[String] {
        &self.providers
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    schema: String,
    source_id: String,
    epoch: u64,
    sequence: u64,
    observed_unix_ms: u64,
}

/// One process owns a private source cursor for its lifetime. A crash may skip a
/// reserved sequence; it cannot reuse one, including after an ambiguous ACK.
pub struct Publisher {
    enrollment: Enrollment,
    state: PathBuf,
    _lock: File,
    cursor: Cursor,
}

/// Publication ACK is a read-only observation claim, never provider execution.
#[derive(Debug, Serialize)]
pub struct Publication {
    /// Exact source-scoped namespace provider.
    pub provider: String,
    /// Persisted before the first target write.
    pub sequence: u64,
    /// Whether native state was obtained; false publishes an immediate withdrawal.
    pub available: bool,
    /// Typed native failure; raw diagnostics and credentials are not published.
    pub reason: Option<UnavailableReason>,
    /// Exact canonical payload digest verified in the receiver status.
    pub sha256: String,
}

fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("invalid snapshot-state-file");
    }
    #[cfg(unix)]
    if metadata.mode() & 0o077 != 0
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.nlink() != 1
    {
        bail!("invalid snapshot-state-permissions");
    }
    Ok(file)
}

impl Publisher {
    /// A library bridge cannot redirect a compiled publisher to another mount.
    pub fn require_mount(&self, mount: &str) -> Result<()> {
        if self.enrollment.mount != mount {
            bail!("invalid snapshot-mount");
        }
        Ok(())
    }

    /// Open an absolute private state directory. Missing cursors are initialized
    /// only in a newly created source directory; deletion is never a reset.
    pub fn open(enrollment: Enrollment, state_root: &Path) -> Result<Self> {
        if !state_root.is_absolute() {
            bail!("invalid snapshot-state-root");
        }
        let root = state_root
            .canonicalize()
            .context("snapshot state root must already exist")?;
        let metadata = root.metadata()?;
        #[cfg(unix)]
        if metadata.mode() & 0o077 != 0 || metadata.uid() != rustix::process::geteuid().as_raw() {
            bail!("invalid snapshot-state-root-permissions");
        }
        if !metadata.is_dir() {
            bail!("invalid snapshot-state-root");
        }
        let state = root.join(format!(
            "{}-epoch-{}",
            enrollment.source_id, enrollment.epoch
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        let created = match builder.create(&state) {
            Ok(()) => true,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(e) => return Err(e.into()),
        };
        let metadata = fs::symlink_metadata(&state)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!("invalid snapshot-state-directory");
        }
        #[cfg(unix)]
        if metadata.mode() & 0o077 != 0 || metadata.uid() != rustix::process::geteuid().as_raw() {
            bail!("invalid snapshot-state-permissions");
        }
        let lock_path = state.join("owner.lock");
        let lock = if created {
            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            options.open(&lock_path)?
        } else {
            private_file(&lock_path)?
        };
        lock.try_lock_exclusive()
            .context("snapshot source already has an owner")?;
        let cursor = if created {
            Cursor {
                schema: "cohesix-snapshot-cursor/v1".into(),
                source_id: enrollment.source_id.clone(),
                epoch: enrollment.epoch,
                sequence: 0,
                observed_unix_ms: 0,
            }
        } else {
            let mut bytes = Vec::new();
            private_file(&state.join("cursor.json"))?
                .take(1025)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 1024 {
                bail!("invalid snapshot-cursor-size");
            }
            serde_json::from_slice::<Cursor>(&bytes)?
        };
        if cursor.schema != "cohesix-snapshot-cursor/v1"
            || cursor.source_id != enrollment.source_id
            || cursor.epoch != enrollment.epoch
        {
            bail!("invalid snapshot-cursor-domain");
        }
        let mut publisher = Self {
            enrollment,
            state,
            _lock: lock,
            cursor,
        };
        if created {
            publisher.persist()?;
            File::open(root)?.sync_all()?;
        }
        Ok(publisher)
    }

    fn persist(&mut self) -> Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(&self.state)?;
        temp.write_all(&serde_json::to_vec(&self.cursor)?)?;
        temp.as_file().sync_all()?;
        temp.persist(self.state.join("cursor.json"))
            .map_err(|e| e.error)?;
        File::open(&self.state)?.sync_all()?;
        Ok(())
    }

    /// Generated provider enrollment, also used for polling rather than legacy
    /// target-preseeded directories.
    pub fn providers(&self) -> &[String] {
        self.enrollment.providers()
    }

    fn prepare(
        &mut self,
        provider: &str,
        observed_unix_ms: u64,
        ttl_ms: u64,
        observation: std::result::Result<Vec<Entry>, UnavailableReason>,
    ) -> Result<(Snapshot, Vec<u8>)> {
        if observed_unix_ms < self.cursor.observed_unix_ms {
            bail!("clock_regression snapshot-source");
        }
        let sequence = self
            .cursor
            .sequence
            .checked_add(1)
            .ok_or_else(|| anyhow!("exhausted snapshot-sequence"))?;
        let (entries, reason) = match observation {
            Ok(entries) => (entries, None),
            Err(reason) => (Vec::new(), Some(reason)),
        };
        let snapshot = Snapshot {
            schema: SCHEMA.into(),
            provider: provider.into(),
            source_id: self.enrollment.source_id.clone(),
            epoch: self.enrollment.epoch,
            sequence,
            observed_unix_ms,
            ttl_ms,
            available: reason.is_none(),
            reason,
            entries,
        };
        let bytes = self
            .enrollment
            .with_limits(|limits| snapshot.encode(limits))
            .map_err(|_| anyhow!("invalid snapshot-payload"))?;
        self.cursor.sequence = sequence;
        self.cursor.observed_unix_ms = observed_unix_ms;
        self.persist()?;
        Ok((snapshot, bytes))
    }

    /// Capture native state, reserve its sequence durably, upload and verify the
    /// exact receiver digest. Collection consumes TTL before upload starts.
    pub fn publish<T: Transport>(
        &mut self,
        transport: &mut T,
        session: &Session,
        provider: &str,
    ) -> Result<Publication> {
        if !self
            .enrollment
            .providers
            .iter()
            .any(|selected| selected == provider)
        {
            bail!("not_enabled snapshot-provider");
        }
        let started = Instant::now();
        let observed = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
        let mut ttl = self.enrollment.max_ttl_ms;
        let deadline = started + std::time::Duration::from_millis(ttl.min(10000));
        let mut observation = if matches!(provider, "modbus" | "dnp3") {
            match std::env::var_os("COHESIX_FIELD_BUS_STATE_ROOT") {
                Some(root) => match crate::field_bus::collect(provider, Path::new(&root), observed)
                {
                    Ok((entries, remaining)) => {
                        ttl = ttl.min(remaining);
                        Ok(entries)
                    }
                    Err(_) => Err(UnavailableReason::SourceFailed),
                },
                None => Err(UnavailableReason::NotEnabled),
            }
        } else {
            crate::observations::collect(provider, deadline)
        };
        if let Ok(entries) = &observation {
            // Oversized or malformed native projections withdraw prior state;
            // never silently truncate a host observation to fit the receiver.
            let probe = Snapshot {
                schema: SCHEMA.into(),
                provider: provider.into(),
                source_id: self.enrollment.source_id.clone(),
                epoch: self.enrollment.epoch,
                sequence: 1,
                observed_unix_ms: observed,
                ttl_ms: ttl,
                available: true,
                reason: None,
                entries: entries.clone(),
            };
            if self
                .enrollment
                .with_limits(|limits| probe.encode(limits))
                .is_err()
            {
                observation = Err(UnavailableReason::SourceFailed);
            }
        }
        let elapsed = u64::try_from(started.elapsed().as_millis())?;
        let remaining = ttl
            .checked_sub(elapsed)
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow!("timeout snapshot-capture"))?;
        let (mut snapshot, _) = self.prepare(provider, observed, remaining, observation)?;
        // Disk reservation also consumes freshness; encode only after its fsync.
        snapshot.ttl_ms = ttl
            .checked_sub(u64::try_from(started.elapsed().as_millis())?)
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow!("timeout snapshot-reservation"))?;
        let bytes = self
            .enrollment
            .with_limits(|limits| snapshot.encode(limits))
            .map_err(|_| anyhow!("invalid snapshot-payload"))?;
        let root = format!(
            "{}/snapshots/{}/{}",
            self.enrollment.mount, provider, self.enrollment.source_id
        );
        let control = format!("{root}/ctl");
        let hash = hex::encode(Sha256::digest(&bytes));
        transport.write(
            session,
            &control,
            format!("begin bytes={} sha256={}\n", bytes.len(), hash).as_bytes(),
        )?;
        let encoded = STANDARD.encode(&bytes);
        for chunk in encoded
            .as_bytes()
            .chunks(((cohsh_core::MAX_ECHO_LEN - 5) / 4) * 4)
        {
            if started.elapsed().as_millis() >= u128::from(ttl) {
                bail!("timeout snapshot-upload");
            }
            let mut line = b"b64:".to_vec();
            line.extend_from_slice(chunk);
            line.push(b'\n');
            transport.write(session, &control, &line)?;
        }
        if started.elapsed().as_millis() >= u128::from(ttl) {
            bail!("timeout snapshot-upload");
        }
        transport.write(session, &control, b"end\n")?;
        let status = transport
            .read(session, &format!("{root}/status"))?
            .join("\n");
        let fields: Vec<_> = status.split_ascii_whitespace().collect();
        if !fields.contains(&format!("sequence={}", snapshot.sequence).as_str())
            || !fields.contains(&format!("sha256={hash}").as_str())
        {
            bail!("unverified snapshot-receiver-status");
        }
        Ok(Publication {
            provider: provider.into(),
            sequence: snapshot.sequence,
            available: snapshot.available,
            reason: snapshot.reason,
            sha256: hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_cursor_refuses_reuse_missing_state_and_foreign_domains() -> Result<()> {
        let root = tempfile::tempdir()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))?;
        }
        let enrollment = Enrollment {
            source_id: "host-1".into(),
            providers: vec!["network".into()],
            epoch: 7,
            mount: "/host".into(),
            max_bytes: 8192,
            max_entries: 64,
            max_value_bytes: 1024,
            max_ttl_ms: 30000,
        };
        let mut publisher = Publisher::open(enrollment.clone(), root.path())?;
        publisher.require_mount("/host")?;
        assert!(publisher.require_mount("/other").is_err());
        assert!(Publisher::open(enrollment.clone(), root.path()).is_err());
        let (first, _) =
            publisher.prepare("network", 100, 30000, Err(UnavailableReason::SourceFailed))?;
        assert_eq!(first.sequence, 1);
        assert!(publisher
            .prepare("network", 99, 30000, Ok(Vec::new()))
            .is_err());
        assert!(publisher
            .prepare("docker", 100, 30000, Ok(Vec::new()))
            .is_err());
        drop(publisher);
        let mut resumed = Publisher::open(enrollment.clone(), root.path())?;
        assert_eq!(
            resumed
                .prepare("network", 100, 30000, Ok(Vec::new()))?
                .0
                .sequence,
            2
        );
        fs::remove_file(resumed.state.join("cursor.json"))?;
        drop(resumed);
        assert!(Publisher::open(enrollment, root.path()).is_err());
        Ok(())
    }
}
