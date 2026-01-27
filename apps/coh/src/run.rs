// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide coh run wrapper for lease validation and breadcrumb logging.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::process::{Command, Stdio};

use anyhow::{anyhow, Context, Result};
use cohsh_core::wire::AckStatus;
use serde::{Deserialize, Serialize};

use crate::policy::{CohBreadcrumbPolicy, CohPolicy};
use crate::{validate_component, CohAccess, CohAudit};

const BREADCRUMB_EVENT_START: &str = "START";
const BREADCRUMB_EVENT_EXIT: &str = "EXIT";
const BREADCRUMB_STATUS_RUNNING: &str = "RUNNING";
const BREADCRUMB_STATUS_OK: &str = "OK";
const BREADCRUMB_STATUS_ERR: &str = "ERR";

/// Run request parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    /// GPU identifier to validate.
    pub gpu_id: String,
    /// Command and arguments to execute.
    pub command: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct LeaseEntry {
    schema: String,
    state: String,
    gpu_id: String,
    worker_id: String,
    mem_mb: u32,
    streams: u8,
    ttl_s: u32,
    priority: u8,
}

#[derive(Serialize)]
struct BreadcrumbEntry<'a> {
    schema: &'a str,
    event: &'a str,
    command: &'a str,
    status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
}

/// Execute a command after validating a GPU lease and logging breadcrumbs.
pub fn execute<C: CohAccess>(
    client: &mut C,
    policy: &CohPolicy,
    audit: &mut CohAudit,
    spec: &RunSpec,
) -> Result<()> {
    let gpu_id = spec.gpu_id.trim();
    if gpu_id.is_empty() {
        return Err(anyhow!("gpu id must not be empty"));
    }
    validate_component(gpu_id)?;
    if spec.command.is_empty() || spec.command[0].trim().is_empty() {
        return Err(anyhow!("command must not be empty"));
    }

    let lease_path = format!("/gpu/{gpu_id}/lease");
    let lease_bytes = client
        .read_file(&lease_path, policy.run.lease.max_bytes as usize)
        .with_context(|| format!("read {lease_path}"))?;
    let detail = format!("path={lease_path}");
    audit.push_ack(AckStatus::Ok, "CAT", Some(detail.as_str()));
    let lease_line =
        last_non_empty_line(&lease_bytes).with_context(|| format!("parse {lease_path}"))?;
    let lease_line = lease_line.ok_or_else(|| anyhow!("no active lease for gpu {gpu_id}"))?;
    let lease_entry = parse_lease_entry(&lease_line)?;
    validate_lease(&lease_entry, policy, gpu_id)?;

    let status_path = format!("/gpu/{gpu_id}/status");
    let command_line = spec.command.join(" ");

    let start_line = build_breadcrumb_line(
        &policy.run.breadcrumb,
        BREADCRUMB_EVENT_START,
        BREADCRUMB_STATUS_RUNNING,
        &command_line,
        None,
    )?;
    append_breadcrumb(client, audit, &status_path, &start_line)?;

    let mut child = match spawn_command(&spec.command) {
        Ok(child) => child,
        Err(err) => {
            let exit_line = build_breadcrumb_line(
                &policy.run.breadcrumb,
                BREADCRUMB_EVENT_EXIT,
                BREADCRUMB_STATUS_ERR,
                &command_line,
                None,
            )?;
            append_breadcrumb(client, audit, &status_path, &exit_line)?;
            return Err(err);
        }
    };

    let status = match child.wait() {
        Ok(status) => status,
        Err(err) => {
            let exit_line = build_breadcrumb_line(
                &policy.run.breadcrumb,
                BREADCRUMB_EVENT_EXIT,
                BREADCRUMB_STATUS_ERR,
                &command_line,
                None,
            )?;
            append_breadcrumb(client, audit, &status_path, &exit_line)?;
            return Err(err).context("wait for command");
        }
    };

    let (status_label, exit_code) = if status.success() {
        (BREADCRUMB_STATUS_OK, status.code())
    } else {
        (BREADCRUMB_STATUS_ERR, status.code())
    };
    let exit_line = build_breadcrumb_line(
        &policy.run.breadcrumb,
        BREADCRUMB_EVENT_EXIT,
        status_label,
        &command_line,
        exit_code,
    )?;
    append_breadcrumb(client, audit, &status_path, &exit_line)?;

    if status.success() {
        Ok(())
    } else if let Some(code) = exit_code {
        Err(anyhow!("command exited with code {code}"))
    } else {
        Err(anyhow!("command terminated by signal"))
    }
}

fn spawn_command(command: &[String]) -> Result<std::process::Child> {
    let mut cmd = Command::new(&command[0]);
    if command.len() > 1 {
        cmd.args(&command[1..]);
    }
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn {}", command[0]))
}

fn append_breadcrumb<C: CohAccess>(
    client: &mut C,
    audit: &mut CohAudit,
    path: &str,
    payload: &[u8],
) -> Result<()> {
    let written = client.write_append(path, payload)?;
    let detail = format!("path={path} bytes={written}");
    audit.push_ack(AckStatus::Ok, "ECHO", Some(detail.as_str()));
    Ok(())
}

fn last_non_empty_line(bytes: &[u8]) -> Result<Option<String>> {
    let text = String::from_utf8(bytes.to_vec()).context("lease file is not UTF-8")?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .last())
}

fn parse_lease_entry(line: &str) -> Result<LeaseEntry> {
    serde_json::from_str(line).context("invalid lease JSON")
}

fn validate_lease(entry: &LeaseEntry, policy: &CohPolicy, gpu_id: &str) -> Result<()> {
    if entry.schema != policy.run.lease.schema {
        return Err(anyhow!(
            "lease schema mismatch: expected {} got {}",
            policy.run.lease.schema,
            entry.schema
        ));
    }
    if entry.state != policy.run.lease.active_state {
        return Err(anyhow!("no active lease for gpu {gpu_id}"));
    }
    if entry.gpu_id != gpu_id {
        return Err(anyhow!(
            "lease gpu_id mismatch: expected {} got {}",
            gpu_id,
            entry.gpu_id
        ));
    }
    if entry.streams == 0 {
        return Err(anyhow!("lease streams must be >= 1"));
    }
    if entry.ttl_s == 0 {
        return Err(anyhow!("lease ttl_s must be >= 1"));
    }
    if entry.mem_mb == 0 {
        return Err(anyhow!("lease mem_mb must be >= 1"));
    }
    if entry.worker_id.trim().is_empty() {
        return Err(anyhow!("lease worker_id must not be empty"));
    }
    Ok(())
}

fn build_breadcrumb_line(
    policy: &CohBreadcrumbPolicy,
    event: &str,
    status: &str,
    command: &str,
    exit_code: Option<i32>,
) -> Result<Vec<u8>> {
    let max_line_bytes = policy.max_line_bytes as usize;
    let mut cmd_limit = policy.max_command_bytes as usize;
    if cmd_limit > command.len() {
        cmd_limit = command.len();
    }
    loop {
        let trimmed = truncate_to_bytes(command, cmd_limit);
        let entry = BreadcrumbEntry {
            schema: policy.schema.as_str(),
            event,
            command: trimmed.as_str(),
            status,
            exit_code,
        };
        let json = serde_json::to_string(&entry).context("serialize breadcrumb")?;
        if json.len() <= max_line_bytes {
            let mut bytes = json.into_bytes();
            bytes.push(b'\n');
            return Ok(bytes);
        }
        if cmd_limit == 0 {
            return Err(anyhow!(
                "breadcrumb line exceeds max_line_bytes {}",
                max_line_bytes
            ));
        }
        cmd_limit = cmd_limit.saturating_sub(1);
    }
}

fn truncate_to_bytes(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    let mut out = String::new();
    let mut count = 0usize;
    for ch in input.chars() {
        let len = ch.len_utf8();
        if count + len > max_bytes {
            break;
        }
        out.push(ch);
        count += len;
    }
    out
}
