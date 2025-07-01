// CLASSIFICATION: COMMUNITY
// Filename: trust.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

use crate::prelude::*;
//! Simple trust zone escalation logic for workers.

use std::fs;
use std::path::Path;

pub fn record_failure(worker: &str) {
    let base = Path::new("/srv/trust_zones");
    fs::create_dir_all(base).ok();
    let cnt_path = base.join(format!("{worker}.fails"));
    let mut count = fs::read_to_string(&cnt_path).ok().and_then(|s| s.parse().ok()).unwrap_or(0u32);
    count += 1;
    fs::write(&cnt_path, count.to_string()).ok();
    if count >= 2 {
        escalate(worker, "yellow");
    }
}

pub fn escalate(worker: &str, level: &str) {
    let base = Path::new("/srv/trust_zones");
    fs::create_dir_all(base).ok();
    fs::write(base.join(worker), level).ok();
}

pub fn get_trust(worker: &str) -> String {
    fs::read_to_string(format!("/srv/trust_zones/{worker}")).unwrap_or_else(|_| "green".into()).trim().into()
}

pub fn list_trust() -> Vec<(String, String)> {
    let base = Path::new("/srv/trust_zones");
    if let Ok(entries) = fs::read_dir(base) {
        return entries.filter_map(|e| e.ok()).filter_map(|e| {
            let w = e.file_name().into_string().ok()?;
            let level = fs::read_to_string(e.path()).ok()?;
            Some((w, level.trim().into()))
        }).collect();
    }
    Vec::new()
}
