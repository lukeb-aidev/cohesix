// Author: Lukas Bower
// Purpose: Compile finite host recipe bounds independently of unchanged VM and provider contracts.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    schema: String,
    id: String,
    stages: Vec<String>,
    entrypoints: Vec<String>,
    registered_input_schema: String,
    registration_schema: String,
    max_registration_bytes: u32,
    max_input_bytes: u32,
    max_output_bytes: u32,
    max_parameters: u32,
    max_secret_refs: u32,
    max_disk_bytes: u32,
    submit_action: String,
    cancel_action: String,
    max_stages: u32,
    max_attempts: u32,
    max_journal_bytes: u32,
    max_artifact_bytes: u32,
    max_retained_bytes: u32,
    max_parallel: u32,
    max_reuse_age_ms: u64,
    automatic_retries: u32,
    recovery: String,
    data: String,
    peft: PeftCapabilities,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PeftCapabilities {
    schema: String,
    base_model: String,
    base_revision: String,
    training: Vec<String>,
    import_formats: Vec<String>,
    checkpoint: String,
    checkpoint_files: Vec<String>,
    checkpoint_interval_max: u32,
    serving: String,
    qlora: bool,
}

fn compile(bytes: &str) -> Result<Vec<u8>> {
    let c: Contract = toml::from_str(bytes)?;
    ensure!(
        c.schema == "cohesix-cuda-recipe-contract/v2"
            && c.id == "cuda-reference"
            && c.stages
                == [
                    "plan",
                    "authorise",
                    "execute",
                    "observe",
                    "verify",
                    "recover"
                ]
            && c.entrypoints == ["vadd", "matmul"]
            && c.registered_input_schema == "cohesix-gpu-workload-input/v2"
            && c.registration_schema == "cohesix-cuda-registration/v1"
            && c.max_registration_bytes == 8192
            && c.max_input_bytes == 262144
            && c.max_output_bytes == 262144
            && c.max_parameters == 8
            && c.max_secret_refs == 8
            && c.max_disk_bytes == 33554432
            && c.submit_action == "gpu.workload.submit"
            && c.cancel_action == "gpu.workload.cancel"
            && (1..=8).contains(&c.max_stages)
            && (1..=32).contains(&c.max_attempts)
            && (8192..=262144).contains(&c.max_journal_bytes)
            && (4..=262144).contains(&c.max_artifact_bytes)
            && c.max_retained_bytes >= c.max_artifact_bytes
            && c.max_retained_bytes <= 8388608
            && c.max_parallel == 1
            && c.automatic_retries == 0
            && (1..=3600000).contains(&c.max_reuse_age_ms)
            && !c.recovery.is_empty()
            && c.recovery.len() <= 256
            && !c.data.is_empty()
            && c.data.len() <= 128
            && c.peft.schema == "cohesix-peft-capabilities/v1"
            && c.peft.base_model == "HuggingFaceTB/SmolLM2-135M"
            && c.peft.base_revision == "93efa2f097d58c2a74874c7e644dbc9b0cee75a2"
            && c.peft.training == ["lora"]
            && c.peft.import_formats == ["peft-lora-safetensors"]
            && c.peft.checkpoint == "hf-trainer-full-state/v1"
            && c.peft.checkpoint_files
                == [
                    "adapter_model.safetensors",
                    "adapter_config.json",
                    "optimizer.pt",
                    "scheduler.pt",
                    "rng_state.pth",
                    "trainer_state.json"
                ]
            && (1..=16).contains(&c.peft.checkpoint_interval_max)
            && c.peft.serving == "transformers-serve-local"
            && !c.peft.qlora,
        "invalid CUDA recipe contract"
    );
    let mut output = serde_json::to_vec_pretty(&c)?;
    output.push(b'\n');
    Ok(output)
}

/// Emit a host-only contract; no root schema, namespace or executor is changed.
pub fn emit(source: &Path, output: &Path) -> Result<()> {
    fs::write(output, compile(&fs::read_to_string(source)?)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn recipe_cannot_enable_retries_parallelism_or_unbounded_retention() {
        let source = include_str!("../../../configs/cuda_recipe.toml");
        assert!(super::compile(source).is_ok());
        for (from, to) in [
            (
                "max_registration_bytes = 8192",
                "max_registration_bytes = 8193",
            ),
            ("max_parameters = 8", "max_parameters = 9"),
            ("automatic_retries = 0", "automatic_retries = 1"),
            ("max_parallel = 1", "max_parallel = 2"),
            ("max_attempts = 32", "max_attempts = 33"),
            (
                "max_retained_bytes = 8388608",
                "max_retained_bytes = 8388609",
            ),
            ("qlora = false", "qlora = true"),
            (
                "checkpoint_interval_max = 16",
                "checkpoint_interval_max = 17",
            ),
        ] {
            assert!(super::compile(&source.replace(from, to)).is_err());
        }
    }
}
