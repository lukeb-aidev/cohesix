// Author: Lukas Bower
// Purpose: Verify local Engine lifecycle events and immutable IDs on a separately provisioned disposable container.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{bail, Result};
use host_sidecar_bridge::docker::DockerEngine;
use std::path::Path;
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires a separately provisioned disposable Engine container and authorized socket"]
fn native_docker_lifecycle() -> Result<()> {
    let name = std::env::var("COHESIX_DOCKER_CANARY")?;
    if !name.starts_with("cohesix-conformance-m27b-") {
        bail!("conformance refuses non-disposable container name");
    }
    for action in ["restart", "stop"] {
        let engine = DockerEngine::connect(
            Path::new("/var/run/docker.sock"),
            Instant::now() + Duration::from_secs(20),
        )?;
        let result = engine.transition(&name, action)?;
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "schema": "cohesix-docker-conformance-observation/v1",
                "proof_class": "native_provider_operation", "worker_proof": false,
                "observation": result,
            }))?
        );
    }
    Ok(())
}
