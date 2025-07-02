// CLASSIFICATION: COMMUNITY
// Filename: failover.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-05

use crate::CohError;
/// Queen failover manager promoting a candidate when the primary is unresponsive.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Failover manager checking heartbeat files.
pub struct FailoverManager {
    timeout: Duration,
}

impl FailoverManager {
    /// Create a new manager with the given timeout in seconds.
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    /// Check primary heartbeat and promote the candidate if necessary.
    pub fn check_primary(&self) -> Result<(), CohError> {
        let last = fs::metadata("/srv/queen/primary_heartbeat")
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);
        if SystemTime::now().duration_since(last)?.as_secs() > self.timeout.as_secs() {
            self.promote_candidate()?;
        }
        Ok(())
    }

    fn promote_candidate(&self) -> Result<(), CohError> {
        log_event("promoting QueenCandidate")?;
        if let Ok(entries) = fs::read_dir("/srv/snapshots") {
            for e in entries.flatten() {
                let dst = format!("/srv/worker/backup/{}", e.file_name().to_string_lossy());
                let _ = fs::create_dir_all("/srv/worker/backup");
                let _ = fs::copy(e.path(), dst);
            }
        }
        let _ = fs::rename("/srv/queen/QueenCandidate", "/srv/queen/QueenPrimary");
        Ok(())
    }
}

fn log_event(msg: &str) -> std::io::Result<()> {
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/failover.log")?;
    writeln!(f, "{}", msg)
}
