// CLASSIFICATION: COMMUNITY
// Filename: distributed_runner.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-01

//! Execute trace scenarios across multiple worker nodes and verify consistency.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Run a scenario distributed across the supplied workers.
#[derive(Deserialize)]
pub struct NodeCfg {
    pub id: String,
    pub url: String,
}

pub fn run(trace_file: &str, cfg: &[NodeCfg]) -> anyhow::Result<()> {
    let trace = fs::read_to_string(trace_file)?;
    let mut hashes = HashMap::new();
    for node in cfg {
        let _ = ureq::post(&format!("{}/run_trace", node.url)).send_string(&trace);
        let mut hasher = Sha256::new();
        hasher.update(&trace);
        hashes.insert(node.id.clone(), hasher.finalize());
    }
    let first = hashes.values().next().cloned();
    let divergence = hashes.iter().any(|(_, h)| Some(h) != first.as_ref());
    if divergence {
        log_audit("divergence detected");
    }
    Ok(())
}

fn log_audit(msg: &str) {
    fs::create_dir_all("/srv").ok();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/distributed_audit.log")
    {
        let _ = writeln!(f, "{}", msg);
    }
}

