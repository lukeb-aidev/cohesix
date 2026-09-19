// Author: Lukas Bower
// Purpose: Install only bounded, signed, compiler-owned host artifact sets using external trust.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{anyhow, bail, ensure, Context, Result};
use cohesix_authority::package::{
    ArtifactKind, Profile, Requirement, MANIFEST_SCHEMA, MAX_ARTIFACT_BYTES, MAX_FILES,
    MAX_PACKAGE_BYTES,
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

const MANIFEST_LIMIT: u64 = 256 * 1024;
const DOMAIN: &[u8] = b"cohesix-host-package-signature/v1\0";
const SBOM: &str = "package.sbom.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    requirement: Requirement,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    profile: Profile,
    provider_graph_sha256: String,
    source_sha256: String,
    artifacts: Vec<Artifact>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedManifest {
    manifest: Manifest,
    key_id: String,
    signature: String,
}

/// Independently enrolled package signer and exact release selection. This
/// policy is never loaded from the package being verified.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trust {
    /// Must equal cohesix-host-package-trust/v1.
    pub schema: String,
    /// Exact compiler-owned deployment profile id.
    pub profile_id: String,
    /// Exact release version; no implicit downgrade or upgrade range.
    pub version: String,
    /// SHA-256 of the enrolled source inventory.
    pub source_sha256: String,
    /// Public Ed25519 keys, encoded as lowercase hexadecimal.
    pub keys: BTreeMap<String, String>,
}

/// Non-authoritative inspection result. A valid package signature attests the
/// file set, not provider execution, deployment enrollment or service health.
#[derive(Debug, Serialize)]
pub struct Report {
    schema: &'static str,
    authoritative: bool,
    profile_id: String,
    version: String,
    provider_graph_sha256: String,
    source_sha256: String,
    manifest_sha256: String,
    signer: String,
    artifact_count: usize,
    artifact_bytes: u64,
    required_credentials: Vec<String>,
}

struct Verified {
    report: Report,
    signed: SignedManifest,
    files: BTreeMap<String, Vec<u8>>,
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn hash(bytes: &[u8]) -> String {
    cohesix_evidence::digest(bytes)
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn profile(id: &str) -> Result<(Profile, String)> {
    let registry = cohesix_authority::provider::registry()?;
    let selected = registry["contract"]["deployment_profiles"]
        .as_array()
        .and_then(|profiles| profiles.iter().find(|profile| profile["id"] == id))
        .ok_or_else(|| anyhow!("EPERM package profile not registered"))?;
    let profile: Profile = serde_json::from_value(selected.clone())?;
    profile.validate()?;
    let graph = registry["graph_sha256"]
        .as_str()
        .filter(|value| digest(value))
        .ok_or_else(|| anyhow!("EPERM package provider graph"))?;
    Ok((profile, graph.to_owned()))
}

fn signature_bytes(manifest: &Manifest, key_id: &str) -> Result<Vec<u8>> {
    // Struct serialization fixes field order; no maps occur in the signed body.
    // The domain and key id prevent cross-protocol reuse or signer substitution.
    let mut bytes = DOMAIN.to_vec();
    bytes.extend(serde_json::to_vec(&(key_id, manifest))?);
    Ok(bytes)
}

fn check_components(path: &Path) -> Result<()> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        ensure!(
            !matches!(component, Component::ParentDir),
            "EPERM package parent path"
        );
        current.push(component);
        ensure!(
            !fs::symlink_metadata(&current)?.file_type().is_symlink(),
            "EPERM package symlink"
        );
    }
    Ok(())
}

fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>> {
    check_components(path)?;
    let before = fs::symlink_metadata(path)?;
    ensure!(
        before.is_file() && before.len() <= limit,
        "EPERM package file kind or size"
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            before.dev() == opened.dev() && before.ino() == opened.ino(),
            "EPERM package replaced file"
        );
        ensure!(
            opened.mode() & 0o7022 == 0,
            "EPERM package writable or privileged file"
        );
    }
    ensure!(
        opened.is_file() && opened.len() <= limit,
        "EPERM package opened file"
    );
    let mut bytes = Vec::new();
    (&file).take(limit + 1).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    ensure!(
        bytes.len() as u64 == opened.len()
            && after.len() == opened.len()
            && after.modified()? == opened.modified()?,
        "EPERM package changed file"
    );
    Ok(bytes)
}

fn check_tree(root: &Path, files: &BTreeSet<String>) -> Result<()> {
    check_components(root)?;
    ensure!(fs::symlink_metadata(root)?.is_dir(), "EPERM package root");
    let mut directories = BTreeSet::new();
    for file in files {
        let mut path = Path::new(file).parent();
        while let Some(parent) = path.filter(|parent| !parent.as_os_str().is_empty()) {
            directories.insert(parent.to_string_lossy().into_owned());
            path = parent.parent();
        }
    }
    let mut pending = vec![PathBuf::new()];
    let mut seen = BTreeSet::new();
    let mut entries = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(root.join(&directory))? {
            entries += 1;
            ensure!(entries <= MAX_FILES * 16, "ELIMIT package tree");
            let entry = entry?;
            let relative = directory.join(entry.file_name());
            let name = relative
                .to_str()
                .ok_or_else(|| anyhow!("EPERM package filename"))?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                ensure!(
                    directories.contains(name),
                    "EPERM unexpected package directory"
                );
                pending.push(relative);
            } else {
                ensure!(
                    kind.is_file() && files.contains(name),
                    "EPERM unexpected package file"
                );
                ensure!(seen.insert(name.to_owned()), "EPERM duplicate package file");
            }
        }
    }
    ensure!(&seen == files, "EPERM missing package file");
    Ok(())
}

fn integer(bytes: &[u8], offset: usize, width: usize) -> Result<u64> {
    let field = bytes
        .get(offset..offset + width)
        .ok_or_else(|| anyhow!("EPERM truncated native image"))?;
    Ok(field.iter().enumerate().fold(0, |value, (index, byte)| {
        value | (u64::from(*byte) << (index * 8))
    }))
}

fn native_image(bytes: &[u8], kind: ArtifactKind, architecture: &str) -> Result<()> {
    match kind {
        ArtifactKind::Elf => {
            // ELF64 little-endian executable/PIE, with an executable PT_LOAD
            // segment containing e_entry. See the System V ELF gABI.
            ensure!(
                bytes.starts_with(b"\x7fELF\x02\x01\x01"),
                "EPERM ELF format"
            );
            let machine = if architecture == "aarch64" { 183 } else { 62 };
            ensure!(
                integer(bytes, 18, 2)? == machine
                    && matches!(integer(bytes, 16, 2)?, 2 | 3)
                    && integer(bytes, 20, 4)? == 1
                    && integer(bytes, 52, 2)? == 64
                    && integer(bytes, 54, 2)? == 56,
                "EPERM ELF architecture or header"
            );
            let entry = integer(bytes, 24, 8)?;
            let table = integer(bytes, 32, 8)?;
            let count = integer(bytes, 56, 2)?;
            ensure!(
                (1..=1024).contains(&count)
                    && table >= 64
                    && table
                        .checked_add(count * 56)
                        .is_some_and(|end| end <= bytes.len() as u64),
                "EPERM ELF segments"
            );
            let mut executable = false;
            for index in 0..count {
                let offset = (table + index * 56) as usize;
                let file_offset = integer(bytes, offset + 8, 8)?;
                let size = integer(bytes, offset + 32, 8)?;
                ensure!(
                    file_offset
                        .checked_add(size)
                        .is_some_and(|end| end <= bytes.len() as u64),
                    "EPERM ELF segment bounds"
                );
                if integer(bytes, offset, 4)? == 1 && integer(bytes, offset + 4, 4)? & 1 != 0 {
                    let address = integer(bytes, offset + 16, 8)?;
                    executable |= entry >= address
                        && address.checked_add(size).is_some_and(|end| entry < end);
                }
            }
            ensure!(executable, "EPERM ELF missing executable entry");
        }
        ArtifactKind::MachO => {
            // Single-architecture mach_header_64 and bounded load commands.
            // MH_EXECUTE excludes object files, dylibs, and MH_DYLIB_STUB.
            let cpu = if architecture == "aarch64" {
                0x0100_000c
            } else {
                0x0100_0007
            };
            ensure!(
                integer(bytes, 0, 4)? == 0xfeed_facf
                    && integer(bytes, 4, 4)? == cpu
                    && integer(bytes, 12, 4)? == 2,
                "EPERM Mach-O architecture or kind"
            );
            let count = integer(bytes, 16, 4)?;
            let command_bytes = integer(bytes, 20, 4)? as usize;
            ensure!(
                (1..=1024).contains(&count) && command_bytes <= bytes.len().saturating_sub(32),
                "EPERM Mach-O commands"
            );
            let mut offset = 32;
            let mut entry = None;
            let mut executable = Vec::new();
            for _ in 0..count {
                let command = integer(bytes, offset, 4)?;
                let size = integer(bytes, offset + 4, 4)? as usize;
                ensure!(
                    size >= 8 && size.is_multiple_of(8) && offset + size <= 32 + command_bytes,
                    "EPERM Mach-O command bounds"
                );
                if command == 0x8000_0028 {
                    ensure!(size == 24 && entry.is_none(), "EPERM Mach-O entry command");
                    entry = Some(integer(bytes, offset + 8, 8)?);
                } else if command == 0x19 {
                    ensure!(size >= 72, "EPERM Mach-O segment header");
                    let start = integer(bytes, offset + 40, 8)?;
                    let length = integer(bytes, offset + 48, 8)?;
                    let end = start
                        .checked_add(length)
                        .filter(|end| *end <= bytes.len() as u64)
                        .ok_or_else(|| anyhow!("EPERM Mach-O segment bounds"))?;
                    if integer(bytes, offset + 60, 4)? & 4 != 0 {
                        executable.push(start..end);
                    }
                }
                offset += size;
            }
            ensure!(
                offset == 32 + command_bytes
                    && entry
                        .is_some_and(|entry| executable.iter().any(|range| range.contains(&entry))),
                "EPERM Mach-O missing executable entry"
            );
        }
        _ => bail!("EPERM non-native artifact"),
    }
    Ok(())
}

fn content(requirement: &Requirement, bytes: &[u8], profile: &Profile) -> Result<()> {
    ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= MAX_ARTIFACT_BYTES,
        "ELIMIT package artifact"
    );
    let value = match requirement.kind {
        ArtifactKind::Elf | ArtifactKind::MachO => {
            native_image(bytes, requirement.kind, &profile.architecture)?;
            None
        }
        ArtifactKind::Json => Some(serde_json::from_slice::<Value>(bytes)?),
        ArtifactKind::Toml => Some(serde_json::to_value(toml::from_str::<toml::Value>(
            std::str::from_utf8(bytes)?,
        )?)?),
        ArtifactKind::Text => {
            let text = std::str::from_utf8(bytes)?;
            ensure!(
                !text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
                "EPERM package text"
            );
            None
        }
        // Wheels have their own exact inventory/RECORD validator and clean
        // interpreter install workflow. No selected native profile embeds one.
        ArtifactKind::PythonWheel => bail!("ENOTSUP use the Python package manifest workflow"),
    };
    if let Some(pointer) = &requirement.schema_pointer {
        ensure!(
            value
                .as_ref()
                .and_then(|value| value.pointer(pointer))
                .and_then(Value::as_str)
                == requirement.schema_value.as_deref(),
            "EPERM package artifact schema"
        );
    }
    if requirement.path == "contracts/provider_registry.json" {
        ensure!(
            bytes == cohesix_authority::provider::registry_json().as_bytes(),
            "EPERM package registry differs from compiler"
        );
    }
    if requirement.path == "config/root_task_resolved.json" {
        let value = value
            .as_ref()
            .ok_or_else(|| anyhow!("EPERM package root manifest"))?;
        ensure!(
            value.pointer("/authority/production") == Some(&Value::Bool(true))
                && value.pointer("/authority/debug_memory") == Some(&Value::Bool(false))
                && value.pointer("/authority/execution_wal_required") == Some(&Value::Bool(true)),
            "EPERM package requires production authority"
        );
        let tickets = value["tickets"]
            .as_array()
            .ok_or_else(|| anyhow!("EPERM package ticket references"))?;
        ensure!(
            !tickets.is_empty(),
            "EPERM package missing ticket references"
        );
        for ticket in tickets {
            ensure!(
                ticket
                    .get("secret")
                    .is_none_or(|value| value.is_null() || value.as_str() == Some("")),
                "EPERM package embedded ticket secret"
            );
            cohesix_authority::secret::validate_reference(
                ticket["secret_ref"]
                    .as_str()
                    .ok_or_else(|| anyhow!("EPERM package ticket secret reference"))?,
            )?;
        }
    }
    Ok(())
}

fn sbom(profile: &Profile, artifacts: &[Artifact]) -> Value {
    let components: Vec<Value> = artifacts
        .iter()
        .filter(|artifact| artifact.requirement.path != SBOM)
        .map(|artifact| {
            json!({"type":"file", "name":artifact.requirement.path,
            "version":artifact.requirement.version,
            "hashes":[{"alg":"SHA-256", "content":artifact.sha256}]})
        })
        .collect();
    json!({"bomFormat":"CycloneDX", "specVersion":"1.6", "version":1,
        "metadata":{"component":{"type":"application", "name":profile.id, "version":profile.version},
            "properties":[{"name":"cohesix:inventory-scope", "value":"packaged-files; excludes external runtime dependencies"}]},
        "components":components})
}

fn verify_snapshot(
    root: &Path,
    trust: &Trust,
    expected: &Profile,
    graph: &str,
) -> Result<Verified> {
    ensure!(
        trust.schema == "cohesix-host-package-trust/v1"
            && trust.profile_id == expected.id
            && trust.version == expected.version
            && digest(&trust.source_sha256)
            && !trust.keys.is_empty()
            && trust.keys.len() <= 16
            && trust
                .keys
                .iter()
                .all(|(id, key)| identifier(id) && digest(key)),
        "EPERM package trust"
    );
    let manifest_bytes = read_regular(&root.join("package.json"), MANIFEST_LIMIT)?;
    let signed: SignedManifest = serde_json::from_slice(&manifest_bytes)?;
    let manifest = &signed.manifest;
    ensure!(
        serde_json::to_vec(&signed)? == manifest_bytes,
        "EPERM noncanonical package manifest"
    );
    ensure!(
        manifest.schema == MANIFEST_SCHEMA
            && manifest.profile == *expected
            && manifest.provider_graph_sha256 == graph
            && manifest.source_sha256 == trust.source_sha256
            && manifest.artifacts.len() == expected.artifacts.len(),
        "EPERM package profile or version"
    );
    let key = trust
        .keys
        .get(&signed.key_id)
        .ok_or_else(|| anyhow!("EPERM unenrolled package signer"))?;
    let key: [u8; 32] = hex::decode(key)
        .map_err(|_| anyhow!("EPERM package key encoding"))?
        .try_into()
        .map_err(|_| anyhow!("EPERM package key"))?;
    let signature: [u8; 64] = hex::decode(&signed.signature)
        .map_err(|_| anyhow!("EPERM package signature encoding"))?
        .try_into()
        .map_err(|_| anyhow!("EPERM package signature"))?;
    VerifyingKey::from_bytes(&key)?.verify_strict(
        &signature_bytes(manifest, &signed.key_id)?,
        &Signature::from_bytes(&signature),
    )?;
    let mut names: BTreeSet<String> = expected
        .artifacts
        .iter()
        .map(|artifact| artifact.path.clone())
        .collect();
    names.insert("package.json".to_owned());
    check_tree(root, &names)?;
    let mut files = BTreeMap::new();
    let mut total = 0_u64;
    for (artifact, requirement) in manifest.artifacts.iter().zip(&expected.artifacts) {
        ensure!(
            artifact.requirement == *requirement && digest(&artifact.sha256),
            "EPERM package artifact declaration"
        );
        let path = root.join(&requirement.path);
        let bytes = read_regular(&path, MAX_ARTIFACT_BYTES)?;
        ensure!(
            bytes.len() as u64 == artifact.bytes && hash(&bytes) == artifact.sha256,
            "EPERM package artifact digest"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                (fs::metadata(&path)?.permissions().mode() & 0o111 != 0) == requirement.executable,
                "EPERM package execute permission"
            );
        }
        total += bytes.len() as u64;
        ensure!(total <= MAX_PACKAGE_BYTES, "ELIMIT package bytes");
        content(requirement, &bytes, expected)
            .with_context(|| format!("artifact {}", requirement.path))?;
        let expected_config_hash = match requirement.path.as_str() {
            "config/coh_policy.toml" => Some(crate::policy::CohPolicy::policy_hash()),
            "config/cohsh_policy.toml" => Some(cohsh::policy::CohshPolicy::policy_hash()),
            "config/root_task_resolved.json" => {
                cohesix_authority::provider::registry()?["resolved_manifest_sha256"].as_str()
            }
            _ => None,
        };
        if let Some(expected_hash) = expected_config_hash {
            ensure!(
                artifact.sha256 == expected_hash,
                "EPERM package selected configuration digest"
            );
        }
        files.insert(requirement.path.clone(), bytes);
    }
    let actual_sbom: Value = serde_json::from_slice(
        files
            .get(SBOM)
            .ok_or_else(|| anyhow!("EPERM missing SBOM"))?,
    )?;
    ensure!(
        actual_sbom == sbom(expected, &manifest.artifacts),
        "EPERM package SBOM inventory"
    );
    files.insert("package.json".to_owned(), manifest_bytes.clone());
    let report = Report {
        schema: "cohesix-host-package-report/v1",
        authoritative: false,
        profile_id: expected.id.clone(),
        version: expected.version.clone(),
        provider_graph_sha256: graph.to_owned(),
        source_sha256: trust.source_sha256.clone(),
        manifest_sha256: hash(&manifest_bytes),
        signer: signed.key_id.clone(),
        artifact_count: manifest.artifacts.len(),
        artifact_bytes: total,
        required_credentials: expected.required_credentials.clone(),
    };
    Ok(Verified {
        report,
        signed,
        files,
    })
}

/// Read bounded, explicit external trust. No package-supplied trust fallback exists.
pub fn load_trust(path: &Path) -> Result<Trust> {
    Ok(serde_json::from_slice(&read_regular(
        path,
        MANIFEST_LIMIT,
    )?)?)
}

/// Refuse trust policy sourced from the artifact tree it would authenticate.
pub fn load_external_trust(root: &Path, path: &Path) -> Result<Trust> {
    ensure!(
        !path.canonicalize()?.starts_with(root.canonicalize()?),
        "EPERM package cannot supply its own trust policy"
    );
    load_trust(path)
}

/// Verify an exact registered package without running any packaged executable.
pub fn verify(root: &Path, trust: &Trust) -> Result<Report> {
    let (profile, graph) = profile(&trust.profile_id)?;
    Ok(verify_snapshot(root, trust, &profile, &graph)?.report)
}

/// Check the exact package and resolve only explicitly enrolled credential
/// references. Returned values contain names and status, never credential bytes.
pub fn doctor(root: &Path, trust: &Trust, credential_refs: &Path) -> Result<Value> {
    let report = verify(root, trust)?;
    let references: BTreeMap<String, String> =
        serde_json::from_slice(&read_regular(credential_refs, 32 * 1024)?)?;
    let expected: BTreeSet<&str> = report
        .required_credentials
        .iter()
        .map(String::as_str)
        .collect();
    ensure!(
        references
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == expected,
        "EPERM package credential enrollment set"
    );
    let mut statuses = Vec::new();
    for (name, reference) in references {
        cohesix_authority::secret::validate_reference(&reference)?;
        cohesix_authority::secret::resolve_reference(&reference)
            .with_context(|| format!("credential {name} unavailable"))?;
        statuses.push(json!({"name":name,"status":"resolved"}));
    }
    Ok(
        json!({"schema":"cohesix-package-doctor/v1", "authoritative":false,
        "package": report, "credentials":statuses, "service_health":"not_observed"}),
    )
}

fn destination_parent(destination: &Path) -> Result<(PathBuf, File)> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    check_components(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::metadata(parent)?;
        ensure!(
            meta.mode() & 0o022 == 0,
            "EPERM package destination parent writable by others"
        );
    }
    ensure!(
        destination
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(identifier),
        "EPERM package destination name"
    );
    let lock_path = parent.join(".cohesix-package.lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let lock = options.open(lock_path)?;
    ensure!(lock.metadata()?.is_file(), "EPERM package install lock");
    lock.try_lock_exclusive()?;
    match fs::symlink_metadata(destination) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => bail!("EEXIST package destination"),
    }
    Ok((parent.to_owned(), lock))
}

fn write_artifact(root: &Path, name: &str, bytes: &[u8], executable: bool) -> Result<()> {
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if executable { 0o755 } else { 0o644 });
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn publish(stage: tempfile::TempDir, destination: &Path, parent: &Path) -> Result<()> {
    // The cooperative parent lock covers the existence check through rename;
    // callers must own the non-world-writable installation parent. No upgrade
    // replaces an existing tree, and no service starts from an incomplete tree.
    let mut pending = vec![stage.path().to_owned()];
    let mut directories = Vec::new();
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            }
        }
        directories.push(path);
    }
    for path in directories.iter().rev() {
        File::open(path)?.sync_all()?;
    }
    fs::rename(stage.path(), destination)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

/// Build a signed file inventory from exactly the selected staged artifacts.
/// Native artifact versions are signer-attested build metadata, not inferred
/// from an untrusted binary's output. The builder must enroll the source digest.
pub fn build(
    input: &Path,
    destination: &Path,
    profile_id: &str,
    source_sha256: &str,
    key_id: &str,
    signing_key_ref: &str,
) -> Result<Report> {
    let (profile, graph) = profile(profile_id)?;
    ensure!(
        digest(source_sha256) && identifier(key_id),
        "EPERM package build identity"
    );
    let secret = cohesix_authority::secret::resolve_reference(signing_key_ref)?;
    let seed: [u8; 32] = hex::decode(&secret)
        .map_err(|_| anyhow!("EPERM package signing key encoding"))?
        .try_into()
        .map_err(|_| anyhow!("EPERM package signing key length"))?;
    let key = SigningKey::from_bytes(&seed);
    build_selected(
        input,
        destination,
        &profile,
        &graph,
        source_sha256,
        key_id,
        &key,
    )
}

fn build_selected(
    input: &Path,
    destination: &Path,
    profile: &Profile,
    graph: &str,
    source_sha256: &str,
    key_id: &str,
    key: &SigningKey,
) -> Result<Report> {
    profile.validate()?;
    let names = profile
        .artifacts
        .iter()
        .filter(|artifact| artifact.path != SBOM)
        .map(|artifact| artifact.path.clone())
        .collect();
    check_tree(input, &names)?;
    let (parent, _lock) = destination_parent(destination)?;
    let stage = tempfile::tempdir_in(&parent)?;
    let mut artifacts = Vec::new();
    let mut total = 0_u64;
    for requirement in profile
        .artifacts
        .iter()
        .filter(|artifact| artifact.path != SBOM)
    {
        let bytes = read_regular(&input.join(&requirement.path), MAX_ARTIFACT_BYTES)?;
        total += bytes.len() as u64;
        ensure!(total <= MAX_PACKAGE_BYTES, "ELIMIT package build bytes");
        content(requirement, &bytes, profile)
            .with_context(|| format!("artifact {}", requirement.path))?;
        artifacts.push(Artifact {
            requirement: requirement.clone(),
            bytes: bytes.len() as u64,
            sha256: hash(&bytes),
        });
        write_artifact(
            stage.path(),
            &requirement.path,
            &bytes,
            requirement.executable,
        )?;
    }
    let sbom_bytes = serde_json::to_vec(&sbom(profile, &artifacts))?;
    let requirement = profile
        .artifacts
        .iter()
        .find(|artifact| artifact.path == SBOM)
        .ok_or_else(|| anyhow!("EPERM SBOM profile"))?;
    artifacts.push(Artifact {
        requirement: requirement.clone(),
        bytes: sbom_bytes.len() as u64,
        sha256: hash(&sbom_bytes),
    });
    artifacts.sort_by(|left, right| left.requirement.path.cmp(&right.requirement.path));
    write_artifact(stage.path(), SBOM, &sbom_bytes, false)?;
    let manifest = Manifest {
        schema: MANIFEST_SCHEMA.to_owned(),
        profile: profile.clone(),
        provider_graph_sha256: graph.to_owned(),
        source_sha256: source_sha256.to_owned(),
        artifacts,
    };
    let signature = hex::encode(key.sign(&signature_bytes(&manifest, key_id)?).to_bytes());
    let signed = SignedManifest {
        manifest,
        key_id: key_id.to_owned(),
        signature,
    };
    write_artifact(
        stage.path(),
        "package.json",
        &serde_json::to_vec(&signed)?,
        false,
    )?;
    let trust = Trust {
        schema: "cohesix-host-package-trust/v1".to_owned(),
        profile_id: profile.id.clone(),
        version: profile.version.clone(),
        source_sha256: source_sha256.to_owned(),
        keys: BTreeMap::from([(
            key_id.to_owned(),
            hex::encode(key.verifying_key().to_bytes()),
        )]),
    };
    let verified = verify_snapshot(stage.path(), &trust, profile, graph)?;
    publish(stage, destination, &parent)?;
    Ok(verified.report)
}

/// Install a verified snapshot into a new directory on the matching host.
/// Enrollment secrets and service activation remain separate operator actions.
pub fn install(input: &Path, destination: &Path, trust: &Trust) -> Result<Report> {
    let (profile, graph) = profile(&trust.profile_id)?;
    ensure!(
        profile.os == std::env::consts::OS && profile.architecture == std::env::consts::ARCH,
        "EPERM package install host mismatch"
    );
    let verified = verify_snapshot(input, trust, &profile, &graph)?;
    let (parent, _lock) = destination_parent(destination)?;
    let stage = tempfile::tempdir_in(&parent)?;
    for (name, bytes) in &verified.files {
        let executable =
            verified.signed.manifest.artifacts.iter().any(|artifact| {
                artifact.requirement.path == *name && artifact.requirement.executable
            });
        write_artifact(stage.path(), name, bytes, executable)?;
    }
    let readback = verify_snapshot(stage.path(), trust, &profile, &graph)?;
    publish(stage, destination, &parent)?;
    Ok(readback.report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirement(path: &str, kind: ArtifactKind) -> Requirement {
        Requirement {
            path: path.to_owned(),
            kind,
            executable: matches!(kind, ArtifactKind::Elf | ArtifactKind::MachO),
            version: "1".to_owned(),
            schema_pointer: None,
            schema_value: None,
        }
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, Profile, Trust, SigningKey) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let input = root.join("input");
        fs::create_dir(&input).unwrap();
        let mut sbom = requirement(SBOM, ArtifactKind::Json);
        sbom.schema_pointer = Some("/bomFormat".to_owned());
        sbom.schema_value = Some("CycloneDX".to_owned());
        let mut config = requirement("config/service.json", ArtifactKind::Json);
        config.schema_pointer = Some("/schema".to_owned());
        config.schema_value = Some("test-service/v1".to_owned());
        let profile = Profile {
            schema: cohesix_authority::package::PROFILE_SCHEMA.to_owned(),
            id: "fixture-controller".to_owned(),
            os: "linux".to_owned(),
            architecture: "aarch64".to_owned(),
            version: "1".to_owned(),
            integration_surfaces: vec!["coh".to_owned()],
            required_credentials: vec!["COH_AUTH_TOKEN".to_owned()],
            artifacts: vec![config, sbom],
        };
        write_artifact(
            &input,
            "config/service.json",
            br#"{"schema":"test-service/v1"}"#,
            false,
        )
        .unwrap();
        let key = SigningKey::from_bytes(&[13; 32]);
        let trust = Trust {
            schema: "cohesix-host-package-trust/v1".to_owned(),
            profile_id: profile.id.clone(),
            version: profile.version.clone(),
            source_sha256: "15".repeat(32),
            keys: BTreeMap::from([(
                "unit-fixture".to_owned(),
                hex::encode(key.verifying_key().to_bytes()),
            )]),
        };
        (temp, root, profile, trust, key)
    }

    fn signed_fixture(root: &Path, profile: &Profile, trust: &Trust, key: &SigningKey) {
        build_selected(
            &root.join("input"),
            &root.join("package"),
            profile,
            &"24".repeat(32),
            &trust.source_sha256,
            "unit-fixture",
            key,
        )
        .unwrap();
    }

    fn rewrite_signed(root: &Path, key: &SigningKey, change: impl FnOnce(&mut Manifest)) {
        let path = root.join("package.json");
        let mut signed: SignedManifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        change(&mut signed.manifest);
        signed.signature = hex::encode(
            key.sign(&signature_bytes(&signed.manifest, &signed.key_id).unwrap())
                .to_bytes(),
        );
        fs::write(path, serde_json::to_vec(&signed).unwrap()).unwrap();
    }

    #[test]
    fn signed_inventory_refuses_missing_extra_changed_and_untrusted_artifacts() {
        let (_temp, root, profile, mut trust, key) = fixture();
        signed_fixture(&root, &profile, &trust, &key);
        let package = root.join("package");
        let graph = "24".repeat(32);
        let verified = verify_snapshot(&package, &trust, &profile, &graph).unwrap();
        assert_eq!(verified.report.artifact_count, 2);
        assert!(!verified.report.authoritative);
        fs::write(package.join("extra"), "unexpected").unwrap();
        assert!(verify_snapshot(&package, &trust, &profile, &graph).is_err());
        fs::remove_file(package.join("extra")).unwrap();
        let config = package.join("config/service.json");
        let original = fs::read(&config).unwrap();
        fs::remove_file(&config).unwrap();
        assert!(verify_snapshot(&package, &trust, &profile, &graph).is_err());
        fs::write(&config, b"changed").unwrap();
        assert!(verify_snapshot(&package, &trust, &profile, &graph).is_err());
        fs::write(&config, original).unwrap();
        trust.source_sha256 = "ff".repeat(32);
        assert!(verify_snapshot(&package, &trust, &profile, &graph).is_err());
        trust.source_sha256 = "15".repeat(32);
        trust.keys.insert(
            "unit-fixture".to_owned(),
            hex::encode(SigningKey::from_bytes(&[14; 32]).verifying_key().to_bytes()),
        );
        assert!(verify_snapshot(&package, &trust, &profile, &graph).is_err());
    }

    #[test]
    fn signer_cannot_change_compiler_versions_schema_or_sbom_membership() {
        let (_temp, root, profile, trust, key) = fixture();
        signed_fixture(&root, &profile, &trust, &key);
        let package = root.join("package");
        let original = fs::read(package.join("package.json")).unwrap();
        rewrite_signed(&package, &key, |manifest| {
            manifest.artifacts[0].requirement.version = "2".to_owned()
        });
        assert!(verify_snapshot(&package, &trust, &profile, &"24".repeat(32)).is_err());
        fs::write(package.join("package.json"), &original).unwrap();
        let bytes = br#"{"schema":"wrong/v1"}"#;
        fs::write(package.join("config/service.json"), bytes).unwrap();
        rewrite_signed(&package, &key, |manifest| {
            manifest.artifacts[0].bytes = bytes.len() as u64;
            manifest.artifacts[0].sha256 = hash(bytes);
        });
        assert!(verify_snapshot(&package, &trust, &profile, &"24".repeat(32)).is_err());
        fs::write(package.join("package.json"), &original).unwrap();
        fs::write(
            package.join("config/service.json"),
            br#"{"schema":"test-service/v1"}"#,
        )
        .unwrap();
        let bytes = br#"{"bomFormat":"CycloneDX","specVersion":"1.6","version":1,"components":[]}"#;
        fs::write(package.join(SBOM), bytes).unwrap();
        rewrite_signed(&package, &key, |manifest| {
            manifest.artifacts[1].bytes = bytes.len() as u64;
            manifest.artifacts[1].sha256 = hash(bytes);
        });
        assert!(verify_snapshot(&package, &trust, &profile, &"24".repeat(32)).is_err());
    }

    #[test]
    fn filesystem_boundaries_refuse_symlinks_collisions_and_privileged_modes() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (_temp, root, profile, trust, key) = fixture();
        signed_fixture(&root, &profile, &trust, &key);
        assert!(build_selected(
            &root.join("input"),
            &root.join("package"),
            &profile,
            &"24".repeat(32),
            &trust.source_sha256,
            "unit-fixture",
            &key
        )
        .is_err());
        let package = root.join("package");
        let external_trust = root.join("trust.json");
        fs::write(&external_trust, serde_json::to_vec(&trust).unwrap()).unwrap();
        assert!(load_external_trust(&package, &external_trust).is_ok());
        let internal_trust = package.join("trust.json");
        fs::write(&internal_trust, serde_json::to_vec(&trust).unwrap()).unwrap();
        assert!(load_external_trust(&package, &internal_trust).is_err());
        fs::remove_file(internal_trust).unwrap();
        let config = package.join("config/service.json");
        fs::set_permissions(&config, fs::Permissions::from_mode(0o4644)).unwrap();
        assert!(verify_snapshot(&package, &trust, &profile, &"24".repeat(32)).is_err());
        fs::set_permissions(&config, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_file(&config).unwrap();
        symlink(root.join("input/config/service.json"), &config).unwrap();
        assert!(verify_snapshot(&package, &trust, &profile, &"24".repeat(32)).is_err());
        assert!(
            verify(&package, &trust).is_err(),
            "private unit profile must never be registered"
        );
    }

    #[test]
    fn native_kind_architecture_entry_bounds_and_production_secrets_are_checked() {
        // Independent ELF64 fixture: 64-byte header, one 56-byte RX PT_LOAD,
        // entry 0x400078 maps the four bytes after that program header.
        let mut elf = vec![0_u8; 124];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        for (offset, width, value) in [
            (16, 2, 2_u64),
            (18, 2, 183),
            (20, 4, 1),
            (24, 8, 0x400078),
            (32, 8, 64),
            (52, 2, 64),
            (54, 2, 56),
            (56, 2, 1),
            (64, 4, 1),
            (68, 4, 5),
            (80, 8, 0x400000),
            (96, 8, 124),
            (104, 8, 124),
        ] {
            elf[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
        }
        assert!(native_image(&elf, ArtifactKind::Elf, "aarch64").is_ok());
        assert!(native_image(&elf, ArtifactKind::Elf, "x86_64").is_err());
        assert!(native_image(&elf[..120], ArtifactKind::Elf, "aarch64").is_err());
        assert!(native_image(&elf, ArtifactKind::MachO, "aarch64").is_err());
        elf[24..32].copy_from_slice(&0x500000_u64.to_le_bytes());
        assert!(native_image(&elf, ArtifactKind::Elf, "aarch64").is_err());
        let (_temp, _root, profile, _trust, _key) = fixture();
        let requirement = requirement("config/root_task_resolved.json", ArtifactKind::Json);
        for value in [
            json!({"authority":{"production":false}}),
            json!({"authority":{"production":true,"debug_memory":false,"execution_wal_required":true},
                "tickets":[{"secret":"fixture-secret","secret_ref":"env:COH_TICKET_KEY"}]}),
        ] {
            assert!(content(&requirement, &serde_json::to_vec(&value).unwrap(), &profile).is_err());
        }
        let safe = json!({"authority":{"production":true,"debug_memory":false,"execution_wal_required":true},
            "tickets":[{"secret":null,"secret_ref":"env:COH_TICKET_KEY"}]});
        assert!(content(&requirement, &serde_json::to_vec(&safe).unwrap(), &profile).is_ok());
    }
}
