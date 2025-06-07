// CLASSIFICATION: COMMUNITY
// Filename: trace_consensus.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-05

//! Merge trace logs from peer queens and ensure consensus.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;

#[derive(Debug)]
pub struct ConsensusError;

pub struct TraceConsensus;

impl TraceConsensus {
    /// Merge trace files from peer directories. Returns list of merged ids.
    pub fn merge(peer_dirs: &[&str]) -> Result<Vec<String>, ConsensusError> {
        let mut merged: HashMap<String, String> = HashMap::new();
        for dir in peer_dirs {
            if let Ok(entries) = fs::read_dir(dir) {
                for e in entries.flatten() {
                    if let Ok(data) = fs::read_to_string(e.path()) {
                        let name = e.file_name().to_string_lossy().into_owned();
                        if let Some(existing) = merged.get(&name) {
                            if existing != &data {
                                return Err(ConsensusError);
                            }
                        } else {
                            merged.insert(name, data);
                        }
                    }
                }
            }
        }
        fs::create_dir_all("/srv/trace/consensus").ok();
        for (id, data) in &merged {
            let _ = fs::write(format!("/srv/trace/consensus/{}", id), data);
        }
        log_event("consensus merge complete");
        Ok(merged.keys().cloned().collect())
    }
}

fn log_event(msg: &str) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/trace/consensus/consensus.log")
    {
        let _ = writeln!(f, "{}", msg);
    }
}
