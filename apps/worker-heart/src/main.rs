// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the worker-heart binary entrypoint.
// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![doc = "Heartbeat worker entry points for host and seL4 builds."]

#[cfg(target_os = "none")]
mod kernel;

#[cfg(not(target_os = "none"))]
use anyhow::Result;

#[cfg(not(target_os = "none"))]
fn main() -> Result<()> {
    use secure9p_codec::SessionId;
    use worker_heart::HeartbeatWorker;

    let worker = HeartbeatWorker::new(SessionId::from_raw(0));
    let payload = worker.emit(0)?;
    println!("{payload}");
    Ok(())
}
