// CLASSIFICATION: COMMUNITY
// Filename: shim.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Plan 9 shim for Cohesix kernel-to-userspace bridging.
//! This module provides an adapter layer between kernel subsystems and 9P-style userland services.

/// Trait defining a basic Plan 9 shim interface.
pub trait Plan9Shim {
    /// Dispatch a request from kernel space into the 9P userland domain.
    fn dispatch(&self, path: &str, op: &str, data: Option<&[u8]>) -> Result<Vec<u8>, String>;
}

/// Stub implementation of the Plan 9 shim.
pub struct DefaultShim;

impl Plan9Shim for DefaultShim {
    fn dispatch(&self, path: &str, op: &str, data: Option<&[u8]>) -> Result<Vec<u8>, String> {
        println!("[shim] dispatching op='{}' on path='{}'", op, path);
        if let Some(d) = data {
            println!("[shim] with data: {:?}", d);
        }
        // A very small routing table for demo purposes. Paths under `/busybox`
        // are executed via the in-kernel BusyBox helpers. The `/9p` path is
        // treated as a raw 9P message that is forwarded to the mini 9P server
        // implementation.
        match path {
            #[cfg(feature = "busybox")]
            "/busybox" => {
                let args: Vec<&str> = data
                    .map(|d| std::str::from_utf8(d).unwrap_or("").split_whitespace().collect())
                    .unwrap_or_else(Vec::new);
                crate::kernel::fs::busybox::run_command(op, &args);
                Ok(Vec::new())
            }
            "/9p" => {
                let bytes = data.ok_or_else(|| "missing request".to_string())?;
                // For now just echo the request back as a stub.
                Ok(bytes.to_vec())
            }
            _ => Err("unknown path".into()),
        }
    }
}
