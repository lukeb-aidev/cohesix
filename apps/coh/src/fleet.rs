// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide read-only multi-hive fleet fan-in helpers for coh.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Context, Result};
use cohesix_rest::GatewayClient;

/// One read-only fleet hive target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiveTarget {
    /// Human-readable hive identifier.
    pub name: String,
    /// Hive gateway base URL.
    pub rest_url: String,
}

/// Parse repeatable `name=url` fleet target specs.
pub fn parse_hive_targets(
    specs: &[String],
    local_rest_url: Option<&str>,
) -> Result<Vec<HiveTarget>> {
    let mut out = Vec::<HiveTarget>::new();
    for spec in specs {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (name, rest_url) = trimmed
            .split_once('=')
            .ok_or_else(|| anyhow!("fleet hive must use name=url syntax: {trimmed}"))?;
        let name = normalize_token("fleet hive name", name)?;
        let rest_url = normalize_rest_url(rest_url)?;
        if out.iter().any(|entry| entry.name == name) {
            return Err(anyhow!("duplicate fleet hive name '{name}'"));
        }
        out.push(HiveTarget { name, rest_url });
    }
    if out.is_empty() {
        if let Some(local) = local_rest_url {
            out.push(HiveTarget {
                name: "local".to_owned(),
                rest_url: normalize_rest_url(local)?,
            });
        }
    }
    if out.is_empty() {
        return Err(anyhow!(
            "fleet command requires --hive name=url (or --rest-url for a local hive)"
        ));
    }
    out.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(out)
}

/// Read-only fleet status fan-in (`/proc/lifecycle/state` + `/proc/root/reachable`).
pub fn fleet_status(targets: &[HiveTarget], request_auth_token: Option<&str>) -> Vec<String> {
    targets
        .iter()
        .map(|target| {
            let lifecycle =
                read_first_line(target, "/proc/lifecycle/state", 64, request_auth_token);
            let reachable = read_first_line(target, "/proc/root/reachable", 64, request_auth_token);
            format!(
                "hive={} lifecycle={} reachable={}",
                target.name,
                render_result(lifecycle),
                render_result(reachable)
            )
        })
        .collect()
}

/// Read-only fleet lease summary fan-in (`/proc/lease/summary`).
pub fn fleet_lease_summary(
    targets: &[HiveTarget],
    request_auth_token: Option<&str>,
) -> Vec<String> {
    targets
        .iter()
        .map(|target| {
            let lease_summary =
                read_first_line(target, "/proc/lease/summary", 160, request_auth_token);
            format!(
                "hive={} lease_summary={}",
                target.name,
                render_result(lease_summary)
            )
        })
        .collect()
}

/// Read-only fleet pressure fan-in (`/proc/pressure/*`).
pub fn fleet_pressure(targets: &[HiveTarget], request_auth_token: Option<&str>) -> Vec<String> {
    targets
        .iter()
        .map(|target| {
            let busy = read_first_line(target, "/proc/pressure/busy", 64, request_auth_token);
            let quota = read_first_line(target, "/proc/pressure/quota", 64, request_auth_token);
            let cut = read_first_line(target, "/proc/pressure/cut", 64, request_auth_token);
            let policy = read_first_line(target, "/proc/pressure/policy", 64, request_auth_token);
            format!(
                "hive={} busy={} quota={} cut={} policy={}",
                target.name,
                render_result(busy),
                render_result(quota),
                render_result(cut),
                render_result(policy)
            )
        })
        .collect()
}

fn read_first_line(
    target: &HiveTarget,
    path: &str,
    max_bytes: u32,
    request_auth_token: Option<&str>,
) -> Result<String> {
    let mut client = GatewayClient::new(target.rest_url.clone());
    if let Some(token) = request_auth_token {
        client = client.with_request_auth_token(token.to_owned());
    }
    let lines = client
        .read(path, max_bytes)
        .with_context(|| format!("hive {} read {}", target.name, path))?;
    Ok(lines
        .into_iter()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_else(|| "empty".to_owned()))
}

fn render_result(value: Result<String>) -> String {
    match value {
        Ok(line) => format!("\"{}\"", sanitize_line(line.as_str())),
        Err(err) => format!("\"err:{}\"", sanitize_line(err.to_string().as_str())),
    }
}

fn sanitize_line(line: &str) -> String {
    line.trim().replace('"', "'").replace(['\n', '\r'], " ")
}

fn normalize_token(label: &str, value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("{label} must not be empty"));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
    {
        return Err(anyhow!("{label} contains invalid characters"));
    }
    Ok(trimmed.to_owned())
}

fn normalize_rest_url(value: &str) -> Result<String> {
    let mut trimmed = value.trim().to_owned();
    if trimmed.is_empty() {
        return Err(anyhow!("fleet hive URL must not be empty"));
    }
    while trimmed.ends_with('/') {
        trimmed.pop();
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(anyhow!(
            "fleet hive URL must start with http:// or https://"
        ));
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hives_is_deterministic() {
        let specs = vec![
            "b=http://127.0.0.1:8081/".to_owned(),
            "a=http://127.0.0.1:8080".to_owned(),
        ];
        let parsed = parse_hive_targets(&specs, None)
            .unwrap_or_else(|err| unreachable!("parse hives: {err}"));
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "a");
        assert_eq!(parsed[1].name, "b");
        assert_eq!(parsed[1].rest_url, "http://127.0.0.1:8081");
    }

    #[test]
    fn parse_hives_accepts_local_fallback() {
        let parsed = parse_hive_targets(&[], Some("http://127.0.0.1:8080/"))
            .unwrap_or_else(|err| unreachable!("local hive: {err}"));
        assert_eq!(parsed[0].name, "local");
        assert_eq!(parsed[0].rest_url, "http://127.0.0.1:8080");
    }
}
