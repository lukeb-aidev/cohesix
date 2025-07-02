// CLASSIFICATION: COMMUNITY
// Filename: queen_watchdog.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-01

use crate::prelude::*;
/// Worker-side watchdog monitoring queen heartbeat.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn queen_dir() -> String {
    std::env::var("COHESIX_QUEEN_DIR").unwrap_or_else(|_| "/srv/queen".into())
}

/// Monitor the queen heartbeat and self-promote on failure.
pub struct QueenWatchdog {
    miss_count: u32,
    threshold: u32,
}

impl QueenWatchdog {
    /// Create a new watchdog with allowed missed heartbeats.
    pub fn new(threshold: u32) -> Self {
        Self {
            miss_count: 0,
            threshold,
        }
    }

    /// Check queen heartbeat and elect self if needed.
    pub fn check(&mut self) {
        let hb_path = format!("{}/heartbeat", queen_dir());
        let ts = fs::metadata(&hb_path)
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);
        let age = SystemTime::now().duration_since(ts).unwrap_or_default();
        if age > Duration::from_millis(500) {
            self.miss_count += 1;
        } else {
            self.miss_count = 0;
        }
        if self.miss_count >= self.threshold {
            self.promote();
        }
    }

    fn promote(&self) {
        let qdir = queen_dir();
        fs::create_dir_all(&qdir).ok();
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .write(true)
            .open(format!("{}/role", qdir))
        {
            let _ = write!(f, "QueenPrimary");
        }
        fs::create_dir_all("/log").ok();
        if let Ok(mut l) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/log/mesh_reconfig.log")
        {
            let _ = writeln!(l, "promoted to QueenPrimary");
        }
    }
}
