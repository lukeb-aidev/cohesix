// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide REST-backed CohAccess helpers for coh.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Context, Result};
use cohesix_rest::{BoundsResponse, GatewayClient};

use crate::CohAccess;

/// Coh access wrapper backed by the hive-gateway REST API.
pub struct RestSession {
    client: GatewayClient,
    bounds: Option<BoundsResponse>,
}

impl RestSession {
    /// Connect to the REST gateway.
    pub fn connect(base_url: impl Into<String>, request_auth_token: Option<String>) -> Self {
        let mut client = GatewayClient::new(base_url);
        if let Some(token) = request_auth_token {
            client = client.with_request_auth_token(token);
        }
        Self {
            client,
            bounds: None,
        }
    }

    /// Configure the caller ticket required for REST mutations.
    pub fn with_delegated_ticket(mut self, ticket: impl Into<String>) -> Self {
        self.client = self.client.with_delegated_ticket(ticket);
        self
    }

    /// Select explicit delegation; absent input retains the configured environment source.
    pub fn with_optional_delegated_ticket(mut self, ticket: Option<&str>) -> Self {
        if let Some(ticket) = ticket {
            self.client.set_delegated_ticket(Some(ticket.to_owned()));
        }
        self
    }

    /// Fetch manifest-derived bounds from the gateway.
    pub fn bounds(&self) -> Result<BoundsResponse> {
        self.client.bounds()
    }

    /// Return the gateway base URL.
    pub fn base_url(&self) -> &str {
        self.client.base_url()
    }

    fn read_max_bytes(&mut self, path: &str, maximum: usize) -> Result<u32> {
        let maximum = u32::try_from(maximum)
            .map_err(|_| anyhow!("read {path} exceeds max bytes {maximum}"))?;
        if self.bounds.is_none() {
            self.bounds = Some(self.client.bounds().context("resolve REST read bounds")?);
        }
        let bounds = self
            .bounds
            .as_ref()
            .context("REST read bounds unavailable")?;
        let maximum = bounds
            .path_byte_bound(path)
            .map_or(maximum, |bound| maximum.min(bound));
        if maximum == 0 {
            return Err(anyhow!("read {path} requires a nonzero byte bound"));
        }
        Ok(maximum)
    }

    fn join_lines(lines: &[String]) -> Vec<u8> {
        if lines.is_empty() {
            return Vec::new();
        }
        let mut out = String::new();
        for (idx, line) in lines.iter().enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            out.push_str(line);
        }
        out.into_bytes()
    }

    fn normalise_payload(payload: &[u8]) -> Result<String> {
        let payload_str = std::str::from_utf8(payload).context("payload must be UTF-8")?;
        let trimmed = payload_str.strip_suffix('\n').unwrap_or(payload_str);
        if trimmed.contains('\n') || trimmed.contains('\r') {
            return Err(anyhow!("payload must be a single line"));
        }
        Ok(trimmed.to_owned())
    }
}

impl CohAccess for RestSession {
    fn list_dir(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>> {
        let entries = self.client.list(path)?;
        let bytes = entries.iter().map(|entry| entry.len()).sum::<usize>();
        if bytes > max_bytes {
            return Err(anyhow!("read {path} exceeds max bytes {max_bytes}"));
        }
        Ok(entries)
    }

    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let max_bytes_u32 = self.read_max_bytes(path, max_bytes)?;
        let lines = self.client.read(path, max_bytes_u32)?;
        let payload = Self::join_lines(&lines);
        if payload.len() > max_bytes_u32 as usize {
            return Err(anyhow!("read {path} exceeds max bytes {max_bytes_u32}"));
        }
        Ok(payload)
    }

    fn tail_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let max_bytes_u32 = self.read_max_bytes(path, max_bytes)?;
        let lines = self.client.tail(path, max_bytes_u32)?;
        let payload = Self::join_lines(&lines);
        if payload.len() > max_bytes_u32 as usize {
            return Err(anyhow!("tail {path} exceeds max bytes {max_bytes_u32}"));
        }
        Ok(payload)
    }

    fn write_append(&mut self, path: &str, payload: &[u8]) -> Result<usize> {
        let trimmed = Self::normalise_payload(payload)?;
        self.client.echo(path, trimmed.as_str())?;
        Ok(payload.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    const BOUNDS: &str = r#"{
            "manifest_sha256": "deadbeef",
            "secure9p": {"msize": 8192, "walk_depth": 8},
            "console": {
                "max_line_len": 2304,
                "max_path_len": 96,
                "max_json_len": 192,
                "max_id_len": 32,
                "max_echo_len": 2048,
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

    fn serve(
        responses: Vec<(u16, String)>,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind owned loopback socket");
        let address = listener.local_addr().expect("loopback address");
        let (sender, receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().expect("accept bounded request");
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("read deadline");
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).expect("read request header");
                    request.push(byte[0]);
                    assert!(request.len() <= 16384, "bounded request header");
                }
                sender
                    .send(String::from_utf8(request).expect("HTTP request text"))
                    .expect("capture request");
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                stream
                    .write_all(response.as_bytes())
                    .expect("write response");
            }
        });
        (format!("http://{address}"), receiver, server)
    }

    fn file_response(path: &str, value: &str) -> String {
        serde_json::json!({"status":"OK", "verb":"CAT", "path":path, "end":true, "lines":[value]})
            .to_string()
    }

    #[test]
    fn rest_reads_and_tails_use_cached_path_bounds_and_reject_oversized_responses() {
        let (url, requests, server) = serve(vec![
            (200, BOUNDS.to_owned()),
            (200, file_response("/proc/schedule/summary", "queue=0")),
            (200, file_response("/proc/lease/summary", "active=0")),
            (200, file_response("/proc/boot", &"x".repeat(65))),
            (
                200,
                file_response("/proc/lease/by-id/example", &"x".repeat(257)),
            ),
        ]);
        let mut session = RestSession::connect(url, None);
        assert_eq!(
            session
                .read_file("/proc/schedule/summary", 65536)
                .expect("schedule read"),
            b"queue=0"
        );
        assert_eq!(
            session
                .tail_file("/proc/lease/summary", 65536)
                .expect("lease tail"),
            b"active=0"
        );
        assert!(session
            .read_file("/proc/boot", 64)
            .expect_err("caller limit")
            .to_string()
            .contains("exceeds max bytes 64"));
        assert!(session
            .read_file("/proc/lease/by-id/example", 65536)
            .expect_err("advertised limit")
            .to_string()
            .contains("exceeds max bytes 256"));
        for expected in [
            "GET /v1/meta/bounds HTTP/1.1",
            "GET /v1/fs/cat?path=%2Fproc%2Fschedule%2Fsummary&max_bytes=128 HTTP/1.1",
            "GET /v1/fs/tail?path=%2Fproc%2Flease%2Fsummary&max_bytes=160 HTTP/1.1",
            "GET /v1/fs/cat?path=%2Fproc%2Fboot&max_bytes=64 HTTP/1.1",
            "GET /v1/fs/cat?path=%2Fproc%2Flease%2Fby-id%2Fexample&max_bytes=256 HTTP/1.1",
        ] {
            let actual = requests
                .recv_timeout(Duration::from_secs(2))
                .expect("captured request");
            assert_eq!(actual.lines().next(), Some(expected));
        }
        server.join().expect("bounded server exits");
    }

    #[test]
    fn missing_bounds_do_not_fall_back_to_an_unbounded_request() {
        let (url, requests, server) = serve(vec![(503, "unavailable".to_owned())]);
        let mut session = RestSession::connect(url, None);
        assert!(session
            .read_file("/proc/schedule/summary", 65536)
            .expect_err("missing bounds")
            .to_string()
            .contains("resolve REST read bounds"));
        assert_eq!(
            requests
                .recv_timeout(Duration::from_secs(2))
                .expect("bounds request")
                .lines()
                .next(),
            Some("GET /v1/meta/bounds HTTP/1.1")
        );
        server.join().expect("bounded server exits");
        assert!(session.bounds.is_none());
    }

    #[test]
    fn requested_read_windows_never_grow_or_accept_zero() {
        let mut session = RestSession::connect("http://127.0.0.1:1", None);
        session.bounds = Some(serde_json::from_str(BOUNDS).expect("bounds fixture"));
        assert_eq!(
            session
                .read_max_bytes("/proc/schedule/summary", 8)
                .expect("caller bound"),
            8
        );
        assert_eq!(
            session
                .read_max_bytes("/proc/boot", 64)
                .expect("ordinary path"),
            64
        );
        assert!(session.read_max_bytes("/proc/boot", 0).is_err());
        session
            .bounds
            .as_mut()
            .expect("fixture")
            .observability
            .proc_lease
            .summary_bytes = 0;
        assert!(session
            .read_max_bytes("/proc/lease/summary", 65536)
            .is_err());
        if usize::BITS > 32 {
            assert!(session.read_max_bytes("/proc/boot", usize::MAX).is_err());
        }
    }
}
