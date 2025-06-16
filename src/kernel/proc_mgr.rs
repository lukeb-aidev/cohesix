// CLASSIFICATION: COMMUNITY
// Filename: proc_mgr.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-26

//! Minimal userspace process manager for the Cohesix kernel.

use once_cell::sync::Lazy;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Terminated,
}

pub struct CohProc {
    pub pid: u32,
    pub name: &'static str,
    pub entry_point: usize,
    pub state: ProcessState,
    pub stack: [u8; 4096],
}

static PROC_TABLE: Lazy<Mutex<Vec<CohProc>>> = Lazy::new(|| Mutex::new(Vec::new()));

pub fn spawn(name: &'static str, entry_point: usize) -> u32 {
    let mut table = PROC_TABLE.lock().unwrap();
    if table.len() >= 8 {
        return 0;
    }
    let pid = (table.len() as u32) + 1;
    let proc = CohProc {
        pid,
        name,
        entry_point,
        state: ProcessState::Ready,
        stack: [0u8; 4096],
    };
    table.push(proc);
    pid
}

pub fn kill(pid: u32) {
    let mut table = PROC_TABLE.lock().unwrap();
    table.retain(|p| p.pid != pid);
}

pub fn list() -> Vec<String> {
    PROC_TABLE
        .lock()
        .unwrap()
        .iter()
        .map(|p| format!("{}:{}", p.pid, p.name))
        .collect()
}
