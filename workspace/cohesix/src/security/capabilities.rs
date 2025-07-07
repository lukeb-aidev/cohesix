// CLASSIFICATION: COMMUNITY
// Filename: capabilities.rs v0.4
// Author: Lukas Bower
// Date Modified: 2026-10-29
#![cfg(feature = "std")]

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Runtime capability map loaded from `/etc/cohcap.json`.
/// Maps roles to allowed syscall verbs and path prefixes.
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

#[derive(Deserialize, Serialize, Default)]
struct RoleCaps {
    verbs: Vec<String>,
    paths: Vec<String>,
}

static CAP_MAP: Lazy<HashMap<String, RoleCaps>> = Lazy::new(|| {
    let path = "/etc/cohcap.json";
    if let Ok(data) = fs::read_to_string(path) {
        if let Ok(map) = serde_json::from_str::<HashMap<String, RoleCaps>>(&data) {
            return map;
        }
    }
    default_caps()
});

fn default_caps() -> HashMap<String, RoleCaps> {
    use std::iter::FromIterator;
    HashMap::from_iter([
        (
            "QueenPrimary".into(),
            RoleCaps {
                verbs: vec!["open".into(), "exec".into()],
                paths: vec!["/".into()],
            },
        ),
        (
            "RegionalQueen".into(),
            RoleCaps {
                verbs: vec!["open".into(), "exec".into()],
                paths: vec!["/".into()],
            },
        ),
        (
            "BareMetalQueen".into(),
            RoleCaps {
                verbs: vec!["open".into(), "exec".into()],
                paths: vec!["/".into()],
            },
        ),
        (
            "DroneWorker".into(),
            RoleCaps {
                verbs: vec!["open".into(), "exec".into()],
                paths: vec!["/sim".into(), "/srv/cuda".into()],
            },
        ),
        (
            "InteractiveAiBooth".into(),
            RoleCaps {
                verbs: vec!["open".into()],
                paths: vec!["/input".into(), "/mnt".into(), "/srv/cuda".into()],
            },
        ),
        (
            "KioskInteractive".into(),
            RoleCaps {
                verbs: vec!["open".into()],
                paths: vec!["/srv/console".into()],
            },
        ),
        (
            "GlassesAgent".into(),
            RoleCaps {
                verbs: vec!["open".into()],
                paths: vec!["/input".into(), "/srv/cuda".into()],
            },
        ),
        (
            "SensorRelay".into(),
            RoleCaps {
                verbs: vec!["open".into()],
                paths: vec!["/input".into(), "/srv/telemetry".into()],
            },
        ),
        (
            "SimulatorTest".into(),
            RoleCaps {
                verbs: vec!["open".into()],
                paths: vec!["/sim/data".into(), "/srv/testlog".into()],
            },
        ),
    ])
}

/// Check if a role may perform a verb on a path.
pub fn role_allows(role: &str, verb: &str, path: &str) -> bool {
    if let Some(caps) = CAP_MAP.get(role) {
        if !caps.verbs.iter().any(|v| v == verb) {
            return false;
        }
        caps.paths.iter().any(|p| path.starts_with(p))
    } else {
        false
    }
}
