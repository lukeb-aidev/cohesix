// Author: Lukas Bower
// Purpose: Pin accepted historical causal evidence for offline reference playback.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
//! Embedded files retain their canonical graph, independently enrolled trust and CAS formats.
use crate::workbench::{run_host, HostRequest};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FILES: &[(&str, &[u8])] = &[
    ("cuda/cas/008707f99b2b378e47880d94784b6c50c7ffbc50991a00ea632953564396b946", include_bytes!("../reference/cuda/cas/008707f99b2b378e47880d94784b6c50c7ffbc50991a00ea632953564396b946")),
    ("cuda/cas/1967d8df4564848b45f662fd1b6b2435965fb8322e65182086dce8a94ee391b1", include_bytes!("../reference/cuda/cas/1967d8df4564848b45f662fd1b6b2435965fb8322e65182086dce8a94ee391b1")),
    ("cuda/cas/283af355c5e0752392387f4f2ade29d04665823c4a4349daa1e6d0d4536efc96", include_bytes!("../reference/cuda/cas/283af355c5e0752392387f4f2ade29d04665823c4a4349daa1e6d0d4536efc96")),
    ("cuda/cas/2ce952427453509f62eb4034e497605ef13e53dbcd7770aca0aabeac3da6e464", include_bytes!("../reference/cuda/cas/2ce952427453509f62eb4034e497605ef13e53dbcd7770aca0aabeac3da6e464")),
    ("cuda/cas/92d7d90e0d1686a45818068257abb902d2f1addb9fd725e3e65e36e994ff0fd8", include_bytes!("../reference/cuda/cas/92d7d90e0d1686a45818068257abb902d2f1addb9fd725e3e65e36e994ff0fd8")),
    ("cuda/cas/d57aaebfe85affae42f7863300fb5758febe896e2ce3408c18b8f3b1c48f82dc", include_bytes!("../reference/cuda/cas/d57aaebfe85affae42f7863300fb5758febe896e2ce3408c18b8f3b1c48f82dc")),
    ("cuda/graph.json", include_bytes!("../reference/cuda/graph.json")),
    ("cuda/trust.json", include_bytes!("../reference/cuda/trust.json")),
    ("lora/cas/1559c03fda4ebdb11660c0233fb9ada3213108b15eff617136ad328ca0ffe68b", include_bytes!("../reference/lora/cas/1559c03fda4ebdb11660c0233fb9ada3213108b15eff617136ad328ca0ffe68b")),
    ("lora/cas/6e581a3dec5a1f5866a89d89d1c53f702d3bbd4d2eafe7f2e772e7a9c07c92da", include_bytes!("../reference/lora/cas/6e581a3dec5a1f5866a89d89d1c53f702d3bbd4d2eafe7f2e772e7a9c07c92da")),
    ("lora/cas/974d3a589d408ebcc482e3ced3b5fef2f17a0b62d7f55e4636e1a1497f8a0537", include_bytes!("../reference/lora/cas/974d3a589d408ebcc482e3ced3b5fef2f17a0b62d7f55e4636e1a1497f8a0537")),
    ("lora/cas/9dbc9f0219866e58bb69848782dbe17aabfe5257c529c1a69a7408f3cd8ce99d", include_bytes!("../reference/lora/cas/9dbc9f0219866e58bb69848782dbe17aabfe5257c529c1a69a7408f3cd8ce99d")),
    ("lora/cas/b7089505458d8f76922e3af9a43c63e88a3937cbb5c8e990dc60133c1329aece", include_bytes!("../reference/lora/cas/b7089505458d8f76922e3af9a43c63e88a3937cbb5c8e990dc60133c1329aece")),
    ("lora/cas/e3f436df692aedfb396d3a52ac62af4ee21a0de2954e8ca01a72390253756967", include_bytes!("../reference/lora/cas/e3f436df692aedfb396d3a52ac62af4ee21a0de2954e8ca01a72390253756967")),
    ("lora/graph.json", include_bytes!("../reference/lora/graph.json")),
    ("lora/trust.json", include_bytes!("../reference/lora/trust.json")),
    ("recovery/cas/214a77cf7d1cc5809b11382cc9938cac0d9577a821c183f1267016aca8d55902", include_bytes!("../reference/recovery/cas/214a77cf7d1cc5809b11382cc9938cac0d9577a821c183f1267016aca8d55902")),
    ("recovery/cas/23934437fdf8939867ec8b8c36552a9f1c9f95c5cea89faee88c7ce03a1a5a64", include_bytes!("../reference/recovery/cas/23934437fdf8939867ec8b8c36552a9f1c9f95c5cea89faee88c7ce03a1a5a64")),
    ("recovery/cas/3dd2878dc49ec1387e46321b027b4f16a02fabeb10b104d630cc6bdf05550cd5", include_bytes!("../reference/recovery/cas/3dd2878dc49ec1387e46321b027b4f16a02fabeb10b104d630cc6bdf05550cd5")),
    ("recovery/cas/62c163054b829b9ed8155e6eafb13baa22f8b28f2727cfcc63beefdf8170c0e6", include_bytes!("../reference/recovery/cas/62c163054b829b9ed8155e6eafb13baa22f8b28f2727cfcc63beefdf8170c0e6")),
    ("recovery/cas/6e581a3dec5a1f5866a89d89d1c53f702d3bbd4d2eafe7f2e772e7a9c07c92da", include_bytes!("../reference/recovery/cas/6e581a3dec5a1f5866a89d89d1c53f702d3bbd4d2eafe7f2e772e7a9c07c92da")),
    ("recovery/cas/dfba6b60e87f934d71f12ecac5ad11be9f960bd7e3fbd2d7d7054ab412ef86e5", include_bytes!("../reference/recovery/cas/dfba6b60e87f934d71f12ecac5ad11be9f960bd7e3fbd2d7d7054ab412ef86e5")),
    ("recovery/graph.json", include_bytes!("../reference/recovery/graph.json")),
    ("recovery/trust.json", include_bytes!("../reference/recovery/trust.json")),
];

/// Materialize byte-pinned historical public evidence, then invoke the owning verifier.
pub fn open(name: &str, data: &Path, tools: &Path) -> Result<serde_json::Value, String> {
    if !["cuda", "lora", "recovery"].contains(&name) {
        return Err("invalid_reference: choose CUDA, LoRA or recovery".into());
    }
    let root = materialize(data)?;
    let selected: PathBuf = root.join(name);
    let values: BTreeMap<_, _> = [
        ("input", selected.join("graph.json")),
        ("trust", selected.join("trust.json")),
        ("cas", selected.join("cas")),
    ]
    .into_iter()
    .map(|(key, path)| (key.into(), vec![path.to_string_lossy().into_owned()]))
    .collect();
    let result = run_host(
        tools,
        &HostRequest {
            operation: vec!["evidence".into(), "story".into()],
            values,
        },
        None,
        true,
    )?;
    if !result.success {
        return Err(format!("reference_refused: {}", result.stderr));
    }
    let mut story: serde_json::Value = serde_json::from_str(&result.stdout)
        .map_err(|_| "reference_format: invalid verifier projection")?;
    story["mode"] = json!("REPLAY");
    story["reference"] = json!(name);
    Ok(story)
}

/// Retain only byte-identical embedded artifacts; refuse existing tampering or symlinks.
pub fn materialize(data: &Path) -> Result<PathBuf, String> {
    let root = data.join("cohesix/reference-m27f");
    for (path, bytes) in FILES {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| "reference_io: cannot create artifact directory")?;
        }
        if std::fs::symlink_metadata(&target).is_ok() {
            if !std::fs::symlink_metadata(&target)
                .map_err(|_| "reference_io")?
                .is_file()
            {
                return Err("reference_tampered: reference must be a regular file".into());
            }
            if crate::workbench::read_artifact(&target, 1024 * 1024)? != *bytes {
                return Err(
                    "reference_tampered: retained reference differs from the installed application"
                        .into(),
                );
            }
        } else {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|_| "reference_io: cannot retain artifact")?;
            file.write_all(bytes)
                .map_err(|_| "reference_io: cannot retain artifact")?;
        }
    }
    Ok(root)
}
