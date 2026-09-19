// Author: Lukas Bower
// Purpose: Exercise immutable-source Xcode build, test and archive through the production adapter.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![cfg(target_os = "macos")]

use cohesix_authority::mac_release::{Operation, Source, Target};
use host_sidecar_bridge::mac_release::{artifact_digest, execute};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires the documented owned Xcode conformance source"]
fn owned_xcode_release_results() -> anyhow::Result<()> {
    let root = PathBuf::from(std::env::var("COHESIX_MACOS_RELEASE_REFERENCE")?);
    let output = PathBuf::from(std::env::var("COHESIX_MACOS_RELEASE_RESULTS")?);
    let source = Source {
        tree_sha256: artifact_digest(&root)?,
        root,
        project: "CohesixNativeReference.xcodeproj".into(),
        scheme: "NativeReference".into(),
    };
    for operation in [
        Operation::Build {
            source: source.clone(),
        },
        Operation::Test {
            source: source.clone(),
        },
        Operation::Archive { source },
    ] {
        let action = operation.action().to_owned();
        let target = Target {
            id: "native-reference".into(),
            operation,
        };
        let result = execute(
            &target,
            &output.join(&action),
            Instant::now() + Duration::from_secs(180),
        )?;
        println!("{}", serde_json::json!({"action":action,"result":result}));
        assert!(execute(
            &target,
            &output.join(&action),
            Instant::now() + Duration::from_secs(1)
        )
        .is_err());
    }
    Ok(())
}
