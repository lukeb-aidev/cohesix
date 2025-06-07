// CLASSIFICATION: COMMUNITY
// Filename: cluster.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-01

//! Simple cluster coordination for multiple Queen nodes.
//!
//! Keeps track of participating nodes and elects a primary orchestrator. The
//! election algorithm is intentionally naive: the lexicographically smallest
//! node id becomes primary. Decisions are appended to `/srv/orchestration.log`.

use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default, Clone)]
pub struct QueenCluster {
    nodes: Vec<String>,
    primary: Option<String>,
}

impl QueenCluster {
    /// Create an empty cluster record.
    pub fn new() -> Self {
        Self { nodes: Vec::new(), primary: None }
    }

    /// Register a Queen node by id.
    pub fn register(&mut self, id: &str) {
        if !self.nodes.contains(&id.to_string()) {
            self.nodes.push(id.into());
        }
    }

    /// Elect the primary orchestrator from the known nodes.
    pub fn elect_primary(&mut self) -> Option<String> {
        if self.nodes.is_empty() {
            return None;
        }
        self.nodes.sort();
        let candidate = self.nodes[0].clone();
        if self.primary.as_ref() != Some(&candidate) {
            self.primary = Some(candidate.clone());
            Self::log(&format!("primary_elected {candidate}"));
        }
        self.primary.clone()
    }

    /// Get the current primary id if any.
    pub fn primary(&self) -> Option<&String> {
        self.primary.as_ref()
    }

    fn log(msg: &str) {
        create_dir_all("/srv").ok();
        let path = "/srv/orchestration.log";
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "{} {}", ts, msg);
        }
    }
}

