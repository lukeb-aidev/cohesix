// Author: Lukas Bower
// Purpose: Share compiler-owned exact host package requirements with builders and installers.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use alloc::{collections::BTreeSet, string::String, vec::Vec};
use core::fmt;
use serde::{Deserialize, Serialize};

pub const PROFILE_SCHEMA: &str = "cohesix-host-package-profile/v1";
pub const MANIFEST_SCHEMA: &str = "cohesix-host-package/v1";
pub const MAX_FILES: usize = 128;
pub const MAX_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub id: String,
    pub os: String,
    pub architecture: String,
    pub version: String,
    pub integration_surfaces: Vec<String>,
    pub required_credentials: Vec<String>,
    pub artifacts: Vec<Requirement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Elf,
    MachO,
    Json,
    Toml,
    Text,
    PythonWheel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub path: String,
    pub kind: ArtifactKind,
    pub executable: bool,
    pub version: String,
    /// Optional exact schema field, expressed as a JSON pointer. TOML is
    /// projected to the same value model before checking this field.
    pub schema_pointer: Option<String>,
    pub schema_value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidProfile;

impl fmt::Display for InvalidProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EPERM host-package-profile")
    }
}

impl std::error::Error for InvalidProfile {}

pub fn relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && path.split('/').count() <= 16
        && path.split('/').all(|component| {
            !matches!(component, "" | "." | "..")
                && component
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

impl Profile {
    /// Refuse ambiguous paths, duplicate entries and unbounded package inputs.
    pub fn validate(&self) -> Result<(), InvalidProfile> {
        if self.schema != PROFILE_SCHEMA
            || !identifier(&self.id)
            || !identifier(&self.version)
            || !matches!(self.os.as_str(), "macos" | "linux")
            || !matches!(self.architecture.as_str(), "aarch64" | "x86_64")
            || self.artifacts.is_empty()
            || self.artifacts.len() > MAX_FILES
            || self.integration_surfaces.is_empty()
            || self.integration_surfaces.len() > 64
            || self.required_credentials.len() > 32
        {
            return Err(InvalidProfile);
        }
        for values in [&self.integration_surfaces, &self.required_credentials] {
            if values.iter().any(|value| !identifier(value))
                || values.iter().collect::<BTreeSet<_>>().len() != values.len()
            {
                return Err(InvalidProfile);
            }
        }
        if self.required_credentials.iter().any(|name| {
            name.bytes().enumerate().any(|(index, byte)| {
                !(byte.is_ascii_uppercase() || byte == b'_' || (index > 0 && byte.is_ascii_digit()))
            })
        }) {
            return Err(InvalidProfile);
        }
        let mut paths: BTreeSet<&String> = BTreeSet::new();
        for artifact in &self.artifacts {
            if !relative_path(&artifact.path)
                || !identifier(&artifact.version)
                || paths.iter().any(|previous| {
                    artifact.path.starts_with(&alloc::format!("{previous}/"))
                        || previous.starts_with(&alloc::format!("{}/", artifact.path))
                })
                || !paths.insert(&artifact.path)
                || artifact.path == "package.json"
                || artifact.path.starts_with("package.json/")
                || artifact.executable
                    != matches!(artifact.kind, ArtifactKind::Elf | ArtifactKind::MachO)
                || (artifact.kind == ArtifactKind::Elf && self.os != "linux")
                || (artifact.kind == ArtifactKind::MachO && self.os != "macos")
            {
                return Err(InvalidProfile);
            }
            match (&artifact.schema_pointer, &artifact.schema_value) {
                (None, None) => {}
                (Some(pointer), Some(value))
                    if matches!(artifact.kind, ArtifactKind::Json | ArtifactKind::Toml)
                        && pointer.starts_with('/')
                        && pointer.len() <= 128
                        && pointer.bytes().all(|byte| byte.is_ascii_graphic())
                        && !value.is_empty()
                        && value.len() <= 128
                        && value.bytes().all(|byte| byte.is_ascii_graphic()) => {}
                _ => return Err(InvalidProfile),
            }
        }
        if !self.artifacts.iter().any(|artifact| {
            artifact.path == "package.sbom.json"
                && artifact.kind == ArtifactKind::Json
                && artifact.schema_pointer.as_deref() == Some("/bomFormat")
                && artifact.schema_value.as_deref() == Some("CycloneDX")
        }) {
            return Err(InvalidProfile);
        }
        Ok(())
    }
}
