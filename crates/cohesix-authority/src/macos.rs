// Author: Lukas Bower
// Purpose: Bind macOS native service control to compiler-selected labels, plist and executable identities.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Component, PathBuf};

/// Host-owned service selection. Ticket operands resolve ids, never native paths.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchdTarget {
    /// Stable provider target id.
    pub id: String,
    /// Explicit system, user/UID, or gui/UID bootstrap domain.
    pub domain: String,
    /// Exact launchd label.
    pub label: String,
    /// Approved job definition; its digest is checked before every operation.
    pub plist: PathBuf,
    /// Immutable selected job definition digest.
    pub plist_sha256: String,
    /// Native executable identity required by process observations.
    pub executable_sha256: String,
}

fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Reject duplicate services, unsafe paths and ambiguous bootstrap domains.
pub fn validate_launchd(targets: &[LaunchdTarget]) -> Result<(), &'static str> {
    if targets.len() > 32 {
        return Err("ELIMIT launchd-targets");
    }
    let mut ids = BTreeSet::new();
    let mut services = BTreeSet::new();
    for target in targets {
        if crate::validate_id(&target.id).is_err()
            || crate::validate_id(&target.label).is_err()
            || target.label.len() > 96
            || !target.label.as_bytes()[0].is_ascii_alphanumeric()
            || !ids.insert(&target.id)
            || !services.insert((&target.domain, &target.label))
            || !target.plist.is_absolute()
            || target.plist.as_os_str().len() > 1024
            || target
                .plist
                .components()
                .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
            || !hash(&target.plist_sha256)
            || !hash(&target.executable_sha256)
        {
            return Err("EPERM launchd-target-map");
        }
        if target.domain != "system" {
            let (kind, uid) = target
                .domain
                .split_once('/')
                .ok_or("EPERM launchd-domain")?;
            let parsed: u32 = uid.parse().map_err(|_| "EPERM launchd-domain")?;
            if !matches!(kind, "user" | "gui") || parsed.to_string() != uid {
                return Err("EPERM launchd-domain");
            }
        }
    }
    Ok(())
}

/// Read the selected compiled map without allowing environment or caller overrides.
pub fn launchd_targets() -> Result<Vec<LaunchdTarget>, crate::provider::ProviderError> {
    let value = crate::provider::registry()?["contract"]
        .get("launchd_targets")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let targets: Vec<LaunchdTarget> = serde_json::from_value(value)
        .map_err(|_| crate::provider::ProviderError::InvalidRegistry)?;
    validate_launchd(&targets).map_err(|_| crate::provider::ProviderError::InvalidRegistry)?;
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn services_require_unique_exact_native_identity() {
        let target = LaunchdTarget {
            id: "owned-job".into(),
            domain: "gui/501".into(),
            label: "org.cohesix.test".into(),
            plist: "/tmp/owned.plist".into(),
            plist_sha256: "a".repeat(64),
            executable_sha256: "b".repeat(64),
        };
        assert!(validate_launchd(&[target.clone()]).is_ok());
        assert!(validate_launchd(&[target.clone(), target.clone()]).is_err());
        for domain in ["gui/0501", "gui/-1", "user/1/job", "pid/12", "system/root"] {
            let mut changed = target.clone();
            changed.domain = domain.into();
            assert!(validate_launchd(&[changed]).is_err());
        }
        let mut changed = target.clone();
        changed.plist = "/tmp/../owned.plist".into();
        assert!(validate_launchd(&[changed]).is_err());
        let mut changed = target;
        changed.executable_sha256 = "F".repeat(64);
        assert!(validate_launchd(&[changed]).is_err());
    }
}
