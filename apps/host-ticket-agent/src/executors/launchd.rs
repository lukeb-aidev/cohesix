// Author: Lukas Bower
// Purpose: Observe exact configured macOS service incarnations after separately admitted lifecycle changes.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use super::{observation, provider_pending, ExecutorConfig};
use crate::HostTicketSpec;
use anyhow::{anyhow, ensure, Result};
use host_sidecar_bridge::launchd::{dispatch, observe, postcondition, preflight, process};
use std::time::{Duration, Instant};

/// Lifecycle state is terminal only when correlated native process observations agree.
pub fn execute(spec: &HostTicketSpec, config: &ExecutorConfig) -> Result<String> {
    ensure!(
        spec.schema == "host-ticket/v1",
        "not_supported launchd-worker-receipt"
    );
    let service = spec.args["service"]
        .as_str()
        .ok_or_else(|| anyhow!("EPERM launchd-service"))?;
    let target = cohesix_authority::macos::launchd_targets()?
        .into_iter()
        .find(|target| target.id == service)
        .ok_or_else(|| anyhow!("not_enabled launchd-target"))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    preflight(&target, spec.action != "launchd.status-check", deadline)?;
    let before = observe(&target, deadline)?;
    observation::prepare(config)?;
    let no_dispatch = spec.action == "launchd.status-check"
        || (spec.action == "launchd.start" && before.process.is_some())
        || (spec.action == "launchd.stop" && before.process.is_none());
    if !no_dispatch {
        dispatch(&target, &spec.action, deadline)
            .map_err(|e| provider_pending(format!("launchd dispatch outcome ambiguous: {e}")))?;
    }
    loop {
        let after = observe(&target, deadline)
            .map_err(|e| provider_pending(format!("launchd observation unavailable: {e}")))?;
        let old = if spec.action == "launchd.stop" {
            before
                .process
                .as_ref()
                .map(|p| process(p.pid, deadline))
                .transpose()?
        } else {
            None
        };
        if postcondition(&spec.action, &before, &after, old.as_ref()) {
            let native = after.process.as_ref().or(before.process.as_ref());
            let identity = format!(
                "launchd:{}:{}:{}:{}",
                target.label,
                native.map_or(0, |p| p.pid),
                native.and_then(|p| p.start_seconds).unwrap_or(0),
                native.and_then(|p| p.start_microseconds).unwrap_or(0)
            );
            return observation::retain_native(
                config,
                spec,
                &serde_json::json!({
                    "schema":"cohesix-launchd-operation/v1", "target_id":target.id,
                    "before":before, "after":after, "previous_process":old,
                    "dispatch_required":!no_dispatch, "source":"launchctl-and-libproc",
                    "device_attested":false,
                }),
                &identity,
            );
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "unconfirmed launchd-native-postcondition"
        );
        std::thread::sleep(remaining.min(Duration::from_millis(25)));
    }
}
