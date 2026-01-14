// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Expose coh-rtc manifest compilation helpers for tests and the CLI.
// Author: Lukas Bower

pub mod codegen;
pub mod ir;

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub manifest_path: PathBuf,
    pub out_dir: PathBuf,
    pub manifest_out: PathBuf,
    pub cli_script_out: PathBuf,
    pub doc_snippet_out: PathBuf,
    pub cbor_snippet_out: PathBuf,
}

pub fn compile(options: &CompileOptions) -> Result<codegen::GeneratedArtifacts> {
    if !options.manifest_path.is_file() {
        bail!(
            "manifest path does not exist or is not a file: {}",
            options.manifest_path.display()
        );
    }

    let manifest = ir::load_manifest(&options.manifest_path)?;
    manifest.validate()?;

    let resolved_json = ir::serialize_manifest(&manifest)?;
    let manifest_hash = codegen::hash_bytes(&resolved_json);

    let docs = codegen::DocFragments::from_manifest(&manifest, &manifest_hash);

    codegen::emit_all(
        &manifest,
        &manifest_hash,
        &resolved_json,
        options,
        &docs,
    )
}

pub fn default_doc_snippet_path() -> PathBuf {
    Path::new("docs").join("snippets").join("root_task_manifest.md")
}

pub fn default_cli_script_path() -> PathBuf {
    Path::new("scripts").join("cohsh").join("boot_v0.coh")
}

pub fn default_cbor_snippet_path() -> PathBuf {
    Path::new("docs")
        .join("snippets")
        .join("telemetry_cbor_schema.md")
}
