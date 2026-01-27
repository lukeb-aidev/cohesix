// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Encode and decode Cohesix CAS manifest CBOR payloads.
// Author: Lukas Bower
#![no_std]

extern crate alloc;

use alloc::{borrow::ToOwned, string::String, vec::Vec};
use core::fmt;

/// CAS manifest schema identifier.
pub const CAS_MANIFEST_SCHEMA: &str = "cohesix-cas/manifest-v1";

/// CAS manifest representation used across Cohesix components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CasManifest {
    /// Schema identifier (must match [`CAS_MANIFEST_SCHEMA`]).
    pub schema: String,
    /// Update epoch label.
    pub epoch: String,
    /// Fixed chunk size in bytes.
    pub chunk_bytes: u32,
    /// Total payload bytes represented by `chunks`.
    pub payload_bytes: u64,
    /// SHA-256 of the assembled payload (base or base+delta).
    pub payload_sha256: [u8; 32],
    /// SHA-256 chunk digests for the payload represented by this manifest.
    pub chunks: Vec<[u8; 32]>,
    /// Optional delta metadata.
    pub delta: Option<CasDelta>,
    /// Optional Ed25519 signature (64 bytes).
    pub signature: Option<[u8; 64]>,
}

/// Delta metadata for CAS manifests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CasDelta {
    /// Base epoch that the delta applies to.
    pub base_epoch: String,
    /// SHA-256 digest of the base payload.
    pub base_sha256: [u8; 32],
}

/// Errors returned by manifest encode/decode operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CasManifestError {
    /// The CBOR payload ended unexpectedly.
    UnexpectedEof,
    /// The manifest did not match the expected schema.
    InvalidSchema,
    /// The CBOR payload contained an invalid type or value.
    InvalidCbor(&'static str),
    /// The manifest failed UTF-8 validation.
    InvalidUtf8,
    /// The manifest used an unsupported field encoding.
    InvalidField(&'static str),
}

impl fmt::Display for CasManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of manifest payload"),
            Self::InvalidSchema => write!(f, "unsupported cas manifest schema"),
            Self::InvalidCbor(reason) => write!(f, "invalid manifest cbor: {reason}"),
            Self::InvalidUtf8 => write!(f, "manifest contains invalid utf-8"),
            Self::InvalidField(reason) => write!(f, "invalid manifest field: {reason}"),
        }
    }
}

impl CasManifest {
    /// Encode the manifest as CBOR, including the current signature (or null).
    pub fn encode_signed(&self) -> Result<Vec<u8>, CasManifestError> {
        encode_manifest(self, self.signature.as_ref())
    }

    /// Encode the manifest as CBOR for signing (signature field is null).
    pub fn signature_payload(&self) -> Result<Vec<u8>, CasManifestError> {
        encode_manifest(self, None)
    }

    /// Decode a manifest from CBOR bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, CasManifestError> {
        decode_manifest(bytes)
    }
}

fn encode_manifest(
    manifest: &CasManifest,
    signature: Option<&[u8; 64]>,
) -> Result<Vec<u8>, CasManifestError> {
    let mut encoder = Encoder::new();
    encoder.array_len(8)?;
    encoder.text(&manifest.schema)?;
    encoder.text(&manifest.epoch)?;
    encoder.unsigned(manifest.chunk_bytes as u64)?;
    encoder.unsigned(manifest.payload_bytes)?;
    encoder.bytes(&manifest.payload_sha256)?;
    encoder.array_len(manifest.chunks.len())?;
    for digest in &manifest.chunks {
        encoder.bytes(digest)?;
    }
    if let Some(delta) = &manifest.delta {
        encoder.array_len(2)?;
        encoder.text(&delta.base_epoch)?;
        encoder.bytes(&delta.base_sha256)?;
    } else {
        encoder.null();
    }
    if let Some(sig) = signature {
        encoder.bytes(sig)?;
    } else {
        encoder.null();
    }
    Ok(encoder.finish())
}

fn decode_manifest(bytes: &[u8]) -> Result<CasManifest, CasManifestError> {
    let mut decoder = Decoder::new(bytes);
    let len = decoder.array_len()?;
    if len != 8 {
        return Err(CasManifestError::InvalidField("manifest array length"));
    }
    let schema = decoder.text()?;
    if schema != CAS_MANIFEST_SCHEMA {
        return Err(CasManifestError::InvalidSchema);
    }
    let epoch = decoder.text()?;
    let chunk_bytes = decoder.unsigned()? as u32;
    let payload_bytes = decoder.unsigned()?;
    let payload_sha256 = decoder.bytes_fixed_32()?;
    let chunk_count = decoder.array_len()?;
    let mut chunks = Vec::new();
    for _ in 0..chunk_count {
        chunks.push(decoder.bytes_fixed_32()?);
    }
    let delta = match decoder.peek_type()? {
        CborType::Null => {
            decoder.null()?;
            None
        }
        CborType::Array => {
            let delta_len = decoder.array_len()?;
            if delta_len != 2 {
                return Err(CasManifestError::InvalidField("delta array length"));
            }
            let base_epoch = decoder.text()?;
            let base_sha256 = decoder.bytes_fixed_32()?;
            Some(CasDelta {
                base_epoch,
                base_sha256,
            })
        }
        _ => return Err(CasManifestError::InvalidField("delta field type")),
    };
    let signature = match decoder.peek_type()? {
        CborType::Null => {
            decoder.null()?;
            None
        }
        CborType::Bytes => Some(decoder.bytes_fixed_64()?),
        _ => return Err(CasManifestError::InvalidField("signature field type")),
    };
    decoder.ensure_eof()?;
    Ok(CasManifest {
        schema,
        epoch,
        chunk_bytes,
        payload_bytes,
        payload_sha256,
        chunks,
        delta,
        signature,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CborType {
    Array,
    Bytes,
    Text,
    Unsigned,
    Null,
}

struct Encoder {
    data: Vec<u8>,
}

impl Encoder {
    fn new() -> Self {
        Self { data: Vec::new() }
    }

    fn finish(self) -> Vec<u8> {
        self.data
    }

    fn array_len(&mut self, len: usize) -> Result<(), CasManifestError> {
        self.major_len(4, len as u64)
    }

    fn text(&mut self, value: &str) -> Result<(), CasManifestError> {
        self.major_len(3, value.len() as u64)?;
        self.data.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn bytes(&mut self, value: &[u8]) -> Result<(), CasManifestError> {
        self.major_len(2, value.len() as u64)?;
        self.data.extend_from_slice(value);
        Ok(())
    }

    fn unsigned(&mut self, value: u64) -> Result<(), CasManifestError> {
        self.major_len(0, value)
    }

    fn null(&mut self) {
        self.data.push(0xf6);
    }

    fn major_len(&mut self, major: u8, value: u64) -> Result<(), CasManifestError> {
        let header = major << 5;
        if value < 24 {
            self.data.push(header | value as u8);
            return Ok(());
        }
        if value <= u8::MAX as u64 {
            self.data.push(header | 24);
            self.data.push(value as u8);
            return Ok(());
        }
        if value <= u16::MAX as u64 {
            self.data.push(header | 25);
            self.data.extend_from_slice(&(value as u16).to_be_bytes());
            return Ok(());
        }
        if value <= u32::MAX as u64 {
            self.data.push(header | 26);
            self.data.extend_from_slice(&(value as u32).to_be_bytes());
            return Ok(());
        }
        self.data.push(header | 27);
        self.data.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }
}

struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn ensure_eof(&self) -> Result<(), CasManifestError> {
        if self.pos == self.data.len() {
            Ok(())
        } else {
            Err(CasManifestError::InvalidCbor("trailing bytes"))
        }
    }

    fn peek_type(&self) -> Result<CborType, CasManifestError> {
        let byte = *self
            .data
            .get(self.pos)
            .ok_or(CasManifestError::UnexpectedEof)?;
        match byte {
            0xf6 => Ok(CborType::Null),
            _ => {
                let major = byte >> 5;
                match major {
                    0 => Ok(CborType::Unsigned),
                    2 => Ok(CborType::Bytes),
                    3 => Ok(CborType::Text),
                    4 => Ok(CborType::Array),
                    _ => Err(CasManifestError::InvalidCbor("unexpected major type")),
                }
            }
        }
    }

    fn read_byte(&mut self) -> Result<u8, CasManifestError> {
        let byte = *self
            .data
            .get(self.pos)
            .ok_or(CasManifestError::UnexpectedEof)?;
        self.pos = self.pos.saturating_add(1);
        Ok(byte)
    }

    fn read_len(&mut self, major: u8) -> Result<u64, CasManifestError> {
        let byte = self.read_byte()?;
        if byte == 0xf6 {
            return Err(CasManifestError::InvalidCbor("unexpected null"));
        }
        let byte_major = byte >> 5;
        if byte_major != major {
            return Err(CasManifestError::InvalidCbor("unexpected major type"));
        }
        let additional = byte & 0x1f;
        match additional {
            value if value < 24 => Ok(u64::from(value)),
            24 => Ok(u64::from(self.read_byte()?)),
            25 => {
                let bytes = self.read_bytes(2)?;
                let mut buf = [0u8; 2];
                buf.copy_from_slice(bytes);
                Ok(u64::from(u16::from_be_bytes(buf)))
            }
            26 => {
                let bytes = self.read_bytes(4)?;
                let mut buf = [0u8; 4];
                buf.copy_from_slice(bytes);
                Ok(u64::from(u32::from_be_bytes(buf)))
            }
            27 => {
                let bytes = self.read_bytes(8)?;
                let mut buf = [0u8; 8];
                buf.copy_from_slice(bytes);
                Ok(u64::from_be_bytes(buf))
            }
            _ => Err(CasManifestError::InvalidCbor("invalid additional value")),
        }
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], CasManifestError> {
        let end = self.pos.saturating_add(len);
        if end > self.data.len() {
            return Err(CasManifestError::UnexpectedEof);
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn array_len(&mut self) -> Result<usize, CasManifestError> {
        let len = self.read_len(4)?;
        Ok(len as usize)
    }

    fn text(&mut self) -> Result<String, CasManifestError> {
        let len = self.read_len(3)? as usize;
        let bytes = self.read_bytes(len)?;
        core::str::from_utf8(bytes)
            .map(|text| text.to_owned())
            .map_err(|_| CasManifestError::InvalidUtf8)
    }

    fn bytes_fixed_32(&mut self) -> Result<[u8; 32], CasManifestError> {
        let len = self.read_len(2)? as usize;
        if len != 32 {
            return Err(CasManifestError::InvalidField("expected 32-byte digest"));
        }
        let bytes = self.read_bytes(len)?;
        let mut out = [0u8; 32];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    fn bytes_fixed_64(&mut self) -> Result<[u8; 64], CasManifestError> {
        let len = self.read_len(2)? as usize;
        if len != 64 {
            return Err(CasManifestError::InvalidField("expected 64-byte signature"));
        }
        let bytes = self.read_bytes(len)?;
        let mut out = [0u8; 64];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    fn unsigned(&mut self) -> Result<u64, CasManifestError> {
        self.read_len(0)
    }

    fn null(&mut self) -> Result<(), CasManifestError> {
        let byte = self.read_byte()?;
        if byte != 0xf6 {
            return Err(CasManifestError::InvalidCbor("expected null"));
        }
        Ok(())
    }
}
