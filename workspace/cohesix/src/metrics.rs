// CLASSIFICATION: COMMUNITY
// Filename: metrics.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-12-31
#![cfg(feature = "std")]

use cohesix_9p::fs::global_fs;
use serde_json::json;
use std::fs;

/// Update metrics with the current secure9p session count.
pub fn update(secure_sessions: usize) {
    let payload = json!({
        "secure9p_sessions": secure_sessions,
    });
    let data = payload.to_string();
    global_fs().update_metrics(data.as_bytes());
    let _ = fs::write("/metrics", data);
}
