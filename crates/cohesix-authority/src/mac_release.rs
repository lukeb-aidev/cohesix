// Author: Lukas Bower
// Purpose: Select immutable macOS release inputs and host-owned credentials before ticket dispatch.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

/// A target fixes one action and all native operands. Tickets carry only its id.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub id: String,
    pub operation: Operation,
}

/// Credentials are references to operator-owned stores, never ticket fields.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", deny_unknown_fields)]
pub enum Operation {
    #[serde(rename = "mac_release.build")]
    Build { source: Source },
    #[serde(rename = "mac_release.test")]
    Test { source: Source },
    #[serde(rename = "mac_release.archive")]
    Archive { source: Source },
    #[serde(rename = "mac_release.codesign")]
    Codesign {
        input: Artifact,
        identity_sha1: String,
    },
    #[serde(rename = "mac_release.notarize")]
    Notarize {
        input: Artifact,
        keychain_profile: String,
    },
    #[serde(rename = "mac_release.upload")]
    Upload {
        input: Artifact,
        app_id: String,
        version: String,
        api_key_id: String,
        issuer_id: String,
        private_key_path_ref: String,
        jwt_ref: String,
    },
    #[serde(rename = "endpoint_compliance.observe")]
    Compliance,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub root: PathBuf,
    pub tree_sha256: String,
    pub project: PathBuf,
    pub scheme: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub path: PathBuf,
    /// Regular files use SHA-256; directories use cohesix-native-tree/v1.
    pub sha256: String,
}

impl Operation {
    pub fn action(&self) -> &'static str {
        match self {
            Self::Build { .. } => "mac_release.build",
            Self::Test { .. } => "mac_release.test",
            Self::Archive { .. } => "mac_release.archive",
            Self::Codesign { .. } => "mac_release.codesign",
            Self::Notarize { .. } => "mac_release.notarize",
            Self::Upload { .. } => "mac_release.upload",
            Self::Compliance => "endpoint_compliance.observe",
        }
    }
}

fn digest(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn token(value: &str) -> bool {
    crate::validate_id(value).is_ok() && value.as_bytes()[0].is_ascii_alphanumeric()
}
fn path(value: &Path, absolute: bool) -> bool {
    value.is_absolute() == absolute
        && !value.as_os_str().is_empty()
        && value.as_os_str().len() <= 1024
        && value
            .to_str()
            .is_some_and(|s| !s.bytes().any(|c| c.is_ascii_control()))
        && value
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::RootDir))
}

/// Bound the map and reject credential values, aliases, path escapes and ambiguous actions.
pub fn validate(targets: &[Target]) -> Result<(), &'static str> {
    if targets.len() > 32 {
        return Err("ELIMIT macos-targets");
    }
    let mut ids = BTreeSet::new();
    for t in targets {
        if !token(&t.id) || !ids.insert(&t.id) {
            return Err("EPERM macos-target-id");
        }
        let input = match &t.operation {
            Operation::Build { source }
            | Operation::Test { source }
            | Operation::Archive { source } => {
                if !path(&source.root, true)
                    || !path(&source.project, false)
                    || source.project.extension().is_none_or(|x| x != "xcodeproj")
                    || !digest(&source.tree_sha256, 64)
                    || !token(&source.scheme)
                {
                    return Err("EPERM xcode-source");
                }
                None
            }
            Operation::Codesign {
                input,
                identity_sha1,
            } => {
                if !digest(identity_sha1, 40) {
                    return Err("EPERM codesign-identity");
                }
                Some(input)
            }
            Operation::Notarize {
                input,
                keychain_profile,
            } => {
                if !token(keychain_profile) {
                    return Err("EPERM notary-profile");
                }
                Some(input)
            }
            Operation::Upload {
                input,
                app_id,
                version,
                api_key_id,
                issuer_id,
                private_key_path_ref,
                jwt_ref,
            } => {
                if ![app_id, version, api_key_id, issuer_id]
                    .iter()
                    .all(|s| token(s))
                    || !app_id.bytes().all(|b| b.is_ascii_digit())
                    || crate::secret::validate_reference(private_key_path_ref).is_err()
                    || crate::secret::validate_reference(jwt_ref).is_err()
                {
                    return Err("EPERM app-store-configuration");
                }
                Some(input)
            }
            Operation::Compliance => None,
        };
        if input.is_some_and(|i| !path(&i.path, true) || !digest(&i.sha256, 64)) {
            return Err("EPERM macos-artifact");
        }
    }
    Ok(())
}

pub fn targets() -> Result<Vec<Target>, crate::provider::ProviderError> {
    let value = crate::provider::registry()?["contract"]
        .get("macos_targets")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let targets: Vec<Target> = serde_json::from_value(value)
        .map_err(|_| crate::provider::ProviderError::InvalidRegistry)?;
    validate(&targets).map_err(|_| crate::provider::ProviderError::InvalidRegistry)?;
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn nested(mut value: serde_json::Value) -> serde_json::Value {
        let id = value.as_object_mut().unwrap().remove("id").unwrap();
        serde_json::json!({"id":id,"operation":value})
    }
    #[test]
    fn release_map_rejects_escape_credentials_and_duplicate_targets() {
        let value = serde_json::json!({"id":"release","action":"mac_release.build","source":{"root":"/owned/source","project":"App.xcodeproj","scheme":"App","tree_sha256":"a".repeat(64)}});
        let t: Target = serde_json::from_value(nested(value.clone())).unwrap();
        assert!(validate(core::slice::from_ref(&t)).is_ok());
        assert!(validate(&[t.clone(), t]).is_err());
        let mut escaped = value;
        escaped["source"]["project"] = "../Other.xcodeproj".into();
        assert!(validate(&[serde_json::from_value(nested(escaped)).unwrap()]).is_err());
        assert!(serde_json::from_value::<Target>(
            serde_json::json!({"id":"scan","action":"endpoint_compliance.observe","command":"sh"})
        )
        .is_err());
        let bad: Target = serde_json::from_value(nested(serde_json::json!({"id":"upload","action":"mac_release.upload","input":{"path":"/owned/App.pkg","sha256":"a".repeat(64)},"app_id":"123","version":"1","api_key_id":"KEY","issuer_id":"issuer","private_key_path_ref":"password","jwt_ref":"env:ASC_JWT"}))).unwrap();
        assert!(validate(&[bad]).is_err());
    }
}
