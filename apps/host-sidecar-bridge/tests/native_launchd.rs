// Author: Lukas Bower
// Purpose: Check one independently owned native launchd service without borrowing discovery output as lifecycle proof.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]

use host_sidecar_bridge::launchd::{dispatch, observe, postcondition, process};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires the documented owned-service launchd conformance fixture"]
fn owned_service_native_lifecycle() -> anyhow::Result<()> {
    let selected = std::env::var("COHESIX_LAUNCHD_CONFORMANCE_TARGET")?;
    let target: cohesix_authority::macos::LaunchdTarget = serde_json::from_str(&selected)?;
    let mut before = observe(&target, Instant::now() + Duration::from_secs(10))?;
    anyhow::ensure!(before.process.is_none(), "fixture must start stopped");
    let mut wrong = target.clone();
    wrong.executable_sha256 = "0".repeat(64);
    assert!(dispatch(
        &wrong,
        "launchd.start",
        Instant::now() + Duration::from_secs(5)
    )
    .unwrap_err()
    .to_string()
    .contains("launchd-executable-identity"));
    assert_eq!(
        before,
        observe(&target, Instant::now() + Duration::from_secs(5))?
    );
    for action in ["launchd.start", "launchd.restart", "launchd.stop"] {
        let deadline = Instant::now() + Duration::from_secs(15);
        dispatch(&target, action, deadline)?;
        loop {
            let after = observe(&target, deadline)?;
            let old = if action == "launchd.stop" {
                before
                    .process
                    .as_ref()
                    .map(|p| process(p.pid, deadline))
                    .transpose()?
            } else {
                None
            };
            if postcondition(action, &before, &after, old.as_ref()) {
                println!(
                    "{}",
                    serde_json::json!({"action":action,"before":before,"after":after,"previous_process":old})
                );
                before = after;
                break;
            }
            anyhow::ensure!(Instant::now() < deadline, "native postcondition deadline");
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    Ok(())
}
