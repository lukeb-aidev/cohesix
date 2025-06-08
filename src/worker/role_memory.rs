// CLASSIFICATION: COMMUNITY
// Filename: role_memory.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-12

//! Persist and replay worker role context for failover recovery.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::trace::recorder;

pub struct RoleMemory;

impl RoleMemory {
    /// Persist current role, goals and trace snapshot.
    pub fn persist(role: &str, goals: &str, trace: &str) {
        let base = Path::new("/history/failover");
        let _ = fs::create_dir_all(base.join("traces"));
        let _ = fs::write(base.join("last_role.txt"), role);
        let _ = fs::write(base.join("assigned_goals.json"), goals);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let trace_path = base.join("traces").join(format!("{ts}.log"));
        let _ = fs::copy(trace, trace_path);
        Self::trim_traces(base.join("traces"));
    }

    /// Replay recent traces up to the given limit.
    pub fn replay_last(limit: usize) {
        let base = Path::new("/history/failover/traces");
        if let Ok(entries) = fs::read_dir(base) {
            let mut files: Vec<_> = entries.flatten().collect();
            files.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH));
            files.reverse();
            for f in files.into_iter().take(limit) {
                if let Some(p) = f.path().to_str() {
                    let _ = recorder::replay(p);
                }
            }
        }
    }

    fn trim_traces(dir: impl AsRef<Path>) {
        if let Ok(entries) = fs::read_dir(dir.as_ref()) {
            let mut files: Vec<_> = entries.flatten().collect();
            let keep = 5usize;
            if files.len() <= keep {
                return;
            }
            files.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH));
            let excess = files.len() - keep;
            for e in files.iter().take(excess) {
                let _ = fs::remove_file(e.path());
            }
        }
    }

    /// Load the last known role if queen is absent.
    pub fn load_role() -> Option<String> {
        fs::read_to_string("/history/failover/last_role.txt").ok()
    }
}
