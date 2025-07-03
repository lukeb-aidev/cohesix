// CLASSIFICATION: COMMUNITY
// Filename: slm_manage_registry.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-22
// SPDX-License-Identifier: Apache-2.0
// SLM Action: manage
// Target: registry

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Registry for Small Language Models (SLMs) available to the Queen.
/// Models are discovered under `/srv/slm/available` and can be
/// activated for dispatch to workers.
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub name: String,
    pub version: String,
    pub size: u64,
    pub sandboxed: bool,
    pub signed: bool,
    pub gpu_required: bool,
}

/// SLM registry backed by simple directory scanning.
#[derive(Default)]
pub struct SlmRegistry {
    models: HashMap<String, ModelInfo>,
}

impl SlmRegistry {
    /// Load models from `/srv/slm/available`.
    pub fn load() -> Self {
        let mut reg = SlmRegistry::default();
        let base = Path::new("/srv/slm/available");
        if let Ok(entries) = fs::read_dir(base) {
            for ent in entries.flatten() {
                if let Ok(meta) = ent.metadata() {
                    if meta.is_file() {
                        let name = ent.file_name().to_string_lossy().into_owned();
                        let info = ModelInfo {
                            name: name.clone(),
                            version: "0.1".into(),
                            size: meta.len(),
                            sandboxed: true,
                            signed: true,
                            gpu_required: false,
                        };
                        reg.models.insert(name, info);
                    }
                }
            }
        }
        reg
    }

    /// Retrieve information for a model.
    pub fn get(&self, name: &str) -> Option<&ModelInfo> {
        self.models.get(name)
    }

    /// List all model names.
    pub fn list(&self) -> Vec<String> {
        self.models.keys().cloned().collect()
    }
}
