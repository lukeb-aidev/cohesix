// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate SwarmUI manifest dependency policy without nested Cargo metadata work.
// Author: Lukas Bower

use std::fs;
use std::path::{Path, PathBuf};

use toml::Value as TomlValue;

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    manifest_dir()
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

fn manifest() -> TomlValue {
    let manifest_path = manifest_dir().join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path).expect("read SwarmUI manifest");
    toml::from_str(&manifest).expect("parse SwarmUI manifest")
}

#[test]
fn swarmui_rest_projection_is_default_but_feature_scoped() {
    let manifest = manifest();
    let features = manifest["features"].as_table().expect("features table");
    let dependencies = manifest["dependencies"]
        .as_table()
        .expect("dependencies table");
    let cohsh = dependencies["cohsh"].as_table().expect("cohsh dependency");
    let default_features = features["default"]
        .as_array()
        .expect("default features")
        .iter()
        .map(|value| value.as_str().expect("feature name"))
        .collect::<Vec<_>>();
    let rest_features = features["rest"]
        .as_array()
        .expect("rest features")
        .iter()
        .map(|value| value.as_str().expect("feature name"))
        .collect::<Vec<_>>();

    assert!(default_features.contains(&"offline-cache"));
    assert!(
        default_features.contains(&"rest"),
        "docs describe REST as enabled by default"
    );
    assert_eq!(
        rest_features,
        vec!["cohsh/rest", "dep:cohesix-rest"],
        "REST projection and status schema must stay feature-scoped"
    );
    assert_eq!(
        cohsh["default-features"].as_bool(),
        Some(false),
        "SwarmUI --no-default-features must strip cohsh REST defaults"
    );
}

#[test]
fn transitive_dependency_policy_lives_in_test_plan_script() {
    let script = repo_root().join("scripts/ci/check_swarmui_dependencies.py");
    assert!(
        script.is_file(),
        "SwarmUI transitive dependency policy must run outside cargo test"
    );
}
