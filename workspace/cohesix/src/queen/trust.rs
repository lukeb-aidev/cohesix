// CLASSIFICATION: COMMUNITY
// Filename: trust.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Simple trust zone escalation logic for workers.
use std::fs;
use std::future::Future;
use std::path::Path;
use tokio::runtime::Runtime;

use crate::orchestrator::protocol::{ClusterStateRequest, TrustUpdateRequest};
use crate::queen::orchestrator::QueenOrchestrator;
use crate::{new_err, CohError};

pub fn record_failure(worker: &str) {
    let base = Path::new("/srv/trust_zones");
    fs::create_dir_all(base).ok();
    let cnt_path = base.join(format!("{worker}.fails"));
    let mut count = fs::read_to_string(&cnt_path)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0u32);
    count += 1;
    fs::write(&cnt_path, count.to_string()).ok();
    if count >= 2 {
        escalate(worker, "yellow");
    }
}

pub fn escalate(worker: &str, level: &str) {
    let worker_key = worker.to_string();
    let level_value = level.to_string();
    if let Err(err) = orchestrator_call(async move {
        let mut client = QueenOrchestrator::connect_default_client().await?;
        client
            .update_trust(TrustUpdateRequest {
                worker_id: worker_key,
                level: level_value,
            })
            .await
            .map_err(|e| new_err(format!("trust update failed: {e}")))?;
        Ok(())
    }) {
        eprintln!("trust escalation via gRPC failed for {worker}: {err}");
    }
    let base = Path::new("/srv/trust_zones");
    fs::create_dir_all(base).ok();
    fs::write(base.join(worker), level).ok();
}

pub fn get_trust(worker: &str) -> String {
    if let Ok(state) = orchestrator_call(fetch_cluster_state()) {
        if let Some(entry) = state.workers.into_iter().find(|w| w.worker_id == worker) {
            return entry.trust;
        }
    }
    fs::read_to_string(format!("/srv/trust_zones/{worker}"))
        .unwrap_or_else(|_| "green".into())
        .trim()
        .into()
}

pub fn list_trust() -> Vec<(String, String)> {
    if let Ok(state) = orchestrator_call(fetch_cluster_state()) {
        return state
            .workers
            .into_iter()
            .map(|w| {
                (
                    w.worker_id,
                    if w.trust.is_empty() {
                        "green".into()
                    } else {
                        w.trust
                    },
                )
            })
            .collect();
    }
    let base = Path::new("/srv/trust_zones");
    if let Ok(entries) = fs::read_dir(base) {
        return entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let w = e.file_name().into_string().ok()?;
                let level = fs::read_to_string(e.path()).ok()?;
                Some((w, level.trim().into()))
            })
            .collect();
    }
    Vec::new()
}

fn orchestrator_call<F, T>(future: F) -> Result<T, CohError>
where
    F: Future<Output = Result<T, CohError>> + Send + 'static,
    T: Send + 'static,
{
    Runtime::new()
        .map_err(|e| new_err(format!("failed to start tokio runtime: {e}")))?
        .block_on(future)
}

async fn fetch_cluster_state(
) -> Result<crate::orchestrator::protocol::ClusterStateResponse, CohError> {
    let mut client = QueenOrchestrator::connect_default_client().await?;
    let response = client
        .get_cluster_state(ClusterStateRequest {})
        .await
        .map_err(|e| new_err(format!("cluster state request failed: {e}")))?;
    Ok(response.into_inner())
}
