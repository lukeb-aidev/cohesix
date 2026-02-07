// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Implement the REST transport backend for the Cohesix shell.
// Author: Lukas Bower
//! REST transport backend for the Cohesix shell.

use std::collections::VecDeque;

use anyhow::{anyhow, Context, Result};
use cohesix_rest::{BoundsResponse, GatewayClient};
use cohesix_ticket::Role;
use cohsh_core::wire::{render_ack, AckLine, AckStatus};
use cohsh_core::{normalize_ticket, role_label, ConsoleVerb, TicketPolicy};
use secure9p_codec::SessionId;

use crate::{CohshPolicy, Session, Transport, TransportMetrics};

const DEFAULT_SESSION_ID: SessionId = SessionId::BOOTSTRAP;

trait GatewayClientTail {
    fn tail(&self, path: &str, max_bytes: u32) -> Result<Vec<String>>;
}

impl GatewayClientTail for GatewayClient {
    fn tail(&self, path: &str, max_bytes: u32) -> Result<Vec<String>> {
        self.read(path, max_bytes)
    }
}

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
    pub fn new(base_url: impl Into<String>) -> Self {
        let policy = CohshPolicy::from_generated();
        Self {
            client: GatewayClient::new(base_url),
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
            let _ = normalize_ticket(role, Some(ticket), TicketPolicy::tcp()).map_err(|err| {
                anyhow!("ticket invalid for rest transport: {err}")
            })?;
        }
        self.attached = true;
        let session = Session::new(DEFAULT_SESSION_ID, role);
        let detail = format!("role={}", role_label(role));
        self.push_ack(AckStatus::Ok, ConsoleVerb::Attach.ack_label(), Some(detail.as_str()));
        Ok(session)
    }

    fn kind(&self) -> &'static str {
        "rest"
    }

    fn ping(&mut self, _session: &Session) -> Result<String> {
        self.ensure_attached()?;
        let bounds = self.client.bounds().context("rest ping bounds")?;
        self.bounds = Some(bounds.clone());
        self.push_ack(AckStatus::Ok, ConsoleVerb::Ping.ack_label(), None);
        Ok(format!("gateway ok manifest={}", bounds.manifest_sha256))
    }

    fn tail(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        self.ensure_attached()?;
        let max_bytes = self.tail_max_bytes(path);
        match self.client.tail(path, max_bytes) {
            Ok(lines) => {
                let detail = format!("path={path}");
                self.push_ack(AckStatus::Ok, ConsoleVerb::Tail.ack_label(), Some(detail.as_str()));
                Ok(lines)
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
                let detail = format!("path={path}");
                self.push_ack(AckStatus::Ok, ConsoleVerb::Cat.ack_label(), Some(detail.as_str()));
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
                self.push_ack(AckStatus::Ok, ConsoleVerb::Ls.ack_label(), Some(detail.as_str()));
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
                    format!("path={path} bytes={}", trimmed.as_bytes().len())
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
        ConsoleBounds, ControlPlaneBounds, ExportBounds, LeaseBounds, ObservabilityBounds,
        PathBounds, PolicyBounds, ProcLeaseBounds, ProcScheduleBounds, ScheduleBounds,
        Secure9pBounds,
    };

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
        let mut transport = RestTransport::new("http://example")
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
}
