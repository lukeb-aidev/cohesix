// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide deterministic host environment checks for coh doctor.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::Role;
use cohsh_core::{normalize_ticket, role_label, TicketPolicy};
use gpu_bridge_host::auto_bridge;

use crate::mount;
use crate::policy::{load_policy, CohPolicy};
use crate::CohAudit;

/// Doctor configuration derived from CLI arguments.
#[derive(Debug, Clone)]
pub struct DoctorConfig {
    /// Role to validate for ticket checks.
    pub role: Role,
    /// Optional ticket payload to validate.
    pub ticket: Option<String>,
    /// Path to coh policy TOML.
    pub policy_path: PathBuf,
    /// Use mock mode (skip NVML + mount checks).
    pub mock: bool,
}

/// Run the doctor checks and emit audit lines.
pub fn run(config: DoctorConfig, audit: &mut CohAudit) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    let policy = match load_policy(&config.policy_path) {
        Ok(policy) => {
            let detail = format!(
                "check=policy path={} manifest_sha256={} policy_sha256={}",
                config.policy_path.display(),
                CohPolicy::manifest_hash(),
                CohPolicy::policy_hash()
            );
            audit.push_ack(cohsh_core::wire::AckStatus::Ok, "DOCTOR", Some(detail.as_str()));
            Some(policy)
        }
        Err(err) => {
            let detail = format!(
                "check=policy path={} reason={err}",
                config.policy_path.display()
            );
            audit.push_ack(cohsh_core::wire::AckStatus::Err, "DOCTOR", Some(detail.as_str()));
            errors.push(err.to_string());
            None
        }
    };

    check_ticket(config.role, config.ticket.as_deref(), audit, &mut errors);

    if config.mock {
        audit.push_ack(
            cohsh_core::wire::AckStatus::Ok,
            "DOCTOR",
            Some("check=mount status=skip reason=mock"),
        );
        audit.push_ack(
            cohsh_core::wire::AckStatus::Ok,
            "DOCTOR",
            Some("check=nvml status=skip reason=mock"),
        );
    } else {
        check_mount(policy.as_ref(), audit, &mut errors);
        check_nvml(audit, &mut errors);
    }

    check_runtime("python3", config.mock, audit, &mut errors);
    check_runtime("qemu-system-aarch64", config.mock, audit, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("doctor failed: {} check(s) failed", errors.len()))
    }
}

fn check_ticket(role: Role, ticket: Option<&str>, audit: &mut CohAudit, errors: &mut Vec<String>) {
    let label = role_label(role);
    match normalize_ticket(role, ticket, TicketPolicy::tcp()) {
        Ok(_) => {
            let detail = format!("check=ticket role={label}");
            audit.push_ack(cohsh_core::wire::AckStatus::Ok, "DOCTOR", Some(detail.as_str()));
        }
        Err(err) => {
            let detail = format!("check=ticket role={label} reason={err}");
            audit.push_ack(cohsh_core::wire::AckStatus::Err, "DOCTOR", Some(detail.as_str()));
            errors.push(err.to_string());
        }
    }
}

fn check_mount(policy: Option<&CohPolicy>, audit: &mut CohAudit, errors: &mut Vec<String>) {
    let policy = match policy {
        Some(policy) => policy,
        None => {
            audit.push_ack(
                cohsh_core::wire::AckStatus::Err,
                "DOCTOR",
                Some("check=mount reason=policy-unavailable"),
            );
            errors.push("policy unavailable".to_owned());
            return;
        }
    };
    if let Err(err) = mount::validate_mount(policy) {
        let detail = format!("check=mount reason={err}");
        audit.push_ack(cohsh_core::wire::AckStatus::Err, "DOCTOR", Some(detail.as_str()));
        errors.push(err.to_string());
        return;
    }
    if !cfg!(feature = "fuse") {
        let err = "fuse support disabled; rebuild coh with --features fuse or use --mock";
        let detail = format!("check=mount reason={err}");
        audit.push_ack(cohsh_core::wire::AckStatus::Err, "DOCTOR", Some(detail.as_str()));
        errors.push(err.to_owned());
        return;
    }
    if !fuse_device_present() {
        let err = "fuse device not detected";
        let detail = format!("check=mount reason={err}");
        audit.push_ack(cohsh_core::wire::AckStatus::Err, "DOCTOR", Some(detail.as_str()));
        errors.push(err.to_owned());
        return;
    }
    let detail = format!(
        "check=mount root={} allowlist={}",
        policy.mount.root,
        policy.mount.allowlist.len()
    );
    audit.push_ack(cohsh_core::wire::AckStatus::Ok, "DOCTOR", Some(detail.as_str()));
}

fn check_nvml(audit: &mut CohAudit, errors: &mut Vec<String>) {
    match auto_bridge(false)
        .and_then(|bridge| bridge.serialise_namespace())
        .map(|_| ())
    {
        Ok(()) => {
            audit.push_ack(
                cohsh_core::wire::AckStatus::Ok,
                "DOCTOR",
                Some("check=nvml"),
            );
        }
        Err(err) => {
            let detail = format!("check=nvml reason={err}");
            audit.push_ack(cohsh_core::wire::AckStatus::Err, "DOCTOR", Some(detail.as_str()));
            errors.push(err.to_string());
        }
    }
}

fn check_runtime(tool: &str, mock: bool, audit: &mut CohAudit, errors: &mut Vec<String>) {
    if mock && tool == "qemu-system-aarch64" {
        audit.push_ack(
            cohsh_core::wire::AckStatus::Ok,
            "DOCTOR",
            Some("check=runtime tool=qemu-system-aarch64 status=skip reason=mock"),
        );
        return;
    }
    match tool_version(tool) {
        Ok(version) => {
            let detail = format!("check=runtime tool={tool} version={version}");
            audit.push_ack(cohsh_core::wire::AckStatus::Ok, "DOCTOR", Some(detail.as_str()));
        }
        Err(err) => {
            let detail = format!("check=runtime tool={tool} reason={err}");
            audit.push_ack(cohsh_core::wire::AckStatus::Err, "DOCTOR", Some(detail.as_str()));
            errors.push(err.to_string());
        }
    }
}

fn tool_version(tool: &str) -> Result<String> {
    let output = Command::new(tool)
        .arg("--version")
        .output()
        .with_context(|| format!("invoke {tool}"))?;
    if !output.status.success() {
        return Err(anyhow!("{} exited with {}", tool, output.status));
    }
    let mut text = String::new();
    if !output.stdout.is_empty() {
        text.push_str(std::str::from_utf8(&output.stdout).unwrap_or(""));
    }
    if text.trim().is_empty() && !output.stderr.is_empty() {
        text.push_str(std::str::from_utf8(&output.stderr).unwrap_or(""));
    }
    let version = text
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    if version.is_empty() {
        Ok("unknown".to_owned())
    } else {
        Ok(version)
    }
}

fn fuse_device_present() -> bool {
    if cfg!(target_os = "linux") {
        return Path::new("/dev/fuse").exists();
    }
    if cfg!(target_os = "macos") {
        return Path::new("/dev/osxfuse").exists() || Path::new("/dev/fuse").exists();
    }
    false
}
