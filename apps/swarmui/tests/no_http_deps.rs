// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Ensure SwarmUI has no HTTP/REST dependencies.
// Author: Lukas Bower

use std::collections::{HashMap, HashSet};
use std::process::Command;

use serde_json::Value;

#[test]
fn swarmui_has_no_http_deps() {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value =
        serde_json::from_slice(&output.stdout).expect("parse cargo metadata");

    let packages = metadata["packages"]
        .as_array()
        .expect("metadata packages");
    let resolve = metadata["resolve"]["nodes"]
        .as_array()
        .expect("metadata resolve nodes");

    let mut package_name_by_id = HashMap::new();
    for pkg in packages {
        let id = pkg["id"].as_str().expect("package id");
        let name = pkg["name"].as_str().expect("package name");
        package_name_by_id.insert(id.to_owned(), name.to_owned());
    }

    let swarmui_id = package_name_by_id
        .iter()
        .find(|(_, name)| *name == "swarmui")
        .map(|(id, _)| id.to_owned())
        .expect("swarmui package id");

    let mut stack = vec![swarmui_id];
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let node = resolve
            .iter()
            .find(|node| node["id"].as_str() == Some(id.as_str()))
            .expect("resolve node");
        let deps = node["deps"].as_array().expect("resolve deps");
        for dep in deps {
            let dep_id = dep["pkg"].as_str().expect("dep id");
            stack.push(dep_id.to_owned());
        }
    }

    let mut names = HashSet::new();
    for id in seen {
        if let Some(name) = package_name_by_id.get(&id) {
            names.insert(name.to_owned());
        }
    }

    let banned = [
        "actix-web",
        "axum",
        "hyper",
        "hyper-util",
        "isahc",
        "reqwest",
        "reqwest-middleware",
        "rocket",
        "surf",
        "tower-http",
        "ureq",
        "warp",
    ];
    let mut found = Vec::new();
    for name in banned {
        if names.contains(name) {
            found.push(name);
        }
    }

    assert!(
        found.is_empty(),
        "forbidden HTTP dependencies detected: {}",
        found.join(", ")
    );
}
