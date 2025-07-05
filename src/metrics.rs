// CLASSIFICATION: COMMUNITY
// Filename: metrics.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

use crate::telemetry::core::GpuTelemetry;
use cohesix_9p::fs::global_fs;
use std::fs;
use serde_json::json;

pub fn update(gpu: &GpuTelemetry, queue_depth: usize, active_jobs: usize) {
    let used = gpu.mem_total.saturating_sub(gpu.mem_free);
    let payload = json!({
        "gpu_memory_used": used,
        "active_jobs": active_jobs,
        "secure9p_sessions": 0,
        "last_error": gpu.fallback_reason,
    });
    let data = payload.to_string();
    global_fs().update_metrics(data.as_bytes());
    let _ = fs::write("/metrics", data);
}
