// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide shared helpers for the coh host bridge CLI.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Shared helpers for the Cohesix host bridge CLI.

/// TCP console-backed helpers.
pub mod console;
/// GPU inventory and lease helpers.
pub mod gpu;
/// Secure9P-backed mount adapter.
pub mod mount;
/// PEFT/LoRA lifecycle helpers.
pub mod peft;
/// Manifest-derived policy loader.
pub mod policy;
/// Runtime command wrapper helpers.
pub mod run;
/// Telemetry pull helpers.
pub mod telemetry;
/// TCP transport wrapper for Secure9P.
pub mod transport;

use anyhow::{anyhow, Context, Result};
use cohsh::client::CohClient;
use cohsh_core::wire::{render_ack, AckLine, AckStatus};
use cohsh_core::Secure9pTransport;
use secure9p_codec::OpenMode;

/// Maximum allowed Secure9P walk depth.
pub const MAX_PATH_COMPONENTS: usize = 8;
/// Maximum bytes read when listing a directory.
pub const MAX_DIR_LIST_BYTES: usize = 64 * 1024;

/// Buffered audit transcript used by coh commands.
#[derive(Debug, Default)]
pub struct CohAudit {
    lines: Vec<String>,
}

impl CohAudit {
    /// Create a new empty audit transcript.
    #[must_use]
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Borrow the collected transcript lines.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Consume the audit and return the captured lines.
    #[must_use]
    pub fn into_lines(self) -> Vec<String> {
        self.lines
    }

    /// Append an acknowledgement line to the transcript.
    pub fn push_ack(&mut self, status: AckStatus, verb: &str, detail: Option<&str>) {
        self.lines.push(render_ack_line(status, verb, detail));
    }

    /// Append a plain output line to the transcript.
    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }
}

pub(crate) fn render_ack_line(status: AckStatus, verb: &str, detail: Option<&str>) -> String {
    let ack = AckLine {
        status,
        verb,
        detail,
    };
    let mut line = String::new();
    render_ack(&mut line, &ack).expect("render ack line");
    line
}

pub(crate) fn read_file<T: Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let fid = client
        .open(path, OpenMode::read_only())
        .with_context(|| format!("open {path} for read"))?;
    let mut offset = 0u64;
    let mut buffer = Vec::new();
    let count = client.negotiated_msize();
    loop {
        let chunk = client
            .read(fid, offset, count)
            .with_context(|| format!("read {path}"))?;
        if chunk.is_empty() {
            break;
        }
        if buffer.len().saturating_add(chunk.len()) > max_bytes {
            let _ = client.clunk(fid);
            return Err(anyhow!("read {path} exceeds max bytes {max_bytes}"));
        }
        buffer.extend_from_slice(&chunk);
        offset = offset
            .checked_add(chunk.len() as u64)
            .context("read offset overflow")?;
        if chunk.len() < count as usize {
            break;
        }
    }
    client.clunk(fid).with_context(|| format!("clunk {path}"))?;
    Ok(buffer)
}

pub(crate) fn list_dir<T: Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
    max_bytes: usize,
) -> Result<Vec<String>> {
    let bytes = read_file(client, path, max_bytes)?;
    let text = String::from_utf8(bytes)
        .with_context(|| format!("directory listing {path} is not UTF-8"))?;
    let entries = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    Ok(entries)
}

pub(crate) fn write_append<T: Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
    payload: &[u8],
) -> Result<usize> {
    let fid = client
        .open(path, OpenMode::write_append())
        .with_context(|| format!("open {path} for append"))?;
    let written = client
        .write(fid, u64::MAX, payload)
        .with_context(|| format!("write {path}"))?;
    let clunk_result = client.clunk(fid);
    if written as usize != payload.len() {
        return Err(anyhow!(
            "short write to {path}: expected {} bytes, wrote {written}",
            payload.len()
        ));
    }
    clunk_result.with_context(|| format!("clunk {path}"))?;
    Ok(written as usize)
}

pub(crate) fn validate_component(component: &str) -> Result<()> {
    if component.is_empty() {
        return Err(anyhow!("path component must not be empty"));
    }
    if component == "." || component == ".." {
        return Err(anyhow!("path component '{component}' is not permitted"));
    }
    if component.contains('/') {
        return Err(anyhow!("path component '{component}' contains '/'"));
    }
    if component.as_bytes().iter().any(|byte| *byte == 0) {
        return Err(anyhow!("path component contains NUL byte"));
    }
    Ok(())
}

/// Minimal file operations used by coh subcommands.
pub trait CohAccess {
    /// List directory entries at the supplied path.
    fn list_dir(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>>;
    /// Read an entire file into memory.
    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>>;
    /// Append payload bytes to a file.
    fn write_append(&mut self, path: &str, payload: &[u8]) -> Result<usize>;
}

impl<T: Secure9pTransport> CohAccess for CohClient<T> {
    fn list_dir(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>> {
        list_dir(self, path, max_bytes)
    }

    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        read_file(self, path, max_bytes)
    }

    fn write_append(&mut self, path: &str, payload: &[u8]) -> Result<usize> {
        write_append(self, path, payload)
    }
}
