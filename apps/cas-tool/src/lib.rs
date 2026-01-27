// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CAS bundle packaging helpers for host tooling.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Helpers for packaging Cohesix CAS bundles and manifests.

use anyhow::{bail, Context, Result};
use cohesix_cas::{CasDelta, CasManifest, CAS_MANIFEST_SCHEMA};
use ed25519_dalek::{Signature, SigningKey};
use serde_json::Value;
use sha2::{Digest, Sha256};
use signature::Signer;
use std::fs;
use std::path::{Path, PathBuf};

/// CAS template configuration derived from a coh-rtc template JSON.
#[derive(Debug, Clone)]
pub struct CasTemplateConfig {
    /// Chunk size in bytes.
    pub chunk_bytes: usize,
    /// Whether delta manifests are allowed.
    pub delta_allowed: bool,
    /// Whether signatures are required.
    pub signing_required: bool,
}

/// A chunked CAS payload.
#[derive(Debug, Clone)]
pub struct CasChunk {
    /// SHA-256 digest of the chunk.
    pub digest: [u8; 32],
    /// Raw chunk bytes.
    pub data: Vec<u8>,
}

/// Packaged CAS bundle with manifest and chunks.
#[derive(Debug, Clone)]
pub struct CasBundle {
    /// Update epoch.
    pub epoch: String,
    /// Manifest metadata.
    pub manifest: CasManifest,
    /// Encoded manifest bytes (CBOR).
    pub manifest_cbor: Vec<u8>,
    /// Chunk payloads.
    pub chunks: Vec<CasChunk>,
}

/// Delta base information loaded from a bundle directory.
#[derive(Debug, Clone)]
pub struct DeltaBase {
    /// Base epoch.
    pub epoch: String,
    /// Base payload hash.
    pub payload_sha256: [u8; 32],
    /// Base payload bytes.
    pub payload: Vec<u8>,
    /// Chunk size in bytes.
    pub chunk_bytes: usize,
}

/// Load CAS template configuration from JSON output.
pub fn load_template_config(path: &Path) -> Result<CasTemplateConfig> {
    let contents = fs::read(path).with_context(|| format!("read template {}", path.display()))?;
    let value: Value = serde_json::from_slice(&contents)
        .with_context(|| format!("parse template {}", path.display()))?;
    let chunk_bytes = value
        .get("chunk_bytes")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| anyhow::anyhow!("template missing chunk_bytes"))?;
    let delta_allowed = match value.get("delta") {
        Some(Value::Null) | None => false,
        Some(_) => true,
    };
    let signing_required = match value.get("signature") {
        Some(Value::Null) | None => false,
        Some(_) => true,
    };
    Ok(CasTemplateConfig {
        chunk_bytes: chunk_bytes as usize,
        delta_allowed,
        signing_required,
    })
}

/// Load an Ed25519 signing key from a hex file.
pub fn load_signing_key(path: &Path) -> Result<[u8; 32]> {
    let contents =
        fs::read(path).with_context(|| format!("read signing key {}", path.display()))?;
    let text = std::str::from_utf8(&contents)
        .with_context(|| format!("signing key {} is not utf-8", path.display()))?;
    let raw = hex::decode(text.trim())
        .map_err(|err| anyhow::anyhow!("signing key {} must be hex: {err}", path.display()))?;
    if raw.len() != 32 {
        bail!(
            "signing key {} must be 32 bytes (got {})",
            path.display(),
            raw.len()
        );
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

/// Split a payload into fixed-size CAS chunks.
pub fn chunk_payload(payload: &[u8], chunk_bytes: usize) -> Result<Vec<CasChunk>> {
    if chunk_bytes == 0 {
        bail!("chunk_bytes must be > 0");
    }
    if payload.len() % chunk_bytes != 0 {
        bail!(
            "payload length {} is not a multiple of chunk_bytes {}",
            payload.len(),
            chunk_bytes
        );
    }
    let mut chunks = Vec::new();
    for block in payload.chunks(chunk_bytes) {
        let digest = Sha256::digest(block);
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        chunks.push(CasChunk {
            digest: out,
            data: block.to_vec(),
        });
    }
    Ok(chunks)
}

/// Load a base bundle for delta computation.
pub fn load_delta_base(bundle_dir: &Path) -> Result<DeltaBase> {
    let manifest_path = bundle_dir.join("manifest.cbor");
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("read base manifest {}", manifest_path.display()))?;
    let manifest = CasManifest::decode(&manifest_bytes).map_err(|err| {
        anyhow::anyhow!("decode base manifest {}: {err}", manifest_path.display())
    })?;
    let mut payload = Vec::new();
    let chunks_dir = bundle_dir.join("chunks");
    for digest in &manifest.chunks {
        let name = hex::encode(digest);
        let chunk_path = chunks_dir.join(name);
        let chunk_bytes = fs::read(&chunk_path)
            .with_context(|| format!("read base chunk {}", chunk_path.display()))?;
        let actual = Sha256::digest(&chunk_bytes);
        if actual.as_slice() != digest {
            bail!("base chunk {} hash mismatch", chunk_path.display());
        }
        payload.extend_from_slice(&chunk_bytes);
    }
    Ok(DeltaBase {
        epoch: manifest.epoch,
        payload_sha256: manifest.payload_sha256,
        payload,
        chunk_bytes: manifest.chunk_bytes as usize,
    })
}

/// Build a CAS bundle from a payload and optional delta base.
pub fn build_bundle(
    epoch: &str,
    payload: &[u8],
    template: &CasTemplateConfig,
    delta_base: Option<DeltaBase>,
    signing_key: Option<[u8; 32]>,
) -> Result<CasBundle> {
    if template.signing_required && signing_key.is_none() {
        bail!("signing key required by template");
    }
    if delta_base.is_some() && !template.delta_allowed {
        bail!("delta packs are disabled by template");
    }
    let chunk_bytes = template.chunk_bytes;
    let chunks = chunk_payload(payload, chunk_bytes)?;
    if chunks.is_empty() {
        bail!("payload produced zero chunks");
    }
    if let Some(base) = &delta_base {
        if base.chunk_bytes != chunk_bytes {
            bail!(
                "delta base chunk_bytes {} does not match template {}",
                base.chunk_bytes,
                chunk_bytes
            );
        }
    }
    let payload_bytes = (chunks.len() * chunk_bytes) as u64;
    let mut hasher = Sha256::new();
    if let Some(base) = &delta_base {
        hasher.update(&base.payload);
    }
    hasher.update(payload);
    let digest = hasher.finalize();
    let mut payload_sha256 = [0u8; 32];
    payload_sha256.copy_from_slice(&digest);

    let delta = delta_base.as_ref().map(|base| CasDelta {
        base_epoch: base.epoch.clone(),
        base_sha256: base.payload_sha256,
    });

    let mut manifest = CasManifest {
        schema: CAS_MANIFEST_SCHEMA.to_owned(),
        epoch: epoch.to_owned(),
        chunk_bytes: chunk_bytes as u32,
        payload_bytes,
        payload_sha256,
        chunks: chunks.iter().map(|chunk| chunk.digest).collect(),
        delta,
        signature: None,
    };

    if let Some(key) = signing_key {
        let signing_key = SigningKey::from_bytes(&key);
        let payload = manifest
            .signature_payload()
            .map_err(|err| anyhow::anyhow!("render signing payload: {err}"))?;
        let signature: Signature = signing_key.sign(&payload);
        manifest.signature = Some(signature.to_bytes());
    }

    let manifest_cbor = manifest
        .encode_signed()
        .map_err(|err| anyhow::anyhow!("encode manifest: {err}"))?;

    Ok(CasBundle {
        epoch: epoch.to_owned(),
        manifest,
        manifest_cbor,
        chunks,
    })
}

/// Write a CAS bundle to disk.
pub fn write_bundle(bundle: &CasBundle, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("create bundle dir {}", out_dir.display()))?;
    let chunks_dir = out_dir.join("chunks");
    fs::create_dir_all(&chunks_dir)
        .with_context(|| format!("create chunks dir {}", chunks_dir.display()))?;
    let manifest_path = out_dir.join("manifest.cbor");
    fs::write(&manifest_path, &bundle.manifest_cbor)
        .with_context(|| format!("write manifest {}", manifest_path.display()))?;
    for chunk in &bundle.chunks {
        let name = hex::encode(chunk.digest);
        let chunk_path = chunks_dir.join(name);
        fs::write(&chunk_path, &chunk.data)
            .with_context(|| format!("write chunk {}", chunk_path.display()))?;
    }
    Ok(())
}

/// Collect bundle paths (manifest + chunk files) for upload.
pub fn bundle_paths(bundle_dir: &Path) -> Result<(PathBuf, PathBuf)> {
    let manifest_path = bundle_dir.join("manifest.cbor");
    let chunks_dir = bundle_dir.join("chunks");
    if !manifest_path.is_file() {
        bail!("manifest not found at {}", manifest_path.display());
    }
    if !chunks_dir.is_dir() {
        bail!("chunks dir not found at {}", chunks_dir.display());
    }
    Ok((manifest_path, chunks_dir))
}
