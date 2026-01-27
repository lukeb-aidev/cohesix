// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Content-addressed update storage and validation for NineDoor.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use cohesix_cas::{CasManifest, CasManifestError, CAS_MANIFEST_SCHEMA};
use ed25519_dalek::{Signature, VerifyingKey};
use secure9p_codec::ErrorCode;
use sha2::{Digest, Sha256};
use signature::Verifier;
use trace_model::TraceLevel;

use super::cbor::{CborError, CborWriter};
use super::ui::UI_MAX_STREAM_BYTES;
use crate::NineDoorError;

const CAS_MAX_CHUNKS: usize = 8;
const CAS_MAX_UPDATES: usize = 8;
const CAS_MAX_MODELS: usize = 8;
const CAS_QUARANTINE_LIMIT: usize = 8;
const CAS_MANIFEST_MAX_BYTES: usize = 2048;
const MAX_EPOCH_LEN: usize = 20;

/// CAS runtime configuration derived from the manifest.
#[derive(Debug, Clone)]
pub struct CasConfig {
    enabled: bool,
    models_enabled: bool,
    chunk_bytes: usize,
    delta_enabled: bool,
    signing_required: bool,
    signing_key: Option<[u8; 32]>,
}

impl CasConfig {
    /// Construct a disabled CAS configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            models_enabled: false,
            chunk_bytes: 0,
            delta_enabled: false,
            signing_required: false,
            signing_key: None,
        }
    }

    /// Construct an enabled CAS configuration.
    pub fn enabled(
        chunk_bytes: usize,
        delta_enabled: bool,
        signing_required: bool,
        signing_key: Option<[u8; 32]>,
        models_enabled: bool,
    ) -> Self {
        Self {
            enabled: true,
            models_enabled,
            chunk_bytes,
            delta_enabled,
            signing_required,
            signing_key,
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn models_enabled(&self) -> bool {
        self.enabled && self.models_enabled
    }

    pub(crate) fn chunk_bytes(&self) -> usize {
        self.chunk_bytes
    }

    fn signing_key(&self) -> Option<&[u8; 32]> {
        self.signing_key.as_ref()
    }
}

#[derive(Debug)]
pub struct CasStore {
    config: CasConfig,
    updates: BTreeMap<String, UpdateBundle>,
    chunks: BTreeMap<[u8; 32], Vec<u8>>,
    pending_chunks: BTreeMap<[u8; 32], Vec<u8>>,
    models: BTreeMap<[u8; 32], ModelBundle>,
    quarantine: VecDeque<QuarantineEntry>,
    events: VecDeque<CasEvent>,
    bytes_used: usize,
}

/// UI provider payloads for update status.
#[derive(Debug, Clone)]
pub struct UpdateStatusPayloads {
    /// Text payload bytes.
    pub text: Vec<u8>,
    /// CBOR payload bytes.
    pub cbor: Vec<u8>,
}

impl CasStore {
    pub fn new(config: CasConfig) -> Self {
        Self {
            config,
            updates: BTreeMap::new(),
            chunks: BTreeMap::new(),
            pending_chunks: BTreeMap::new(),
            models: BTreeMap::new(),
            quarantine: VecDeque::new(),
            events: VecDeque::new(),
            bytes_used: 0,
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.is_enabled()
    }

    pub fn models_enabled(&self) -> bool {
        self.config.models_enabled()
    }

    pub fn drain_events(&mut self) -> Vec<CasEvent> {
        self.events.drain(..).collect()
    }

    pub fn list_updates(&self) -> Vec<String> {
        self.updates.keys().cloned().collect()
    }

    pub fn list_models(&self) -> Vec<String> {
        self.models
            .keys()
            .map(|digest| hex::encode(digest))
            .collect()
    }

    pub fn list_update_chunks(&self, epoch: &str) -> Vec<String> {
        let Some(manifest) = self
            .updates
            .get(epoch)
            .and_then(|bundle| bundle.manifest.as_ref())
        else {
            return Vec::new();
        };
        let mut entries: Vec<String> = manifest
            .chunks
            .iter()
            .map(|digest| hex::encode(digest))
            .collect();
        entries.sort();
        entries
    }

    pub fn list_model_entries(&self, digest: &[u8; 32]) -> Vec<String> {
        let Some(model) = self.models.get(digest) else {
            return Vec::new();
        };
        let mut entries = vec!["weights".to_owned()];
        if model.schema.is_some() {
            entries.push("schema".to_owned());
        }
        if model.signature.is_some() {
            entries.push("signature".to_owned());
        }
        entries.sort();
        entries
    }

    pub fn read_manifest(
        &self,
        epoch: &str,
        offset: u64,
        count: u32,
    ) -> Result<Vec<u8>, NineDoorError> {
        let bundle = self.updates.get(epoch).ok_or_else(|| {
            NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("update epoch {epoch} not found"),
            )
        })?;
        let data = bundle.manifest_bytes.as_deref().ok_or_else(|| {
            NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("update manifest {epoch} not committed"),
            )
        })?;
        Ok(read_slice(data, offset, count))
    }

    pub fn read_chunk(
        &self,
        digest: &[u8; 32],
        offset: u64,
        count: u32,
    ) -> Result<Vec<u8>, NineDoorError> {
        let data = self.chunks.get(digest).ok_or_else(|| {
            NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("chunk {} not found", hex::encode(digest)),
            )
        })?;
        Ok(read_slice(data, offset, count))
    }

    pub fn read_model_file(
        &self,
        digest: &[u8; 32],
        kind: ModelFileKind,
        offset: u64,
        count: u32,
    ) -> Result<Vec<u8>, NineDoorError> {
        let model = self.models.get(digest).ok_or_else(|| {
            NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("model {} not found", hex::encode(digest)),
            )
        })?;
        match kind {
            ModelFileKind::Weights => self.read_chunk(digest, offset, count),
            ModelFileKind::Schema => {
                let data = model.schema.as_deref().ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("model schema {} not found", hex::encode(digest)),
                    )
                })?;
                Ok(read_slice(data, offset, count))
            }
            ModelFileKind::Signature => {
                let data = model.signature.as_deref().ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("model signature {} not found", hex::encode(digest)),
                    )
                })?;
                Ok(read_slice(data, offset, count))
            }
        }
    }

    /// Build update status payloads for UI providers.
    pub fn update_status_payloads(
        &self,
        epoch: &str,
    ) -> Result<UpdateStatusPayloads, NineDoorError> {
        let snapshot = self.update_status_snapshot(epoch)?;
        let text = build_update_status_text(&snapshot)?;
        let cbor = build_update_status_cbor(&snapshot)?;
        Ok(UpdateStatusPayloads { text, cbor })
    }

    pub fn append_manifest(
        &mut self,
        epoch: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorError> {
        self.ensure_enabled()?;
        self.ensure_epoch(epoch)?;
        let payload = decode_payload(data)?;
        let expected_offset = {
            let bundle = self.updates.get(epoch).expect("update bundle must exist");
            if bundle.manifest_bytes.is_some() {
                return Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    "manifest already committed",
                ));
            }
            bundle.manifest_pending.len() as u64
        };
        let provided_offset = if offset == u64::MAX {
            expected_offset
        } else {
            offset
        };
        if provided_offset != expected_offset {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                format!(
                    "manifest append offset rejected: expected {expected_offset} got {provided_offset}"
                ),
            ));
        }
        let new_len = expected_offset as usize + payload.len();
        if new_len > CAS_MANIFEST_MAX_BYTES {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                "manifest exceeds max bytes",
            ));
        }
        {
            let bundle = self
                .updates
                .get_mut(epoch)
                .expect("update bundle must exist");
            bundle.manifest_pending.extend_from_slice(&payload);
        }
        let pending_snapshot = {
            let bundle = self.updates.get(epoch).expect("update bundle must exist");
            bundle.manifest_pending.clone()
        };
        match CasManifest::decode(&pending_snapshot) {
            Ok(manifest) => {
                if let Err(err) = self.validate_manifest(epoch, &manifest) {
                    let bundle = self
                        .updates
                        .get_mut(epoch)
                        .expect("update bundle must exist");
                    bundle.manifest_pending.clear();
                    return Err(err);
                }
                let bundle = self
                    .updates
                    .get_mut(epoch)
                    .expect("update bundle must exist");
                bundle.manifest_bytes = Some(pending_snapshot);
                bundle.manifest_pending.clear();
                bundle.manifest = Some(manifest);
                Ok(data.len() as u32)
            }
            Err(CasManifestError::UnexpectedEof) => Ok(data.len() as u32),
            Err(err) => {
                let bundle = self
                    .updates
                    .get_mut(epoch)
                    .expect("update bundle must exist");
                bundle.manifest_pending.clear();
                Err(NineDoorError::protocol(
                    ErrorCode::Invalid,
                    format!("manifest invalid: {err}"),
                ))
            }
        }
    }

    pub fn append_chunk(
        &mut self,
        epoch: &str,
        digest: &[u8; 32],
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorError> {
        self.ensure_enabled()?;
        self.ensure_epoch(epoch)?;
        self.append_chunk_internal(epoch, digest, offset, data)
    }

    fn append_chunk_internal(
        &mut self,
        label: &str,
        digest: &[u8; 32],
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorError> {
        let payload = decode_payload(data)?;
        if let Some(existing) = self.chunks.get(digest) {
            if offset != 0 && offset != u64::MAX {
                return Err(NineDoorError::protocol(
                    ErrorCode::Invalid,
                    "chunk already committed",
                ));
            }
            if existing.as_slice() == payload {
                return Ok(data.len() as u32);
            }
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "chunk already committed",
            ));
        }
        if payload.len() > self.config.chunk_bytes() {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!(
                    "chunk payload {} exceeds chunk_bytes {}",
                    payload.len(),
                    self.config.chunk_bytes()
                ),
            ));
        }
        let chunk_bytes = self.config.chunk_bytes();
        let pending_len = self
            .pending_chunks
            .get(digest)
            .map_or(0, |pending| pending.len());
        let expected_offset = pending_len as u64;
        let provided_offset = if offset == u64::MAX {
            expected_offset
        } else {
            offset
        };
        if provided_offset != expected_offset {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                format!(
                    "chunk append offset rejected: expected {expected_offset} got {provided_offset}"
                ),
            ));
        }
        if !self.can_reserve_bytes(payload.len()) {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                "cas store capacity exceeded",
            ));
        }
        {
            let pending = self.pending_chunks.entry(*digest).or_default();
            pending.extend_from_slice(&payload);
        }
        self.bytes_used = self.bytes_used.saturating_add(payload.len());
        let pending_len = self
            .pending_chunks
            .get(digest)
            .map_or(0, |pending| pending.len());
        if pending_len < chunk_bytes {
            return Ok(data.len() as u32);
        }
        if pending_len > chunk_bytes {
            if let Some(pending) = self.pending_chunks.get_mut(digest) {
                pending.clear();
            }
            self.bytes_used = self.bytes_used.saturating_sub(pending_len);
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!("chunk payload exceeded chunk_bytes (len={pending_len})"),
            ));
        }
        let actual = {
            let pending = self
                .pending_chunks
                .get(digest)
                .expect("pending chunk must exist");
            Sha256::digest(pending)
        };
        if actual.as_slice() != digest {
            self.quarantine_chunk(label, digest, &actual, pending_len);
            self.bytes_used = self.bytes_used.saturating_sub(pending_len);
            if let Some(pending) = self.pending_chunks.get_mut(digest) {
                pending.clear();
            }
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "chunk hash mismatch",
            ));
        }
        let committed = self.pending_chunks.remove(digest).unwrap_or_default();
        self.chunks.insert(*digest, committed);
        Ok(data.len() as u32)
    }

    pub fn append_model_file(
        &mut self,
        digest: &[u8; 32],
        kind: ModelFileKind,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorError> {
        self.ensure_models_enabled()?;
        self.ensure_model(digest)?;
        match kind {
            ModelFileKind::Weights => {
                let weights_committed = self
                    .models
                    .get(digest)
                    .expect("model must exist")
                    .weights_committed;
                if weights_committed {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Permission,
                        "model weights are read-only",
                    ));
                }
                let count = self.append_chunk_internal("model", digest, offset, data)?;
                if self.chunks.contains_key(digest) {
                    let model = self.models.get_mut(digest).expect("model must exist");
                    model.weights_committed = true;
                }
                Ok(count)
            }
            ModelFileKind::Schema => {
                let existing_len = self
                    .models
                    .get(digest)
                    .and_then(|model| model.schema.as_ref())
                    .map(|data| data.len());
                if existing_len.is_some() {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Permission,
                        "model schema is read-only",
                    ));
                }
                let payload = decode_payload(data)?;
                if payload.len() > self.config.chunk_bytes() {
                    return Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "model schema exceeds chunk_bytes",
                    ));
                }
                let expected_offset = existing_len.unwrap_or(0) as u64;
                let provided_offset = if offset == u64::MAX {
                    expected_offset
                } else {
                    offset
                };
                if provided_offset != expected_offset {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "model schema append offset rejected",
                    ));
                }
                if !self.can_reserve_bytes(payload.len()) {
                    return Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "cas store capacity exceeded",
                    ));
                }
                let model = self.models.get_mut(digest).expect("model must exist");
                model
                    .schema
                    .get_or_insert_with(Vec::new)
                    .extend_from_slice(&payload);
                self.bytes_used = self.bytes_used.saturating_add(payload.len());
                Ok(data.len() as u32)
            }
            ModelFileKind::Signature => {
                let existing_len = self
                    .models
                    .get(digest)
                    .and_then(|model| model.signature.as_ref())
                    .map(|data| data.len());
                if existing_len.is_some() {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Permission,
                        "model signature is read-only",
                    ));
                }
                let payload = decode_payload(data)?;
                if payload.len() > self.config.chunk_bytes() {
                    return Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "model signature exceeds chunk_bytes",
                    ));
                }
                let expected_offset = existing_len.unwrap_or(0) as u64;
                let provided_offset = if offset == u64::MAX {
                    expected_offset
                } else {
                    offset
                };
                if provided_offset != expected_offset {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "model signature append offset rejected",
                    ));
                }
                if !self.can_reserve_bytes(payload.len()) {
                    return Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "cas store capacity exceeded",
                    ));
                }
                let model = self.models.get_mut(digest).expect("model must exist");
                model
                    .signature
                    .get_or_insert_with(Vec::new)
                    .extend_from_slice(&payload);
                self.bytes_used = self.bytes_used.saturating_add(payload.len());
                Ok(data.len() as u32)
            }
        }
    }

    fn validate_manifest(
        &mut self,
        epoch: &str,
        manifest: &CasManifest,
    ) -> Result<(), NineDoorError> {
        if manifest.schema != CAS_MANIFEST_SCHEMA {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "manifest schema mismatch",
            ));
        }
        if manifest.epoch != epoch {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "manifest epoch mismatch",
            ));
        }
        if manifest.chunk_bytes as usize != self.config.chunk_bytes() {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "manifest chunk_bytes mismatch",
            ));
        }
        let expected_bytes =
            (manifest.chunks.len() as u64).saturating_mul(manifest.chunk_bytes as u64);
        if manifest.payload_bytes != expected_bytes {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "manifest payload_bytes mismatch",
            ));
        }
        if manifest.chunks.len() > CAS_MAX_CHUNKS {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                "manifest chunk count exceeds limit",
            ));
        }
        if let Some(delta) = &manifest.delta {
            if !self.config.delta_enabled {
                return Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    "delta manifests disabled",
                ));
            }
            let base = self
                .updates
                .get(&delta.base_epoch)
                .and_then(|bundle| bundle.manifest.as_ref())
                .ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("delta base epoch {} missing", delta.base_epoch),
                    )
                })?;
            if base.delta.is_some() {
                return Err(NineDoorError::protocol(
                    ErrorCode::Invalid,
                    "delta base epoch must be non-delta",
                ));
            }
            if base.payload_sha256 != delta.base_sha256 {
                return Err(NineDoorError::protocol(
                    ErrorCode::Invalid,
                    "delta base hash mismatch",
                ));
            }
        }
        if self.config.signing_required && manifest.signature.is_none() {
            self.events.push_back(CasEvent::warn(format!(
                "cas-manifest rejected epoch={} reason=missing-signature",
                epoch
            )));
            return Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "manifest signature required",
            ));
        }
        if let Some(signature) = manifest.signature {
            let key = self.config.signing_key().ok_or_else(|| {
                self.events.push_back(CasEvent::warn(format!(
                    "cas-manifest rejected epoch={} reason=signing-key-missing",
                    epoch
                )));
                NineDoorError::protocol(ErrorCode::Permission, "signing key missing")
            })?;
            let verifying_key = VerifyingKey::from_bytes(key)
                .map_err(|_| NineDoorError::protocol(ErrorCode::Invalid, "signing key invalid"))?;
            let payload = manifest.signature_payload().map_err(|err| {
                NineDoorError::protocol(
                    ErrorCode::Invalid,
                    format!("manifest signing payload {err}"),
                )
            })?;
            let signature = Signature::from_bytes(&signature);
            if verifying_key.verify(&payload, &signature).is_err() {
                self.events.push_back(CasEvent::warn(format!(
                    "cas-manifest rejected epoch={} reason=signature-failed",
                    epoch
                )));
                return Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    "manifest signature invalid",
                ));
            }
        }
        let payload = self.assemble_payload(manifest)?;
        let computed = Sha256::digest(&payload);
        if computed.as_slice() != manifest.payload_sha256 {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "manifest payload hash mismatch",
            ));
        }
        let delta_label = if manifest.delta.is_some() {
            "delta"
        } else {
            "base"
        };
        let payload_hex = hex::encode(manifest.payload_sha256);
        self.events.push_back(CasEvent::info(format!(
            "cas-manifest accepted epoch={} kind={} payload_sha256={payload_hex} chunks={}",
            epoch,
            delta_label,
            manifest.chunks.len()
        )));
        Ok(())
    }

    fn assemble_payload(&self, manifest: &CasManifest) -> Result<Vec<u8>, NineDoorError> {
        let mut payload = Vec::new();
        if let Some(delta) = &manifest.delta {
            let base = self
                .updates
                .get(&delta.base_epoch)
                .and_then(|bundle| bundle.manifest.as_ref())
                .ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("delta base epoch {} missing", delta.base_epoch),
                    )
                })?;
            for digest in &base.chunks {
                let chunk = self.chunks.get(digest).ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("delta base chunk {} missing", hex::encode(digest)),
                    )
                })?;
                payload.extend_from_slice(chunk);
            }
        }
        for digest in &manifest.chunks {
            let chunk = self.chunks.get(digest).ok_or_else(|| {
                NineDoorError::protocol(
                    ErrorCode::NotFound,
                    format!("manifest chunk {} missing", hex::encode(digest)),
                )
            })?;
            payload.extend_from_slice(chunk);
        }
        Ok(payload)
    }

    fn quarantine_chunk(&mut self, epoch: &str, expected: &[u8; 32], actual: &[u8], bytes: usize) {
        let entry = QuarantineEntry {
            epoch: epoch.to_owned(),
            expected: hex::encode(expected),
            actual: hex::encode(actual),
            bytes,
        };
        if self.quarantine.len() >= CAS_QUARANTINE_LIMIT {
            self.quarantine.pop_front();
        }
        self.events.push_back(CasEvent::warn(format!(
            "cas-chunk quarantined epoch={} expected={} actual={} bytes={}",
            entry.epoch, entry.expected, entry.actual, entry.bytes
        )));
        self.quarantine.push_back(entry);
    }

    fn ensure_enabled(&self) -> Result<(), NineDoorError> {
        if self.config.is_enabled() {
            Ok(())
        } else {
            Err(NineDoorError::protocol(ErrorCode::NotFound, "cas disabled"))
        }
    }

    fn ensure_models_enabled(&self) -> Result<(), NineDoorError> {
        if self.config.models_enabled() {
            Ok(())
        } else {
            Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                "models disabled",
            ))
        }
    }

    fn ensure_epoch(&mut self, epoch: &str) -> Result<(), NineDoorError> {
        validate_epoch(epoch)?;
        if self.updates.contains_key(epoch) {
            return Ok(());
        }
        if self.updates.len() >= CAS_MAX_UPDATES {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                "cas update capacity exceeded",
            ));
        }
        self.updates
            .insert(epoch.to_owned(), UpdateBundle::default());
        Ok(())
    }

    fn ensure_model(&mut self, digest: &[u8; 32]) -> Result<(), NineDoorError> {
        if self.models.contains_key(digest) {
            return Ok(());
        }
        if self.models.len() >= CAS_MAX_MODELS {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                "cas model capacity exceeded",
            ));
        }
        self.models.insert(*digest, ModelBundle::default());
        Ok(())
    }

    pub fn ensure_update(&mut self, epoch: &str) -> Result<(), NineDoorError> {
        self.ensure_enabled()?;
        self.ensure_epoch(epoch)
    }

    pub fn ensure_model_entry(&mut self, digest: &[u8; 32]) -> Result<(), NineDoorError> {
        self.ensure_models_enabled()?;
        self.ensure_model(digest)
    }

    fn can_reserve_bytes(&self, additional: usize) -> bool {
        if self.config.chunk_bytes == 0 {
            return false;
        }
        let max_bytes = self.config.chunk_bytes.saturating_mul(CAS_MAX_CHUNKS);
        self.bytes_used.saturating_add(additional) <= max_bytes
    }
}

#[derive(Debug, Default)]
struct UpdateBundle {
    manifest_bytes: Option<Vec<u8>>,
    manifest_pending: Vec<u8>,
    manifest: Option<CasManifest>,
}

#[derive(Debug, Clone)]
struct UpdateStatusSnapshot {
    epoch: String,
    state: &'static str,
    manifest_bytes: usize,
    manifest_pending_bytes: usize,
    chunks_expected: usize,
    chunks_committed: usize,
    chunks_pending: usize,
    chunks_missing: usize,
    payload_bytes: u64,
    payload_sha256: Option<[u8; 32]>,
    delta_base_epoch: Option<String>,
    delta_base_sha256: Option<[u8; 32]>,
}

#[derive(Debug, Default)]
struct ModelBundle {
    weights_committed: bool,
    schema: Option<Vec<u8>>,
    signature: Option<Vec<u8>>,
}

#[derive(Debug)]
struct QuarantineEntry {
    epoch: String,
    expected: String,
    actual: String,
    bytes: usize,
}

#[derive(Debug, Clone)]
pub struct CasEvent {
    pub level: TraceLevel,
    pub message: String,
}

impl CasEvent {
    fn info(message: String) -> Self {
        Self {
            level: TraceLevel::Info,
            message,
        }
    }

    fn warn(message: String) -> Self {
        Self {
            level: TraceLevel::Warn,
            message,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ModelFileKind {
    Weights,
    Schema,
    Signature,
}

impl CasStore {
    fn update_status_snapshot(&self, epoch: &str) -> Result<UpdateStatusSnapshot, NineDoorError> {
        let bundle = self
            .updates
            .get(epoch)
            .ok_or_else(|| NineDoorError::protocol(ErrorCode::NotFound, "update not found"))?;
        let manifest_bytes = bundle.manifest_bytes.as_ref().map_or(0, |data| data.len());
        let manifest_pending_bytes = bundle.manifest_pending.len();
        let mut snapshot = UpdateStatusSnapshot {
            epoch: epoch.to_owned(),
            state: "empty",
            manifest_bytes,
            manifest_pending_bytes,
            chunks_expected: 0,
            chunks_committed: 0,
            chunks_pending: 0,
            chunks_missing: 0,
            payload_bytes: 0,
            payload_sha256: None,
            delta_base_epoch: None,
            delta_base_sha256: None,
        };
        let Some(manifest) = bundle.manifest.as_ref() else {
            if manifest_pending_bytes > 0 {
                snapshot.state = "manifest_pending";
            }
            return Ok(snapshot);
        };
        snapshot.payload_bytes = manifest.payload_bytes;
        snapshot.payload_sha256 = Some(manifest.payload_sha256);
        if let Some(delta) = &manifest.delta {
            snapshot.delta_base_epoch = Some(delta.base_epoch.clone());
            snapshot.delta_base_sha256 = Some(delta.base_sha256);
        }
        snapshot.chunks_expected = manifest.chunks.len();
        for digest in &manifest.chunks {
            if self.chunks.contains_key(digest) {
                snapshot.chunks_committed = snapshot.chunks_committed.saturating_add(1);
                continue;
            }
            if self.pending_chunks.contains_key(digest) {
                snapshot.chunks_pending = snapshot.chunks_pending.saturating_add(1);
            }
        }
        snapshot.chunks_missing = snapshot
            .chunks_expected
            .saturating_sub(snapshot.chunks_committed)
            .saturating_sub(snapshot.chunks_pending);
        if snapshot.chunks_expected == snapshot.chunks_committed {
            snapshot.state = "ready";
        } else {
            snapshot.state = "chunks_pending";
        }
        Ok(snapshot)
    }
}

pub fn decode_payload(data: &[u8]) -> Result<Vec<u8>, NineDoorError> {
    let trimmed = trim_payload(data);
    if let Some(encoded) = trimmed.strip_prefix(b"b64:") {
        return BASE64_STANDARD
            .decode(encoded)
            .map_err(|_| NineDoorError::protocol(ErrorCode::Invalid, "base64 decode failed"));
    }
    Ok(trimmed.to_vec())
}

fn build_update_status_text(snapshot: &UpdateStatusSnapshot) -> Result<Vec<u8>, NineDoorError> {
    let payload_sha = snapshot
        .payload_sha256
        .map(hex::encode)
        .unwrap_or_else(|| "none".to_owned());
    let (delta_epoch, delta_sha) = match (&snapshot.delta_base_epoch, snapshot.delta_base_sha256) {
        (Some(epoch), Some(sha)) => (epoch.as_str(), hex::encode(sha)),
        _ => ("none", "none".to_owned()),
    };
    let mut text = String::new();
    let _ = writeln!(
        text,
        "status epoch={} state={}",
        snapshot.epoch, snapshot.state
    );
    let _ = writeln!(
        text,
        "manifest_bytes={} manifest_pending_bytes={}",
        snapshot.manifest_bytes, snapshot.manifest_pending_bytes
    );
    let _ = writeln!(
        text,
        "chunks_expected={} chunks_committed={} chunks_pending={} chunks_missing={}",
        snapshot.chunks_expected,
        snapshot.chunks_committed,
        snapshot.chunks_pending,
        snapshot.chunks_missing
    );
    let _ = writeln!(
        text,
        "payload_bytes={} payload_sha256={}",
        snapshot.payload_bytes, payload_sha
    );
    let _ = writeln!(
        text,
        "delta_base_epoch={} delta_base_sha256={}",
        delta_epoch, delta_sha
    );
    ensure_stream_len("updates/<epoch>/status", text.len())?;
    Ok(text.into_bytes())
}

fn build_update_status_cbor(snapshot: &UpdateStatusSnapshot) -> Result<Vec<u8>, NineDoorError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer
        .map(11)
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("epoch")
        .and_then(|_| writer.text(&snapshot.epoch))
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("state")
        .and_then(|_| writer.text(snapshot.state))
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("manifest_bytes")
        .and_then(|_| writer.unsigned(snapshot.manifest_bytes as u64))
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("manifest_pending_bytes")
        .and_then(|_| writer.unsigned(snapshot.manifest_pending_bytes as u64))
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("chunks_expected")
        .and_then(|_| writer.unsigned(snapshot.chunks_expected as u64))
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("chunks_committed")
        .and_then(|_| writer.unsigned(snapshot.chunks_committed as u64))
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("chunks_pending")
        .and_then(|_| writer.unsigned(snapshot.chunks_pending as u64))
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("chunks_missing")
        .and_then(|_| writer.unsigned(snapshot.chunks_missing as u64))
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("payload_bytes")
        .and_then(|_| writer.unsigned(snapshot.payload_bytes))
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("payload_sha256")
        .and_then(|_| match snapshot.payload_sha256 {
            Some(sha) => writer.bytes(&sha),
            None => writer.null(),
        })
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    writer
        .text("delta")
        .and_then(
            |_| match (&snapshot.delta_base_epoch, snapshot.delta_base_sha256) {
                (Some(epoch), Some(sha)) => {
                    writer.map(2)?;
                    writer.text("base_epoch")?;
                    writer.text(epoch)?;
                    writer.text("base_sha256")?;
                    writer.bytes(&sha)?;
                    Ok(())
                }
                _ => writer.null(),
            },
        )
        .map_err(|err| cbor_error("updates/<epoch>/status.cbor", err))?;
    Ok(writer.into_bytes())
}

fn ensure_stream_len(label: &str, len: usize) -> Result<(), NineDoorError> {
    if len > UI_MAX_STREAM_BYTES {
        return Err(NineDoorError::protocol(
            ErrorCode::TooBig,
            format!("{label} output exceeds {} bytes", UI_MAX_STREAM_BYTES),
        ));
    }
    Ok(())
}

fn cbor_error(label: &str, err: CborError) -> NineDoorError {
    match err {
        CborError::TooLarge => NineDoorError::protocol(
            ErrorCode::TooBig,
            format!("{label} output exceeds {} bytes", UI_MAX_STREAM_BYTES),
        ),
    }
}

fn trim_payload(data: &[u8]) -> &[u8] {
    let mut end = data.len();
    if end > 0 && data[end - 1] == b'\n' {
        end -= 1;
        if end > 0 && data[end - 1] == b'\r' {
            end -= 1;
        }
    }
    &data[..end]
}

pub fn validate_epoch(epoch: &str) -> Result<(), NineDoorError> {
    let trimmed = epoch.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_EPOCH_LEN {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            "invalid update epoch",
        ));
    }
    if !trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            "invalid update epoch",
        ));
    }
    Ok(())
}

pub fn parse_sha256(hex_str: &str) -> Result<[u8; 32], NineDoorError> {
    if hex_str.len() != 64 || !hex_str.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            "invalid sha256 digest",
        ));
    }
    let mut out = [0u8; 32];
    hex::decode_to_slice(hex_str.as_bytes(), &mut out)
        .map_err(|_| NineDoorError::protocol(ErrorCode::Invalid, "invalid sha256 digest"))?;
    Ok(out)
}

fn read_slice(data: &[u8], offset: u64, count: u32) -> Vec<u8> {
    let start = offset as usize;
    if start >= data.len() {
        return Vec::new();
    }
    let end = start.saturating_add(count as usize).min(data.len());
    data[start..end].to_vec()
}
