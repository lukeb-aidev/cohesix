// Author: Lukas Bower
// Purpose: Bind systemd lifecycle observations to Manager D-Bus jobs and immutable invocation identities.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

//! Native API adapters. D-Bus method completion is dispatch, not terminal proof.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Take};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MAX_NATIVE_BYTES: usize = 65536;
const MANAGER: &str = "/org/freedesktop/systemd1";
const DESTINATION: &str = "org.freedesktop.systemd1";

/// Exact native identity and bounded service state observed over D-Bus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemdObservation {
    /// Native unit name returned by systemd, checked against the requested name.
    pub unit: String,
    /// 128-bit systemd invocation identity; systemd returns an empty array when absent.
    pub invocation_id: Option<String>,
    /// Current ActiveState property.
    pub active_state: String,
    /// Current SubState property.
    pub sub_state: String,
    /// Pending native job id; zero means no job remains on the unit.
    pub job_id: u32,
    /// Service Result property; completion requires the expected result.
    pub service_result: String,
}

/// Reject shell, option, and path operands before a native API call.
pub fn validate_native_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        || value.contains("..")
    {
        bail!("EPERM invalid-native-id");
    }
    Ok(())
}

fn read_bounded(mut input: Take<impl Read>) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input
        .read_to_end(&mut bytes)
        .context("read native API response")?;
    if bytes.len() > MAX_NATIVE_BYTES {
        bail!("ELIMIT native-api-response");
    }
    Ok(bytes)
}

fn busctl(args: &[&str], deadline: Instant) -> Result<Vec<u8>> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|budget| !budget.is_zero())
        .ok_or_else(|| anyhow!("timeout systemd-dbus"))?;
    let timeout_arg = format!(
        "--timeout={}us",
        remaining.min(Duration::from_secs(5)).as_micros()
    );
    if !cfg!(target_os = "linux") {
        bail!("not_supported systemd-host");
    }
    let mut child = Command::new("/usr/bin/busctl")
        .args(["--system", "--json=short", &timeout_arg, "--no-pager"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("not_available systemd-dbus")?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            bail!("not_available systemd-dbus-pipe");
        }
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = std::thread::Builder::new()
        .name("systemd-api-response".into())
        .spawn(move || {
            let _ = sender.send(read_bounded(stdout.take((MAX_NATIVE_BYTES + 1) as u64)));
        });
    let reader = match reader {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error).context("not_available systemd-dbus-reader");
        }
    };
    let received = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    if received.as_ref().is_err() || received.as_ref().is_ok_and(|value| value.is_err()) {
        let _ = child.kill();
    }
    // busctl is an absolute-path, single-process native reader. Killing it
    // closes its only stdout owner; the bounded reader must finish before return.
    // EOF does not imply process exit. Share the call deadline with reaping.
    let status = loop {
        if let Some(status) = child.try_wait().context("observe systemd-dbus exit")? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break child.wait().context("reap expired systemd-dbus")?;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    reader
        .join()
        .map_err(|_| anyhow!("not_available systemd-dbus-reader"))?;
    let bytes = received.map_err(|_| anyhow!("timeout systemd-dbus"))??;
    if !status.success() {
        bail!("not_available systemd-dbus-call");
    }
    Ok(bytes)
}

fn native_object(unit: &str, deadline: Instant) -> Result<String> {
    validate_native_id(unit)?;
    let raw = busctl(
        &[
            "call",
            DESTINATION,
            MANAGER,
            "org.freedesktop.systemd1.Manager",
            "LoadUnit",
            "s",
            unit,
        ],
        deadline,
    )?;
    let value: Value = serde_json::from_slice(&raw)?;
    let object = value
        .get("data")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 1)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("invalid_observation systemd-object"))?;
    if value.get("type").and_then(Value::as_str) != Some("o")
        || !object.starts_with("/org/freedesktop/systemd1/unit/")
        || object.len() > 512
        || !object
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_".contains(&b))
    {
        bail!("invalid_observation systemd-object");
    }
    Ok(object.to_owned())
}

/// Observe a service through canonical typed properties, including InvocationID and Job.
pub fn observe_systemd(unit: &str) -> Result<SystemdObservation> {
    observe_systemd_before(unit, Instant::now() + Duration::from_secs(5))
}

/// Discover currently loaded selected service units directly from the Manager API.
/// The bounded Cohesix/SSH selector avoids treating target-seeded paths as native inventory.
pub fn discover_systemd_units() -> Result<Vec<String>> {
    discover_systemd_units_before(Instant::now() + Duration::from_secs(5))
}

/// Discover within a deadline shared with the complete native observation.
pub fn discover_systemd_units_before(deadline: Instant) -> Result<Vec<String>> {
    let raw = busctl(
        &[
            "call",
            DESTINATION,
            MANAGER,
            "org.freedesktop.systemd1.Manager",
            "ListUnitsByPatterns",
            "asas",
            "0",
            "3",
            "cohesix.service",
            "cohesix-*.service",
            "ssh.service",
        ],
        deadline,
    )?;
    let value: Value = serde_json::from_slice(&raw)?;
    if value["type"] != "a(ssssssouso)" {
        bail!("invalid_observation systemd unit-list signature");
    }
    let rows = value["data"]
        .as_array()
        .filter(|rows| rows.len() == 1)
        .and_then(|rows| rows[0].as_array())
        .filter(|rows| rows.len() <= 64)
        .ok_or_else(|| anyhow!("ELIMIT systemd unit-list"))?;
    let mut units = std::collections::BTreeSet::new();
    for row in rows {
        let row = row
            .as_array()
            .filter(|row| row.len() == 10)
            .ok_or_else(|| anyhow!("invalid_observation systemd unit row"))?;
        let unit = row[0]
            .as_str()
            .ok_or_else(|| anyhow!("invalid_observation systemd unit name"))?;
        validate_native_id(unit)?;
        if !units.insert(unit.to_owned()) {
            bail!("invalid_observation systemd duplicate unit");
        }
    }
    Ok(units.into_iter().collect())
}

/// Observe within the caller's whole-action deadline, shared by every native API call.
pub fn observe_systemd_before(unit: &str, deadline: Instant) -> Result<SystemdObservation> {
    let object = native_object(unit, deadline)?;
    let raw = busctl(
        &[
            "get-property",
            DESTINATION,
            &object,
            "org.freedesktop.systemd1.Unit",
            "Id",
            "ActiveState",
            "SubState",
            "InvocationID",
            "Job",
        ],
        deadline,
    )?;
    let result = busctl(
        &[
            "get-property",
            DESTINATION,
            &object,
            "org.freedesktop.systemd1.Service",
            "Result",
        ],
        deadline,
    )?;
    parse_systemd_observation(unit, &raw, &result)
}

fn string_property(value: &Value) -> Result<String> {
    let text = value
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("invalid_observation systemd-property"))?;
    if value.get("type").and_then(Value::as_str) != Some("s") {
        bail!("invalid_observation systemd-property-type");
    }
    validate_native_id(text)?;
    Ok(text.to_owned())
}

/// Parse the independently specified busctl typed-property representation.
pub fn parse_systemd_observation(
    unit: &str,
    raw: &[u8],
    result: &[u8],
) -> Result<SystemdObservation> {
    validate_native_id(unit)?;
    if raw.len() > MAX_NATIVE_BYTES || result.len() > 1024 {
        bail!("ELIMIT systemd-observation");
    }
    let rows: Vec<Value> = raw
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice)
        .collect::<std::result::Result<_, _>>()?;
    if rows.len() != 5 || string_property(&rows[0])? != unit {
        bail!("invalid_observation systemd-unit-identity");
    }
    let invocation = rows[3]
        .get("data")
        .and_then(Value::as_array)
        .filter(|values| values.is_empty() || values.len() == 16)
        .ok_or_else(|| anyhow!("invalid_observation systemd-invocation"))?;
    if rows[3].get("type").and_then(Value::as_str) != Some("ay")
        || rows[4].get("type").and_then(Value::as_str) != Some("(uo)")
    {
        bail!("invalid_observation systemd-property-type");
    }
    let encoded_invocation = invocation
        .iter()
        .map(|value| {
            value
                .as_u64()
                .and_then(|v| u8::try_from(v).ok())
                .map(|byte| format!("{byte:02x}"))
                .ok_or_else(|| anyhow!("invalid_observation systemd-invocation-byte"))
        })
        .collect::<Result<String>>()?;
    let invocation_id = (!encoded_invocation.is_empty()).then_some(encoded_invocation);
    let job = rows[4]
        .get("data")
        .and_then(Value::as_array)
        .filter(|values| values.len() == 2)
        .ok_or_else(|| anyhow!("invalid_observation systemd-job"))?;
    let job_id = job[0]
        .as_u64()
        .and_then(|id| u32::try_from(id).ok())
        .ok_or_else(|| anyhow!("invalid_observation systemd-job"))?;
    let result: Value = serde_json::from_slice(result)?;
    Ok(SystemdObservation {
        unit: unit.into(),
        invocation_id,
        active_state: string_property(&rows[1])?,
        sub_state: string_property(&rows[2])?,
        job_id,
        service_result: string_property(&result)?,
    })
}

/// Dispatch an admitted native lifecycle action. The returned job path is not a receipt.
pub fn dispatch_systemd(unit: &str, action: &str, deadline: Instant) -> Result<String> {
    validate_native_id(unit)?;
    let method = match action {
        "start" => "StartUnit",
        "stop" => "StopUnit",
        "restart" => "RestartUnit",
        _ => bail!("EPERM unsupported-systemd-action"),
    };
    let raw = busctl(
        &[
            "call",
            DESTINATION,
            MANAGER,
            "org.freedesktop.systemd1.Manager",
            method,
            "ss",
            unit,
            "replace",
        ],
        deadline,
    )?;
    let value: Value = serde_json::from_slice(&raw)?;
    let path = value
        .get("data")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 1)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("invalid_observation systemd-dispatched-job"))?;
    if value.get("type").and_then(Value::as_str) != Some("o")
        || path
            .strip_prefix("/org/freedesktop/systemd1/job/")
            .is_none_or(|id| id.parse::<u32>().is_err())
    {
        bail!("invalid_observation systemd-dispatched-job");
    }
    Ok(path.to_owned())
}

/// Check the native postcondition; stale invocation and pending jobs are non-terminal.
pub fn systemd_postcondition(
    action: &str,
    before: &SystemdObservation,
    after: &SystemdObservation,
) -> bool {
    if before.unit != after.unit || after.job_id != 0 || after.service_result != "success" {
        return false;
    }
    match action {
        "stop" => after.active_state == "inactive",
        "start" | "restart" => {
            after.active_state == "active"
                && after
                    .invocation_id
                    .as_deref()
                    .is_some_and(|id| id.bytes().any(|b| b != b'0'))
                && (action != "restart" || after.invocation_id != before.invocation_id)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation() -> SystemdObservation {
        let raw = br#"{"type":"s","data":"canary.service"}
{"type":"s","data":"active"}
{"type":"s","data":"running"}
{"type":"ay","data":[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]}
{"type":"(uo)","data":[0,"/"]}"#;
        parse_systemd_observation("canary.service", raw, br#"{"type":"s","data":"success"}"#)
            .expect("typed fixture")
    }

    #[test]
    fn typed_native_identity_and_pending_jobs_control_postconditions() {
        let before = observation();
        assert_eq!(
            before.invocation_id.as_deref(),
            Some("000102030405060708090a0b0c0d0e0f")
        );
        assert!(!systemd_postcondition("restart", &before, &before));
        let mut after = before.clone();
        after.invocation_id = Some("100102030405060708090a0b0c0d0e0f".into());
        assert!(systemd_postcondition("restart", &before, &after));
        after.job_id = 42;
        assert!(!systemd_postcondition("restart", &before, &after));
        after.job_id = 0;
        after.service_result = "timeout".into();
        assert!(!systemd_postcondition("restart", &before, &after));
    }

    #[test]
    fn native_shell_targets_and_wrong_property_types_are_rejected() {
        for target in ["", "-root", "../unit", "a/b", "a\nb", "a;echo"] {
            assert!(validate_native_id(target).is_err());
        }
        assert!(parse_systemd_observation("canary.service", b"{}", b"{}").is_err());
        assert!(read_bounded(
            std::io::Cursor::new(vec![0; MAX_NATIVE_BYTES + 1]).take((MAX_NATIVE_BYTES + 1) as u64)
        )
        .is_err());
    }

    #[test]
    fn absent_inactive_invocation_is_explicit_and_cannot_satisfy_start() {
        // systemd 255 publishes an empty ay after the unit leaves its invocation.
        // This is an absent identity, not a fabricated all-zero identifier.
        let raw = br#"{"type":"s","data":"canary.service"}
{"type":"s","data":"inactive"}
{"type":"s","data":"dead"}
{"type":"ay","data":[]}
{"type":"(uo)","data":[0,"/"]}"#;
        let stopped =
            parse_systemd_observation("canary.service", raw, br#"{"type":"s","data":"success"}"#)
                .expect("inactive native properties");
        assert_eq!(stopped.invocation_id, None);
        assert!(systemd_postcondition("stop", &observation(), &stopped));
        assert!(!systemd_postcondition("start", &observation(), &stopped));
        assert!(!systemd_postcondition("restart", &observation(), &stopped));
        let invalid = String::from_utf8(raw.to_vec())
            .expect("fixture UTF-8")
            .replace("\"data\":[]", "\"data\":[1,2]");
        assert!(parse_systemd_observation(
            "canary.service",
            invalid.as_bytes(),
            br#"{"type":"s","data":"success"}"#
        )
        .is_err());
    }
}
