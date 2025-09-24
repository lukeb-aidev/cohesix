// CLASSIFICATION: COMMUNITY
// Filename: worker.rs v1.0
// Author: Lukas Bower
// Date Modified: 2029-01-15
#![cfg(feature = "std")]

/// Worker-side orchestration logic built on the gRPC APIs.
use crate::orchestrator::protocol::{
    AssignRoleRequest, ClusterStateRequest, GpuTelemetry, HeartbeatRequest, HeartbeatResponse,
    JoinRequest, JoinResponse, OrchestratorServiceClient, ScheduleRequest, ScheduleResponse,
    TrustUpdateRequest, TrustUpdateResponse, DEFAULT_ENDPOINT,
};
use crate::queen::orchestrator::endpoint_from_env;
use crate::{new_err, CohError};
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use std::time::{SystemTime, UNIX_EPOCH};
use tonic::transport::Channel;

/// Basic worker orchestrator helper that wraps the gRPC client.
#[derive(Clone)]
pub struct WorkerClient {
    id: String,
    client: OrchestratorServiceClient<Channel>,
}

impl WorkerClient {
    /// Connect to the orchestrator and identify this worker.
    pub async fn connect(id: &str, endpoint: Option<&str>) -> Result<Self, CohError> {
        let target = endpoint
            .map(|s| s.to_string())
            .unwrap_or_else(|| endpoint_from_env());
        let client = OrchestratorServiceClient::connect(target.clone())
            .await
            .map_err(|e| {
                new_err(format!(
                    "failed to connect to orchestrator at {target}: {e}"
                ))
            })?;
        Ok(Self {
            id: id.into(),
            client,
        })
    }

    /// Register this worker with the queen.
    pub async fn join(
        &mut self,
        ip: &str,
        role: &str,
        capabilities: Vec<String>,
        trust: &str,
    ) -> Result<JoinResponse, CohError> {
        let request = JoinRequest {
            worker_id: self.id.clone(),
            ip: ip.into(),
            role: role.into(),
            capabilities,
            trust: trust.into(),
        };
        self.client
            .join(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|e| new_err(format!("join failed: {e}")))
    }

    /// Send a heartbeat to the queen with optional GPU telemetry.
    pub async fn heartbeat(
        &mut self,
        status: &str,
        telemetry: Option<GpuTelemetry>,
    ) -> Result<HeartbeatResponse, CohError> {
        let request = HeartbeatRequest {
            worker_id: self.id.clone(),
            timestamp: timestamp(),
            status: status.into(),
            gpu: telemetry,
        };
        self.client
            .heartbeat(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|e| new_err(format!("heartbeat failed: {e}")))
    }

    /// Request a scheduling decision for an agent.
    pub async fn request_schedule(
        &mut self,
        agent_id: &str,
        require_gpu: bool,
    ) -> Result<ScheduleResponse, CohError> {
        let request = ScheduleRequest {
            agent_id: agent_id.into(),
            require_gpu,
        };
        self.client
            .request_schedule(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|e| new_err(format!("schedule request failed: {e}")))
    }

    /// Update the trust level for this worker.
    pub async fn update_trust(&mut self, level: &str) -> Result<TrustUpdateResponse, CohError> {
        let request = TrustUpdateRequest {
            worker_id: self.id.clone(),
            level: level.into(),
        };
        self.client
            .update_trust(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|e| new_err(format!("trust update failed: {e}")))
    }

    /// Request the current cluster state for diagnostics.
    pub async fn cluster_state(&mut self) -> Result<Vec<String>, CohError> {
        let response = self
            .client
            .get_cluster_state(ClusterStateRequest {})
            .await
            .map_err(|e| new_err(format!("cluster state request failed: {e}")))?
            .into_inner();
        Ok(response.workers.into_iter().map(|w| w.worker_id).collect())
    }

    /// Assign a new role to a worker.
    pub async fn assign_role(&mut self, worker_id: &str, role: &str) -> Result<bool, CohError> {
        let response = self
            .client
            .assign_role(AssignRoleRequest {
                worker_id: worker_id.into(),
                role: role.into(),
            })
            .await
            .map_err(|e| new_err(format!("assign role failed: {e}")))?
            .into_inner();
        Ok(response.updated)
    }
}

impl WorkerClient {
    /// Build GPU telemetry helper for convenience.
    pub fn gpu_telemetry(
        perf_watt: f32,
        mem_total: u64,
        mem_free: u64,
        last_temp: u32,
        gpu_capacity: u32,
        current_load: u32,
        latency_score: u32,
    ) -> GpuTelemetry {
        GpuTelemetry {
            perf_watt,
            mem_total,
            mem_free,
            last_temp,
            gpu_capacity,
            current_load,
            latency_score,
        }
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Provide a default endpoint for worker connections if the queen URL is unspecified.
pub fn default_endpoint() -> &'static str {
    DEFAULT_ENDPOINT
}
