// Author: Lukas Bower
// Purpose: Exercise native D-Bus lifecycle and invocation postconditions on an explicitly provisioned disposable service.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use anyhow::{bail, Result};
use host_sidecar_bridge::native::{
    dispatch_systemd, observe_systemd_before, systemd_postcondition,
};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires a separately provisioned disposable Linux systemd conformance service"]
fn native_systemd_lifecycle() -> Result<()> {
    let unit = std::env::var("COHESIX_SYSTEMD_CANARY_UNIT")?;
    if !unit.starts_with("cohesix-conformance-m27b-") || !unit.ends_with(".service") {
        bail!("conformance refuses a non-disposable service name");
    }
    for action in ["restart", "stop", "start", "stop"] {
        let deadline = Instant::now() + Duration::from_secs(15);
        let before = observe_systemd_before(&unit, deadline)?;
        let job = dispatch_systemd(&unit, action, deadline)?;
        loop {
            let after = observe_systemd_before(&unit, deadline)?;
            if systemd_postcondition(action, &before, &after) {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "schema": "cohesix-systemd-conformance-observation/v1",
                        "action": action, "job": job, "before": before, "after": after,
                        "proof_class": "native_provider_operation", "worker_proof": false,
                    }))?
                );
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("native postcondition not reached for {action}");
            }
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }
    Ok(())
}
