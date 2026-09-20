// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Preserve bounded source observations across live and offline operator tools.
// Author: Lukas Bower
#![forbid(unsafe_code)]

//! Shared read-only operator projections. Availability is separate from consistency;
//! a locally captured observation never establishes target or execution acceptance.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::CohAccess;

/// Reuse the compiler's finite diagnostic-artifact budget for input and output.
pub const MAX_BYTES: usize = cohsh::TRACE_MAX_BYTES as usize;
/// Bound namespace discovery by the existing console path and directory limits.
pub const MAX_FILES: usize = crate::MAX_DIR_LIST_BYTES / cohsh_core::MAX_PATH_LEN;
/// Version of the additive observation saved inside the canonical evidence pack.
pub const SNAPSHOT_SCHEMA: &str = "cohesix-evidence-pack/inspect-v1";

/// Reverify the original graph and retain exact CAS references beside sanitized observations.
/// Trust and verification time remain explicit; this never asserts current target readiness.
pub fn story(input: &Path, trust: &Path, cas: &Path) -> Result<serde_json::Value> {
    use cohesix_evidence::{verify, verify_cas, Trust, MAX_GRAPH_BYTES};
    let bytes = read_bounded(input, MAX_GRAPH_BYTES)?;
    let trust: Trust = serde_json::from_slice(&read_bounded(trust, 65_536)?)?;
    let graph = verify(&bytes, &trust, |artifact| verify_cas(cas, artifact))?;
    let projection = serde_json::to_value(&graph)?;
    let original: cohesix_evidence::Graph = serde_json::from_slice(&bytes)?;
    let mut artifacts = BTreeMap::new();
    let mut remaining = MAX_BYTES;
    for node in &original.records {
        for artifact in &node.record.artifacts {
            if artifacts.contains_key(&artifact.sha256) || artifact.media_type != "application/json"
            {
                continue;
            }
            let value = if artifact.bytes > remaining as u64 {
                serde_json::json!({"status":"omitted", "reason":"artifact-display-bound"})
            } else {
                let content = read_bounded(&cas.join(&artifact.sha256), remaining)?;
                // Recheck the bytes actually presented, closing replacement between verify and read.
                ensure!(
                    digest(&content) == artifact.sha256 && content.len() as u64 == artifact.bytes,
                    "EPERM evidence-artifact-digest"
                );
                remaining -= content.len();
                let sanitized = sanitize(&content)?;
                let mut value: serde_json::Value = serde_json::from_str(&sanitized)?;
                redact_display_content(&mut value);
                serde_json::json!({"status":"observed", "redaction":"canonical-sensitive-fields", "value":value})
            };
            artifacts.insert(artifact.sha256.clone(), value);
        }
    }
    Ok(
        serde_json::json!({"source":input, "proof":"verified causal evidence at enrolled verification time; not current readiness", "graph":projection, "artifacts":artifacts}),
    )
}

/// UI inspection excludes raw inference content in addition to canonical secret redaction.
pub fn redact_display_content(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, child) in fields {
                if matches!(
                    key.to_ascii_lowercase().as_str(),
                    "prompt"
                        | "prompts"
                        | "completion"
                        | "completions"
                        | "model_output"
                        | "raw_output"
                        | "retrieved_content"
                        | "tool_calls"
                ) {
                    *child = serde_json::json!("<content omitted>");
                } else {
                    redact_display_content(child);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_display_content),
        _ => {}
    }
}

const ROOTS: &[&str] = &[
    "/proc",
    "/proc/boot",
    "/proc/lifecycle",
    "/proc/root",
    "/proc/9p/session",
    "/proc/pressure",
    "/proc/spool/status",
    "/proc/attest",
    "/proc/schedule",
    "/proc/lease",
    "/policy/rules",
];

/// As-built directory nodes must retain their kind even when they are empty.
pub(crate) fn is_namespace_directory(path: &str) -> bool {
    matches!(
        path,
        "/proc"
            | "/proc/lifecycle"
            | "/proc/root"
            | "/proc/9p/session"
            | "/proc/pressure"
            | "/proc/attest"
            | "/proc/schedule"
            | "/proc/lease"
            | "/proc/lease/by-id"
    )
}

/// A source observation, including unavailable and unrecognised source states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Availability {
    /// Bounded bytes were observed.
    Observed,
    /// The source explicitly reported absence or disabled functionality.
    Missing,
    /// Reading or interpreting the source failed.
    Error,
    /// The source uses an unrecognised state or has no capture inventory.
    Unknown,
}

/// One sanitized file observation; reasons contain stable tokens, never peer errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    /// Canonical namespace or pack-relative source path.
    pub path: String,
    /// Availability is not a health or acceptance verdict.
    pub status: Availability,
    /// Sanitized text, absent when the source could not be read.
    pub content: Option<String>,
    /// Stable reason for an unavailable observation.
    pub reason: Option<String>,
}

/// Deterministically ordered observations reusable by field tools and UI clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Versioned additive evidence-pack projection.
    pub schema: String,
    /// Capture surface only; this is never target acceptance.
    pub source_class: String,
    /// Canonically ordered observations, without semantic health inference.
    pub observations: Vec<Observation>,
    /// Independently detectable contradictions in source inventory or exact identities.
    pub violations: Vec<String>,
}

/// A single exact field difference; values are serialized as JSON to escape controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Difference {
    /// Ordered source path and field selector.
    pub field: String,
    /// Left value, or absence.
    pub before: Option<String>,
    /// Right value, or absence.
    pub after: Option<String>,
}

/// Binary attestation command result with typed non-attested reasons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationResult {
    /// Stable result schema shared with Python consumers.
    pub schema: String,
    /// PASS, FAIL or UNAVAILABLE; scope distinguishes live from offline verification.
    pub verdict: String,
    /// Stable failure or dependency reason.
    pub reason: String,
    /// Retains measurement-only, unavailable, or unsupported-signed classification.
    pub evidence_class: String,
    /// Capture surface; offline evidence never becomes fresh live evidence.
    pub source_class: String,
    /// Cryptographic verification metadata, absent for non-attested states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<cohesix_attestation::Verified>,
    /// `none`, `offline-signature`, or `live-signature`; proof class remains explicit.
    #[serde(default = "no_proof")]
    pub proof_scope: String,
}

fn no_proof() -> String {
    "none".to_owned()
}

impl Snapshot {
    /// Human-readable stable output with escaped field contents and no health claim.
    pub fn render(&self) -> Result<String> {
        let mut out = format!(
            "schema={}\nsource_class={}\n",
            self.schema, self.source_class
        );
        for item in &self.observations {
            out.push_str(&serde_json::to_string(item)?);
            out.push('\n');
        }
        for violation in &self.violations {
            out.push_str(&format!(
                "invariant={}\n",
                serde_json::to_string(violation)?
            ));
        }
        ensure!(out.len() <= MAX_BYTES, "inspect-output-bound");
        Ok(out)
    }
}

/// Read public operational surfaces using only bounded list and read operations.
pub fn inspect_live<C: CohAccess + ?Sized>(client: &mut C, source_class: &str) -> Result<Snapshot> {
    ensure!(
        matches!(source_class, "live-console" | "host-projection" | "mock"),
        "source-class"
    );
    let mut observations = BTreeMap::new();
    let mut pending: BTreeSet<String> = ROOTS.iter().map(|p| (*p).to_owned()).collect();
    let mut remaining = MAX_BYTES;
    let mut proc_children: Option<BTreeSet<String>> = None;
    while let Some(path) = pending.pop_first() {
        ensure!(
            observations.len() + pending.len() < MAX_FILES,
            "namespace-artifact-count"
        );
        validate_namespace_path(&path)?;
        // A successful parent listing establishes optional-source absence.
        // A denied or failed listing cannot turn a subsequent read error into
        // absence, and the listing does not expand the selected root set.
        if let Some(name) = path
            .strip_prefix("/proc/")
            .and_then(|p| p.split('/').next())
        {
            if proc_children
                .as_ref()
                .is_some_and(|names| !names.contains(name))
            {
                observations.insert(
                    path.clone(),
                    Observation {
                        path,
                        status: Availability::Missing,
                        content: None,
                        reason: Some("source-missing".to_owned()),
                    },
                );
                continue;
            }
        }
        let directory = is_namespace_directory(&path);
        if directory {
            match client.list_dir(&path, remaining.min(crate::MAX_DIR_LIST_BYTES)) {
                Ok(mut entries) => {
                    ensure!(entries.len() <= MAX_FILES, "namespace-artifact-count");
                    entries.sort();
                    entries.dedup();
                    let mut shape = String::new();
                    for name in entries {
                        crate::validate_component(&name)?;
                        ensure!(
                            !name.chars().any(char::is_control),
                            "namespace-component-control"
                        );
                        let child = format!("{path}/{name}");
                        validate_namespace_path(&child)?;
                        ensure!(
                            pending.len() + observations.len() < MAX_FILES,
                            "namespace-artifact-count"
                        );
                        if path != "/proc" {
                            pending.insert(child);
                        }
                        shape.push_str(&name);
                        shape.push('\n');
                    }
                    consume(&mut remaining, shape.len())?;
                    if path == "/proc" {
                        proc_children = Some(shape.lines().map(str::to_owned).collect());
                    }
                    observations.insert(path.clone(), observed(&path, shape.as_bytes())?);
                }
                Err(error) => {
                    observations.insert(path.clone(), unavailable(&path, &error));
                }
            }
        } else {
            match client.read_file(&path, remaining) {
                Ok(bytes) => {
                    consume(&mut remaining, bytes.len())?;
                    observations.insert(path.clone(), observed(&path, &bytes)?);
                }
                Err(error) => {
                    observations.insert(path.clone(), unavailable(&path, &error));
                }
            }
        }
    }
    let mut snapshot = Snapshot {
        schema: SNAPSHOT_SCHEMA.to_owned(),
        source_class: source_class.to_owned(),
        observations: observations.into_values().collect(),
        violations: Vec::new(),
    };
    validate_fields(&mut snapshot);
    snapshot.render()?;
    Ok(snapshot)
}

/// Inspect only inventory-listed files in a canonical pack; never follow symlinks.
pub fn inspect_pack(root: &Path) -> Result<Snapshot> {
    crate::evidence::verify_pack_integrity(root)?;
    let mut remaining = MAX_BYTES;
    let summary = read_bounded(&root.join("summary.json"), remaining)?;
    consume(&mut remaining, summary.len())?;
    let summary: serde_json::Value =
        serde_json::from_slice(&summary).context("pack-summary-json")?;
    ensure!(
        summary.get("schema").and_then(|v| v.as_str()) == Some("cohesix-evidence-pack/summary-v1"),
        "pack-summary-schema"
    );
    let items = summary
        .get("items")
        .and_then(|v| v.as_array())
        .context("pack-summary-items")?;
    ensure!(items.len() <= MAX_FILES, "pack-artifact-count");
    let mut observations = BTreeMap::new();
    let mut violations = Vec::new();
    let mut counts = [0usize; 3];
    for item in items {
        let path = item
            .get("path")
            .and_then(|v| v.as_str())
            .context("pack-item-path")?;
        validate_namespace_path(path)?;
        let relative = item
            .get("saved_as")
            .and_then(|v| v.as_str())
            .context("pack-item-saved-as")?;
        let saved = confined_path(root, relative)?;
        let status = item
            .get("status")
            .and_then(|v| v.as_str())
            .context("pack-item-status")?;
        let observation = match status {
            "captured" => {
                counts[0] += 1;
                match read_bounded(&saved, remaining) {
                    Ok(bytes) => {
                        consume(&mut remaining, bytes.len())?;
                        // Legacy producers recorded pre-redaction sizes. The retained bytes,
                        // not those legacy size hints, determine this reader's budget.
                        observed(path, &bytes)?
                    }
                    Err(_) => {
                        violations.push(format!("captured-file-unreadable:{relative}"));
                        Observation {
                            path: path.to_owned(),
                            status: Availability::Error,
                            content: None,
                            reason: Some("captured-file-unreadable".to_owned()),
                        }
                    }
                }
            }
            "missing" => {
                counts[1] += 1;
                Observation {
                    path: path.to_owned(),
                    status: Availability::Missing,
                    content: None,
                    reason: Some("source-missing".to_owned()),
                }
            }
            "error" => {
                counts[2] += 1;
                Observation {
                    path: path.to_owned(),
                    status: Availability::Error,
                    content: None,
                    reason: Some("source-error".to_owned()),
                }
            }
            _ => Observation {
                path: path.to_owned(),
                status: Availability::Unknown,
                content: None,
                reason: Some("unknown-source-status".to_owned()),
            },
        };
        ensure!(
            observations.insert(path.to_owned(), observation).is_none(),
            "duplicate-pack-path"
        );
    }
    for (key, count) in ["captured", "missing", "errors"].into_iter().zip(counts) {
        if summary.get(key).and_then(|v| v.as_u64()) != Some(count as u64) {
            violations.push(format!("inventory-count:{key}"));
        }
    }
    // These files describe the collecting host; never label their hashes as target identity.
    for relative in ["meta.json", "bounds.json"] {
        let path = confined_path(root, relative)?;
        if path.exists() {
            let bytes = read_bounded(&path, remaining)?;
            consume(&mut remaining, bytes.len())?;
            observations.insert(relative.to_owned(), observed(relative, &bytes)?);
        }
    }
    violations.sort();
    if root.join("artifact_refs.json").exists() {
        let bytes = read_bounded(&confined_path(root, "artifact_refs.json")?, remaining)?;
        consume(&mut remaining, bytes.len())?;
        let refs: serde_json::Value =
            serde_json::from_slice(&bytes).context("artifact-refs-json")?;
        ensure!(
            refs.get("schema").and_then(|v| v.as_str())
                == Some("cohesix-evidence-pack/artifact-refs-v1"),
            "artifact-refs-schema"
        );
        let items = refs
            .get("items")
            .and_then(|v| v.as_object())
            .context("artifact-refs-items")?;
        ensure!(
            items.len() + observations.len() <= MAX_FILES,
            "pack-artifact-count"
        );
        for (relative, reference) in items {
            let bytes = read_bounded(&confined_path(root, relative)?, remaining)?;
            consume(&mut remaining, bytes.len())?;
            if reference.get("sha256").and_then(|v| v.as_str()) != Some(digest(&bytes).as_str())
                || reference.get("bytes").and_then(|v| v.as_u64()) != Some(bytes.len() as u64)
            {
                violations.push(format!("attachment-identity:{relative}"));
            }
            // Trace payloads are handled by the canonical trace reader, never decoded as text.
            if relative.ends_with(".trace") {
                observations.insert(
                    relative.clone(),
                    observed(relative, &serde_json::to_vec(reference)?)?,
                );
            } else {
                observations.insert(relative.clone(), observed(relative, &bytes)?);
            }
        }
    }
    let mut snapshot = Snapshot {
        schema: SNAPSHOT_SCHEMA.to_owned(),
        source_class: "offline-pack".to_owned(),
        observations: observations.into_values().collect(),
        violations,
    };
    validate_fields(&mut snapshot);
    snapshot.render()?;
    Ok(snapshot)
}

fn validate_fields(snapshot: &mut Snapshot) {
    for item in &snapshot.observations {
        let Some(content) = &item.content else {
            continue;
        };
        let expected = match item.path.as_str() {
            "/proc/root/reachable" => Some(("reachable", false)),
            "/proc/root/last_seen_ms" => Some(("last_seen_ms", true)),
            "/proc/pressure/busy" => Some(("busy", true)),
            "/proc/pressure/quota" => Some(("quota", true)),
            "/proc/pressure/cut" => Some(("cut", true)),
            "/proc/pressure/policy" => Some(("policy", true)),
            _ => None,
        };
        if let Some((key, numeric)) = expected {
            let fields: Vec<_> = content
                .split_whitespace()
                .filter_map(|part| part.split_once('='))
                .filter(|(name, _)| *name == key)
                .collect();
            if fields.len() != 1
                || if numeric {
                    fields
                        .first()
                        .is_none_or(|(_, v)| v.parse::<u64>().is_err())
                } else {
                    fields
                        .first()
                        .is_none_or(|(_, v)| !matches!(*v, "yes" | "no"))
                }
            {
                snapshot
                    .violations
                    .push(format!("invalid-field:{}:{key}", item.path));
            }
        }
    }
    snapshot.violations.sort();
    snapshot.violations.dedup();
}

/// Compare exact normalized observations; JSON objects compare by ordered field path.
pub fn diff(left: &Snapshot, right: &Snapshot) -> Result<Vec<Difference>> {
    fn fields(snapshot: &Snapshot) -> Result<BTreeMap<String, String>> {
        let mut fields = BTreeMap::new();
        for item in &snapshot.observations {
            fields.insert(
                format!("{}/@status", item.path),
                serde_json::to_string(&item.status)?,
            );
            if let Some(content) = &item.content {
                match serde_json::from_str::<serde_json::Value>(content) {
                    Ok(value) => flatten(&format!("{}/@content", item.path), &value, &mut fields)?,
                    Err(_) => {
                        fields.insert(format!("{}/@content", item.path), content.clone());
                    }
                }
            }
        }
        Ok(fields)
    }
    let before = fields(left)?;
    let after = fields(right)?;
    let keys: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    Ok(keys
        .into_iter()
        .filter(|key| before.get(*key) != after.get(*key))
        .map(|key| Difference {
            field: key.clone(),
            before: before.get(key).cloned(),
            after: after.get(key).cloned(),
        })
        .collect())
}

fn flatten(
    prefix: &str,
    value: &serde_json::Value,
    out: &mut BTreeMap<String, String>,
) -> Result<()> {
    if let Some(map) = value.as_object().filter(|m| !m.is_empty()) {
        for (key, value) in map {
            flatten(
                &format!("{prefix}/{}", key.replace('~', "~0").replace('/', "~1")),
                value,
                out,
            )?;
        }
    } else {
        out.insert(prefix.to_owned(), serde_json::to_string(value)?);
    }
    Ok(())
}

/// Classify unsigned snapshots; signed evidence requires explicit verifier-owned policy.
pub fn attest(snapshot: &Snapshot) -> AttestationResult {
    let mut result = AttestationResult {
        schema: "cohesix-attestation-result/v1".to_owned(),
        verdict: "UNAVAILABLE".to_owned(),
        reason: "signed-evidence-unavailable".to_owned(),
        evidence_class: "unavailable".to_owned(),
        source_class: snapshot.source_class.clone(),
        verified: None,
        proof_scope: no_proof(),
    };
    if !snapshot.violations.is_empty() {
        result.verdict = "FAIL".to_owned();
        result.reason = "inconsistent-evidence".to_owned();
        return result;
    }
    for item in &snapshot.observations {
        if item.path.starts_with("/proc/attest") || item.path == "/proc/boot" {
            if item.status == Availability::Error {
                result.verdict = "FAIL".to_owned();
                result.reason = "evidence-read-error".to_owned();
                return result;
            }
            if item.content.as_deref().is_some_and(|text| {
                text.contains("evidence_sha256")
                    || text.contains("measurement_only")
                    || text.contains("manifest_sha256")
            }) {
                result.verdict = "FAIL".to_owned();
                result.reason = "unsigned-measurement".to_owned();
                result.evidence_class = "measurement-only".to_owned();
            }
        }
    }
    result
}

fn observed(path: &str, bytes: &[u8]) -> Result<Observation> {
    let content = sanitize(bytes)?;
    Ok(Observation {
        path: path.to_owned(),
        status: Availability::Observed,
        content: Some(content),
        reason: None,
    })
}

fn unavailable(path: &str, error: &anyhow::Error) -> Observation {
    let missing = crate::evidence::is_missing(error);
    Observation {
        path: path.to_owned(),
        status: if missing {
            Availability::Missing
        } else {
            Availability::Error
        },
        content: None,
        reason: Some(
            if missing {
                "source-missing"
            } else {
                "source-read-error"
            }
            .to_owned(),
        ),
    }
}

/// Redact structured secret fields and embedded JSON before any host persistence.
pub fn sanitize(bytes: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(bytes).context("artifact-utf8")?;
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) {
        crate::evidence::redact_sensitive_value(&mut value);
        return Ok(serde_json::to_string(&value)?);
    }
    let mut out = String::new();
    for line in text.lines() {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(line) {
            crate::evidence::redact_sensitive_value(&mut value);
            out.push_str(&serde_json::to_string(&value)?);
        } else if line
            .split(|c: char| c.is_whitespace() || matches!(c, '=' | ':' | '"' | '\''))
            .any(crate::evidence::sensitive_key)
        {
            out.push_str("<redacted>");
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    Ok(out)
}

/// Validate bounded namespace spelling without permitting parent traversal.
pub fn validate_namespace_path(path: &str) -> Result<()> {
    ensure!(
        path.starts_with('/')
            && path.len() <= cohsh_core::MAX_PATH_LEN
            && !path.chars().any(char::is_control),
        "namespace-path"
    );
    if path == "/" {
        return Ok(());
    }
    let parts: Vec<_> = path[1..].split('/').collect();
    ensure!(parts.len() <= crate::MAX_PATH_COMPONENTS, "namespace-depth");
    for part in parts {
        crate::validate_component(part)?;
    }
    Ok(())
}

/// Confine a pack-relative path and reject symlinks, including directory links.
pub fn confined_path(root: &Path, relative: &str) -> Result<std::path::PathBuf> {
    ensure!(
        !relative.is_empty()
            && relative.len() <= cohsh_core::MAX_PATH_LEN
            && !relative.chars().any(char::is_control),
        "pack-path"
    );
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(name) = component else {
            bail!("pack-path-traversal");
        };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) => ensure!(!meta.file_type().is_symlink(), "pack-path-symlink"),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => bail!("pack-path-metadata"),
        }
    }
    Ok(path)
}

/// Bound allocation even if a regular file grows after its metadata is read.
pub fn read_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path).context("artifact-metadata")?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "artifact-not-regular"
    );
    ensure!(meta.len() <= max_bytes as u64, "artifact-size-bound");
    let mut payload = Vec::new();
    File::open(path)
        .context("artifact-open")?
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut payload)
        .context("artifact-read")?;
    ensure!(payload.len() <= max_bytes, "artifact-size-bound");
    Ok(payload)
}

/// Atomically replace one bounded derived artifact with a unique same-directory temp.
pub fn write_atomic(path: &Path, payload: &[u8]) -> Result<()> {
    ensure!(payload.len() <= MAX_BYTES, "artifact-output-bound");
    let parent = path.parent().context("artifact-parent")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(payload)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|_| anyhow::anyhow!("artifact-commit"))?;
    Ok(())
}

/// Digest exact canonical bytes; this detects corruption and is not a signature.
pub fn digest(payload: &[u8]) -> String {
    hex::encode(Sha256::digest(payload))
}

fn consume(remaining: &mut usize, count: usize) -> Result<()> {
    *remaining = remaining
        .checked_sub(count)
        .context("artifact-total-bound")?;
    Ok(())
}
