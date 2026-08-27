// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Implement the REST transport backend for the Cohesix shell.
// Author: Lukas Bower
//! REST transport backend for the Cohesix shell.

use std::collections::VecDeque;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use cohesix_rest::{BoundsResponse, GatewayClient, GatewayStatusResponse};
use cohesix_ticket::Role;
use cohsh_core::wire::{render_ack, AckLine, AckStatus};
use cohsh_core::{normalize_ticket, role_label, ConsoleVerb, TicketPolicy};
use secure9p_codec::SessionId;

use crate::{CohshPolicy, Session, Transport, TransportMetrics};

const DEFAULT_SESSION_ID: SessionId = SessionId::BOOTSTRAP;

/// REST transport backed by the hive-gateway API.
#[derive(Debug)]
pub struct RestTransport {
    client: GatewayClient,
    ack_lines: VecDeque<String>,
    attached: bool,
    max_read_bytes: u32,
    max_tail_bytes: u32,
    bounds: Option<BoundsResponse>,
}

impl RestTransport {
    /// Create a new REST transport for the supplied gateway base URL.
    pub fn new(base_url: impl Into<String>, request_auth_token: Option<String>) -> Self {
        let policy = CohshPolicy::from_generated();
        let mut client = GatewayClient::new(base_url);
        if let Some(token) = request_auth_token {
            client = client.with_request_auth_token(token);
        }
        Self {
            client,
            ack_lines: VecDeque::new(),
            attached: false,
            max_read_bytes: policy.trace.max_bytes,
            max_tail_bytes: policy.trace.max_bytes,
            bounds: None,
        }
    }

    /// Override the maximum bytes requested per read.
    pub fn with_max_read_bytes(mut self, max_bytes: u32) -> Self {
        self.max_read_bytes = max_bytes.max(1);
        self
    }

    /// Override the maximum bytes requested per tail.
    pub fn with_max_tail_bytes(mut self, max_bytes: u32) -> Self {
        self.max_tail_bytes = max_bytes.max(1);
        self
    }

    /// Override the bounded response envelope for gateway file operations.
    pub fn with_operation_response_timeout(mut self, timeout: Duration) -> Result<Self> {
        self.client.set_operation_response_timeout(timeout)?;
        Ok(self)
    }

    /// Return the configured gateway file-operation response envelope.
    pub fn operation_response_timeout(&self) -> Duration {
        self.client.operation_response_timeout()
    }

    /// Fetch gateway connection and broker backpressure status.
    pub fn gateway_status(&self) -> Result<GatewayStatusResponse> {
        self.client.status()
    }

    fn push_ack(&mut self, status: AckStatus, verb: &str, detail: Option<&str>) {
        let mut line = String::new();
        let ack = AckLine {
            status,
            verb,
            detail,
        };
        if render_ack(&mut line, &ack).is_ok() {
            self.ack_lines.push_back(line);
        }
    }

    fn ensure_attached(&self) -> Result<()> {
        if self.attached {
            Ok(())
        } else {
            Err(anyhow!("not attached"))
        }
    }

    fn build_echo_payload(payload: &[u8]) -> Result<String> {
        let payload_str = std::str::from_utf8(payload).context("payload must be UTF-8")?;
        let trimmed = payload_str.strip_suffix('\n').unwrap_or(payload_str);
        if trimmed.contains('\n') || trimmed.contains('\r') {
            return Err(anyhow!("echo payload must be a single line"));
        }
        Ok(trimmed.to_owned())
    }

    fn ensure_bounds(&mut self) {
        if self.bounds.is_none() {
            if let Ok(bounds) = self.client.bounds() {
                self.bounds = Some(bounds);
            }
        }
    }

    fn bound_for_path(path: &str, bounds: &BoundsResponse) -> Option<u32> {
        if path.starts_with("/proc/lease/by-id/") {
            return Some(bounds.observability.proc_lease.active_bytes);
        }
        match path {
            "/proc/schedule/summary" => Some(bounds.observability.proc_schedule.summary_bytes),
            "/proc/schedule/queue" => Some(bounds.observability.proc_schedule.queue_bytes),
            "/proc/lease/summary" => Some(bounds.observability.proc_lease.summary_bytes),
            "/proc/lease/active" => Some(bounds.observability.proc_lease.active_bytes),
            "/proc/lease/preemptions" => Some(bounds.observability.proc_lease.preemptions_bytes),
            _ => {
                if path == bounds.paths.queen_schedule_ctl {
                    Some(bounds.control_plane.schedule.ctl_max_bytes)
                } else if path == bounds.paths.queen_lease_ctl {
                    Some(bounds.control_plane.lease.ctl_max_bytes)
                } else if path == bounds.paths.queen_export_ctl {
                    Some(bounds.control_plane.export.ctl_max_bytes)
                } else if path == bounds.paths.policy_ctl {
                    Some(bounds.policy.ctl_max_bytes)
                } else {
                    None
                }
            }
        }
    }

    fn read_max_bytes(&mut self, path: &str) -> u32 {
        self.ensure_bounds();
        if let Some(bounds) = self.bounds.as_ref() {
            if let Some(bound) = Self::bound_for_path(path, bounds) {
                return self.max_read_bytes.min(bound.max(1));
            }
        }
        self.max_read_bytes
    }

    fn tail_max_bytes(&mut self, path: &str) -> u32 {
        self.ensure_bounds();
        if let Some(bounds) = self.bounds.as_ref() {
            if let Some(bound) = Self::bound_for_path(path, bounds) {
                return self.max_tail_bytes.min(bound.max(1));
            }
        }
        self.max_tail_bytes
    }

    fn render_read_ack_detail(&mut self, path: &str, lines: &[String]) -> String {
        // Match the TCP transport's ergonomics: include a small inline payload preview for
        // single-line reads so `.coh` scripts can assert on the response line deterministically.
        let mut detail = format!("path={path}");

        let preview = match lines {
            [line] => line.as_str(),
            _ => return detail,
        };
        if preview.is_empty() {
            return detail;
        }

        self.ensure_bounds();
        let max_len = self
            .bounds
            .as_ref()
            .map(|bounds| bounds.console.max_line_len)
            .unwrap_or(cohsh_core::MAX_LINE_LEN);
        if max_len == 0 {
            return detail;
        }

        // Conservatively cap `data=` so the full acknowledgement line stays within bounds.
        // `render_ack` prefixes `OK CAT` plus separators, so leave headroom.
        let headroom = 64usize;
        let available = max_len
            .saturating_sub(detail.len())
            .saturating_sub(headroom);
        if available == 0 {
            return detail;
        }

        let clipped = if preview.len() <= available {
            preview
        } else {
            // Avoid slicing on non-UTF-8 boundaries.
            let mut end = available;
            while end > 0 && preview.get(..end).is_none() {
                end = end.saturating_sub(1);
            }
            match preview.get(..end) {
                Some(value) if !value.is_empty() => value,
                _ => return detail,
            }
        };
        detail.push_str(" data=");
        detail.push_str(clipped);
        detail
    }
}

impl Transport for RestTransport {
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        if role != Role::Queen {
            let detail = format!("role {} not supported", role_label(role));
            self.push_ack(
                AckStatus::Err,
                ConsoleVerb::Attach.ack_label(),
                Some(detail.as_str()),
            );
            return Err(anyhow!("rest transport supports queen role only"));
        }
        if let Some(ticket) = ticket {
            let _ = normalize_ticket(role, Some(ticket), TicketPolicy::tcp())
                .map_err(|err| anyhow!("ticket invalid for rest transport: {err}"))?;
        }
        self.attached = true;
        let session = Session::new(DEFAULT_SESSION_ID, role);
        let detail = format!("role={}", role_label(role));
        self.push_ack(
            AckStatus::Ok,
            ConsoleVerb::Attach.ack_label(),
            Some(detail.as_str()),
        );
        Ok(session)
    }

    fn kind(&self) -> &'static str {
        "rest"
    }

    fn ping(&mut self, _session: &Session) -> Result<String> {
        self.ensure_attached()?;
        let status = self.client.status().context("rest ping status")?;
        if !status.connected {
            return Err(anyhow!("gateway backend is not connected"));
        }
        let bounds = self.client.bounds().context("rest ping bounds")?;
        self.bounds = Some(bounds.clone());
        self.push_ack(AckStatus::Ok, ConsoleVerb::Ping.ack_label(), None);
        Ok(format!("gateway ok manifest={}", bounds.manifest_sha256))
    }

    fn tail(&mut self, _session: &Session, path: &str, lines: Option<u16>) -> Result<Vec<String>> {
        self.ensure_attached()?;
        let line_limit = crate::ensure_valid_tail_lines(lines)?;
        let max_bytes = self.tail_max_bytes(path);
        match self.client.tail_with_lines(path, max_bytes, line_limit) {
            Ok(lines) => {
                let detail = format!("path={path}");
                self.push_ack(
                    AckStatus::Ok,
                    ConsoleVerb::Tail.ack_label(),
                    Some(detail.as_str()),
                );
                Ok(crate::apply_tail_line_limit(lines, line_limit))
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(
                    AckStatus::Err,
                    ConsoleVerb::Tail.ack_label(),
                    Some(detail.as_str()),
                );
                Err(err)
            }
        }
    }

    fn read(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        self.ensure_attached()?;
        let max_bytes = self.read_max_bytes(path);
        match self.client.read(path, max_bytes) {
            Ok(lines) => {
                let detail = self.render_read_ack_detail(path, &lines);
                self.push_ack(AckStatus::Ok, ConsoleVerb::Cat.ack_label(), Some(&detail));
                Ok(lines)
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(
                    AckStatus::Err,
                    ConsoleVerb::Cat.ack_label(),
                    Some(detail.as_str()),
                );
                Err(err)
            }
        }
    }

    fn list(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        self.ensure_attached()?;
        match self.client.list(path) {
            Ok(lines) => {
                let detail = format!("path={path}");
                self.push_ack(
                    AckStatus::Ok,
                    ConsoleVerb::Ls.ack_label(),
                    Some(detail.as_str()),
                );
                Ok(lines)
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(
                    AckStatus::Err,
                    ConsoleVerb::Ls.ack_label(),
                    Some(detail.as_str()),
                );
                Err(err)
            }
        }
    }

    fn write(&mut self, _session: &Session, path: &str, payload: &[u8]) -> Result<()> {
        self.ensure_attached()?;
        let trimmed = Self::build_echo_payload(payload)?;
        match self.client.echo(path, trimmed.as_str()) {
            Ok(_) => {
                let detail = if trimmed.is_empty() {
                    format!("path={path}")
                } else {
                    format!("path={path} bytes={}", trimmed.len())
                };
                self.push_ack(
                    AckStatus::Ok,
                    ConsoleVerb::Echo.ack_label(),
                    Some(detail.as_str()),
                );
                Ok(())
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(
                    AckStatus::Err,
                    ConsoleVerb::Echo.ack_label(),
                    Some(detail.as_str()),
                );
                Err(err)
            }
        }
    }

    fn write_batch(
        &mut self,
        _session: &Session,
        path: &str,
        payloads: &[Vec<u8>],
    ) -> Result<usize> {
        self.ensure_attached()?;
        let lines = payloads
            .iter()
            .map(|payload| Self::build_echo_payload(payload.as_slice()))
            .collect::<Result<Vec<_>>>()?;
        match self.client.echo_batch(path, lines.as_slice()) {
            Ok(written) => {
                for line in &lines {
                    let detail = if line.is_empty() {
                        format!("path={path}")
                    } else {
                        format!("path={path} bytes={}", line.len())
                    };
                    self.push_ack(
                        AckStatus::Ok,
                        ConsoleVerb::Echo.ack_label(),
                        Some(detail.as_str()),
                    );
                }
                Ok(written)
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(
                    AckStatus::Err,
                    ConsoleVerb::Echo.ack_label(),
                    Some(detail.as_str()),
                );
                Err(err)
            }
        }
    }

    fn drain_acknowledgements(&mut self) -> Vec<String> {
        self.ack_lines.drain(..).collect()
    }

    fn metrics(&self) -> TransportMetrics {
        TransportMetrics::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cohesix_rest::{
        BrokerStatusResponse, ConsoleBounds, ControlPlaneBounds, ExportBounds, LeaseBounds,
        ObservabilityBounds, PathBounds, PolicyBounds, ProcLeaseBounds, ProcScheduleBounds,
        ScheduleBounds, Secure9pBounds,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    fn serve_json(responses: Vec<String>) -> (String, Receiver<Vec<String>>, JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback test server");
        let address = listener.local_addr().expect("read loopback server address");
        let (requests_tx, requests_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut requests = Vec::with_capacity(responses.len());
            for response_body in responses {
                let (mut stream, _) = listener.accept().expect("accept loopback request");
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("set loopback request timeout");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let count = stream.read(&mut buffer).expect("read request");
                    assert!(count != 0, "request ended before the HTTP headers");
                    request.extend_from_slice(&buffer[..count]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                requests.push(String::from_utf8(request).expect("request is UTF-8"));

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                    response_body.len(),
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write loopback response");
            }
            requests_tx
                .send(requests)
                .expect("publish captured requests");
        });
        (format!("http://{address}"), requests_rx, server)
    }

    fn sample_status(connected: bool) -> GatewayStatusResponse {
        GatewayStatusResponse {
            connected,
            target_host: "127.0.0.1".to_string(),
            target_port: 31337,
            backend_class: None,
            worker_acceptance: None,
            worker_acceptance_diagnostic: None,
            last_error: None,
            last_change_unix_ms: None,
            reconnects: 0,
            connects: u64::from(connected),
            broker: BrokerStatusResponse {
                control_waiters: 0,
                telemetry_waiters: 0,
                control_waiters_high_water: 0,
                telemetry_waiters_high_water: 0,
                control_checkouts: 0,
                telemetry_checkouts: 0,
                pool_exhausted: 0,
                checkout_retries: 0,
                timeout_rejections: 0,
                telemetry_yields: 0,
                proc_cache_hits: 0,
                proc_cache_misses: 0,
                proc_cache_evictions: 0,
                control_write_retryable_errors: 0,
                control_write_retries: 0,
                control_write_retry_sleep_ms: 0,
                control_write_retry_exhaustions: 0,
                control_write_success_after_retry: 0,
                relay_queue_depth: 0,
                relay_deduped: 0,
                relay_remote_write_failures: 0,
            },
        }
    }

    fn sample_bounds() -> BoundsResponse {
        BoundsResponse {
            manifest_sha256: "demo".to_owned(),
            secure9p: Secure9pBounds {
                msize: 8192,
                walk_depth: 8,
            },
            console: ConsoleBounds {
                max_line_len: 256,
                max_path_len: 256,
                max_json_len: 1024,
                max_id_len: 64,
                max_echo_len: 256,
                max_ticket_len: 512,
            },
            paths: PathBounds {
                queen_ctl: "/queen/ctl".to_owned(),
                queen_lifecycle_ctl: "/queen/lifecycle/ctl".to_owned(),
                queen_schedule_ctl: "/queen/schedule/ctl".to_owned(),
                queen_lease_ctl: "/queen/lease/ctl".to_owned(),
                queen_export_ctl: "/queen/export/ctl".to_owned(),
                policy_ctl: "/policy/ctl".to_owned(),
                log: "/log/queen.log".to_owned(),
            },
            control_plane: ControlPlaneBounds {
                schedule: ScheduleBounds {
                    enable: true,
                    queue_max_entries: 32,
                    ctl_max_bytes: 120,
                },
                lease: LeaseBounds {
                    enable: true,
                    active_max_entries: 16,
                    preemptions_max_entries: 8,
                    ctl_max_bytes: 96,
                },
                export: ExportBounds {
                    enable: true,
                    ctl_max_bytes: 80,
                },
            },
            policy: PolicyBounds {
                enable: true,
                queue_max_entries: 8,
                queue_max_bytes: 256,
                ctl_max_bytes: 64,
            },
            observability: ObservabilityBounds {
                proc_schedule: ProcScheduleBounds {
                    summary: true,
                    queue: true,
                    summary_bytes: 160,
                    queue_bytes: 512,
                },
                proc_lease: ProcLeaseBounds {
                    summary: true,
                    active: true,
                    preemptions: true,
                    summary_bytes: 128,
                    active_bytes: 256,
                    preemptions_bytes: 192,
                },
            },
            worker_runtime: None,
        }
    }

    #[test]
    fn bound_for_proc_paths() {
        let bounds = sample_bounds();
        assert_eq!(
            RestTransport::bound_for_path("/proc/schedule/summary", &bounds),
            Some(160)
        );
        assert_eq!(
            RestTransport::bound_for_path("/proc/schedule/queue", &bounds),
            Some(512)
        );
        assert_eq!(
            RestTransport::bound_for_path("/proc/lease/summary", &bounds),
            Some(128)
        );
        assert_eq!(
            RestTransport::bound_for_path("/proc/lease/active", &bounds),
            Some(256)
        );
        assert_eq!(
            RestTransport::bound_for_path("/proc/lease/by-id/lease-1", &bounds),
            Some(256)
        );
        assert_eq!(
            RestTransport::bound_for_path("/proc/lease/preemptions", &bounds),
            Some(192)
        );
    }

    #[test]
    fn bound_for_ctl_paths() {
        let bounds = sample_bounds();
        assert_eq!(
            RestTransport::bound_for_path("/queen/schedule/ctl", &bounds),
            Some(120)
        );
        assert_eq!(
            RestTransport::bound_for_path("/queen/lease/ctl", &bounds),
            Some(96)
        );
        assert_eq!(
            RestTransport::bound_for_path("/queen/export/ctl", &bounds),
            Some(80)
        );
        assert_eq!(
            RestTransport::bound_for_path("/policy/ctl", &bounds),
            Some(64)
        );
    }

    #[test]
    fn read_max_bytes_clamps_to_proc_bound() {
        let mut transport = RestTransport::new("http://example", None)
            .with_max_read_bytes(2048)
            .with_max_tail_bytes(2048);
        transport.bounds = Some(sample_bounds());
        assert_eq!(transport.read_max_bytes("/proc/lease/summary"), 128);
        assert_eq!(transport.tail_max_bytes("/proc/schedule/queue"), 512);
    }

    #[test]
    fn bound_for_unknown_path_is_none() {
        let bounds = sample_bounds();
        assert_eq!(
            RestTransport::bound_for_path("/proc/root/reachable", &bounds),
            None
        );
    }

    #[test]
    fn operation_response_envelope_applies_to_primary_and_pooled_construction() {
        let selected = Duration::from_millis(190_000);
        let primary = RestTransport::new("http://127.0.0.1:8080", None)
            .with_operation_response_timeout(selected)
            .expect("configure primary REST transport");
        let pooled = RestTransport::new("http://127.0.0.1:8080", None)
            .with_operation_response_timeout(selected)
            .expect("configure pooled REST transport");

        assert_eq!(primary.operation_response_timeout(), selected);
        assert_eq!(pooled.operation_response_timeout(), selected);
    }

    #[test]
    fn write_batch_uses_one_rest_request_and_preserves_per_record_ack_order() {
        let response = r#"{"status":"OK","verb":"ECHO_BATCH","path":"/host/tickets/status","end":true,"bytes":6}"#.to_owned();
        let (base_url, requests_rx, server) = serve_json(vec![response]);
        let mut transport = RestTransport::new(base_url, Some("test-token".to_owned()));
        let session = transport
            .attach(Role::Queen, None)
            .expect("attach local REST session");
        let _ = transport.drain_acknowledgements();

        let payloads = vec![b"one\n".to_vec(), b"two\n".to_vec()];
        assert_eq!(
            transport
                .write_batch(&session, "/host/tickets/status", payloads.as_slice())
                .expect("write bounded REST batch"),
            2
        );
        assert_eq!(
            transport.drain_acknowledgements(),
            [
                "OK ECHO path=/host/tickets/status bytes=3",
                "OK ECHO path=/host/tickets/status bytes=3",
            ]
        );

        server.join().expect("join loopback test server");
        let requests = requests_rx.recv().expect("receive captured requests");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("POST /v1/fs/echo-batch HTTP/1.1"));
    }

    #[test]
    fn ping_rejects_disconnected_status_without_fetching_bounds() {
        let status = serde_json::to_string(&sample_status(false)).expect("serialize status");
        let (base_url, requests_rx, server) = serve_json(vec![status]);
        let mut transport = RestTransport::new(base_url, None);
        let session = transport
            .attach(Role::Queen, None)
            .expect("attach local REST session");

        let err = transport
            .ping(&session)
            .expect_err("disconnected gateway status must reject ping");

        assert_eq!(err.to_string(), "gateway backend is not connected");
        assert!(transport.bounds.is_none());
        server.join().expect("join loopback test server");
        let requests = requests_rx.recv().expect("receive captured requests");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /v1/meta/status HTTP/1.1"));
    }

    #[test]
    fn ping_fetches_bounds_only_after_connected_status() {
        let status = serde_json::to_string(&sample_status(true)).expect("serialize status");
        let bounds = serde_json::to_string(&sample_bounds()).expect("serialize bounds");
        let (base_url, requests_rx, server) = serve_json(vec![status, bounds]);
        let mut transport = RestTransport::new(base_url, None);
        let session = transport
            .attach(Role::Queen, None)
            .expect("attach local REST session");

        let response = transport
            .ping(&session)
            .expect("connected gateway status must permit bounds ping");

        assert_eq!(response, "gateway ok manifest=demo");
        assert_eq!(
            transport
                .bounds
                .as_ref()
                .map(|bounds| bounds.manifest_sha256.as_str()),
            Some("demo")
        );
        server.join().expect("join loopback test server");
        let requests = requests_rx.recv().expect("receive captured requests");
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /v1/meta/status HTTP/1.1"));
        assert!(requests[1].starts_with("GET /v1/meta/bounds HTTP/1.1"));
    }
}
