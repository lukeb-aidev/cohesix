// CLASSIFICATION: COMMUNITY
// Filename: fs.rs v0.1
// Date Modified: 2025-05-24
// Author: Lukas Bower

//! TODO: Implement fs.rs.

// CLASSIFICATION: COMMUNITY
// Filename: lib.rs · cohesix-9p v0.1
// Date Modified: 2025-05-31
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────────────────────
// Cohesix‑9P – Plan‑9 style file‑system service crate
//
// This crate exposes a minimal 9P protocol server intended to be shared by
// Queen and Worker roles.  The current implementation is a *stub* that
// compiles cleanly and provides clear extension points.
//
// # Design Notes
// * No network code yet – the transport layer will be injected later.
// * API kept synchronous for now; will migrate to async once design stabilises.
// * Explicit `TODO` markers call out un‑implemented sections so the hydration
//   linter will catch them.
//
// # Public Surface
// * [`FsConfig`] – runtime configuration (root path, port, etc.).
// * [`FsServer`] – lightweight handle controlling the server lifecycle.
// * [`start_server`] – convenience helper to spawn a server with default opts.
// ─────────────────────────────────────────────────────────────────────────────

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::{path::PathBuf, sync::Arc};

use anyhow::{bail, Result};
use log::info;

/// Configuration options for the 9P file‑system server.
///
/// Extend this struct as new runtime knobs become necessary.
#[derive(Debug, Clone)]
pub struct FsConfig {
    /// Root directory the server exposes as its file tree.
    pub root: PathBuf,
    /// TCP/QUIC port to listen on.
    pub port: u16,
    /// Expose the tree as read‑only if `true`.
    pub readonly: bool,
}

impl Default for FsConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("/"),
            port: 564, // the classic Plan‑9 port
            readonly: false,
        }
    }
}

/// Lightweight handle for a running 9P server.
///
/// The starter implementation does **not** launch a real listener yet; it
/// merely records configuration so unit tests can compile.
#[derive(Debug)]
pub struct FsServer {
    cfg: Arc<FsConfig>,
}

impl FsServer {
    /// Create a new server instance *without* starting it.
    pub fn new(cfg: FsConfig) -> Self {
        Self { cfg: Arc::new(cfg) }
    }

    /// Start serving.  Returns immediately for now.
    ///
    /// TODO: spawn actual network listener + request loop.
    pub fn start(&self) -> Result<()> {
        info!(
            "🔥 starting Cohesix‑9P server on port {} (readonly = {})",
            self.cfg.port, self.cfg.readonly
        );
        // Placeholder – replace with real accept loop.
        Ok(())
    }
}

/// Convenience helper: build a server with [`FsConfig::default`] and start it.
pub fn start_server() -> Result<FsServer> {
    let srv = FsServer::new(FsConfig::default());
    srv.start()?;
    Ok(srv)
}

// ─────────────────────────────── tests ──────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_starts_with_defaults() {
        let srv = start_server().expect("server should start");
        assert_eq!(srv.cfg.port, 564);
    }

    #[test]
    fn custom_config_propagates() {
        let cfg = FsConfig {
            root: "/tmp".into(),
            port: 9999,
            readonly: true,
        };
        let srv = FsServer::new(cfg.clone());
        assert_eq!(srv.cfg.port, cfg.port);
    }
}