// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide a small REST client for the Cohesix hive-gateway.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! REST client helpers for the Cohesix hive-gateway.

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default REST timeout applied to hive-gateway requests.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

type HttpResponse = ureq::http::Response<ureq::Body>;

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
        let config = ureq::Agent::config_builder()
            .timeout_send_request(Some(DEFAULT_TIMEOUT))
            .timeout_send_body(Some(DEFAULT_TIMEOUT))
            .timeout_recv_response(Some(DEFAULT_TIMEOUT))
            .timeout_recv_body(Some(DEFAULT_TIMEOUT))
            .http_status_as_error(false)
            .build();
        let agent = config.into();
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
        decode_json_response("BOUNDS", "bounds", self.get(&url))
    }

    /// Fetch gateway connection and broker backpressure status.
    pub fn status(&self) -> Result<GatewayStatusResponse> {
        let url = format!("{}/v1/meta/status", self.base_url);
        decode_json_response("STATUS", "status", self.get(&url))
    }

    /// Issue an LS request via the gateway.
    pub fn list(&self, path: &str) -> Result<Vec<String>> {
        let path = urlencoding::encode(path);
        let url = format!("{}/v1/fs/ls?path={}", self.base_url, path);
        let response = self.get(&url);
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
        let response = self.get(&url);
        let parsed = handle_response("CAT", response)?;
        Ok(parsed.lines)
    }

    /// Issue a TAIL request via the gateway.
    pub fn tail(&self, path: &str, max_bytes: u32) -> Result<Vec<String>> {
        self.tail_with_lines(path, max_bytes, None)
    }

    /// Issue a TAIL request via the gateway with an optional line limit.
    pub fn tail_with_lines(
        &self,
        path: &str,
        max_bytes: u32,
        lines: Option<u16>,
    ) -> Result<Vec<String>> {
        let path = urlencoding::encode(path);
        let mut url = format!(
            "{}/v1/fs/tail?path={}&max_bytes={}",
            self.base_url, path, max_bytes
        );
        if let Some(lines) = lines {
            url.push_str("&lines=");
            url.push_str(&lines.to_string());
        }
        let response = self.get(&url);
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
        let response = self.post_json(&url, &payload);
        let parsed = handle_response("ECHO", response)?;
        Ok(parsed.bytes.unwrap_or(0))
    }

    fn get(&self, url: &str) -> Result<HttpResponse, ureq::Error> {
        let request = self.agent.get(url);
        if let Some(token) = self.request_auth_token.as_deref() {
            request
                .header("Authorization", format!("Bearer {token}"))
                .header("x-cohesix-auth", token)
                .call()
        } else {
            request.call()
        }
    }

    fn post_json<T: Serialize>(&self, url: &str, payload: &T) -> Result<HttpResponse, ureq::Error> {
        let request = self.agent.post(url);
        if let Some(token) = self.request_auth_token.as_deref() {
            request
                .header("Authorization", format!("Bearer {token}"))
                .header("x-cohesix-auth", token)
                .send_json(payload)
        } else {
            request.send_json(payload)
        }
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

/// Gateway connection and broker status returned by `/v1/meta/status`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayStatusResponse {
    /// True when the gateway currently has a console connection.
    pub connected: bool,
    /// Last connection or relay error, when available.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Last connection state change timestamp in Unix milliseconds.
    #[serde(default)]
    pub last_change_unix_ms: Option<u128>,
    /// Number of reconnects attempted by the gateway.
    pub reconnects: u64,
    /// Number of successful gateway console connections.
    pub connects: u64,
    /// Broker wait, retry, cache, and relay counters.
    pub broker: BrokerStatusResponse,
}

/// Gateway broker counters returned by `/v1/meta/status`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BrokerStatusResponse {
    /// Current waiters for control sessions.
    pub control_waiters: u64,
    /// Current waiters for telemetry sessions.
    pub telemetry_waiters: u64,
    /// High-water waiter count for control sessions.
    pub control_waiters_high_water: u64,
    /// High-water waiter count for telemetry sessions.
    pub telemetry_waiters_high_water: u64,
    /// Total control session checkouts.
    pub control_checkouts: u64,
    /// Total telemetry session checkouts.
    pub telemetry_checkouts: u64,
    /// Total pool exhaustion events.
    pub pool_exhausted: u64,
    /// Total checkout retries.
    pub checkout_retries: u64,
    /// Total timeout rejections.
    pub timeout_rejections: u64,
    /// Total telemetry yield events.
    pub telemetry_yields: u64,
    /// Total `/proc` cache hits.
    pub proc_cache_hits: u64,
    /// Total `/proc` cache misses.
    pub proc_cache_misses: u64,
    /// Total `/proc` cache evictions.
    pub proc_cache_evictions: u64,
    /// Total retryable control-write errors.
    pub control_write_retryable_errors: u64,
    /// Total control-write retry attempts.
    pub control_write_retries: u64,
    /// Total milliseconds slept for control-write retries.
    pub control_write_retry_sleep_ms: u64,
    /// Total exhausted control-write retry windows.
    pub control_write_retry_exhaustions: u64,
    /// Total control writes that succeeded after retrying.
    pub control_write_success_after_retry: u64,
    /// Current host relay queue depth.
    pub relay_queue_depth: u64,
    /// Total deduped relay updates.
    pub relay_deduped: u64,
    /// Total remote relay write failures.
    pub relay_remote_write_failures: u64,
}

#[derive(Debug, serde::Serialize)]
struct EchoRequest {
    path: String,
    line: Option<String>,
}

fn handle_response(
    verb: &str,
    response: Result<HttpResponse, ureq::Error>,
) -> Result<GatewayResponse> {
    match response {
        Ok(resp) => {
            let code = resp.status().as_u16();
            let parsed = parse_response(resp);
            if code >= 400 {
                let parsed = parsed.unwrap_or_else(|err| GatewayResponse {
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
                return Err(anyhow!(parsed
                    .error
                    .unwrap_or_else(|| format!("{verb} failed (http {code})"))));
            }
            ensure_ok(verb, parsed?)
        }
        Err(err) => Err(anyhow!(err).context("gateway transport error")),
    }
}

fn decode_json_response<T: DeserializeOwned>(
    verb: &str,
    response_name: &str,
    response: Result<HttpResponse, ureq::Error>,
) -> Result<T> {
    match response {
        Ok(mut resp) => {
            let code = resp.status().as_u16();
            if code >= 400 {
                let body = resp
                    .body_mut()
                    .read_to_string()
                    .map_err(|err| anyhow!(err))
                    .with_context(|| format!("read {response_name} error response"))?;
                if body.is_empty() {
                    return Err(anyhow!("{verb} failed (http {code})"));
                }
                return Err(anyhow!("{verb} failed (http {code}): {body}"));
            }
            resp.body_mut()
                .read_json()
                .map_err(|err| anyhow!(err))
                .with_context(|| format!("decode {response_name} response"))
        }
        Err(err) => Err(anyhow!(err).context("gateway transport error")),
    }
}

fn parse_response(mut resp: HttpResponse) -> Result<GatewayResponse> {
    resp.body_mut()
        .read_json()
        .map_err(|err| anyhow!(err))
        .context("decode gateway response")
}

fn ensure_ok(verb: &str, response: GatewayResponse) -> Result<GatewayResponse> {
    if response.status.eq_ignore_ascii_case("OK") {
        return Ok(response);
    }
    let detail = response.error.unwrap_or_else(|| format!("{verb} failed"));
    Err(anyhow!(detail))
}

#[cfg(test)]
mod tests {
    use super::{BoundsResponse, GatewayClient, GatewayStatusResponse};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    fn serve_once(status: &str, response_body: &str) -> (String, Receiver<String>, JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback test server");
        let address = listener.local_addr().expect("read loopback server address");
        let status = status.to_owned();
        let response_body = response_body.to_owned();
        let (request_tx, request_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept loopback request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set loopback request timeout");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            let header_end = loop {
                let count = stream.read(&mut buffer).expect("read request");
                assert!(count != 0, "request ended before the HTTP headers");
                request.extend_from_slice(&buffer[..count]);
                if let Some(offset) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    break offset + 4;
                }
            };
            let headers =
                std::str::from_utf8(&request[..header_end]).expect("request headers are UTF-8");
            let content_length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .map(|(_, value)| {
                    value
                        .trim()
                        .parse::<usize>()
                        .expect("valid request content length")
                })
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let count = stream.read(&mut buffer).expect("read request body");
                assert!(count != 0, "request ended before the declared body");
                request.extend_from_slice(&buffer[..count]);
            }
            request_tx
                .send(String::from_utf8(request).expect("request is UTF-8"))
                .expect("publish captured request");

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len(),
            );
            stream
                .write_all(response.as_bytes())
                .expect("write loopback response");
        });
        (format!("http://{address}"), request_rx, server)
    }

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
                "max_echo_len": 224,
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

    #[test]
    fn gateway_status_response_parses_broker_counters() {
        let json = r#"{
            "connected": true,
            "last_change_unix_ms": 1782846123456,
            "reconnects": 2,
            "connects": 3,
            "broker": {
                "control_waiters": 1,
                "telemetry_waiters": 57,
                "control_waiters_high_water": 4,
                "telemetry_waiters_high_water": 64,
                "control_checkouts": 10,
                "telemetry_checkouts": 200,
                "pool_exhausted": 5,
                "checkout_retries": 120,
                "timeout_rejections": 3,
                "telemetry_yields": 480,
                "proc_cache_hits": 7,
                "proc_cache_misses": 11,
                "proc_cache_evictions": 2,
                "control_write_retryable_errors": 1080,
                "control_write_retries": 1080,
                "control_write_retry_sleep_ms": 27000,
                "control_write_retry_exhaustions": 1080,
                "control_write_success_after_retry": 0,
                "relay_queue_depth": 9,
                "relay_deduped": 12,
                "relay_remote_write_failures": 1
            }
        }"#;
        let parsed: GatewayStatusResponse = serde_json::from_str(json).expect("status json");
        assert!(parsed.connected);
        assert_eq!(parsed.reconnects, 2);
        assert_eq!(parsed.broker.telemetry_waiters, 57);
        assert_eq!(parsed.broker.control_write_retry_exhaustions, 1080);
        assert_eq!(parsed.broker.relay_queue_depth, 9);
    }

    #[test]
    fn ureq3_echo_sends_trimmed_auth_headers_and_json_body() {
        let (base_url, request_rx, server) = serve_once(
            "200 OK",
            r#"{"status":"OK","verb":"ECHO","path":"/queen/ctl","end":true,"bytes":2}"#,
        );
        let bytes = GatewayClient::new(base_url)
            .with_request_auth_token("  test-token  ")
            .echo("/queen/ctl", "go")
            .expect("authenticated echo succeeds");
        assert_eq!(bytes, 2);

        let request = request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("capture authenticated request");
        server.join().expect("loopback server exits");
        let lowercase = request.to_ascii_lowercase();
        assert!(lowercase.starts_with("post /v1/fs/echo http/1.1\r\n"));
        assert!(lowercase.contains("\r\nauthorization: bearer test-token\r\n"));
        assert!(lowercase.contains("\r\nx-cohesix-auth: test-token\r\n"));
        let (_, body) = request
            .split_once("\r\n\r\n")
            .expect("captured request contains a body");
        let body: serde_json::Value = serde_json::from_str(body).expect("valid request JSON");
        assert_eq!(body["path"], "/queen/ctl");
        assert_eq!(body["line"], "go");
    }

    #[test]
    fn ureq3_json_route_preserves_non_success_status_and_body() {
        let (base_url, request_rx, server) = serve_once("401 Unauthorized", "denied");
        let error = GatewayClient::new(base_url)
            .bounds()
            .expect_err("non-success response must fail");
        request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("capture bounds request");
        server.join().expect("loopback server exits");
        assert_eq!(
            error.to_string(),
            "BOUNDS failed (http 401): denied",
            "the upgraded client must retain the gateway error body",
        );
    }
}
