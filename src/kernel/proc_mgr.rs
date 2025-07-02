// CLASSIFICATION: COMMUNITY
// Filename: proc_mgr.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-27

use crate::prelude::*;
/// Minimal userspace process manager for the Cohesix kernel.
use once_cell::sync::Lazy;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Exited,
    Terminated,
}

pub struct CohProc {
    pub pid: u32,
    pub name: &'static str,
    pub entry_point: usize,
    pub state: ProcessState,
    pub exit_code: Option<u32>,
    pub stack: [u8; 4096],
}

use std::sync::atomic::{AtomicU32, Ordering};

static PROC_TABLE: Lazy<Mutex<Vec<CohProc>>> = Lazy::new(|| Mutex::new(Vec::new()));
static CURRENT_PID: AtomicU32 = AtomicU32::new(0);

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
        exit_code: None,
        stack: [0u8; 4096],
    };
    table.push(proc);
    pid
}

pub fn set_current(pid: u32) {
    CURRENT_PID.store(pid, Ordering::SeqCst);
}

pub fn current_pid() -> u32 {
    CURRENT_PID.load(Ordering::SeqCst)
}

pub fn update_state(pid: u32, state: ProcessState) {
    let mut table = PROC_TABLE.lock().unwrap();
    if let Some(p) = table.iter_mut().find(|p| p.pid == pid) {
        p.state = state;
    }
}

pub fn mark_exited(pid: u32, code: u32) {
    let mut table = PROC_TABLE.lock().unwrap();
    if let Some(p) = table.iter_mut().find(|p| p.pid == pid) {
        p.state = ProcessState::Exited;
        p.exit_code = Some(code);
    }
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
