// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide helpers for /queen/ctl JSON writes.
// Author: Lukas Bower

//! Helpers for constructing `/queen/ctl` JSON payloads.

use anyhow::{anyhow, Result};

/// Return the manifest-derived queen control path.
#[must_use]
pub fn queen_ctl_path() -> &'static str {
    crate::generated_client::CLIENT_QUEEN_CTL_PATH
}

/// Build a spawn payload using the existing CLI argument semantics.
pub fn spawn<'a>(role: &str, args: impl Iterator<Item = &'a str>) -> Result<String> {
    let payload = crate::build_spawn_payload(role, args)?;
    crate::normalise_payload(&payload)
}

/// Build a kill payload for the supplied worker identifier.
pub fn kill(worker_id: &str) -> Result<String> {
    let ident = crate::ensure_json_string(worker_id, "worker id")?;
    let payload = format!("{{\"kill\":\"{ident}\"}}");
    crate::normalise_payload(&payload)
}

/// Build a budget update payload.
pub fn budget(ttl_s: Option<u64>, ops: Option<u64>) -> Result<String> {
    if ttl_s.is_none() && ops.is_none() {
        return Err(anyhow!("budget requires ttl_s or ops"));
    }
    let mut payload = String::from("{\"budget\":{");
    let mut wrote = false;
    if let Some(ttl_s) = ttl_s {
        payload.push_str(&format!("\"ttl_s\":{ttl_s}"));
        wrote = true;
    }
    if let Some(ops) = ops {
        if wrote {
            payload.push(',');
        }
        payload.push_str(&format!("\"ops\":{ops}"));
    }
    payload.push_str("}}");
    crate::normalise_payload(&payload)
}
