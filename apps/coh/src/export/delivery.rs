// Author: Lukas Bower
// Purpose: Persist bounded SIEM delivery attempts and exact destination acknowledgements before advancing a cursor.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{anyhow, ensure, Result};
use cohesix_evidence::VerifiedGraph;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    schema: String,
    enabled: bool,
    endpoint: String,
    credential_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ca_certificate_path_ref: Option<String>,
    maximum_entries: usize,
    maximum_wal_bytes: usize,
    maximum_attempts: u32,
    timeout_ms: u32,
    maximum_backoff_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum State {
    Pending,
    Attempting,
    Acknowledged,
    Deadletter,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    sequence: u64,
    graph_sha256: String,
    payload_sha256: String,
    state: State,
    attempts: u32,
    next_attempt_unix_ms: u64,
    acknowledgement_sha256: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Wal {
    schema: String,
    policy_sha256: String,
    next_sequence: u64,
    entries: Vec<Entry>,
}

/// Exact receiver acknowledgement. The receiver must durably deduplicate the key before replying.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Acknowledgement {
    /// Must equal cohesix-siem-ack/v1.
    pub schema: String,
    /// Exact graph digest sent as Idempotency-Key.
    pub idempotency_key: String,
    /// Exact NDJSON request-body digest.
    pub payload_sha256: String,
    /// Only accepted is terminal.
    pub status: String,
}

/// Read-only delivery projection; an ACK proves receiver acceptance, never provider execution.
#[derive(Debug, Serialize)]
pub struct DeliveryReport {
    schema: &'static str,
    authoritative: bool,
    graph_sha256: String,
    payload_sha256: String,
    sequence: u64,
    state: State,
    attempts: u32,
    next_attempt_unix_ms: u64,
    acknowledgement_sha256: Option<String>,
    reason: Option<String>,
}

impl From<&Entry> for DeliveryReport {
    fn from(entry: &Entry) -> Self {
        Self {
            schema: "cohesix-siem-delivery-report/v1",
            authoritative: false,
            graph_sha256: entry.graph_sha256.clone(),
            payload_sha256: entry.payload_sha256.clone(),
            sequence: entry.sequence,
            state: entry.state,
            attempts: entry.attempts,
            next_attempt_unix_ms: entry.next_attempt_unix_ms,
            acknowledgement_sha256: entry.acknowledgement_sha256.clone(),
            reason: entry.reason.clone(),
        }
    }
}

fn hash(bytes: &[u8]) -> String {
    cohesix_evidence::digest(bytes)
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate(wal: &Wal, policy: &Policy, policy_hash: &str) -> Result<()> {
    ensure!(
        wal.schema == "cohesix-siem-wal/v1" && wal.policy_sha256 == policy_hash,
        "EPERM SIEM WAL schema or destination policy"
    );
    ensure!(
        wal.entries.len() <= policy.maximum_entries,
        "ELIMIT SIEM retained entries"
    );
    let mut ids = std::collections::BTreeSet::new();
    let mut previous = 0;
    for entry in &wal.entries {
        ensure!(
            entry.sequence > previous
                && entry.sequence <= wal.next_sequence
                && digest(&entry.graph_sha256)
                && digest(&entry.payload_sha256)
                && ids.insert(&entry.graph_sha256)
                && entry.attempts <= policy.maximum_attempts
                && (entry.state != State::Acknowledged
                    || entry.acknowledgement_sha256.as_deref().is_some_and(digest))
                && entry
                    .reason
                    .as_ref()
                    .is_none_or(|reason| reason.len() <= 64),
            "EPERM SIEM WAL entry"
        );
        previous = entry.sequence;
    }
    Ok(())
}

fn save(root: &Path, wal: &Wal, policy: &Policy) -> Result<()> {
    let bytes = serde_json::to_vec(wal)?;
    ensure!(
        bytes.len() <= policy.maximum_wal_bytes,
        "ELIMIT SIEM WAL bytes"
    );
    let mut staged = tempfile::NamedTempFile::new_in(root)?;
    staged.write_all(&bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(root.join("delivery.json"))?;
    File::open(root)?.sync_all()?;
    Ok(())
}

fn checked_file(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "EPERM SIEM state file"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn state_file(path: &Path, create: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(create).create_new(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "EPERM SIEM regular file required");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.uid() == rustix::process::geteuid().as_raw()
                && metadata.mode() & 0o022 == 0
                && metadata.nlink() == 1,
            "EPERM SIEM private file ownership"
        );
    }
    Ok(file)
}

fn open(root: &Path, policy: &Policy) -> Result<(File, Wal)> {
    if !root.exists() {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(root)?;
    }
    let meta = fs::symlink_metadata(root)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "EPERM SIEM state directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            meta.mode() & 0o077 == 0 && meta.uid() == rustix::process::geteuid().as_raw(),
            "EPERM SIEM state requires private permissions"
        );
    }
    checked_file(&root.join("delivery.lock"))?;
    checked_file(&root.join("delivery.json"))?;
    let fresh = !root.join("delivery.lock").exists();
    if fresh {
        ensure!(
            fs::read_dir(root)?.next().is_none(),
            "EPERM SIEM orphaned state requires reconciliation"
        );
    }
    let lock = state_file(&root.join("delivery.lock"), fresh)?;
    lock.try_lock_exclusive()
        .map_err(|_| anyhow!("busy SIEM state owner"))?;
    let policy_hash = hash(&serde_json::to_vec(policy)?);
    let wal = match state_file(&root.join("delivery.json"), false) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take((policy.maximum_wal_bytes + 1) as u64)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= policy.maximum_wal_bytes,
                "ELIMIT SIEM WAL bytes"
            );
            serde_json::from_slice(&bytes)?
        }
        Err(error)
            if fresh
                && error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Wal {
                schema: "cohesix-siem-wal/v1".into(),
                policy_sha256: policy_hash.clone(),
                next_sequence: 0,
                entries: vec![],
            }
        }
        Err(error) => return Err(error),
    };
    validate(&wal, policy, &policy_hash)?;
    if fresh {
        lock.sync_all()?;
        save(root, &wal, policy)?;
    }
    verify_retained(root, &wal)?;
    Ok((lock, wal))
}

fn retained_bytes(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    checked_file(path)?;
    let mut bytes = Vec::new();
    state_file(path, false)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= maximum, "ELIMIT retained SIEM object");
    Ok(bytes)
}
fn verify_retained(root: &Path, wal: &Wal) -> Result<()> {
    for entry in &wal.entries {
        let payload = retained_bytes(
            &root.join(format!("payload-{}.ndjson", entry.payload_sha256)),
            65536,
        )?;
        ensure!(
            hash(&payload) == entry.payload_sha256,
            "EPERM retained SIEM payload digest"
        );
        if let Some(digest) = &entry.acknowledgement_sha256 {
            let bytes = retained_bytes(&root.join(format!("ack-{digest}.json")), 4096)?;
            ensure!(
                entry.state == State::Acknowledged && hash(&bytes) == *digest,
                "EPERM retained SIEM acknowledgement digest"
            );
            validate_ack(entry, &bytes)?;
        }
    }
    Ok(())
}
fn validate_ack(entry: &Entry, bytes: &[u8]) -> Result<()> {
    let ack: Acknowledgement = serde_json::from_slice(bytes)?;
    ensure!(
        ack.schema == "cohesix-siem-ack/v1"
            && ack.status == "accepted"
            && ack.idempotency_key == entry.graph_sha256
            && ack.payload_sha256 == entry.payload_sha256,
        "EPERM SIEM acknowledgement binding"
    );
    Ok(())
}
fn acknowledge(entry: &mut Entry, bytes: &[u8]) -> Result<()> {
    validate_ack(entry, bytes)?;
    entry.acknowledgement_sha256 = Some(hash(bytes));
    entry.state = State::Acknowledged;
    entry.reason = None;
    entry.next_attempt_unix_ms = 0;
    Ok(())
}

fn failed(entry: &mut Entry, policy: &Policy, now: u64, permanent: bool, reason: &str) {
    entry.reason = Some(reason.into());
    if permanent || entry.attempts >= policy.maximum_attempts {
        entry.state = State::Deadletter;
        entry.next_attempt_unix_ms = 0;
    } else {
        entry.state = State::Pending;
        let backoff = 1000u64
            .saturating_mul(1u64 << entry.attempts.min(8))
            .min(policy.maximum_backoff_ms);
        entry.next_attempt_unix_ms = now.saturating_add(backoff);
    }
}

/// Enqueue or retry one verified graph. Finite retained entries are never silently evicted.
/// Lost ACK retries reuse the same idempotency key; the sink owns receiver-side deduplication.
pub fn deliver(graph: &VerifiedGraph, root: &Path, now: u64) -> Result<DeliveryReport> {
    let value = &cohesix_authority::provider::registry()?["contract"]["siem_delivery"];
    let policy: Policy = serde_json::from_value(value.clone())?;
    ensure!(policy.enabled, "not_enabled SIEM destination");
    ensure!(now > 0, "invalid SIEM delivery time");
    let endpoint = reqwest::Url::parse(&policy.endpoint)?;
    ensure!(
        endpoint.scheme() == "https"
            && endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.fragment().is_none()
            && endpoint.query().is_none(),
        "EPERM SIEM endpoint"
    );
    let payload = super::render(graph, "siem")?;
    let payload_hash = hash(&payload);
    let (_lock, mut wal) = open(root, &policy)?;
    let index = if let Some(index) = wal
        .entries
        .iter()
        .position(|entry| entry.graph_sha256 == graph.digest())
    {
        ensure!(
            wal.entries[index].payload_sha256 == payload_hash,
            "EPERM SIEM projection changed for graph"
        );
        index
    } else {
        ensure!(
            wal.entries.len() < policy.maximum_entries,
            "backpressure SIEM retained entry capacity"
        );
        let path = root.join(format!("payload-{payload_hash}.ndjson"));
        checked_file(&path)?;
        if path.exists() {
            ensure!(
                retained_bytes(&path, 65536)? == payload,
                "EPERM SIEM retained projection"
            );
        } else {
            let mut file = state_file(&path, true)?;
            file.write_all(&payload)?;
            file.sync_all()?;
            File::open(root)?.sync_all()?;
        }
        wal.next_sequence = wal
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| anyhow!("ELIMIT SIEM sequence"))?;
        wal.entries.push(Entry {
            sequence: wal.next_sequence,
            graph_sha256: graph.digest().into(),
            payload_sha256: payload_hash,
            state: State::Pending,
            attempts: 0,
            next_attempt_unix_ms: now,
            acknowledgement_sha256: None,
            reason: None,
        });
        save(root, &wal, &policy)?;
        wal.entries.len() - 1
    };
    if matches!(
        wal.entries[index].state,
        State::Acknowledged | State::Deadletter
    ) || now < wal.entries[index].next_attempt_unix_ms
    {
        return Ok((&wal.entries[index]).into());
    }
    if wal.entries[index].attempts >= policy.maximum_attempts {
        failed(
            &mut wal.entries[index],
            &policy,
            now,
            true,
            "attempts-exhausted",
        );
        save(root, &wal, &policy)?;
        return Ok((&wal.entries[index]).into());
    }
    let credential = cohesix_authority::secret::resolve_reference(&policy.credential_ref)?;
    let mut builder = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none());
    if let Some(reference) = &policy.ca_certificate_path_ref {
        let path = cohesix_authority::secret::resolve_reference(reference)?;
        builder = builder.tls_certs_only([certificate(Path::new(&path))?]);
    }
    let client = builder.build()?;
    wal.entries[index].state = State::Attempting;
    wal.entries[index].attempts += 1;
    // The durable reservation survives a crash before or after the remote write.
    wal.entries[index].next_attempt_unix_ms = now.saturating_add(policy.maximum_backoff_ms);
    save(root, &wal, &policy)?;
    let response = client
        .post(endpoint)
        .bearer_auth(credential)
        .header("Content-Type", "application/x-ndjson")
        .header("Idempotency-Key", graph.digest())
        .header(
            "X-Cohesix-Payload-SHA256",
            &wal.entries[index].payload_sha256,
        )
        .timeout(Duration::from_millis(u64::from(policy.timeout_ms)))
        .body(payload)
        .send();
    match response {
        Ok(response) => {
            let status = response.status();
            let mut bytes = Vec::new();
            let read = response.take(4097).read_to_end(&mut bytes);
            if read.is_ok()
                && bytes.len() <= 4096
                && status.is_success()
                && acknowledge(&mut wal.entries[index], &bytes).is_ok()
            {
                // ACK bytes are bounded, content-addressed and retained before the WAL cursor advances.
                let digest = hash(&bytes);
                let path = root.join(format!("ack-{digest}.json"));
                checked_file(&path)?;
                if path.exists() {
                    ensure!(
                        retained_bytes(&path, 4096)? == bytes,
                        "EPERM changed SIEM ACK"
                    );
                } else {
                    let mut file = state_file(&path, true)?;
                    file.write_all(&bytes)?;
                    file.sync_all()?;
                    File::open(root)?.sync_all()?;
                }
            } else {
                let permanent = status.is_client_error() && status.as_u16() != 429;
                failed(
                    &mut wal.entries[index],
                    &policy,
                    now,
                    permanent,
                    "receiver-refused-or-invalid-ack",
                );
            }
        }
        Err(_) => failed(
            &mut wal.entries[index],
            &policy,
            now,
            false,
            "transport-or-timeout",
        ),
    }
    save(root, &wal, &policy)?;
    Ok((&wal.entries[index]).into())
}

fn certificate(path: &Path) -> Result<reqwest::Certificate> {
    ensure!(
        path.is_absolute()
            && !path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir)),
        "EPERM SIEM CA path"
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    ensure!(
        file.metadata()?.is_file(),
        "EPERM SIEM CA regular file required"
    );
    let mut bytes = Vec::new();
    file.take(16_385).read_to_end(&mut bytes)?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= 16_384,
        "ELIMIT SIEM CA bytes"
    );
    let mut certificates = reqwest::Certificate::from_pem_bundle(&bytes)?;
    ensure!(
        certificates.len() == 1,
        "EPERM SIEM CA requires one PEM certificate"
    );
    certificates
        .pop()
        .ok_or_else(|| anyhow!("EPERM SIEM CA certificate absent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_requires_exact_ack_and_retry_survives_durable_restart() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("private-state");
        let policy = Policy {
            schema: "cohesix-siem-delivery/v1".into(),
            enabled: true,
            endpoint: "https://example.test/events".into(),
            credential_ref: "env:TEST_SIEM".into(),
            ca_certificate_path_ref: None,
            maximum_entries: 2,
            maximum_wal_bytes: 65536,
            maximum_attempts: 2,
            timeout_ms: 1000,
            maximum_backoff_ms: 300000,
        };
        let (lock, mut wal) = open(&state, &policy).unwrap();
        let payload = b"{}\n";
        let payload_hash = hash(payload);
        fs::write(
            state.join(format!("payload-{payload_hash}.ndjson")),
            payload,
        )
        .unwrap();
        wal.next_sequence = 1;
        wal.entries.push(Entry {
            sequence: 1,
            graph_sha256: "a".repeat(64),
            payload_sha256: payload_hash.clone(),
            state: State::Attempting,
            attempts: 1,
            next_attempt_unix_ms: 1000,
            acknowledgement_sha256: None,
            reason: None,
        });
        save(&state, &wal, &policy).unwrap();
        drop(lock);
        let (_lock, mut wal) = open(&state, &policy).unwrap();
        assert_eq!(wal.entries[0].state, State::Attempting);
        failed(
            &mut wal.entries[0],
            &policy,
            1000,
            false,
            "transport-or-timeout",
        );
        assert_eq!(wal.entries[0].next_attempt_unix_ms, 3000);
        let mut ack = serde_json::json!({"schema":"cohesix-siem-ack/v1","idempotency_key":"a".repeat(64),
            "payload_sha256":"c".repeat(64),"status":"accepted"});
        assert!(acknowledge(&mut wal.entries[0], &serde_json::to_vec(&ack).unwrap()).is_err());
        assert_eq!(wal.entries[0].state, State::Pending);
        ack["payload_sha256"] = serde_json::json!(payload_hash);
        acknowledge(&mut wal.entries[0], &serde_json::to_vec(&ack).unwrap()).unwrap();
        assert_eq!(wal.entries[0].state, State::Acknowledged);
        assert!(
            verify_retained(&state, &wal).is_err(),
            "a WAL flag cannot replace absent destination ACK bytes"
        );
        let bytes = serde_json::to_vec(&ack).unwrap();
        let ack_path = state.join(format!("ack-{}.json", hash(&bytes)));
        fs::write(&ack_path, &bytes).unwrap();
        verify_retained(&state, &wal).unwrap();
        fs::write(&ack_path, b"{}").unwrap();
        assert!(
            verify_retained(&state, &wal).is_err(),
            "retained ACK mutation must fail before returning delivered"
        );
        wal.entries[0].attempts = 2;
        failed(
            &mut wal.entries[0],
            &policy,
            5000,
            false,
            "transport-or-timeout",
        );
        assert_eq!(wal.entries[0].state, State::Deadletter);
        drop(_lock);
        fs::remove_file(state.join("delivery.json")).unwrap();
        assert!(
            open(&state, &policy).is_err(),
            "existing owner fence cannot reset a missing delivery WAL"
        );
    }

    #[test]
    fn configured_ca_requires_bounded_regular_pem_without_fallback() {
        let root = tempfile::tempdir().unwrap();
        assert!(certificate(root.path()).is_err());
        assert!(certificate(Path::new("relative.pem")).is_err());
        let path = root.path().join("ca.pem");
        fs::write(&path, b"not a certificate").unwrap();
        assert!(certificate(&path).is_err());
        fs::write(&path, vec![b'a'; 16_385]).unwrap();
        assert!(certificate(&path).is_err());
    }
}
