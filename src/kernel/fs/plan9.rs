// CLASSIFICATION: COMMUNITY
// Filename: plan9.rs v1.1
// Author: Lukas Bower
// Date Modified: 2025-06-20

use crate::prelude::*;
/// Plan 9 file protocol integration layer for Cohesix kernel-space.
/// Provides abstraction and mount logic for exposing namespaces via 9P.

/// Placeholder for a 9P mount entry.
#[derive(Copy, Clone)]
pub struct Plan9Mount {
    pub path: &'static str,
    pub target: &'static str,
}

use once_cell::sync::Lazy;
use std::sync::Mutex;

/// Simulated mount table.
static MOUNT_TABLE: Lazy<Mutex<Vec<Plan9Mount>>> = Lazy::new(|| Mutex::new(Vec::new()));
const MAX_MOUNTS: usize = 8;

/// Mount a namespace path to a target service.
pub fn mount(path: &'static str, target: &'static str) -> bool {
    let mut table = MOUNT_TABLE.lock().unwrap();
    if table.len() >= MAX_MOUNTS {
        println!("[9P] Mount table full");
        return false;
    }
    table.push(Plan9Mount { path, target });
    println!("[9P] Mounted {} → {}", path, target);
    true
}

/// List all active 9P mounts.
pub fn list_mounts() {
    let table = MOUNT_TABLE.lock().unwrap();
    for mount in table.iter() {
        println!("[9P] {} → {}", mount.path, mount.target);
    }
}

/// Number of active mounts (for testing).
pub fn mount_count() -> usize {
    MOUNT_TABLE.lock().unwrap().len()
}

/// Reset mount table (tests only).
pub fn reset_mounts() {
    MOUNT_TABLE.lock().unwrap().clear();
}
