// CLASSIFICATION: COMMUNITY
// Filename: sync_daemon.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-08

use crate::prelude::*;
/// Queen<->Worker world model sync daemon.

use crate::world_model::WorldModelSnapshot;
use std::fs;
use std::thread;
use std::time::Duration;

pub struct QueenSyncDaemon {
    pub workers: Vec<String>,
    pub last_snapshot: Option<WorldModelSnapshot>,
}

impl Default for QueenSyncDaemon {
    fn default() -> Self {
        Self::new()
    }
}

impl QueenSyncDaemon {
    /// Create a new sync daemon.
    pub fn new() -> Self {
        Self { workers: Vec::new(), last_snapshot: None }
    }

    /// Register a worker node for updates.
    pub fn add_worker(&mut self, id: &str) {
        self.workers.push(id.into());
    }

    /// Run the sync loop. Every 50ms push the current snapshot if changed.
    pub fn run(&mut self, path: &str) {
        loop {
            if let Ok(snap) = WorldModelSnapshot::load(path) {
                if self.last_snapshot.as_ref() != Some(&snap) {
                    self.last_snapshot = Some(snap.clone());
                    self.push_diff(&snap);
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn push_diff(&self, snap: &WorldModelSnapshot) {
        let data = serde_json::to_vec(snap).unwrap_or_default();
        for w in &self.workers {
            let path = format!("/srv/world_sync/{}.json", w);
            let _ = fs::create_dir_all("/srv/world_sync");
            fs::write(path, &data).ok();
        }
    }
}


