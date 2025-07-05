// CLASSIFICATION: COMMUNITY
// Filename: job_manager.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

use alloc::vec::Vec;
use spin::Mutex;
use super::runtime::CudaExecutor;
use std::thread;

/// Represents a single CUDA job with isolation context.
pub struct CudaJob {
    pub id: usize,
    pub ptx: Vec<u8>,
}

pub struct CudaJobManager {
    jobs: Mutex<Vec<CudaJob>>, 
    next_id: Mutex<usize>,
}

impl CudaJobManager {
    pub fn new() -> Self {
        Self { jobs: Mutex::new(Vec::new()), next_id: Mutex::new(0) }
    }

    pub fn submit(&self, ptx: Vec<u8>) {
        let mut id_lock = self.next_id.lock();
        let job_id = *id_lock;
        *id_lock += 1;
        drop(id_lock);
        let job = CudaJob { id: job_id, ptx };
        self.jobs.lock().push(job);
        let jobs = self.jobs.clone();
        thread::spawn(move || {
            if let Some(mut job) = jobs.lock().iter_mut().find(|j| j.id == job_id) {
                let mut exec = CudaExecutor::new();
                if exec.load_kernel(Some(&job.ptx)).is_ok() {
                    let _ = exec.launch();
                }
            }
            jobs.lock().retain(|j| j.id != job_id);
        });
    }
}
