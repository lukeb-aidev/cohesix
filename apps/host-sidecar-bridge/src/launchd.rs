// Author: Lukas Bower
// Purpose: Correlate configured launchd services with kernel-observed process incarnations and bounded lifecycle postconditions.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{anyhow, bail, ensure, Context, Result};
use cohesix_authority::macos::LaunchdTarget;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Instant;

/// A libproc process incarnation; executable bytes are observed on its native path.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProcessIdentity {
    /// Versioned native helper response.
    pub schema: String,
    /// Native PID, independent of ticket labels.
    pub pid: u32,
    /// False only on a native ESRCH observation.
    pub alive: bool,
    /// Native effective uid while alive.
    pub uid: Option<u32>,
    /// Kernel process birth timestamp.
    pub start_seconds: Option<u64>,
    /// Subsecond component of process birth.
    pub start_microseconds: Option<u32>,
    /// SHA-256 of the bounded native executable file while the incarnation remains stable.
    pub executable_sha256: Option<String>,
}

/// Launchd state is correlated with a kernel process observation, not treated as execution proof alone.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Exact bootstrap domain and label.
    pub service: String,
    /// Selected plist identity.
    pub plist_sha256: String,
    /// Native service lifecycle summary.
    pub state: String,
    /// Launchd invocation count.
    pub runs: u64,
    /// Process identity when running.
    pub process: Option<ProcessIdentity>,
}

fn bytes(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && metadata.len() <= maximum as u64,
        "EPERM native-file-kind-or-bound"
    );
    let mut value = Vec::new();
    File::open(path)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut value)?;
    ensure!(value.len() <= maximum, "ELIMIT native-file");
    Ok(value)
}

/// The helper path and hash are deployment-owned; neither comes from a ticket.
pub fn process(pid: u32, deadline: Instant) -> Result<ProcessIdentity> {
    ensure!(cfg!(target_os = "macos"), "not_supported launchd-host");
    ensure!(pid > 0 && pid <= i32::MAX as u32, "EPERM native-pid");
    let helper = std::env::var("COHESIX_MACOS_PROCESS_HELPER")
        .context("not_enabled launchd-process-helper")?;
    let expected = std::env::var("COHESIX_MACOS_PROCESS_HELPER_SHA256")
        .context("not_enabled launchd-process-helper-identity")?;
    let helper = Path::new(&helper);
    ensure!(
        helper.is_absolute()
            && hex::encode(Sha256::digest(bytes(helper, 16 * 1024 * 1024)?)) == expected,
        "EPERM native-helper-identity"
    );
    let output = crate::observations::command(helper, &[&pid.to_string()], deadline)?;
    let result: ProcessIdentity = serde_json::from_slice(&output)?;
    ensure!(
        result.schema == "cohesix-macos-process/v1" && result.pid == pid,
        "EPERM native-process-response"
    );
    ensure!(
        if result.alive {
            result.uid.is_some()
                && result.start_seconds.is_some_and(|v| v > 0)
                && result.start_microseconds.is_some_and(|v| v < 1_000_000)
                && result.executable_sha256.as_ref().is_some_and(|v| {
                    v.len() == 64
                        && v.bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                })
        } else {
            result.uid.is_none()
                && result.start_seconds.is_none()
                && result.start_microseconds.is_none()
                && result.executable_sha256.is_none()
        },
        "EPERM incomplete-process-identity"
    );
    Ok(result)
}

/// A configured file's digest and label must match before even a read-only probe.
pub fn preflight(target: &LaunchdTarget, mutation: bool, deadline: Instant) -> Result<()> {
    ensure!(cfg!(target_os = "macos"), "not_supported launchd-host");
    cohesix_authority::macos::validate_launchd(std::slice::from_ref(target))
        .map_err(anyhow::Error::msg)?;
    ensure!(
        hex::encode(Sha256::digest(bytes(&target.plist, 65536)?)) == target.plist_sha256,
        "EPERM launchd-plist-identity"
    );
    let plist = target
        .plist
        .to_str()
        .ok_or_else(|| anyhow!("EPERM launchd-plist-path"))?;
    let converted = crate::observations::command(
        Path::new("/usr/bin/plutil"),
        &["-convert", "json", "-o", "-", plist],
        deadline,
    )?;
    let value: Value = serde_json::from_slice(&converted)?;
    ensure!(value["Label"] == target.label, "EPERM launchd-plist-label");
    let executable = value
        .get("Program")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("ProgramArguments")
                .and_then(Value::as_array)
                .and_then(|args| args.first())
                .and_then(Value::as_str)
        })
        .ok_or_else(|| anyhow!("EPERM launchd-executable-path"))?;
    ensure!(
        Path::new(executable).is_absolute()
            && hex::encode(Sha256::digest(bytes(
                Path::new(executable),
                256 * 1024 * 1024
            )?)) == target.executable_sha256,
        "EPERM launchd-executable-identity"
    );
    if mutation {
        // A keepalive job would immediately undo a stop; socket/Mach activation
        // would independently create a new invocation and obscure its owner.
        ensure!(
            !value
                .get("KeepAlive")
                .is_some_and(|v| v != &Value::Bool(false))
                && value.get("Sockets").is_none()
                && value.get("MachServices").is_none(),
            "not_supported independently-activated-launchd-control"
        );
    }
    Ok(())
}

fn parse_status(target: &LaunchdTarget, raw: &[u8]) -> Result<(String, u64, Option<u32>)> {
    let text = std::str::from_utf8(raw)?;
    ensure!(
        text.lines().next().is_some_and(
            |line| line.starts_with(&format!("{}/{} = {{", target.domain, target.label))
        ),
        "EPERM launchd-service-identity"
    );
    let mut values = BTreeMap::new();
    for line in text.lines() {
        if !line.starts_with('\t') || line.starts_with("\t\t") {
            continue;
        }
        if let Some((key, value)) = line.trim().split_once(" = ") {
            if ["state", "pid", "runs"].contains(&key) {
                ensure!(
                    values.insert(key, value).is_none(),
                    "EPERM duplicate-launchd-property"
                );
            }
        }
    }
    let state = values
        .get("state")
        .ok_or_else(|| anyhow!("EPERM missing-launchd-state"))?;
    ensure!(
        matches!(
            *state,
            "running" | "not running" | "waiting" | "spawn scheduled"
        ),
        "unavailable launchd-transient-state"
    );
    let runs = values
        .get("runs")
        .ok_or_else(|| anyhow!("EPERM missing-launchd-invocation"))?
        .parse()?;
    let pid = values.get("pid").map(|v| v.parse()).transpose()?;
    ensure!(
        (*state == "running") == pid.is_some(),
        "unavailable launchd-process-transition"
    );
    Ok(((*state).into(), runs, pid))
}

/// Read one exact service plus kernel process identity, with no argv/environment export.
pub fn observe(target: &LaunchdTarget, deadline: Instant) -> Result<Observation> {
    preflight(target, false, deadline)?;
    let service = format!("{}/{}", target.domain, target.label);
    let raw =
        crate::observations::command(Path::new("/bin/launchctl"), &["print", &service], deadline)?;
    let (state, runs, pid) = parse_status(target, &raw)?;
    let process = pid.map(|pid| process(pid, deadline)).transpose()?;
    if let Some(identity) = &process {
        ensure!(
            identity.alive
                && identity.executable_sha256.as_deref() == Some(&target.executable_sha256),
            "EPERM launchd-executable-identity"
        );
        if let Some((_, uid)) = target.domain.split_once('/') {
            ensure!(
                Some(uid.parse::<u32>()?) == identity.uid,
                "EPERM launchd-process-owner"
            );
        }
    }
    Ok(Observation {
        service,
        plist_sha256: target.plist_sha256.clone(),
        state,
        runs,
        process,
    })
}

/// Dispatch exactly one allowlisted lifecycle operation; the caller must subsequently observe it.
pub fn dispatch(target: &LaunchdTarget, action: &str, deadline: Instant) -> Result<()> {
    preflight(target, true, deadline)?;
    let service = format!("{}/{}", target.domain, target.label);
    let args = match action {
        "launchd.start" => vec!["kickstart", &service],
        "launchd.restart" => vec!["kickstart", "-k", &service],
        "launchd.stop" => vec!["kill", "SIGTERM", &service],
        _ => bail!("EPERM unsupported-launchd-action"),
    };
    crate::observations::command(Path::new("/bin/launchctl"), &args, deadline)?;
    Ok(())
}

/// Restart/start require a native incarnation and invocation; stop requires the old incarnation's absence.
pub fn postcondition(
    action: &str,
    before: &Observation,
    after: &Observation,
    old: Option<&ProcessIdentity>,
) -> bool {
    if before.service != after.service || before.plist_sha256 != after.plist_sha256 {
        return false;
    }
    match action {
        "launchd.status-check" => true,
        "launchd.start" if before.process.is_some() => {
            before.process == after.process && before.runs == after.runs
        }
        "launchd.start" | "launchd.restart" => {
            after.state == "running"
                && after.process.as_ref().is_some_and(|p| p.alive)
                && after.process != before.process
                && after.runs > before.runs
        }
        "launchd.stop" => {
            after.process.is_none()
                && before.runs == after.runs
                && match before.process.as_ref() {
                    None => true,
                    Some(prior) => old.is_some_and(|p| {
                        p.pid == prior.pid
                            && (!p.alive
                                || p.start_seconds != prior.start_seconds
                                || p.start_microseconds != prior.start_microseconds)
                    }),
                }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_incarnation_and_counter_are_required_for_lifecycle_proof() {
        let process = ProcessIdentity {
            schema: "cohesix-macos-process/v1".into(),
            pid: 42,
            alive: true,
            uid: Some(501),
            start_seconds: Some(100),
            start_microseconds: Some(1),
            executable_sha256: Some("a".repeat(64)),
        };
        let before = Observation {
            service: "gui/501/org.cohesix.owned".into(),
            plist_sha256: "b".repeat(64),
            state: "running".into(),
            runs: 1,
            process: Some(process.clone()),
        };
        assert!(postcondition("launchd.start", &before, &before, None));
        assert!(!postcondition("launchd.restart", &before, &before, None));
        let mut after = before.clone();
        after.runs = 2;
        assert!(!postcondition("launchd.restart", &before, &after, None));
        after.process.as_mut().unwrap().start_seconds = Some(101);
        assert!(postcondition("launchd.restart", &before, &after, None));
        after.process = None;
        after.state = "waiting".into();
        after.runs = 1;
        assert!(!postcondition(
            "launchd.stop",
            &before,
            &after,
            Some(&process)
        ));
        let gone = ProcessIdentity {
            alive: false,
            uid: None,
            start_seconds: None,
            start_microseconds: None,
            executable_sha256: None,
            ..process
        };
        assert!(postcondition("launchd.stop", &before, &after, Some(&gone)));
        after.service = "gui/501/other".into();
        assert!(!postcondition("launchd.stop", &before, &after, Some(&gone)));
    }
}
