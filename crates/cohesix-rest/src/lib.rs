// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide a bounded REST client for the Cohesix hive-gateway, including stable approved-job starts.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! REST client helpers for the Cohesix hive-gateway.

use anyhow::{anyhow, Context, Result};
use cohesix_net_constants::{
    COHESIX_TRANSPORT_COMMAND_BATCH_MAX, HIVE_GATEWAY_BROKER_QUEUE_WAIT_LIMIT_MS,
    HIVE_GATEWAY_REST_IO_TIMEOUT_MS, HIVE_GATEWAY_REST_OPERATION_RESPONSE_TIMEOUT_MS,
    HIVE_GATEWAY_REST_RESPONSE_GRACE_MS,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Short timeout retained for metadata, resolution, connection, and response bodies.
const DEFAULT_IO_TIMEOUT: Duration = Duration::from_millis(HIVE_GATEWAY_REST_IO_TIMEOUT_MS);
/// Default `/v1/fs/*` response envelope covering queue, target, and delivery bounds.
pub const DEFAULT_OPERATION_RESPONSE_TIMEOUT: Duration =
    Duration::from_millis(HIVE_GATEWAY_REST_OPERATION_RESPONSE_TIMEOUT_MS);
const DEFAULT_OPERATION_GLOBAL_TIMEOUT: Duration = Duration::from_millis(
    HIVE_GATEWAY_REST_OPERATION_RESPONSE_TIMEOUT_MS + HIVE_GATEWAY_REST_IO_TIMEOUT_MS * 3,
);

/// Compose the client deadline from the gateway's selected broker timeouts.
pub fn compose_operation_response_timeout(
    control_response_timeout: Duration,
    telemetry_response_timeout: Duration,
) -> Result<Duration> {
    let queue_wait = Duration::from_millis(HIVE_GATEWAY_BROKER_QUEUE_WAIT_LIMIT_MS);
    let response_grace = Duration::from_millis(HIVE_GATEWAY_REST_RESPONSE_GRACE_MS);
    if control_response_timeout < queue_wait || telemetry_response_timeout < queue_wait {
        return Err(anyhow!(
            "gateway broker response timeouts must be at least {}ms",
            queue_wait.as_millis()
        ));
    }
    queue_wait
        .checked_add(control_response_timeout.max(telemetry_response_timeout))
        .and_then(|timeout| timeout.checked_add(response_grace))
        .ok_or_else(|| anyhow!("gateway operation response timeout overflow"))
}

/// Absolute response-body bound, including metadata and error responses.
/// Streaming decoding enforces this before trusting peer Content-Length.
pub const MAX_REST_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

type HttpResponse = ureq::http::Response<ureq::Body>;

/// REST client for the hive-gateway API.
#[derive(Debug, Clone)]
pub struct GatewayClient {
    base_url: String,
    metadata_agent: ureq::Agent,
    operation_agent: Box<ureq::Agent>,
    request_auth_token: Option<String>,
    delegated_ticket: Option<String>,
}

impl GatewayClient {
    /// Construct a new gateway client with default timeouts.
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut base = base_url.into();
        while base.ends_with('/') {
            base.pop();
        }
        Self {
            base_url: base,
            metadata_agent: Self::build_agent(DEFAULT_IO_TIMEOUT, DEFAULT_IO_TIMEOUT),
            operation_agent: Box::new(Self::build_agent(
                DEFAULT_OPERATION_RESPONSE_TIMEOUT,
                DEFAULT_OPERATION_GLOBAL_TIMEOUT,
            )),
            request_auth_token: None,
            delegated_ticket: std::env::var("COH_REST_TICKET").ok(),
        }
    }

    /// Override the bounded response envelope for `/v1/fs/*` operations.
    ///
    /// The timeout must cover the gateway's queue-wait limit, its selected
    /// control/telemetry response timeout, and the fixed HTTP delivery grace.
    pub fn with_operation_response_timeout(mut self, timeout: Duration) -> Result<Self> {
        self.set_operation_response_timeout(timeout)?;
        Ok(self)
    }

    /// Cap the entire HTTP mutation by an external WAL-backed caller deadline.
    /// An elapsed deadline is an ambiguous outcome; it never authorizes retry.
    pub fn with_operation_deadline(mut self, timeout: Duration) -> Result<Self> {
        if timeout.is_zero() || timeout > Duration::from_secs(3600) {
            return Err(anyhow!("operation deadline must be within 1ns..=3600s"));
        }
        *self.operation_agent = Self::build_agent(timeout, timeout);
        Ok(self)
    }

    /// Configure `/v1/fs/*` timing from selected gateway broker timeouts.
    pub fn with_broker_response_timeouts(
        self,
        control_response_timeout: Duration,
        telemetry_response_timeout: Duration,
    ) -> Result<Self> {
        self.with_operation_response_timeout(compose_operation_response_timeout(
            control_response_timeout,
            telemetry_response_timeout,
        )?)
    }

    /// Set the bounded response envelope for `/v1/fs/*` operations.
    pub fn set_operation_response_timeout(&mut self, timeout: Duration) -> Result<()> {
        let minimum = Duration::from_millis(
            HIVE_GATEWAY_BROKER_QUEUE_WAIT_LIMIT_MS
                .saturating_mul(2)
                .saturating_add(HIVE_GATEWAY_REST_RESPONSE_GRACE_MS),
        );
        if timeout < minimum {
            return Err(anyhow!(
                "gateway operation response timeout must be at least {}ms",
                minimum.as_millis()
            ));
        }
        let global_timeout = Self::operation_global_timeout(timeout)
            .ok_or_else(|| anyhow!("gateway operation global timeout overflow"))?;
        *self.operation_agent = Self::build_agent(timeout, global_timeout);
        Ok(())
    }

    /// Return the configured `/v1/fs/*` response envelope.
    pub fn operation_response_timeout(&self) -> Duration {
        self.operation_agent
            .config()
            .timeouts()
            .recv_response
            .unwrap_or(DEFAULT_OPERATION_RESPONSE_TIMEOUT)
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

    /// Configure the caller ticket or explicit env:/file: credential reference.
    /// References are resolved at the request boundary to support rotation.
    pub fn with_delegated_ticket(mut self, ticket: impl Into<String>) -> Self {
        self.delegated_ticket = Some(ticket.into());
        self
    }

    /// Replace caller delegation without changing the upstream gateway identity.
    pub fn set_delegated_ticket(&mut self, ticket: Option<String>) {
        self.delegated_ticket = ticket;
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
        let response = self.get_operation(&url);
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
        let response = self.get_operation(&url);
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
        let response = self.get_operation(&url);
        let parsed = handle_response("TAIL", response)?;
        Ok(parsed.lines)
    }

    /// Issue an ECHO request via the gateway.
    pub fn echo(&self, path: &str, line: &str) -> Result<usize> {
        let ticket = self.validate_write_credentials()?;
        let url = format!("{}/v1/fs/echo", self.base_url);
        let payload = EchoRequest {
            path: path.to_owned(),
            line: Some(line.to_owned()),
        };
        let response = self.post_json(&url, &payload, &ticket);
        let parsed = handle_response("ECHO", response)?;
        Ok(parsed.bytes.unwrap_or(0))
    }

    /// Append an ordered, bounded set of lines in one target transport activation.
    pub fn echo_batch(&self, path: &str, lines: &[String]) -> Result<usize> {
        if lines.is_empty() {
            return Err(anyhow!("ECHO_BATCH requires at least one line"));
        }
        if lines.len() > COHESIX_TRANSPORT_COMMAND_BATCH_MAX {
            return Err(anyhow!(
                "ECHO_BATCH exceeds {} lines",
                COHESIX_TRANSPORT_COMMAND_BATCH_MAX
            ));
        }
        let ticket = self.validate_write_credentials()?;
        let url = format!("{}/v1/fs/echo-batch", self.base_url);
        let payload = EchoBatchRequest {
            path: path.to_owned(),
            lines: lines.to_vec(),
        };
        let response = self.post_json(&url, &payload, &ticket);
        let _ = handle_response("ECHO_BATCH", response)?;
        Ok(lines.len())
    }

    /// Submit one complete selected intent. A transport error is uncertain;
    /// callers must reconcile this exact admission ID before another effect.
    pub fn submit_selected_job(&self, request: &serde_json::Value) -> Result<serde_json::Value> {
        let encoded = serde_json::to_vec(request)?;
        if encoded.len() > 4096 || !request.is_object() {
            return Err(anyhow!("ELIMIT selected job request"));
        }
        let ticket = self.validate_write_credentials()?;
        let url = format!("{}/v1/jobs", self.base_url);
        decode_json_response(
            "JOB SUBMIT",
            "selected job",
            self.post_json(&url, request, &ticket),
        )
    }

    /// Start an operator-selected standing service scope with a stable request
    /// identity. A lost response must be reconciled under `mac-{request_id}`.
    pub fn start_approved_job(
        &self,
        scope_id: &str,
        request_id: &str,
    ) -> Result<serde_json::Value> {
        cohesix_authority::validate_id(scope_id).map_err(|_| anyhow!("EPERM scope id"))?;
        cohesix_authority::validate_id(request_id).map_err(|_| anyhow!("EPERM request id"))?;
        if request_id.len() > 96 {
            return Err(anyhow!("ELIMIT approved request id"));
        }
        let ticket = self.validate_write_credentials()?;
        let url = format!("{}/v1/jobs/approved/{scope_id}/start", self.base_url);
        decode_json_response(
            "JOB START APPROVED",
            "approved job",
            self.post_json(
                &url,
                &serde_json::json!({"request_id": request_id}),
                &ticket,
            ),
        )
    }

    /// List the delegated subject's currently usable standing service scopes.
    /// A listed choice is advisory; each start is rechecked by the gateway.
    pub fn available_standing_scopes(&self) -> Result<serde_json::Value> {
        let url = format!("{}/v1/standing/scopes/available", self.base_url);
        decode_json_response(
            "STANDING SCOPES",
            "available standing scopes",
            self.get_operation(&url),
        )
    }

    /// Read one retained execution and independent delivery state.
    pub fn selected_job_status(&self, admission_id: &str) -> Result<serde_json::Value> {
        cohesix_authority::validate_id(admission_id).map_err(|_| anyhow!("EPERM job id"))?;
        let url = format!("{}/v1/jobs/{admission_id}", self.base_url);
        decode_json_response(
            "JOB STATUS",
            "selected job status",
            self.get_operation(&url),
        )
    }

    /// Request cancellation once without treating the request as termination.
    pub fn request_selected_job_cancel(&self, admission_id: &str) -> Result<serde_json::Value> {
        self.selected_job_control(admission_id, "cancel")
    }

    /// Inspect the retained record and exact target results without replay.
    pub fn reconcile_selected_job(&self, admission_id: &str) -> Result<serde_json::Value> {
        self.selected_job_control(admission_id, "reconcile")
    }

    /// Inspect durable scope spending under a separately scoped admin ticket.
    pub fn inspect_standing_scope(&self, scope_id: &str) -> Result<serde_json::Value> {
        self.standing_scope_control(scope_id, "inspect")
    }

    /// Revoke future effects without claiming termination of dispatched work.
    pub fn revoke_standing_scope(&self, scope_id: &str) -> Result<serde_json::Value> {
        self.standing_scope_control(scope_id, "revoke")
    }

    fn selected_job_control(
        &self,
        admission_id: &str,
        operation: &str,
    ) -> Result<serde_json::Value> {
        cohesix_authority::validate_id(admission_id).map_err(|_| anyhow!("EPERM job id"))?;
        let ticket = self.validate_write_credentials()?;
        let url = format!("{}/v1/jobs/{admission_id}/{operation}", self.base_url);
        decode_json_response(
            "JOB CONTROL",
            "selected job control",
            self.post_json(&url, &serde_json::json!({}), &ticket),
        )
    }

    fn standing_scope_control(&self, scope_id: &str, operation: &str) -> Result<serde_json::Value> {
        cohesix_authority::validate_id(scope_id).map_err(|_| anyhow!("EPERM scope id"))?;
        let ticket = self.validate_write_credentials()?;
        let url = format!(
            "{}/v1/standing/scopes/{scope_id}/{operation}",
            self.base_url
        );
        decode_json_response(
            "STANDING SCOPE",
            "standing scope",
            self.post_json(&url, &serde_json::json!({}), &ticket),
        )
    }

    fn get(&self, url: &str) -> Result<HttpResponse, ureq::Error> {
        Self::get_with_agent(
            &self.metadata_agent,
            self.resolved_request_auth()?.as_deref(),
            self.resolved_delegation()?.as_deref(),
            url,
        )
    }

    fn get_operation(&self, url: &str) -> Result<HttpResponse, ureq::Error> {
        Self::get_with_agent(
            &self.operation_agent,
            self.resolved_request_auth()?.as_deref(),
            self.resolved_delegation()?.as_deref(),
            url,
        )
    }

    fn get_with_agent(
        agent: &ureq::Agent,
        request_auth_token: Option<&str>,
        delegated_ticket: Option<&str>,
        url: &str,
    ) -> Result<HttpResponse, ureq::Error> {
        let mut request = agent.get(url);
        if let Some(ticket) = delegated_ticket {
            request = request.header("x-cohesix-ticket", ticket);
        }
        if let Some(token) = request_auth_token {
            request
                .header("Authorization", format!("Bearer {token}"))
                .header("x-cohesix-auth", token)
                .call()
        } else {
            request.call()
        }
    }

    fn post_json<T: Serialize>(
        &self,
        url: &str,
        payload: &T,
        ticket: &str,
    ) -> Result<HttpResponse, ureq::Error> {
        let request = self
            .operation_agent
            .post(url)
            .header("x-cohesix-ticket", ticket);
        if let Some(token) = self.resolved_request_auth()? {
            request
                .header("Authorization", format!("Bearer {token}"))
                .header("x-cohesix-auth", &token)
                .send_json(payload)
        } else {
            request.send_json(payload)
        }
    }

    fn resolved_request_auth(&self) -> Result<Option<String>, ureq::Error> {
        self.request_auth_token
            .as_deref()
            .map(cohesix_authority::secret::resolve_value)
            .transpose()
            .map_err(|error| {
                ureq::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    error,
                ))
            })
    }

    fn resolved_delegation(&self) -> Result<Option<String>, ureq::Error> {
        self.delegated_ticket
            .as_deref()
            .map(cohesix_authority::secret::resolve_value)
            .transpose()
            .map_err(|error| {
                ureq::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    error,
                ))
            })
    }

    fn validate_write_credentials(&self) -> Result<String> {
        let auth = self
            .request_auth_token
            .as_deref()
            .ok_or_else(|| anyhow!("EPERM request-auth-required"))?;
        cohesix_authority::secret::resolve_value(auth)?;
        let ticket = self
            .resolved_delegation()?
            .ok_or_else(|| anyhow!("EPERM delegated-ticket-required"))?;
        if ticket.len() > cohsh_core::MAX_TICKET_LEN {
            return Err(anyhow!("ELIMIT delegated-ticket-length"));
        }
        // Structural validation is a client convenience. Only the gateway
        // verifies the issuer MAC and adjudicates authority/quota state.
        cohesix_ticket::TicketToken::decode_unverified(&ticket)
            .map_err(|_| anyhow!("EPERM delegated-ticket-malformed"))?;
        Ok(ticket)
    }

    fn operation_global_timeout(response_timeout: Duration) -> Option<Duration> {
        response_timeout
            .checked_add(DEFAULT_IO_TIMEOUT)
            .and_then(|timeout| timeout.checked_add(DEFAULT_IO_TIMEOUT))
            .and_then(|timeout| timeout.checked_add(DEFAULT_IO_TIMEOUT))
    }

    fn build_agent(response_timeout: Duration, global_timeout: Duration) -> ureq::Agent {
        // ureq 3.x carries send-phase deadlines forward while awaiting
        // response headers, so request send, body send, and response receive
        // share the composed broker bound. Resolve, connect, and body-read keep
        // their short phase bounds; the global cap includes all three.
        ureq::Agent::config_builder()
            .timeout_global(Some(global_timeout))
            .timeout_resolve(Some(DEFAULT_IO_TIMEOUT))
            .timeout_connect(Some(DEFAULT_IO_TIMEOUT))
            .timeout_send_request(Some(response_timeout))
            .timeout_send_body(Some(response_timeout))
            .timeout_recv_response(Some(response_timeout))
            .timeout_recv_body(Some(DEFAULT_IO_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into()
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
    /// Optional compiler-generated Worker runtime declaration and namespace bounds.
    #[serde(default)]
    pub worker_runtime: Option<WorkerRuntimeBounds>,
}

impl BoundsResponse {
    /// Advertised byte bound for a bounded namespace projection.
    ///
    /// Clients cap their requested read window at this value. This does not
    /// enable the path or grant read authority; the gateway still checks both.
    pub fn path_byte_bound(&self, path: &str) -> Option<u32> {
        if path.starts_with("/proc/lease/by-id/") {
            return Some(self.observability.proc_lease.active_bytes);
        }
        match path {
            "/proc/schedule/summary" => Some(self.observability.proc_schedule.summary_bytes),
            "/proc/schedule/queue" => Some(self.observability.proc_schedule.queue_bytes),
            "/proc/lease/summary" => Some(self.observability.proc_lease.summary_bytes),
            "/proc/lease/active" => Some(self.observability.proc_lease.active_bytes),
            "/proc/lease/preemptions" => Some(self.observability.proc_lease.preemptions_bytes),
            _ if path == self.paths.queen_schedule_ctl => {
                Some(self.control_plane.schedule.ctl_max_bytes)
            }
            _ if path == self.paths.queen_lease_ctl => Some(self.control_plane.lease.ctl_max_bytes),
            _ if path == self.paths.queen_export_ctl => {
                Some(self.control_plane.export.ctl_max_bytes)
            }
            _ if path == self.paths.policy_ctl => Some(self.policy.ctl_max_bytes),
            _ => None,
        }
    }
}

/// Compiler-generated Worker runtime declaration and namespace bounds.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkerRuntimeBounds {
    /// Canonical bounded role matrix.
    pub roles: Vec<WorkerRoleBounds>,
    /// Worker task ABI schema identifier.
    pub task_abi_schema: String,
    /// Worker task ABI numeric version.
    pub task_abi_version: u16,
    /// Worker observation schema identifier.
    pub worker_observation_schema: String,
    /// Worker integration-evidence schema identifier.
    pub worker_integration_evidence_schema: String,
    /// Maximum simultaneously live executable Worker tasks.
    pub maximum_live_tasks: u16,
    /// Canonical sharded Worker telemetry path template.
    pub canonical_telemetry_template: String,
    /// Generated Worker shard selector width.
    pub shard_bits: u8,
    /// Whether the legacy `/worker` alias remains enabled.
    pub legacy_worker_alias: bool,
}

/// One generated Worker role declaration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkerRoleBounds {
    /// Canonical role label.
    pub role: String,
    /// Static implementation declaration.
    pub declaration: WorkerDeclaration,
    /// Compiler-admitted executable slots for this role.
    pub executable_slots: u16,
}

/// Static generated Worker implementation declaration.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerDeclaration {
    /// A real target child image is selected.
    Executable,
    /// Host/session modelling only; no target task exists.
    ModelOnly,
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
    /// Optional gateway-enforced identity and quota/audit counters.
    #[serde(default)]
    pub authority: Option<cohesix_authority::policy::DelegationStatus>,
    /// True when the gateway currently has a console connection.
    pub connected: bool,
    /// Normalized configured backend TCP target host, never the REST bind host.
    pub target_host: String,
    /// Configured backend TCP target port, never the REST bind port.
    pub target_port: u16,
    /// Optional transport implementation class; connectivity is a separate axis.
    #[serde(default)]
    pub backend_class: Option<BackendClass>,
    /// Optional summary derived by the shared strict Worker evidence validator.
    #[serde(default)]
    pub worker_acceptance: Option<WorkerAcceptanceSummary>,
    /// Typed reason no Worker acceptance summary is available.
    #[serde(default)]
    pub worker_acceptance_diagnostic: Option<WorkerAcceptanceDiagnostic>,
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

/// Gateway backend implementation class; it never supplies target proof.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BackendClass {
    /// In-process host model.
    HostModel,
    /// Console/Secure9P projection to a target.
    ConsoleProjection,
    /// Older or unclassified gateway.
    Unknown,
}

/// Redacted target acceptance state for one executable role.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkerAcceptanceRoleSummary {
    /// Canonical Worker role.
    pub role: String,
    /// Runtime lifecycle state.
    pub lifecycle: String,
    /// Artifact identity state.
    pub artifact: String,
    /// Durable receipt state.
    pub receipt: String,
    /// Exact execution proof class.
    pub execution_proof: String,
    /// Compiler-admitted per-role slot.
    pub slot: u16,
    /// Root-resolved logical lease epoch.
    pub lease_epoch: u64,
    /// Worker-supervisor generation.
    pub supervisor_generation: u64,
    /// Revocable capability-bundle generation.
    pub cap_generation: u64,
    /// Exact executable image digest.
    pub image_sha256: String,
    /// Last durable READY sequence.
    pub ready_sequence: u64,
    /// Last durable completion sequence.
    pub completion_sequence: u64,
    /// Generated zero-based CPU core.
    pub core: u8,
    /// Exact generated active or passive scheduling-context parameters.
    pub scheduling_context: WorkerSchedulingContextSummary,
    /// Exact generated per-instance object counts; no capability addresses.
    pub object_inventory: KernelObjectInventorySummary,
}

/// Redacted active or passive scheduling-context parameters for one accepted Worker.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkerSchedulingContextSummary {
    /// Execution budget in microseconds, or zero when passive.
    pub budget_us: u32,
    /// Replenishment period in microseconds, or zero when passive.
    pub period_us: u32,
}

/// Redacted kernel-object inventory containing counts only.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct KernelObjectInventorySummary {
    /// TCB count.
    pub tcbs: u32,
    /// Scheduling-context count.
    pub scheduling_contexts: u32,
    /// Reply-object count.
    pub reply_objects: u32,
    /// VSpace-root count.
    pub vspaces: u32,
    /// CSpace-root count.
    pub cnodes: u32,
    /// Page-table count.
    pub page_tables: u32,
    /// ASID count.
    pub asids: u32,
    /// Frame count.
    pub frames: u32,
    /// Endpoint count.
    pub endpoints: u32,
    /// Notification count.
    pub notifications: u32,
    /// Standard fault-cap count.
    pub fault_caps: u32,
    /// Timeout fault-cap count.
    pub timeout_fault_caps: u32,
    /// Admitted CSpace-slot count.
    pub cspace_slots: u32,
    /// Retyped untyped-memory extent in bytes.
    pub untyped_bytes: u64,
}

/// Redacted current-target artifact identity bound to accepted evidence.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TargetSessionSummary {
    /// SHA-256 of the exact independently supplied target-session bytes.
    pub target_session_sha256: String,
    /// Exact resolved-manifest digest.
    pub manifest_sha256: String,
    /// Exact root image digest.
    pub root_image_sha256: String,
    /// Exact Worker archive digest.
    pub worker_archive_sha256: String,
    /// Exact Worker image-manifest digest.
    pub worker_image_manifest_sha256: String,
    /// Exact Worker ABI bundle digest.
    pub worker_abi_sha256: String,
}

/// Redacted summary of a locally validated Worker acceptance record.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkerAcceptanceSummary {
    /// Exact evidence schema.
    pub schema: String,
    /// Exact validated record kind.
    pub record_kind: String,
    /// SHA-256 of the exact imported record bytes.
    pub evidence_sha256: String,
    /// Strict evidence verdict.
    pub verdict: String,
    /// Direct target when the record covers one target.
    #[serde(default)]
    pub target: Option<String>,
    /// Exact proof class, never inferred from connectivity.
    pub execution_proof: String,
    /// Exact current target-session identity matched against this evidence.
    pub target_session: TargetSessionSummary,
    /// Compiler-owned accepted topology digest.
    pub topology_sha256: String,
    /// Per-role state when present in a target-component record.
    #[serde(default)]
    pub workers: Vec<WorkerAcceptanceRoleSummary>,
}

/// Typed reason no Worker acceptance summary is exposed.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkerAcceptanceDiagnostic {
    /// Stable diagnostic code.
    pub code: WorkerAcceptanceDiagnosticCode,
}

/// Fail-closed Worker acceptance import diagnostic.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerAcceptanceDiagnosticCode {
    /// No import was configured.
    NotConfigured,
    /// The root, evidence, and target-session paths were not all configured.
    IncompleteConfiguration,
    /// The explicit root was absent, a symlink, or not a directory.
    UnsafeRoot,
    /// The evidence file was outside the explicit root.
    OutsideRoot,
    /// A symlink appeared below the explicit root.
    SymlinkTraversal,
    /// The selected path was not a regular file.
    NotRegularFile,
    /// The record exceeded the fixed input bound.
    RecordTooLarge,
    /// The bounded file could not be read.
    ReadFailed,
    /// The shared validator rejected the record.
    InvalidEvidence,
    /// The independently supplied target-session record was invalid.
    InvalidTargetSession,
    /// Accepted evidence did not name the exact current target session.
    TargetSessionMismatch,
    /// Current target manifest did not match this generated gateway policy.
    ManifestMismatch,
    /// The valid record kind is not an acceptance projection.
    UnsupportedRecordKind,
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

#[derive(Debug, serde::Serialize)]
struct EchoBatchRequest {
    path: String,
    lines: Vec<String>,
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
                    .with_config()
                    .limit(MAX_REST_RESPONSE_BYTES)
                    .read_to_string()
                    .map_err(|err| anyhow!(err))
                    .with_context(|| format!("read {response_name} error response"))?;
                if body.is_empty() {
                    return Err(anyhow!("{verb} failed (http {code})"));
                }
                return Err(anyhow!("{verb} failed (http {code}): {body}"));
            }
            resp.body_mut()
                .with_config()
                .limit(MAX_REST_RESPONSE_BYTES)
                .read_json()
                .map_err(|err| anyhow!(err))
                .with_context(|| format!("decode {response_name} response"))
        }
        Err(err) => Err(anyhow!(err).context("gateway transport error")),
    }
}

fn parse_response(mut resp: HttpResponse) -> Result<GatewayResponse> {
    resp.body_mut()
        .with_config()
        .limit(MAX_REST_RESPONSE_BYTES)
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
    use super::{
        compose_operation_response_timeout, BoundsResponse, GatewayClient, GatewayStatusResponse,
        COHESIX_TRANSPORT_COMMAND_BATCH_MAX, DEFAULT_IO_TIMEOUT, DEFAULT_OPERATION_GLOBAL_TIMEOUT,
        DEFAULT_OPERATION_RESPONSE_TIMEOUT,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    #[test]
    fn gateway_client_separates_metadata_and_operation_deadlines() {
        let client = GatewayClient::new("http://127.0.0.1:1");
        let metadata = client.metadata_agent.config().timeouts();
        let operation = client.operation_agent.config().timeouts();

        assert_eq!(metadata.global, Some(DEFAULT_IO_TIMEOUT));
        assert_eq!(metadata.resolve, Some(DEFAULT_IO_TIMEOUT));
        assert_eq!(metadata.connect, Some(DEFAULT_IO_TIMEOUT));
        assert_eq!(metadata.send_request, Some(DEFAULT_IO_TIMEOUT));
        assert_eq!(metadata.send_body, Some(DEFAULT_IO_TIMEOUT));
        assert_eq!(metadata.recv_response, Some(DEFAULT_IO_TIMEOUT));
        assert_eq!(metadata.recv_body, Some(DEFAULT_IO_TIMEOUT));

        assert_eq!(operation.global, Some(DEFAULT_OPERATION_GLOBAL_TIMEOUT));
        assert_eq!(operation.resolve, Some(DEFAULT_IO_TIMEOUT));
        assert_eq!(operation.connect, Some(DEFAULT_IO_TIMEOUT));
        assert_eq!(
            operation.send_request,
            Some(DEFAULT_OPERATION_RESPONSE_TIMEOUT)
        );
        assert_eq!(
            operation.send_body,
            Some(DEFAULT_OPERATION_RESPONSE_TIMEOUT)
        );
        assert_eq!(
            operation.recv_response,
            Some(DEFAULT_OPERATION_RESPONSE_TIMEOUT)
        );
        assert_eq!(operation.recv_body, Some(DEFAULT_IO_TIMEOUT));
    }

    #[test]
    fn gateway_client_validates_and_applies_operation_deadline_override() {
        let minimum = Duration::from_millis(
            cohesix_net_constants::HIVE_GATEWAY_BROKER_QUEUE_WAIT_LIMIT_MS * 2
                + cohesix_net_constants::HIVE_GATEWAY_REST_RESPONSE_GRACE_MS,
        );
        let err = GatewayClient::new("http://127.0.0.1:1")
            .with_operation_response_timeout(minimum - Duration::from_millis(1))
            .expect_err("queue plus delivery without a response budget must fail");
        assert_eq!(
            err.to_string(),
            "gateway operation response timeout must be at least 15000ms"
        );

        let selected = Duration::from_millis(190_000);
        let client = GatewayClient::new("http://127.0.0.1:1")
            .with_operation_response_timeout(selected)
            .expect("valid operation timeout");
        let operation = client.operation_agent.config().timeouts();
        assert_eq!(client.operation_response_timeout(), selected);
        assert_eq!(
            operation.global,
            GatewayClient::operation_global_timeout(selected)
        );
        assert_eq!(operation.send_request, Some(selected));
        assert_eq!(operation.send_body, Some(selected));
        assert_eq!(operation.recv_response, Some(selected));
        assert_eq!(operation.resolve, Some(DEFAULT_IO_TIMEOUT));
        assert_eq!(operation.connect, Some(DEFAULT_IO_TIMEOUT));
        assert_eq!(operation.recv_body, Some(DEFAULT_IO_TIMEOUT));

        let err = GatewayClient::new("http://127.0.0.1:1")
            .with_operation_response_timeout(Duration::MAX)
            .expect_err("global timeout overflow must fail closed");
        assert_eq!(err.to_string(), "gateway operation global timeout overflow");
    }

    #[test]
    fn operation_deadline_composes_queue_larger_response_and_delivery_grace() {
        assert_eq!(
            DEFAULT_OPERATION_RESPONSE_TIMEOUT,
            Duration::from_millis(130_000)
        );
        assert_eq!(
            compose_operation_response_timeout(
                Duration::from_millis(120_000),
                Duration::from_millis(120_000),
            )
            .expect("canonical timeout composition"),
            Duration::from_millis(130_000)
        );
        assert_eq!(
            compose_operation_response_timeout(
                Duration::from_millis(120_000),
                Duration::from_millis(180_000),
            )
            .expect("telemetry override composition"),
            Duration::from_millis(190_000)
        );
        assert!(compose_operation_response_timeout(
            Duration::from_millis(4_999),
            Duration::from_millis(120_000),
        )
        .is_err());
        assert!(
            compose_operation_response_timeout(Duration::MAX, Duration::from_millis(120_000),)
                .is_err()
        );
    }

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
        let parsed: BoundsResponse = serde_json::from_str(json).expect("bounds json");
        assert_eq!(parsed.manifest_sha256, "deadbeef");
        assert_eq!(parsed.secure9p.msize, 8192);
        assert_eq!(parsed.observability.proc_schedule.queue_bytes, 256);
        assert!(parsed.worker_runtime.is_none());
        for (path, expected) in [
            ("/proc/schedule/summary", Some(128)),
            ("/proc/schedule/queue", Some(256)),
            ("/proc/lease/summary", Some(160)),
            ("/proc/lease/active", Some(256)),
            ("/proc/lease/by-id/example", Some(256)),
            ("/proc/lease/preemptions", Some(256)),
            ("/queen/schedule/ctl", Some(8192)),
            ("/queen/lease/ctl", Some(8192)),
            ("/queen/export/ctl", Some(2048)),
            ("/policy/ctl", Some(2048)),
            ("/proc/boot", None),
            ("/proc/lease/by-id-other/example", None),
        ] {
            assert_eq!(parsed.path_byte_bound(path), expected, "{path}");
        }
    }

    #[test]
    fn gateway_status_response_parses_broker_counters() {
        let json = r#"{
            "connected": true,
            "target_host": "192.168.10.2",
            "target_port": 31337,
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
        assert_eq!(parsed.target_host, "192.168.10.2");
        assert_eq!(parsed.target_port, 31337);
        assert!(parsed.backend_class.is_none());
        assert!(parsed.worker_acceptance.is_none());
        assert_eq!(parsed.reconnects, 2);
        assert_eq!(parsed.broker.telemetry_waiters, 57);
        assert_eq!(parsed.broker.control_write_retry_exhaustions, 1080);
        assert_eq!(parsed.broker.relay_queue_depth, 9);
    }

    #[test]
    fn worker_runtime_and_acceptance_extensions_parse_without_inference() {
        let bounds = r#"{
            "manifest_sha256":"deadbeef",
            "secure9p":{"msize":8192,"walk_depth":8},
            "console":{"max_line_len":2304,"max_path_len":96,"max_json_len":192,"max_id_len":32,"max_echo_len":2048,"max_ticket_len":224},
            "paths":{"queen_ctl":"/queen/ctl","queen_lifecycle_ctl":"/queen/lifecycle/ctl","queen_schedule_ctl":"/queen/schedule/ctl","queen_lease_ctl":"/queen/lease/ctl","queen_export_ctl":"/queen/export/ctl","policy_ctl":"/policy/ctl","log":"/log/queen.log"},
            "control_plane":{"schedule":{"enable":true,"queue_max_entries":64,"ctl_max_bytes":8192},"lease":{"enable":true,"active_max_entries":64,"preemptions_max_entries":64,"ctl_max_bytes":8192},"export":{"enable":true,"ctl_max_bytes":2048}},
            "policy":{"enable":true,"queue_max_entries":32,"queue_max_bytes":4096,"ctl_max_bytes":2048},
            "observability":{"proc_schedule":{"summary":true,"queue":true,"summary_bytes":128,"queue_bytes":256},"proc_lease":{"summary":true,"active":true,"preemptions":true,"summary_bytes":160,"active_bytes":256,"preemptions_bytes":256}},
            "worker_runtime":{"roles":[{"role":"worker-heartbeat","declaration":"executable","executable_slots":1},{"role":"worker-bus","declaration":"model-only","executable_slots":0}],"task_abi_schema":"worker-task-abi/v2","task_abi_version":2,"worker_observation_schema":"cohesix-worker-observation/v1","worker_integration_evidence_schema":"cohesix-worker-integration-evidence/v1","maximum_live_tasks":1,"canonical_telemetry_template":"/shard/<label>/worker/<id>/telemetry","shard_bits":8,"legacy_worker_alias":true}
        }"#;
        let parsed: BoundsResponse = serde_json::from_str(bounds).expect("extended bounds JSON");
        let runtime = parsed.worker_runtime.expect("Worker runtime extension");
        assert_eq!(runtime.maximum_live_tasks, 1);
        assert_eq!(
            runtime.roles[1].declaration,
            super::WorkerDeclaration::ModelOnly
        );

        let status = r#"{
            "connected":true,
            "target_host":"pi4.local",
            "target_port":31337,
            "backend_class":"console-projection",
            "worker_acceptance":{
                "schema":"cohesix-worker-task-evidence/v1",
                "record_kind":"target-component",
                "evidence_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "verdict":"PASS",
                "target":"qemu",
                "execution_proof":"qemu",
                "target_session":{
                    "target_session_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "manifest_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                    "root_image_sha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                    "worker_archive_sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                    "worker_image_manifest_sha256":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                    "worker_abi_sha256":"1111111111111111111111111111111111111111111111111111111111111111"
                },
                "topology_sha256":"2222222222222222222222222222222222222222222222222222222222222222",
                "workers":[{
                    "role":"worker-gpu",
                    "lifecycle":"ready",
                    "artifact":"verified",
                    "receipt":"confirmed",
                    "execution_proof":"qemu",
                    "slot":1,
                    "lease_epoch":2,
                    "supervisor_generation":3,
                    "cap_generation":4,
                    "image_sha256":"3333333333333333333333333333333333333333333333333333333333333333",
                    "ready_sequence":5,
                    "completion_sequence":6,
                    "core":1,
                    "scheduling_context":{"budget_us":0,"period_us":0},
                    "object_inventory":{"tcbs":1,"scheduling_contexts":1,"reply_objects":0,"vspaces":1,"cnodes":1,"page_tables":8,"asids":1,"frames":16,"endpoints":0,"notifications":1,"fault_caps":1,"timeout_fault_caps":1,"cspace_slots":64,"untyped_bytes":1048576}
                }]
            },
            "reconnects":0,"connects":1,
            "broker":{"control_waiters":0,"telemetry_waiters":0,"control_waiters_high_water":0,"telemetry_waiters_high_water":0,"control_checkouts":0,"telemetry_checkouts":0,"pool_exhausted":0,"checkout_retries":0,"timeout_rejections":0,"telemetry_yields":0,"proc_cache_hits":0,"proc_cache_misses":0,"proc_cache_evictions":0,"control_write_retryable_errors":0,"control_write_retries":0,"control_write_retry_sleep_ms":0,"control_write_retry_exhaustions":0,"control_write_success_after_retry":0,"relay_queue_depth":0,"relay_deduped":0,"relay_remote_write_failures":0}
        }"#;
        let parsed: GatewayStatusResponse = serde_json::from_str(status).expect("extended status");
        let serialized = serde_json::to_vec(&parsed).expect("serialize extended status");
        let parsed: GatewayStatusResponse =
            serde_json::from_slice(&serialized).expect("round-trip extended status");
        assert_eq!(
            parsed.backend_class,
            Some(super::BackendClass::ConsoleProjection)
        );
        let acceptance = parsed.worker_acceptance.expect("acceptance summary");
        assert_eq!(acceptance.workers[0].lifecycle, "ready");
        assert_eq!(acceptance.workers[0].slot, 1);
        assert_eq!(acceptance.target_session.manifest_sha256, "c".repeat(64));
        assert_eq!(acceptance.execution_proof, "qemu");
        assert_eq!(
            acceptance.workers[0].scheduling_context,
            super::WorkerSchedulingContextSummary {
                budget_us: 0,
                period_us: 0,
            }
        );
    }

    fn delegated_fixture() -> String {
        use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer};
        TicketIssuer::new("test-issuer-key")
            .issue(TicketClaims::new(
                Role::Queen,
                BudgetSpec::unbounded(),
                None,
                MountSpec::empty(),
                0,
            ))
            .expect("issue fixture")
            .encode()
            .expect("encode fixture")
    }

    #[test]
    fn credential_files_resolve_for_reads_and_writes_and_fail_closed_after_removal() {
        let root = tempfile::tempdir().expect("private credentials");
        let path = root.path().join("delegation");
        let ticket = delegated_fixture();
        std::fs::write(&path, format!("{ticket}\n")).expect("credential file");
        let reference = format!("file:{}", path.display());
        for write in [false, true] {
            let (base_url, request_rx, server) = serve_once(
                "200 OK",
                r#"{"status":"OK","verb":"ECHO","path":"/host/tickets/spec","end":true,"bytes":1,"lines":[]}"#,
            );
            let client = GatewayClient::new(base_url)
                .with_request_auth_token("test-token")
                .with_delegated_ticket(&reference);
            if write {
                client
                    .echo("/host/tickets/spec", "x")
                    .expect("file-authenticated write");
            } else {
                client.list("/").expect("file-authenticated read");
            }
            let request = request_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("capture request");
            server.join().expect("server exits");
            assert!(request
                .to_ascii_lowercase()
                .contains(&format!("\r\nx-cohesix-ticket: {ticket}\r\n")));
            assert!(!request.contains(&reference));
        }
        let client = GatewayClient::new("http://127.0.0.1:1")
            .with_request_auth_token("test-token")
            .with_delegated_ticket(&reference);
        std::fs::remove_file(&path).expect("credential rotation/removal");
        assert!(client.resolved_delegation().is_err());
        assert!(client.validate_write_credentials().is_err());
    }

    #[test]
    fn external_deadline_caps_the_whole_request_without_delivery_grace() {
        let timeout = Duration::from_millis(1500);
        let client = GatewayClient::new("http://127.0.0.1:1")
            .with_operation_deadline(timeout)
            .expect("relay deadline");
        assert_eq!(
            client.operation_agent.config().timeouts().global,
            Some(timeout)
        );
        assert_eq!(client.operation_response_timeout(), timeout);
        assert!(GatewayClient::new("http://127.0.0.1:1")
            .with_operation_deadline(Duration::ZERO)
            .is_err());
    }

    #[test]
    fn read_requests_carry_the_same_delegated_identity_as_mutations() {
        let (base_url, request_rx, server) = serve_once(
            "200 OK",
            r#"{"status":"OK","verb":"LS","path":"/","end":true,"lines":[]}"#,
        );
        GatewayClient::new(base_url)
            .with_request_auth_token("test-token")
            .with_delegated_ticket(delegated_fixture())
            .list("/")
            .expect("bounded read");
        let request = request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("capture");
        server.join().expect("loopback server exits");
        assert!(request.to_ascii_lowercase().contains(&format!(
            "\r\nx-cohesix-ticket: {}\r\n",
            delegated_fixture()
        )));
    }

    #[test]
    fn ureq3_echo_sends_trimmed_auth_headers_and_json_body() {
        let (base_url, request_rx, server) = serve_once(
            "200 OK",
            r#"{"status":"OK","verb":"ECHO","path":"/queen/ctl","end":true,"bytes":2}"#,
        );
        let bytes = GatewayClient::new(base_url)
            .with_request_auth_token("  test-token  ")
            .with_delegated_ticket(delegated_fixture())
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
        assert!(lowercase.contains(&format!(
            "\r\nx-cohesix-ticket: {}\r\n",
            delegated_fixture()
        )));
        let (_, body) = request
            .split_once("\r\n\r\n")
            .expect("captured request contains a body");
        let body: serde_json::Value = serde_json::from_str(body).expect("valid request JSON");
        assert_eq!(body["path"], "/queen/ctl");
        assert_eq!(body["line"], "go");
    }

    #[test]
    fn echo_batch_sends_one_authenticated_bounded_ordered_request() {
        let (base_url, request_rx, server) = serve_once(
            "200 OK",
            r#"{"status":"OK","verb":"ECHO_BATCH","path":"/host/tickets/status","end":true,"bytes":6}"#,
        );
        let lines = vec!["one".to_owned(), "two".to_owned()];
        let written = GatewayClient::new(base_url)
            .with_request_auth_token("test-token")
            .with_delegated_ticket(delegated_fixture())
            .echo_batch("/host/tickets/status", lines.as_slice())
            .expect("authenticated echo batch succeeds");
        assert_eq!(written, 2);

        let request = request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("capture authenticated batch request");
        server.join().expect("loopback server exits");
        let lowercase = request.to_ascii_lowercase();
        assert!(lowercase.starts_with("post /v1/fs/echo-batch http/1.1\r\n"));
        assert!(lowercase.contains("\r\nauthorization: bearer test-token\r\n"));
        let (_, body) = request
            .split_once("\r\n\r\n")
            .expect("captured request contains a body");
        let body: serde_json::Value = serde_json::from_str(body).expect("valid request JSON");
        assert_eq!(body["path"], "/host/tickets/status");
        assert_eq!(body["lines"], serde_json::json!(["one", "two"]));
    }

    #[test]
    fn echo_batch_rejects_empty_and_oversized_inputs_before_network_io() {
        let client = GatewayClient::new("http://127.0.0.1:1");
        assert!(client.echo_batch("/host/tickets/status", &[]).is_err());
        let oversized = vec![String::new(); COHESIX_TRANSPORT_COMMAND_BATCH_MAX + 1];
        assert!(client
            .echo_batch("/host/tickets/status", oversized.as_slice())
            .is_err());
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

    #[test]
    fn selected_job_submission_uses_one_authenticated_request_and_keeps_identity() {
        let (base_url, request_rx, server) = serve_once(
            "202 Accepted",
            r#"{"schema":"cohesix-selected-job-response/v1","submission":"target_write_ack"}"#,
        );
        let response = GatewayClient::new(base_url)
            .with_request_auth_token("test-auth")
            .with_delegated_ticket(delegated_fixture())
            .submit_selected_job(&serde_json::json!({
                "binding":{"admission_id":"admit-1"},
                "ticket":{"id":"ticket-1"}
            }))
            .expect("selected response");
        assert_eq!(response["submission"], "target_write_ack");
        let request = request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("capture");
        server.join().expect("one request");
        assert!(request.starts_with("POST /v1/jobs HTTP/1.1\r\n"));
        let body = request.split_once("\r\n\r\n").expect("HTTP body").1;
        let submitted: serde_json::Value = serde_json::from_str(body).expect("JSON request");
        assert_eq!(submitted["binding"]["admission_id"], "admit-1");
        assert!(GatewayClient::new("http://127.0.0.1:1")
            .selected_job_status("../job")
            .is_err());
    }

    #[test]
    fn approved_start_sends_only_scope_and_stable_request_identity() {
        let (base_url, request_rx, server) = serve_once(
            "202 Accepted",
            r#"{"schema":"cohesix-selected-job-response/v1","submission":"target_write_ack"}"#,
        );
        let result = GatewayClient::new(base_url)
            .with_request_auth_token("test-auth")
            .with_delegated_ticket(delegated_fixture())
            .start_approved_job("service-1", "run-123")
            .expect("approved start response");
        assert_eq!(result["submission"], "target_write_ack");
        let request = request_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        server.join().unwrap();
        assert!(request.starts_with("POST /v1/jobs/approved/service-1/start HTTP/1.1\r\n"));
        let body = request.split_once("\r\n\r\n").unwrap().1;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap(),
            serde_json::json!({"request_id":"run-123"})
        );
        assert!(GatewayClient::new("http://127.0.0.1:1")
            .start_approved_job("service-1", "../escape")
            .is_err());
    }

    #[test]
    fn available_scopes_use_delegated_read_without_a_write() {
        let (base_url, request_rx, server) = serve_once(
            "200 OK",
            r#"{"schema":"cohesix-available-scopes/v1","scopes":[]}"#,
        );
        let response = GatewayClient::new(base_url)
            .with_request_auth_token("test-auth")
            .with_delegated_ticket(delegated_fixture())
            .available_standing_scopes()
            .expect("available scopes");
        assert_eq!(response["schema"], "cohesix-available-scopes/v1");
        let request = request_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        server.join().unwrap();
        assert!(request.starts_with("GET /v1/standing/scopes/available HTTP/1.1\r\n"));
    }
}
