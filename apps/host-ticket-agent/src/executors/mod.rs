// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Dispatch host ticket actions to bounded executor adapters.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use coh::CohAccess;
use cohsh::{Session, Transport};
use serde_json::Value;

use crate::HostTicketSpec;

/// Docker remediation executor.
pub mod docker;
/// GPU lease executor.
pub mod gpu;
/// Kubernetes coexistence executor.
pub mod k8s;
/// PEFT lifecycle executor.
pub mod peft;
/// systemd remediation executor.
pub mod systemd;

/// Runtime configuration used by executors.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Host namespace mount root.
    pub mount: String,
    /// Default PEFT registry root.
    pub registry_root: PathBuf,
}

/// Execute one host ticket action.
pub fn execute_action(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    if spec.action.starts_with("gpu.lease.") {
        return gpu::execute(transport, session, spec, config);
    }
    if spec.action.starts_with("peft.") {
        return peft::execute(transport, session, spec, config);
    }
    if spec.action.starts_with("systemd.") {
        return systemd::execute(transport, session, spec, config);
    }
    if spec.action.starts_with("docker.") {
        return docker::execute(transport, session, spec, config);
    }
    if spec.action.starts_with("k8s.") {
        return k8s::execute(transport, session, spec, config);
    }
    Err(anyhow!("unsupported ticket action {}", spec.action))
}

/// Adapter allowing `coh::peft` helpers to operate on a `cohsh::Transport` session.
pub struct TransportAccess<'a> {
    transport: &'a mut dyn Transport,
    session: &'a Session,
}

impl<'a> TransportAccess<'a> {
    /// Construct an access wrapper.
    pub fn new(transport: &'a mut dyn Transport, session: &'a Session) -> Self {
        Self { transport, session }
    }
}

impl CohAccess for TransportAccess<'_> {
    fn list_dir(&mut self, path: &str, _max_bytes: usize) -> Result<Vec<String>> {
        self.transport.list(self.session, path)
    }

    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let lines = self.transport.read(self.session, path)?;
        lines_to_bytes(&lines, max_bytes)
    }

    fn tail_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let lines = self.transport.tail(self.session, path)?;
        lines_to_bytes(&lines, max_bytes)
    }

    fn write_append(&mut self, path: &str, payload: &[u8]) -> Result<usize> {
        self.transport.write(self.session, path, payload)?;
        Ok(payload.len())
    }
}

/// Borrow a string argument from `spec.args`.
pub fn arg_str<'a>(spec: &'a HostTicketSpec, key: &str) -> Option<&'a str> {
    spec.args
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Borrow an integer argument from `spec.args`.
pub fn arg_u64(spec: &HostTicketSpec, key: &str) -> Option<u64> {
    spec.args.get(key).and_then(Value::as_u64)
}

/// Borrow a boolean argument from `spec.args`.
pub fn arg_bool(spec: &HostTicketSpec, key: &str) -> Option<bool> {
    spec.args.get(key).and_then(Value::as_bool)
}

/// Parse a path-like target into non-empty components.
pub fn target_components(spec: &HostTicketSpec) -> Vec<&str> {
    spec.target
        .as_deref()
        .map(|target| {
            target
                .split('/')
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// Read the last non-empty line from a path.
pub fn read_last_line(
    transport: &mut dyn Transport,
    session: &Session,
    path: &str,
) -> Result<Option<String>> {
    let lines = transport.tail(session, path)?;
    Ok(lines
        .iter()
        .rev()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .map(str::to_owned))
}

fn lines_to_bytes(lines: &[String], max_bytes: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for line in lines {
        if !out.is_empty() {
            out.push(b'\n');
        }
        out.extend_from_slice(line.as_bytes());
        if out.len() > max_bytes {
            return Err(anyhow!("payload exceeds max bytes {max_bytes}"));
        }
    }
    if !out.is_empty() {
        out.push(b'\n');
    }
    Ok(out)
}
