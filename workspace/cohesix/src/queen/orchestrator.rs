// CLASSIFICATION: COMMUNITY
// Filename: orchestrator.rs v1.0
// Author: Lukas Bower
// Date Modified: 2029-01-15

/// Queen orchestrator for managing worker nodes over gRPC.
use crate::orchestrator::protocol::{
    AssignRoleRequest, AssignRoleResponse, ClusterStateRequest, ClusterStateResponse, GpuTelemetry,
    HeartbeatRequest, HeartbeatResponse, JoinRequest, JoinResponse, OrchestratorService,
    OrchestratorServiceClient, OrchestratorServiceServer, ScheduleRequest, ScheduleResponse,
    TrustUpdateRequest, TrustUpdateResponse, WorkerStatus, DEFAULT_ENDPOINT,
};
use crate::trace::recorder;
use crate::{new_err, CohError};
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};
use ureq::Agent as HttpAgent;

/// Record of a worker node registered with the queen.
#[derive(Clone, Debug)]
pub struct WorkerRecord {
    pub id: String,
    pub ip: String,
    pub status: String,
    pub boot_ts: u64,
    pub last_seen: u64,
    pub role: String,
    pub trust: String,
    pub capabilities: Vec<String>,
}

/// GPU-aware metadata for scheduling decisions.
#[derive(Clone, Debug, Default)]
pub struct GpuNode {
    pub perf_watt: f32,
    pub mem_total: u64,
    pub mem_free: u64,
    pub last_temp: u32,
    pub gpu_capacity: u32,
    pub current_load: u32,
    pub latency_score: u32,
    pub jobs: Vec<String>,
}

struct OrchestratorState {
    workers: HashMap<String, WorkerRecord>,
    gpu_nodes: HashMap<String, GpuNode>,
    next_idx: usize,
    policy: SchedulePolicy,
}

struct InnerState {
    state: RwLock<OrchestratorState>,
    timeout: Duration,
    queen_id: String,
}

/// Queen orchestrator state shared between the gRPC server and local helpers.
#[derive(Clone)]
pub struct QueenOrchestrator {
    inner: Arc<InnerState>,
}

#[derive(Clone)]
struct GrpcOrchestrator {
    orchestrator: QueenOrchestrator,
}

#[derive(Clone, Copy, Debug)]
pub enum SchedulePolicy {
    RoundRobin,
    GpuPriority,
    LatencyAware,
}

impl QueenOrchestrator {
    /// Initialize the orchestrator with a heartbeat timeout and scheduling policy.
    pub fn new(timeout_secs: u64, policy: SchedulePolicy) -> Self {
        let inner = InnerState {
            state: RwLock::new(OrchestratorState {
                workers: HashMap::new(),
                gpu_nodes: HashMap::new(),
                next_idx: 0,
                policy,
            }),
            timeout: Duration::from_secs(timeout_secs),
            queen_id: env::var("COHESIX_QUEEN_ID").unwrap_or_else(|_| "cohesix-queen".into()),
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Return the identifier of the queen instance hosting this orchestrator.
    pub fn queen_id(&self) -> String {
        self.inner.queen_id.clone()
    }

    /// Return the configured heartbeat timeout.
    pub fn timeout(&self) -> Duration {
        self.inner.timeout
    }

    /// Provide a gRPC service implementation bound to this orchestrator state.
    fn into_service(self) -> OrchestratorServiceServer<GrpcOrchestrator> {
        OrchestratorServiceServer::new(GrpcOrchestrator { orchestrator: self })
    }

    /// Bind and serve the gRPC API on the provided address.
    pub async fn serve(self, addr: SocketAddr) -> Result<(), CohError> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| new_err(format!("failed to bind orchestrator listener: {e}")))?;
        self.serve_with_listener(listener, async {}).await
    }

    /// Serve using an existing listener with shutdown support.
    pub async fn serve_with_listener<S>(
        self,
        listener: TcpListener,
        shutdown: S,
    ) -> Result<(), CohError>
    where
        S: Future<Output = ()> + Send + 'static,
    {
        let service = self.clone().into_service();
        Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown)
            .await
            .map_err(|e| new_err(format!("orchestrator server error: {e}")))
    }

    /// Convenience helper for creating a gRPC client using the default endpoint.
    pub async fn connect_default_client() -> Result<OrchestratorServiceClient<Channel>, CohError> {
        Self::connect_client(&endpoint_from_env()).await
    }

    /// Create a gRPC client targeting a custom endpoint.
    pub async fn connect_client(
        endpoint: &str,
    ) -> Result<OrchestratorServiceClient<Channel>, CohError> {
        OrchestratorServiceClient::connect(endpoint.to_string())
            .await
            .map_err(|e| {
                new_err(format!(
                    "failed to connect to orchestrator at {endpoint}: {e}"
                ))
            })
    }

    async fn handle_join(&self, req: JoinRequest) -> Result<JoinResponse, Status> {
        let (boot_ts, active_snapshot, gpu_snapshot) = {
            let mut state = self.inner.state.write().await;
            let now = timestamp();
            let has_cuda;
            let boot_time;
            {
                let entry = state
                    .workers
                    .entry(req.worker_id.clone())
                    .or_insert(WorkerRecord {
                        id: req.worker_id.clone(),
                        ip: req.ip.clone(),
                        status: "booting".into(),
                        boot_ts: now,
                        last_seen: now,
                        role: if req.role.is_empty() {
                            "unknown".into()
                        } else {
                            req.role.clone()
                        },
                        trust: if req.trust.is_empty() {
                            "green".into()
                        } else {
                            req.trust.clone()
                        },
                        capabilities: req.capabilities.clone(),
                    });
                entry.ip = req.ip.clone();
                if !req.role.is_empty() {
                    entry.role = req.role.clone();
                }
                if !req.trust.is_empty() {
                    entry.trust = req.trust.clone();
                }
                entry.capabilities = req.capabilities.clone();
                entry.status = "booting".into();
                entry.boot_ts = entry.boot_ts.min(now);
                entry.last_seen = now;
                has_cuda = entry
                    .capabilities
                    .iter()
                    .any(|cap| cap.eq_ignore_ascii_case("cuda"));
                boot_time = entry.boot_ts;
            }
            if has_cuda {
                state
                    .gpu_nodes
                    .entry(req.worker_id.clone())
                    .or_insert_with(GpuNode::default);
            }

            (boot_time, snapshot_active(&state), snapshot_gpu(&state))
        };

        self.write_active_registry(active_snapshot)
            .await
            .map_err(to_status)?;
        self.write_gpu_registry(gpu_snapshot)
            .await
            .map_err(to_status)?;

        Ok(JoinResponse {
            queen_id: self.queen_id(),
            boot_timestamp: boot_ts,
        })
    }

    async fn handle_heartbeat(&self, req: HeartbeatRequest) -> Result<HeartbeatResponse, Status> {
        let HeartbeatRequest {
            worker_id,
            timestamp: hb_timestamp,
            status,
            gpu,
        } = req;
        let (trust_level, active_snapshot, gpu_snapshot) = {
            let mut state = self.inner.state.write().await;
            let trust_level = match state.workers.get_mut(&worker_id) {
                Some(record) => {
                    let ts = if hb_timestamp > 0 {
                        hb_timestamp
                    } else {
                        timestamp()
                    };
                    record.last_seen = ts;
                    if !status.is_empty() {
                        record.status = status.clone();
                    }
                    record.trust.clone()
                }
                None => {
                    return Err(Status::not_found(format!(
                        "worker {} not registered",
                        worker_id
                    )));
                }
            };
            if let Some(ref telemetry) = gpu {
                let node = state
                    .gpu_nodes
                    .entry(worker_id.clone())
                    .or_insert_with(GpuNode::default);
                node.perf_watt = telemetry.perf_watt;
                node.mem_total = telemetry.mem_total;
                node.mem_free = telemetry.mem_free;
                node.last_temp = telemetry.last_temp;
                node.gpu_capacity = telemetry.gpu_capacity;
                node.current_load = telemetry.current_load;
                node.latency_score = telemetry.latency_score;
            }
            (
                trust_level,
                Some(snapshot_active(&state)),
                Some(snapshot_gpu(&state)),
            )
        };

        if let Some(snapshot) = active_snapshot {
            self.write_active_registry(snapshot)
                .await
                .map_err(to_status)?;
        }
        if let Some(snapshot) = gpu_snapshot {
            self.write_gpu_registry(snapshot).await.map_err(to_status)?;
        }

        Ok(HeartbeatResponse {
            acknowledged: true,
            trust_level,
        })
    }

    async fn handle_schedule(&self, req: ScheduleRequest) -> Result<ScheduleResponse, Status> {
        self.ensure_timeouts().await.map_err(to_status)?;
        let (assignment, policy, snapshot) = {
            let mut state = self.inner.state.write().await;
            let selected = select_worker(&mut state, &req.agent_id, req.require_gpu);
            (selected, state.policy, snapshot_active(&state))
        };
        self.write_active_registry(snapshot)
            .await
            .map_err(to_status)?;

        if let Some(worker_id) = assignment.clone() {
            recorder::event(
                "orchestrator",
                "schedule",
                &format!("{} -> {}", req.agent_id, worker_id),
            );
            Ok(ScheduleResponse {
                assigned: true,
                worker_id,
                scheduling_policy: format!("{:?}", policy),
            })
        } else {
            Ok(ScheduleResponse {
                assigned: false,
                worker_id: String::new(),
                scheduling_policy: format!("{:?}", policy),
            })
        }
    }

    async fn handle_trust_update(
        &self,
        req: TrustUpdateRequest,
    ) -> Result<TrustUpdateResponse, Status> {
        let snapshot = {
            let mut state = self.inner.state.write().await;
            if let Some(record) = state.workers.get_mut(&req.worker_id) {
                record.trust = req.level.clone();
                Some(snapshot_active(&state))
            } else {
                None
            }
        };
        if let Some(snapshot) = snapshot {
            self.write_active_registry(snapshot)
                .await
                .map_err(to_status)?;
            Ok(TrustUpdateResponse { level: req.level })
        } else {
            Err(Status::not_found(format!(
                "worker {} not registered",
                req.worker_id
            )))
        }
    }

    async fn handle_assign_role(
        &self,
        req: AssignRoleRequest,
    ) -> Result<AssignRoleResponse, Status> {
        let snapshot = {
            let mut state = self.inner.state.write().await;
            if let Some(record) = state.workers.get_mut(&req.worker_id) {
                record.role = req.role.clone();
                Some(snapshot_active(&state))
            } else {
                None
            }
        };
        if let Some(snapshot) = snapshot {
            self.write_active_registry(snapshot)
                .await
                .map_err(to_status)?;
            Ok(AssignRoleResponse { updated: true })
        } else {
            Err(Status::not_found(format!(
                "worker {} not registered",
                req.worker_id
            )))
        }
    }

    async fn handle_cluster_state(&self) -> Result<ClusterStateResponse, Status> {
        self.ensure_timeouts().await.map_err(to_status)?;
        let (queen_id, timeout, workers) = {
            let state = self.inner.state.read().await;
            let statuses = state
                .workers
                .values()
                .map(|record| to_worker_status(record, state.gpu_nodes.get(&record.id)))
                .collect::<Vec<_>>();
            (
                self.queen_id(),
                self.inner.timeout.as_secs() as u32,
                statuses,
            )
        };
        Ok(ClusterStateResponse {
            queen_id,
            generated_at: timestamp(),
            timeout_seconds: timeout,
            workers,
        })
    }

    async fn ensure_timeouts(&self) -> Result<(), CohError> {
        let (snapshot, restarts) = {
            let mut state = self.inner.state.write().await;
            let now = timestamp();
            let mut restart_targets = Vec::new();
            for record in state.workers.values_mut() {
                if now.saturating_sub(record.last_seen) > self.inner.timeout.as_secs()
                    && record.status != "restarting"
                {
                    record.status = "restarting".into();
                    restart_targets.push((record.id.clone(), record.ip.clone()));
                    record.last_seen = now;
                }
            }
            (snapshot_active(&state), restart_targets)
        };

        self.write_active_registry(snapshot).await?;
        self.trigger_restarts(restarts).await;
        Ok(())
    }

    async fn trigger_restarts(&self, targets: Vec<(String, String)>) {
        for (worker_id, ip) in targets {
            let url = format!("http://{}/reboot", ip);
            let _ = task::spawn_blocking(move || {
                let _ = HttpAgent::new_with_defaults().post(&url).send_empty();
            })
            .await;
            recorder::event(
                "orchestrator",
                "restart",
                &format!("{} -> {}", worker_id, ip),
            );
        }
    }

    async fn write_active_registry(&self, entries: Vec<serde_json::Value>) -> Result<(), CohError> {
        let json = serde_json::to_string_pretty(&entries)
            .map_err(|e| new_err(format!("failed to encode active registry: {e}")))?;
        task::spawn_blocking(move || -> Result<(), CohError> {
            fs::create_dir_all("/srv/agents")
                .map_err(|e| new_err(format!("failed to create /srv/agents: {e}")))?;
            fs::write("/srv/agents/active.json", &json)
                .map_err(|e| new_err(format!("failed to write active.json: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| new_err(format!("active registry write task failed: {e}")))??;
        Ok(())
    }

    async fn write_gpu_registry(&self, entries: Vec<serde_json::Value>) -> Result<(), CohError> {
        let json = serde_json::to_string_pretty(&entries)
            .map_err(|e| new_err(format!("failed to encode gpu registry: {e}")))?;
        task::spawn_blocking(move || -> Result<(), CohError> {
            fs::write("/srv/gpu_registry.json", &json)
                .map_err(|e| new_err(format!("failed to write gpu_registry.json: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| new_err(format!("gpu registry write task failed: {e}")))??;
        Ok(())
    }

    /// Send a spawn command to a worker. This maintains backwards compatibility with
    /// file-based agent dispatch by writing to `/srv/agents/<worker>/spawn`.
    pub async fn spawn_worker_agent(
        &self,
        worker_id: &str,
        agent: &str,
        args: &[&str],
    ) -> Result<(), CohError> {
        let worker_id = worker_id.to_string();
        let agent = agent.to_string();
        let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        task::spawn_blocking(move || -> Result<(), CohError> {
            let dir = format!("/srv/agents/{worker_id}");
            fs::create_dir_all(&dir)
                .map_err(|e| new_err(format!("failed to create {dir}: {e}")))?;
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(format!("{dir}/spawn"))
                .map_err(|e| new_err(format!("failed to open spawn file: {e}")))?;
            writeln!(file, "{} {:?}", agent, argv)
                .map_err(|e| new_err(format!("failed to write spawn command: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| new_err(format!("spawn command task failed: {e}")))??;
        Ok(())
    }
}

#[tonic::async_trait]
impl OrchestratorService for GrpcOrchestrator {
    async fn join(&self, request: Request<JoinRequest>) -> Result<Response<JoinResponse>, Status> {
        let response = self.orchestrator.handle_join(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let response = self
            .orchestrator
            .handle_heartbeat(request.into_inner())
            .await?;
        Ok(Response::new(response))
    }

    async fn request_schedule(
        &self,
        request: Request<ScheduleRequest>,
    ) -> Result<Response<ScheduleResponse>, Status> {
        let response = self
            .orchestrator
            .handle_schedule(request.into_inner())
            .await?;
        Ok(Response::new(response))
    }

    async fn update_trust(
        &self,
        request: Request<TrustUpdateRequest>,
    ) -> Result<Response<TrustUpdateResponse>, Status> {
        let response = self
            .orchestrator
            .handle_trust_update(request.into_inner())
            .await?;
        Ok(Response::new(response))
    }

    async fn assign_role(
        &self,
        request: Request<AssignRoleRequest>,
    ) -> Result<Response<AssignRoleResponse>, Status> {
        let response = self
            .orchestrator
            .handle_assign_role(request.into_inner())
            .await?;
        Ok(Response::new(response))
    }

    async fn get_cluster_state(
        &self,
        _request: Request<ClusterStateRequest>,
    ) -> Result<Response<ClusterStateResponse>, Status> {
        let response = self.orchestrator.handle_cluster_state().await?;
        Ok(Response::new(response))
    }
}

fn select_worker(
    state: &mut OrchestratorState,
    agent_id: &str,
    require_gpu: bool,
) -> Option<String> {
    let candidates: Vec<String> = state
        .workers
        .iter()
        .filter(|(_, record)| record.status != "restarting")
        .filter(|(_, record)| !require_gpu || record.capabilities.iter().any(|cap| cap == "cuda"))
        .map(|(id, _)| id.clone())
        .collect();
    if candidates.is_empty() {
        return None;
    }
    match state.policy {
        SchedulePolicy::RoundRobin => {
            let idx = state.next_idx % candidates.len();
            state.next_idx = state.next_idx.wrapping_add(1);
            candidates.get(idx).cloned()
        }
        SchedulePolicy::GpuPriority => select_gpu_priority(state, agent_id, candidates),
        SchedulePolicy::LatencyAware => select_low_latency(state, candidates),
    }
}

fn select_gpu_priority(
    state: &mut OrchestratorState,
    agent_id: &str,
    candidates: Vec<String>,
) -> Option<String> {
    let mut best: Option<(String, f32)> = None;
    for id in candidates {
        if let Some(node) = state.gpu_nodes.get(&id) {
            if best
                .as_ref()
                .map(|(_, weight)| node.perf_watt > *weight)
                .unwrap_or(true)
            {
                best = Some((id.clone(), node.perf_watt));
            }
        } else if best.is_none() {
            best = Some((id.clone(), 0.0));
        }
    }
    if let Some((selected, _)) = best {
        if let Some(node) = state.gpu_nodes.get_mut(&selected) {
            node.jobs.push(agent_id.into());
        }
        Some(selected)
    } else {
        None
    }
}

fn select_low_latency(state: &OrchestratorState, candidates: Vec<String>) -> Option<String> {
    let mut best: Option<(String, u32)> = None;
    for id in candidates {
        let latency = state
            .gpu_nodes
            .get(&id)
            .map(|node| node.latency_score)
            .unwrap_or(u32::MAX);
        if best
            .as_ref()
            .map(|(_, best_latency)| latency < *best_latency)
            .unwrap_or(true)
        {
            best = Some((id.clone(), latency));
        }
    }
    best.map(|(id, _)| id)
}

fn snapshot_active(state: &OrchestratorState) -> Vec<serde_json::Value> {
    state
        .workers
        .values()
        .map(|w| {
            json!({
                "worker_id": w.id,
                "role": w.role,
                "status": w.status,
                "ip": w.ip,
                "trust": w.trust,
                "last_seen": w.last_seen,
            })
        })
        .collect()
}

fn snapshot_gpu(state: &OrchestratorState) -> Vec<serde_json::Value> {
    state
        .gpu_nodes
        .iter()
        .map(|(id, node)| {
            json!({
                "worker_id": id,
                "perf_watt": node.perf_watt,
                "gpu_capacity": node.gpu_capacity,
                "current_load": node.current_load,
                "latency_score": node.latency_score,
                "mem_total": node.mem_total,
                "mem_free": node.mem_free,
            })
        })
        .collect()
}

fn to_worker_status(record: &WorkerRecord, gpu: Option<&GpuNode>) -> WorkerStatus {
    WorkerStatus {
        worker_id: record.id.clone(),
        role: record.role.clone(),
        status: record.status.clone(),
        ip: record.ip.clone(),
        trust: record.trust.clone(),
        boot_ts: record.boot_ts,
        last_seen: record.last_seen,
        capabilities: record.capabilities.clone(),
        gpu: gpu.map(|node| GpuTelemetry {
            perf_watt: node.perf_watt,
            mem_total: node.mem_total,
            mem_free: node.mem_free,
            last_temp: node.last_temp,
            gpu_capacity: node.gpu_capacity,
            current_load: node.current_load,
            latency_score: node.latency_score,
        }),
    }
}

fn to_status(err: CohError) -> Status {
    Status::internal(err.to_string())
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Determine the orchestrator endpoint, prioritising configuration.
pub fn endpoint_from_env() -> String {
    env::var("COHESIX_ORCH_ADDR").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string())
}
