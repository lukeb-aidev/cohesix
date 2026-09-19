// Author: Lukas Bower
// Purpose: Restrict host signing to enrolled phase custody and durably append immutable causal records without replaying an operation.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use crate::{
    digest, validate_prefix, verify, verify_cas, Artifact, Binding, Error, Graph, Kind, Outcome,
    Record, SignedRecord, Trust, TrustedKey, MAX_GRAPH_BYTES, SCHEMA,
};
use ed25519_dalek::{Signer, SigningKey};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Per-operation enrollment supplied independently of the evidence pack and request.
/// Each custodian holds its own key reference; the public trust/binding is identical.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub schema: String,
    pub store: PathBuf,
    pub key_id: String,
    pub signing_key_ref: String,
    pub trust: Trust,
    pub expires_unix_ms: u64,
}

/// Enrolled operation with private key custody and an exclusively owned journal.
pub struct Operation {
    pub trust: Trust,
    journal: Journal,
    producer: Producer,
    expires_unix_ms: u64,
}

impl Operation {
    /// Public key identity permits runtime checks for separate custodians;
    /// private key bytes never leave the producer.
    pub fn public_key(&self) -> &str {
        &self.producer.enrollment.public_key
    }

    /// Resolve the independently verified signer of a retained phase.
    pub fn phase_public_key(&self, kind: Kind) -> Result<&str, Error> {
        let signed = self
            .journal
            .graph
            .records
            .iter()
            .find(|node| node.record.kind == kind)
            .ok_or(Error::Causality)?;
        self.trust
            .keys
            .iter()
            .find(|key| key.id == signed.key_id)
            .map(|key| key.public_key.as_str())
            .ok_or(Error::Signature)
    }

    /// Fixed enrollment expiry; retries cannot extend it.
    pub fn expires_unix_ms(&self) -> u64 {
        self.expires_unix_ms
    }
    /// Select an exact request enrollment from a private operator-owned directory.
    /// Neither a ticket body nor an imported record can select a path or trust key.
    pub fn open(
        directory: &Path,
        id: &str,
        idempotency: &str,
        custody: Custody,
        now: u64,
    ) -> Result<Self, Error> {
        private_directory(directory)?;
        let path = directory.join(format!("{}.json", operation_key(id, idempotency)?));
        let bytes = read_private(&path, MAX_GRAPH_BYTES)?;
        let enrollment: Enrollment = serde_json::from_slice(&bytes).map_err(|_| Error::Schema)?;
        if enrollment.schema != "cohesix-producer-enrollment/v1"
            || enrollment.trust.expected.ticket_id != id
            || enrollment.trust.expected.idempotency_key != idempotency
            || now >= enrollment.expires_unix_ms
        {
            return Err(Error::Identity);
        }
        let key = enrollment
            .trust
            .keys
            .iter()
            .find(|key| key.id == enrollment.key_id)
            .ok_or(Error::Signature)?
            .clone();
        let producer = Producer::open(&enrollment.signing_key_ref, key, custody)?;
        let journal = Journal::open(&enrollment.store, enrollment.trust.expected.clone())?;
        let mut trust = enrollment.trust;
        trust.verification_unix_ms = now;
        if !journal.graph.records.is_empty() {
            let bytes = journal.bytes()?;
            validate_prefix(&bytes, &trust, |artifact| {
                verify_cas(&journal.cas, artifact)
            })?;
            if journal
                .graph
                .records
                .iter()
                .any(|node| node.record.kind == Kind::Terminal)
            {
                verify(&bytes, &trust, |artifact| {
                    verify_cas(&journal.cas, artifact)
                })?;
            }
        }
        Ok(Self {
            trust,
            journal,
            producer,
            expires_unix_ms: enrollment.expires_unix_ms,
        })
    }

    /// Check the caller's compiled profile before a native or upstream side effect.
    pub fn bind(
        &self,
        action: &str,
        epoch: u64,
        manifest: &str,
        component: &str,
        executable: &str,
    ) -> Result<(), Error> {
        let expected = &self.trust.expected;
        let registry = cohesix_authority::provider::registry().map_err(|_| Error::Identity)?;
        if expected.action != action
            || expected.writer_epoch != epoch
            || expected.target_manifest_sha256 != manifest
            || registry["graph_sha256"].as_str() != Some(expected.provider_graph_sha256.as_str())
            || expected.component_sha256.get(component).map(String::as_str) != Some(executable)
        {
            return Err(Error::Identity);
        }
        Ok(())
    }

    /// A phase query is valid only after open verified the complete retained prefix.
    pub fn record(&self, kind: Kind) -> Option<&Record> {
        self.journal
            .graph
            .records
            .iter()
            .find(|node| node.record.kind == kind)
            .map(|node| &node.record)
    }

    /// Match admitted request semantics against the signature-verified intent bytes.
    /// Only an explicitly null `args` is equivalent to an omitted `args` in v1.
    pub fn require_json(
        &self,
        kind: Kind,
        expected: &serde_json::Value,
        null_args: bool,
    ) -> Result<(), Error> {
        let mut actual = self.read_json(kind)?;
        if null_args && actual.get("args").is_some_and(serde_json::Value::is_null) {
            actual.as_object_mut().ok_or(Error::Schema)?.remove("args");
        }
        if actual != *expected {
            return Err(Error::Identity);
        }
        Ok(())
    }

    /// Read the single canonical JSON artifact only after signature and CAS
    /// verification. Consumers still validate the artifact's specific contract.
    pub fn read_json(&self, kind: Kind) -> Result<serde_json::Value, Error> {
        let record = self.record(kind).ok_or(Error::Causality)?;
        if record.artifacts.len() != 1 {
            return Err(Error::Causality);
        }
        let artifact = &record.artifacts[0];
        verify_cas(&self.journal.cas, artifact)?;
        let bytes = read_private(&self.journal.cas.join(&artifact.sha256), MAX_GRAPH_BYTES)?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Schema)
    }

    /// Retain an observed payload and append once. An exact retry keeps the original
    /// signature and expiry; changed facts or a later phase cannot replace history.
    pub fn emit(
        &mut self,
        kind: Kind,
        payload: &[u8],
        native: Option<(&str, u64)>,
        outcome: Outcome,
        now: u64,
    ) -> Result<(), Error> {
        self.trust.verification_unix_ms = now;
        let artifact = self.journal.retain(payload, "application/json")?;
        let (native_identity, resource_generation) = match native {
            Some((identity, generation)) => (Some(identity.to_owned()), generation),
            None => (None, 0),
        };
        if let Some(record) = self.record(kind) {
            if record.source != self.producer.enrollment.source
                || record.artifacts != [artifact]
                || record.native_identity != native_identity
                || record.resource_generation != resource_generation
                || record.outcome != outcome
                || record.expires_unix_ms != self.expires_unix_ms
            {
                return Err(Error::Causality);
            }
            validate_prefix(&self.journal.bytes()?, &self.trust, |artifact| {
                verify_cas(&self.journal.cas, artifact)
            })?;
            return Ok(());
        }
        self.journal.append(
            &self.producer,
            &self.trust,
            Event {
                kind,
                expires_unix_ms: self.expires_unix_ms,
                artifacts: vec![artifact],
                native_identity,
                resource_generation,
                event_cursor: None,
                outcome,
            },
        )
    }
}

/// Collision-free request enrollment key, independent of client-controlled paths.
pub fn operation_key(id: &str, idempotency: &str) -> Result<String, Error> {
    crate::bounded_id(id)?;
    crate::bounded_id(idempotency)?;
    Ok(digest(
        &serde_json::to_vec(&(id, idempotency)).map_err(|_| Error::Schema)?,
    ))
}

/// Hash the running native component, with a fixed bound, for enrolled provenance.
pub fn executable_digest() -> Result<String, Error> {
    use sha2::{Digest, Sha256};
    let mut file = File::open(std::env::current_exe().map_err(|_| Error::Missing)?)
        .map_err(|_| Error::Missing)?;
    if file.metadata().map_err(|_| Error::Missing)?.len() > 512 * 1024 * 1024 {
        return Err(Error::Limit);
    }
    let mut hasher = Sha256::new();
    let mut total = 0usize;
    let mut buffer = [0; 8192];
    loop {
        let size = file.read(&mut buffer).map_err(|_| Error::Missing)?;
        if size == 0 {
            break;
        }
        total = total.checked_add(size).ok_or(Error::Limit)?;
        if total > 512 * 1024 * 1024 {
            return Err(Error::Limit);
        }
        hasher.update(&buffer[..size]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// App-selected custody, independent of any imported graph or client request.
#[derive(Debug, Clone, Copy)]
pub enum Custody {
    /// The gateway witnesses its identity decision and the authenticated Root admission.
    GatewayAdmission,
    /// A native executor witnesses its own operation and observed postcondition.
    NativeOperation,
    /// A receiver witnesses the exact Root-pinned executable Worker terminal record.
    WorkerWitness,
}

impl Custody {
    fn kinds(self) -> BTreeSet<Kind> {
        BTreeSet::from_iter(match self {
            Self::GatewayAdmission => vec![Kind::Intent, Kind::Facts, Kind::Approval, Kind::Grant],
            Self::NativeOperation => vec![
                Kind::Execution,
                Kind::Observation,
                Kind::Verification,
                Kind::Terminal,
            ],
            Self::WorkerWitness => vec![Kind::Worker, Kind::Terminal],
        })
    }
}

/// Private key custody. This type deliberately has no Debug or serialization.
pub struct Producer {
    key: SigningKey,
    enrollment: TrustedKey,
}

impl Producer {
    /// Resolve one explicit secret reference and match the independently enrolled key.
    /// This does not enroll keys, grant actions, or accept key material from a request.
    pub fn open(reference: &str, enrollment: TrustedKey, custody: Custody) -> Result<Self, Error> {
        crate::bounded_id(&enrollment.id)?;
        crate::bounded_id(&enrollment.source)?;
        if enrollment.kinds.is_empty()
            || !enrollment.kinds.is_subset(&custody.kinds())
            || enrollment.not_before_unix_ms >= enrollment.not_after_unix_ms
        {
            return Err(Error::Signature);
        }
        let encoded = cohesix_authority::secret::resolve_reference(reference)
            .map_err(|_| Error::Signature)?;
        let seed: [u8; 32] = hex::decode(encoded.trim())
            .map_err(|_| Error::Signature)?
            .try_into()
            .map_err(|_| Error::Signature)?;
        let key = SigningKey::from_bytes(&seed);
        if hex::encode(key.verifying_key().to_bytes()) != enrollment.public_key {
            return Err(Error::Signature);
        }
        Ok(Self { key, enrollment })
    }

    fn extend(&self, graph: &Graph, trust: &Trust, event: Event) -> Result<Graph, Error> {
        if graph.binding != trust.expected
            || !self.enrollment.kinds.contains(&event.kind)
            || !trust.keys.iter().any(|key| {
                key.id == self.enrollment.id
                    && key.source == self.enrollment.source
                    && key.public_key == self.enrollment.public_key
                    && key.kinds == self.enrollment.kinds
                    && key.not_before_unix_ms == self.enrollment.not_before_unix_ms
                    && key.not_after_unix_ms == self.enrollment.not_after_unix_ms
            })
        {
            return Err(Error::Signature);
        }
        if graph
            .records
            .iter()
            .any(|signed| signed.record.kind == event.kind || signed.record.kind == Kind::Terminal)
        {
            return Err(Error::Causality);
        }
        let record = Record {
            schema: "cohesix-causal-record/v1".into(),
            binding: graph.binding.clone(),
            kind: event.kind,
            source: self.enrollment.source.clone(),
            sequence: graph.records.len() as u64 + 1,
            observed_unix_ms: trust.verification_unix_ms,
            expires_unix_ms: event.expires_unix_ms,
            parents: graph
                .records
                .iter()
                .map(|node| node.sha256.clone())
                .collect(),
            artifacts: event.artifacts,
            native_identity: event.native_identity,
            resource_generation: event.resource_generation,
            event_cursor: event.event_cursor,
            outcome: event.outcome,
        };
        let bytes = serde_json::to_vec(&record).map_err(|_| Error::Schema)?;
        let mut next = graph.clone();
        next.records.push(SignedRecord {
            record,
            sha256: digest(&bytes),
            key_id: self.enrollment.id.clone(),
            signature: hex::encode(self.key.sign(&bytes).to_bytes()),
        });
        Ok(next)
    }
}

/// Native facts supplied by the owning application at the observed lifecycle edge.
/// The application must not populate these fields from a client's claimed result.
pub struct Event {
    pub kind: Kind,
    pub expires_unix_ms: u64,
    pub artifacts: Vec<Artifact>,
    pub native_identity: Option<String>,
    pub resource_generation: u64,
    pub event_cursor: Option<String>,
    pub outcome: Outcome,
}

/// One locked operation journal and immutable CAS. A partial journal is never a receipt.
/// Operator-controlled parent directories must remain private for the process lifetime.
pub struct Journal {
    directory: PathBuf,
    cas: PathBuf,
    graph: Graph,
    _lock: File,
}

impl Journal {
    /// Open an existing private store. A lost existing graph fails closed, never resets.
    pub fn open(root: &Path, binding: Binding) -> Result<Self, Error> {
        crate::binding_valid(&binding)?;
        if !root.is_absolute() {
            return Err(Error::Identity);
        }
        private_directory(root)?;
        let cas = root.join("cas");
        let fresh_cas = create_private_directory(&cas)?;
        if fresh_cas {
            private_file(&cas.join("owner.lock"), true)?
                .sync_all()
                .map_err(|_| Error::Missing)?;
            sync_directory(&cas)?;
        }
        let admission_lock = private_file(&cas.join("owner.lock"), false)?;
        admission_lock
            .try_lock_exclusive()
            .map_err(|_| Error::Causality)?;
        let key = digest(&serde_json::to_vec(&binding).map_err(|_| Error::Schema)?);
        let directory = root.join(key);
        if !directory.exists()
            && fs::read_dir(root)
                .map_err(|_| Error::Missing)?
                .take(4096)
                .count()
                >= 4096
        {
            return Err(Error::Limit);
        }
        let fresh = create_private_directory(&directory)?;
        let lock_path = directory.join("owner.lock");
        let lock = private_file(&lock_path, fresh)?;
        lock.try_lock_exclusive().map_err(|_| Error::Causality)?;
        let path = directory.join("graph.json");
        let graph = if fresh {
            let graph = Graph {
                schema: SCHEMA.into(),
                binding,
                records: Vec::new(),
            };
            replace(
                &path,
                &serde_json::to_vec(&graph).map_err(|_| Error::Schema)?,
            )?;
            graph
        } else {
            let bytes = read_private(&path, MAX_GRAPH_BYTES)?;
            let graph: Graph = serde_json::from_slice(&bytes).map_err(|_| Error::Schema)?;
            if graph.binding != binding
                || graph.schema != SCHEMA
                || serde_json::to_vec(&graph).map_err(|_| Error::Schema)? != bytes
            {
                return Err(Error::Identity);
            }
            graph
        };
        Ok(Self {
            directory,
            cas,
            graph,
            _lock: lock,
        })
    }

    /// Retain actual bytes before linking them from a signed record. Never fetch URLs.
    pub fn retain(&self, bytes: &[u8], media_type: &str) -> Result<Artifact, Error> {
        if bytes.is_empty() || bytes.len() > crate::MAX_ARTIFACT_BYTES as usize {
            return Err(Error::Limit);
        }
        let artifact = Artifact {
            sha256: digest(bytes),
            bytes: bytes.len() as u64,
            media_type: media_type.into(),
        };
        let lock = private_file(&self.cas.join("owner.lock"), false)?;
        lock.try_lock_exclusive().map_err(|_| Error::Causality)?;
        let path = self.cas.join(&artifact.sha256);
        match fs::symlink_metadata(&path) {
            Ok(_) => verify_cas(&self.cas, &artifact)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if fs::read_dir(&self.cas)
                    .map_err(|_| Error::Missing)?
                    .take(4096)
                    .count()
                    >= 4096
                {
                    return Err(Error::Limit);
                }
                let mut file = private_file(&path, true)?;
                file.write_all(bytes).map_err(|_| Error::Missing)?;
                file.sync_all().map_err(|_| Error::Missing)?;
                sync_directory(&self.cas)?;
            }
            Err(_) => return Err(Error::Missing),
        }
        Ok(artifact)
    }

    /// Import a verified prefix from another enrolled custodian without overwriting history.
    /// Referenced CAS bytes must already have been retained and verified locally.
    pub fn import(&mut self, bytes: &[u8], trust: &Trust) -> Result<(), Error> {
        let (graph, _) = validate_prefix(bytes, trust, |artifact| verify_cas(&self.cas, artifact))?;
        if !graph.records.starts_with(&self.graph.records) || graph.binding != self.graph.binding {
            return Err(Error::Causality);
        }
        if graph
            .records
            .iter()
            .any(|signed| signed.record.kind == Kind::Terminal)
        {
            verify(bytes, trust, |artifact| verify_cas(&self.cas, artifact))?;
        }
        replace(&self.directory.join("graph.json"), bytes)?;
        self.graph = graph;
        Ok(())
    }

    /// Append exactly one lifecycle edge after validating the entire inherited chain.
    /// A restart retains existing signatures; duplicate phases cannot mint a fresh TTL.
    pub fn append(
        &mut self,
        producer: &Producer,
        trust: &Trust,
        event: Event,
    ) -> Result<(), Error> {
        let graph = producer.extend(&self.graph, trust, event)?;
        let bytes = serde_json::to_vec(&graph).map_err(|_| Error::Schema)?;
        self.import(&bytes, trust)
    }

    /// Canonical bytes for transport to the next custodian or final shared verification.
    pub fn bytes(&self) -> Result<Vec<u8>, Error> {
        serde_json::to_vec(&self.graph).map_err(|_| Error::Schema)
    }
}

fn private_directory(path: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|_| Error::Missing)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Identity);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
            return Err(Error::Identity);
        }
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<bool, Error> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    let fresh = match builder.create(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(_) => return Err(Error::Missing),
    };
    private_directory(path)?;
    if fresh {
        sync_directory(path.parent().ok_or(Error::Identity)?)?;
    }
    Ok(fresh)
}

fn private_file(path: &Path, create: bool) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options.read(true).write(create).create_new(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|_| Error::Missing)?;
    let metadata = file.metadata().map_err(|_| Error::Missing)?;
    if !metadata.is_file() {
        return Err(Error::Identity);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return Err(Error::Identity);
        }
    }
    Ok(file)
}

fn read_private(path: &Path, limit: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    private_file(path, false)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Missing)?;
    if bytes.len() > limit {
        return Err(Error::Limit);
    }
    Ok(bytes)
}

fn replace(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() > MAX_GRAPH_BYTES {
        return Err(Error::Limit);
    }
    let parent = path.parent().ok_or(Error::Identity)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| Error::Missing)?;
    file.write_all(bytes).map_err(|_| Error::Missing)?;
    file.as_file().sync_all().map_err(|_| Error::Missing)?;
    file.persist(path).map_err(|_| Error::Missing)?;
    sync_directory(parent)
}

fn sync_directory(path: &Path) -> Result<(), Error> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| Error::Missing)
}
