// CLASSIFICATION: COMMUNITY
// Filename: cluster.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-03

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Simple cluster coordination for multiple Queen nodes.
//
/// Keeps track of participating nodes and elects a primary orchestrator. The
/// election algorithm is intentionally naive: the lexicographically smallest
/// node id becomes primary. Decisions are appended to `/srv/orchestration.log`.
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Default)]
pub struct NodeRecord {
    pub id: String,
    pub boot_ts: u64,
    pub healthy: bool,
}

#[derive(Default, Clone)]
pub struct QueenCluster {
    nodes: Vec<NodeRecord>,
    primary: Option<String>,
}

impl QueenCluster {
    /// Create an empty cluster record.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            primary: None,
        }
    }

    /// Register a Queen node by id.
    pub fn register(&mut self, id: &str, boot_ts: u64) {
        if !self.nodes.iter().any(|n| n.id == id) {
            self.nodes.push(NodeRecord {
                id: id.into(),
                boot_ts,
                healthy: true,
            });
            Self::log(&format!("register {id}"));
        }
    }

    /// Update health status for a node.
    pub fn update_health(&mut self, id: &str, healthy: bool) {
        if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
            n.healthy = healthy;
            Self::log(&format!("health {id} {healthy}"));
        }
    }

    /// Rotate to the next healthy node if the current primary failed.
    pub fn rotate_primary(&mut self) -> Option<String> {
        if let Some(ref cur) = self.primary {
            if let Some(node) = self.nodes.iter().find(|n| n.id == *cur) {
                if node.healthy {
                    return Some(cur.clone());
                }
            }
        }
        self.primary = None;
        let p = self.elect_primary();
        if let Some(ref id) = p {
            Self::log(&format!("primary_rotated {}", id));
        }
        p
    }

    /// Elect the primary orchestrator from the known nodes.
    pub fn elect_primary(&mut self) -> Option<String> {
        if self.nodes.is_empty() {
            return None;
        }
        self.nodes.sort_by_key(|n| n.boot_ts);
        Self::log(&format!("quorum {:?}", self.nodes));
        if let Some(node) = self
            .nodes
            .iter()
            .filter(|n| n.healthy)
            .min_by_key(|n| n.boot_ts)
            .cloned()
        {
            if self.primary.as_ref() != Some(&node.id) {
                self.primary = Some(node.id.clone());
                Self::log(&format!("primary_elected {}", node.id));
            }
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
