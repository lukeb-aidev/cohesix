// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide host-side GPU worker descriptors and ticket claims.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! GPU worker scaffolding executing vector and matrix workloads in host mode.
//! The worker verifies payload hashes before simulating job execution so tests
//! can exercise the end-to-end lease and submission flow described in
//! `docs/GPU_NODES.md`.

use std::fmt;

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims};
use rand::{rngs::OsRng, TryRngCore};
use secure9p_codec::SessionId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Identifier assigned to GPU jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobId(String);

impl JobId {
    /// Generate a random job identifier with a `job-` prefix.
    pub fn random() -> Self {
        let mut bytes = [0u8; 8];
        OsRng
            .try_fill_bytes(&mut bytes)
            .expect("os entropy source unavailable");
        Self(format!("job-{}", hex::encode(bytes)))
    }

    /// Borrow the identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Supported kernels surfaced through the submission protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KernelKind {
    /// Element-wise vector addition.
    Vadd,
    /// Matrix multiply (row-major).
    Matmul,
}

impl fmt::Display for KernelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vadd => write!(f, "vadd"),
            Self::Matmul => write!(f, "matmul"),
        }
    }
}

/// Submission descriptor mirroring `docs/GPU_NODES.md §5`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobDescriptor {
    /// Stable job identifier.
    pub job: JobId,
    /// Kernel variant to launch.
    pub kernel: KernelKind,
    /// Grid dimensions.
    pub grid: [u32; 3],
    /// Block dimensions.
    pub block: [u32; 3],
    /// Expected SHA-256 hash for the payload bytes.
    pub bytes_hash: String,
    /// Input artefacts (paths mirrored into the VM namespace).
    pub inputs: Vec<String>,
    /// Output artefacts to be written by the host bridge.
    pub outputs: Vec<String>,
    /// Deadline in milliseconds before the job is cancelled.
    pub timeout_ms: u32,
    /// Optional inline payload encoded as Base64 for mock flows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_b64: Option<String>,
}

impl JobDescriptor {
    /// Validate the descriptor semantics and optional payload hash.
    pub fn validate(&self) -> Result<()> {
        if !self.bytes_hash.starts_with("sha256:") {
            return Err(anyhow!("bytes_hash must use sha256:<hex> format"));
        }
        let expected = &self.bytes_hash[7..];
        if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(anyhow!("bytes_hash must contain 64 hex characters"));
        }
        if let Some(encoded) = &self.payload_b64 {
            let payload = BASE64_STANDARD
                .decode(encoded)
                .context("payload_b64 is not valid base64")?;
            let mut hasher = Sha256::new();
            hasher.update(&payload);
            let hash = hasher.finalize();
            if hex::encode(hash) != expected {
                return Err(anyhow!("payload hash mismatch"));
            }
        }
        Ok(())
    }
}

/// GPU worker representation capturing the assigned lease.
#[derive(Debug, Clone)]
pub struct GpuWorker {
    ticket: TicketClaims,
    session: SessionId,
    lease: GpuLease,
}

impl GpuWorker {
    /// Construct a GPU worker with the provided lease specification.
    pub fn new(session: SessionId, lease: GpuLease) -> Self {
        let ticket = TicketClaims::new(
            Role::WorkerGpu,
            BudgetSpec::default_gpu(),
            None,
            MountSpec::empty(),
            0,
        );
        Self {
            ticket,
            session,
            lease,
        }
    }

    /// Borrow the GPU lease information.
    #[must_use]
    pub fn lease(&self) -> &GpuLease {
        &self.lease
    }

    /// Retrieve the associated capability ticket template.
    #[must_use]
    pub fn ticket(&self) -> &TicketClaims {
        &self.ticket
    }

    /// Retrieve the assigned session identifier.
    #[must_use]
    pub fn session(&self) -> SessionId {
        self.session
    }

    /// Produce a validated job descriptor for a vector add workload.
    pub fn vector_add(&self, lhs: &[f32], rhs: &[f32]) -> Result<JobDescriptor> {
        if lhs.len() != rhs.len() {
            return Err(anyhow!("vector lengths must match"));
        }
        let payload = serialize_operands(lhs, rhs, &[]);
        self.build_descriptor(KernelKind::Vadd, payload)
    }

    /// Produce a validated job descriptor for a matrix multiply workload.
    pub fn matmul(
        &self,
        a: &[f32],
        b: &[f32],
        dims: (usize, usize, usize),
    ) -> Result<JobDescriptor> {
        let (m, k, n) = dims;
        if a.len() != m * k || b.len() != k * n {
            return Err(anyhow!("matrix operand dimensions do not match"));
        }
        let payload = serialize_operands(a, b, &[]);
        self.build_descriptor(KernelKind::Matmul, payload)
    }

    fn build_descriptor(&self, kernel: KernelKind, payload: Vec<u8>) -> Result<JobDescriptor> {
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let hash = hasher.finalize();
        let hash_hex = hex::encode(hash);
        let encoded = BASE64_STANDARD.encode(payload);
        let descriptor = JobDescriptor {
            job: JobId::random(),
            kernel,
            grid: [1, 1, 1],
            block: [self.lease.streams as u32, 1, 1],
            bytes_hash: format!("sha256:{hash_hex}"),
            inputs: vec![format!("/bundles/{}.ptx", kernel)],
            outputs: vec![format!("/worker/{}/result", self.lease.worker_id)],
            timeout_ms: 5_000,
            payload_b64: Some(encoded),
        };
        descriptor.validate()?;
        Ok(descriptor)
    }
}

/// Lease specification used during spawn flows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuLease {
    /// Identifier of the GPU requested by the worker.
    pub gpu_id: String,
    /// Memory budget in mebibytes.
    pub mem_mb: u32,
    /// Maximum concurrent streams permitted.
    pub streams: u8,
    /// Lease time-to-live in seconds.
    pub ttl_s: u32,
    /// Priority value surfaced to the host bridge.
    pub priority: u8,
    /// Identifier for the worker owning the lease.
    pub worker_id: String,
}

impl GpuLease {
    /// Create a GPU lease bound to a worker identifier.
    pub fn new(
        gpu_id: impl Into<String>,
        mem_mb: u32,
        streams: u8,
        ttl_s: u32,
        priority: u8,
        worker_id: impl Into<String>,
    ) -> Result<Self> {
        if streams == 0 {
            return Err(anyhow!("streams must be at least 1"));
        }
        Ok(Self {
            gpu_id: gpu_id.into(),
            mem_mb,
            streams,
            ttl_s,
            priority,
            worker_id: worker_id.into(),
        })
    }
}

fn serialize_operands(a: &[f32], b: &[f32], extra: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity((a.len() + b.len() + extra.len()) * 4);
    for slice in [a, b, extra] {
        for value in slice {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_descriptor_validates_hash() {
        let lease = GpuLease::new("GPU-0", 4096, 2, 120, 7, "gpu-worker-1").unwrap();
        let worker = GpuWorker::new(SessionId::from_raw(9), lease);
        let lhs = vec![1.0f32, 2.0, 3.0];
        let rhs = vec![4.0f32, 5.0, 6.0];
        let descriptor = worker.vector_add(&lhs, &rhs).unwrap();
        descriptor.validate().unwrap();
        assert_eq!(descriptor.kernel, KernelKind::Vadd);
        assert!(descriptor.bytes_hash.starts_with("sha256:"));
        assert!(descriptor.payload_b64.is_some());
    }

    #[test]
    fn matmul_descriptor_respects_dimensions() {
        let lease = GpuLease::new("GPU-0", 4096, 4, 120, 5, "gpu-worker-2").unwrap();
        let worker = GpuWorker::new(SessionId::from_raw(3), lease);
        let a = vec![1.0f32; 6];
        let b = vec![1.0f32; 6];
        let descriptor = worker.matmul(&a, &b, (2, 3, 2)).unwrap();
        descriptor.validate().unwrap();
        assert_eq!(descriptor.kernel, KernelKind::Matmul);
    }

    #[test]
    fn invalid_hash_rejected() {
        let descriptor = JobDescriptor {
            job: JobId("job-test".into()),
            kernel: KernelKind::Vadd,
            grid: [1, 1, 1],
            block: [1, 1, 1],
            bytes_hash: "sha256:not-hex".into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            timeout_ms: 1,
            payload_b64: None,
        };
        assert!(descriptor.validate().is_err());
    }

    #[test]
    fn payload_mismatch_detected() {
        let mut descriptor = JobDescriptor {
            job: JobId("job-test".into()),
            kernel: KernelKind::Matmul,
            grid: [1, 1, 1],
            block: [1, 1, 1],
            bytes_hash: format!("sha256:{}", "00".repeat(32)),
            inputs: Vec::new(),
            outputs: Vec::new(),
            timeout_ms: 1,
            payload_b64: None,
        };
        descriptor.payload_b64 = Some(BASE64_STANDARD.encode(vec![1, 2, 3]));
        assert!(descriptor.validate().is_err());
    }
}
