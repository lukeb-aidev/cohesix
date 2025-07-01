// CLASSIFICATION: COMMUNITY
// Filename: edge_fallback.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-12

use crate::prelude::*;
//! Edge-only fallback coordinator for temporary queen loss.

use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct EdgeFallback {
    miss: u32,
    threshold: u32,
    in_fallback: bool,
}

impl EdgeFallback {
    pub fn new(threshold: u32) -> Self {
        Self { miss: 0, threshold, in_fallback: false }
    }

    pub fn check(&mut self) {
        if Self::queen_alive() {
            self.miss = 0;
            if self.in_fallback {
                self.demote();
            }
        } else {
            self.miss += 1;
            if !self.in_fallback && self.miss >= self.threshold {
                self.promote();
            }
        }
    }

    fn queen_alive() -> bool {
        let ts = fs::metadata("/srv/queen/heartbeat")
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);
        SystemTime::now()
            .duration_since(ts)
            .map(|d| d < Duration::from_secs(3))
            .unwrap_or(false)
    }

    fn promote(&mut self) {
        self.in_fallback = true;
        let _ = fs::write("/srv/cohrole", "EdgeFallbackCoordinator");
        let _ = fs::write("/srv/cohrole_prev", "DroneWorker");
        let _ = fs::create_dir_all("/srv/slm");
        let _ = fs::write("/srv/slm/fallback", "on");
    }

    fn demote(&mut self) {
        self.in_fallback = false;
        if let Ok(prev) = fs::read_to_string("/srv/cohrole_prev") {
            let _ = fs::write("/srv/cohrole", prev);
        }
        let _ = fs::remove_file("/srv/slm/fallback");
    }
}
