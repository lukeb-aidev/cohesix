// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the worker-gpu binary entrypoint.
// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![doc = "GPU worker entry points for host and seL4 builds."]

#[cfg(target_os = "none")]
mod kernel;

#[cfg(not(target_os = "none"))]
use anyhow::Result;

#[cfg(not(target_os = "none"))]
fn main() -> Result<()> {
    use secure9p_codec::SessionId;
    use worker_gpu::{GpuLease, GpuWorker};

    let lease = GpuLease::new("GPU-0", 256, 1, 60, 5, "worker-gpu-demo")?;
    let worker = GpuWorker::new(SessionId::from_raw(0), lease);
    let descriptor = worker.vector_add(&[1.0f32, 2.0, 3.0], &[4.0f32, 5.0, 6.0])?;
    println!(
        "job={} kernel={} hash={}",
        descriptor.job, descriptor.kernel, descriptor.bytes_hash
    );
    Ok(())
}
