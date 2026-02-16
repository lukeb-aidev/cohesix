// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide a small REST client for the Cohesix hive-gateway.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! REST client helpers for the Cohesix hive-gateway.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default REST timeout applied to hive-gateway requests.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// REST client for the hive-gateway API.
#[derive(Debug, Clone)]
pub struct GatewayClient {
    base_url: String,
    agent: ureq::Agent,
    request_auth_token: Option<String>,
}

impl GatewayClient {
    /// Construct a new gateway client with default timeouts.
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut base = base_url.into();
        while base.ends_with('/') {
            base.pop();
        }
        let agent = ureq::AgentBuilder::new()
            .timeout_read(DEFAULT_TIMEOUT)
            .timeout_write(DEFAULT_TIMEOUT)
            .build();
        Self {
            base_url: base,
            agent,
            request_auth_token: None,
        }
    }

    /// Configure a per-request auth token for mutating routes.
    pub fn with_request_auth_token(mut self, token: impl Into<String>) -> Self {
        let token = token.into();
        let trimmed = token.trim();
        if trimmed.is_empty() {
            self.request_auth_token = None;
        } else {
            self.request_auth_token = Some(trimmed.to_owned());
        }
        self
    }

    /// Return the configured base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Fetch manifest-derived bounds from the gateway.
    pub fn bounds(&self) -> Result<BoundsResponse> {
        let url = format!("{}/v1/meta/bounds", self.base_url);
        match self.with_auth(self.agent.get(&url)).call() {
            Ok(resp) => resp
                .into_json()
                .map_err(|err| anyhow!(err))
                .context("decode bounds response"),
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                if body.is_empty() {
                    Err(anyhow!("BOUNDS failed (http {code})"))
                } else {
                    Err(anyhow!("BOUNDS failed (http {code}): {body}"))
                }
            }
            Err(ureq::Error::Transport(err)) => Err(anyhow!(err).context("gateway transport error")),
        }
    }

    /// Issue an LS request via the gateway.
    pub fn list(&self, path: &str) -> Result<Vec<String>> {
        let path = urlencoding::encode(path);
        let url = format!("{}/v1/fs/ls?path={}", self.base_url, path);
        let response = self.with_auth(self.agent.get(&url)).call();
        let parsed = handle_response("LS", response)?;
        Ok(parsed.lines)
    }

    /// Issue a CAT request via the gateway.
    pub fn read(&self, path: &str, max_bytes: u32) -> Result<Vec<String>> {
        let path = urlencoding::encode(path);
        let url = format!(
            "{}/v1/fs/cat?path={}&max_bytes={}",
            self.base_url, path, max_bytes
        );
        let response = self.with_auth(self.agent.get(&url)).call();
        let parsed = handle_response("CAT", response)?;
        Ok(parsed.lines)
    }

    /// Issue a TAIL request via the gateway.
    pub fn tail(&self, path: &str, max_bytes: u32) -> Result<Vec<String>> {
        let path = urlencoding::encode(path);
        let url = format!(
            "{}/v1/fs/tail?path={}&max_bytes={}",
            self.base_url, path, max_bytes
        );
        let response = self.with_auth(self.agent.get(&url)).call();
        let parsed = handle_response("TAIL", response)?;
        Ok(parsed.lines)
    }

    /// Issue an ECHO request via the gateway.
    pub fn echo(&self, path: &str, line: &str) -> Result<usize> {
        let url = format!("{}/v1/fs/echo", self.base_url);
        let payload = EchoRequest {
            path: path.to_owned(),
            line: Some(line.to_owned()),
        };
        let response = self
            .with_auth(self.agent.post(&url))
            .send_json(payload);
        let parsed = handle_response("ECHO", response)?;
        Ok(parsed.bytes.unwrap_or(0))
    }

    fn with_auth(&self, request: ureq::Request) -> ureq::Request {
        let Some(token) = self.request_auth_token.as_deref() else {
            return request;
        };
        request
            .set("Authorization", format!("Bearer {token}").as_str())
            .set("x-cohesix-auth", token)
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GatewayResponse {
    status: String,
    verb: String,
    path: String,
    end: bool,
    #[serde(default)]
    lines: Vec<String>,
    #[serde(default)]
    bytes: Option<usize>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    manifest_sha256: Option<String>,
    #[serde(default)]
    secure9p: Option<Secure9pBounds>,
    #[serde(default)]
    console: Option<ConsoleBounds>,
    #[serde(default)]
    paths: Option<PathBounds>,
    #[serde(default)]
    control_plane: Option<ControlPlaneBounds>,
    #[serde(default)]
    policy: Option<PolicyBounds>,
    #[serde(default)]
    observability: Option<ObservabilityBounds>,
}

/// Manifest-derived bounds returned by the gateway.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoundsResponse {
    /// Manifest fingerprint from coh-rtc.
    pub manifest_sha256: String,
    /// Secure9P bounds.
    pub secure9p: Secure9pBounds,
    /// Console bounds.
    pub console: ConsoleBounds,
    /// Canonical paths.
    pub paths: PathBounds,
    /// Control plane bounds.
    pub control_plane: ControlPlaneBounds,
    /// Policy bounds.
    pub policy: PolicyBounds,
    /// Observability bounds.
    pub observability: ObservabilityBounds,
}

/// Secure9P protocol bounds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Secure9pBounds {
    /// Maximum 9P message size.
    pub msize: u32,
    /// Maximum walk depth.
    pub walk_depth: u8,
}

/// Console framing bounds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConsoleBounds {
    /// Maximum line length for console frames.
    pub max_line_len: usize,
    /// Maximum path length.
    pub max_path_len: usize,
    /// Maximum JSON length.
    pub max_json_len: usize,
    /// Maximum ID length.
    pub max_id_len: usize,
    /// Maximum echo payload length.
    pub max_echo_len: usize,
    /// Maximum ticket length.
    pub max_ticket_len: usize,
}

/// Canonical control and log paths.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PathBounds {
    /// Queen control path.
    pub queen_ctl: String,
    /// Queen lifecycle control path.
    pub queen_lifecycle_ctl: String,
    /// Queen schedule control path.
    pub queen_schedule_ctl: String,
    /// Queen lease control path.
    pub queen_lease_ctl: String,
    /// Queen export control path.
    pub queen_export_ctl: String,
    /// Policy control path.
    pub policy_ctl: String,
    /// Queen log path.
    pub log: String,
}

/// Control plane bounds for schedule/lease/export.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ControlPlaneBounds {
    /// Schedule control bounds.
    pub schedule: ScheduleBounds,
    /// Lease control bounds.
    pub lease: LeaseBounds,
    /// Export control bounds.
    pub export: ExportBounds,
}

/// Schedule queue bounds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScheduleBounds {
    /// Whether scheduling is enabled.
    pub enable: bool,
    /// Queue size bound.
    pub queue_max_entries: u32,
    /// Control log byte bound.
    pub ctl_max_bytes: u32,
}

/// Lease control bounds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LeaseBounds {
    /// Whether leasing is enabled.
    pub enable: bool,
    /// Active entries bound.
    pub active_max_entries: u32,
    /// Preemptions bound.
    pub preemptions_max_entries: u32,
    /// Control log byte bound.
    pub ctl_max_bytes: u32,
}

/// Export control bounds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExportBounds {
    /// Whether export control is enabled.
    pub enable: bool,
    /// Control log byte bound.
    pub ctl_max_bytes: u32,
}

/// Policy gate bounds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PolicyBounds {
    /// Whether policy gating is enabled.
    pub enable: bool,
    /// Queue size bound.
    pub queue_max_entries: u32,
    /// Queue byte bound.
    pub queue_max_bytes: u32,
    /// Control log byte bound.
    pub ctl_max_bytes: u32,
}

/// Observability bounds for /proc paths.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservabilityBounds {
    /// Schedule /proc bounds.
    pub proc_schedule: ProcScheduleBounds,
    /// Lease /proc bounds.
    pub proc_lease: ProcLeaseBounds,
}

/// /proc schedule bounds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcScheduleBounds {
    /// Whether /proc/schedule/summary is enabled.
    pub summary: bool,
    /// Whether /proc/schedule/queue is enabled.
    pub queue: bool,
    /// Summary byte bound.
    pub summary_bytes: u32,
    /// Queue byte bound.
    pub queue_bytes: u32,
}

/// /proc lease bounds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcLeaseBounds {
    /// Whether /proc/lease/summary is enabled.
    pub summary: bool,
    /// Whether /proc/lease/active is enabled.
    pub active: bool,
    /// Whether /proc/lease/preemptions is enabled.
    pub preemptions: bool,
    /// Summary byte bound.
    pub summary_bytes: u32,
    /// Active byte bound.
    pub active_bytes: u32,
    /// Preemptions byte bound.
    pub preemptions_bytes: u32,
}

#[derive(Debug, serde::Serialize)]
struct EchoRequest {
    path: String,
    line: Option<String>,
}

fn handle_response(
    verb: &str,
    response: Result<ureq::Response, ureq::Error>,
) -> Result<GatewayResponse> {
    match response {
        Ok(resp) => ensure_ok(verb, parse_response(resp)?),
        Err(ureq::Error::Status(code, resp)) => {
            let parsed = parse_response(resp).unwrap_or_else(|err| GatewayResponse {
                status: "ERR".to_owned(),
                verb: verb.to_owned(),
                path: String::new(),
                end: true,
                lines: Vec::new(),
                bytes: None,
                error: Some(format!("http {code}: {err}")),
                manifest_sha256: None,
                secure9p: None,
                console: None,
                paths: None,
                control_plane: None,
                policy: None,
                observability: None,
            });
            Err(anyhow!(parsed
                .error
                .unwrap_or_else(|| format!("{verb} failed (http {code})"))))
        }
        Err(ureq::Error::Transport(err)) => Err(anyhow!(err).context("gateway transport error")),
    }
}

fn parse_response(resp: ureq::Response) -> Result<GatewayResponse> {
    resp.into_json()
        .map_err(|err| anyhow!(err))
        .context("decode gateway response")
}

fn ensure_ok(verb: &str, response: GatewayResponse) -> Result<GatewayResponse> {
    if response.status.eq_ignore_ascii_case("OK") {
        return Ok(response);
    }
    let detail = response
        .error
        .unwrap_or_else(|| format!("{verb} failed"));
    Err(anyhow!(detail))
}

#[cfg(test)]
mod tests {
    use super::BoundsResponse;

    #[test]
    fn bounds_response_parses_without_status_wrapper() {
        let json = r#"{
            "manifest_sha256": "deadbeef",
            "secure9p": {"msize": 8192, "walk_depth": 8},
            "console": {
                "max_line_len": 256,
                "max_path_len": 96,
                "max_json_len": 192,
                "max_id_len": 32,
                "max_echo_len": 128,
                "max_ticket_len": 224
            },
            "paths": {
                "queen_ctl": "/queen/ctl",
                "queen_lifecycle_ctl": "/queen/lifecycle/ctl",
                "queen_schedule_ctl": "/queen/schedule/ctl",
                "queen_lease_ctl": "/queen/lease/ctl",
                "queen_export_ctl": "/queen/export/ctl",
                "policy_ctl": "/policy/ctl",
                "log": "/log/queen.log"
            },
            "control_plane": {
                "schedule": {"enable": true, "queue_max_entries": 64, "ctl_max_bytes": 8192},
                "lease": {"enable": true, "active_max_entries": 64, "preemptions_max_entries": 64, "ctl_max_bytes": 8192},
                "export": {"enable": true, "ctl_max_bytes": 2048}
            },
            "policy": {"enable": true, "queue_max_entries": 32, "queue_max_bytes": 4096, "ctl_max_bytes": 2048},
            "observability": {
                "proc_schedule": {"summary": true, "queue": true, "summary_bytes": 128, "queue_bytes": 256},
                "proc_lease": {
                    "summary": true,
                    "active": true,
                    "preemptions": true,
                    "summary_bytes": 160,
                    "active_bytes": 256,
                    "preemptions_bytes": 256
                }
            }
        }"#;
        let parsed: BoundsResponse = serde_json::from_str(json).expect("bounds json");
        assert_eq!(parsed.manifest_sha256, "deadbeef");
        assert_eq!(parsed.secure9p.msize, 8192);
        assert_eq!(parsed.observability.proc_schedule.queue_bytes, 256);
    }
}
