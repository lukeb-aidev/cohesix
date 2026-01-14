// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Emit CAS manifest templates and hashes from the root-task manifest.
// Author: Lukas Bower

use crate::codegen::hash_bytes;
use crate::ir::Manifest;
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

const TEMPLATE_EPOCH: &str = "<epoch>";
const TEMPLATE_PAYLOAD_BYTES: &str = "<payload-bytes>";
const TEMPLATE_SHA256: &str = "<sha256-hex>";
const TEMPLATE_SIGNATURE: &str = "<ed25519-signature-hex>";

#[derive(Debug, Clone)]
pub struct CasTemplate {
    pub json: String,
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct CasArtifacts {
    pub template_json: PathBuf,
    pub template_hash: PathBuf,
}

pub fn build_cas_template(manifest: &Manifest) -> CasTemplate {
    let mut map = Map::new();
    map.insert(
        "schema".to_owned(),
        Value::String(cohesix_cas::CAS_MANIFEST_SCHEMA.to_owned()),
    );
    map.insert("epoch".to_owned(), Value::String(TEMPLATE_EPOCH.to_owned()));
    map.insert(
        "chunk_bytes".to_owned(),
        Value::Number(serde_json::Number::from(u64::from(
            manifest.cas.store.chunk_bytes,
        ))),
    );
    map.insert(
        "payload_bytes".to_owned(),
        Value::String(TEMPLATE_PAYLOAD_BYTES.to_owned()),
    );
    map.insert(
        "payload_sha256".to_owned(),
        Value::String(TEMPLATE_SHA256.to_owned()),
    );
    map.insert(
        "chunks".to_owned(),
        Value::Array(vec![Value::String(TEMPLATE_SHA256.to_owned())]),
    );
    let delta_value = if manifest.cas.delta.enable {
        let mut delta = Map::new();
        delta.insert(
            "base_epoch".to_owned(),
            Value::String(TEMPLATE_EPOCH.to_owned()),
        );
        delta.insert(
            "base_sha256".to_owned(),
            Value::String(TEMPLATE_SHA256.to_owned()),
        );
        Value::Object(delta)
    } else {
        Value::Null
    };
    map.insert("delta".to_owned(), delta_value);
    let signing_required = manifest
        .cas
        .signing
        .as_ref()
        .map(|signing| signing.required)
        .unwrap_or(false);
    let signature_value = if signing_required {
        Value::String(TEMPLATE_SIGNATURE.to_owned())
    } else {
        Value::Null
    };
    map.insert("signature".to_owned(), signature_value);

    let json = serde_json::to_string_pretty(&Value::Object(map))
        .expect("render cas manifest template json");
    let hash = hash_bytes(json.as_bytes());
    CasTemplate { json, hash }
}

pub fn emit_cas_template(template: &CasTemplate, path: &Path) -> Result<CasArtifacts> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, template.json.as_bytes())
        .with_context(|| format!("failed to write cas template {}", path.display()))?;
    let hash_path = path.with_extension("json.sha256");
    let hash_contents = format!(
        "# Author: Lukas Bower\n# Purpose: SHA-256 fingerprint for {}.\n{}  {}\n",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("cas_manifest_template.json"),
        template.hash,
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("cas_manifest_template.json")
    );
    fs::write(&hash_path, hash_contents).with_context(|| {
        format!("failed to write cas template hash {}", hash_path.display())
    })?;
    Ok(CasArtifacts {
        template_json: path.to_path_buf(),
        template_hash: hash_path,
    })
}
