// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v0.2
// Date Modified: 2025-07-22
// Author: Lukas Bower

//! Minimal filesystem layer for Cohesix-9P.
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
// * Explicit notes call out unimplemented sections so the hydration
//   linter will catch them.
//
// # Public Surface
// * [`FsConfig`] – runtime configuration (root path, port, etc.).
// * [`FsServer`] – lightweight handle controlling the server lifecycle.
// * [`start_server`] – convenience helper to spawn a server with default opts.
// ─────────────────────────────────────────────────────────────────────────────

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::PathBuf;

use anyhow::{Result, bail};
// Note: we avoid using private modules from the `p9` crate for now.

pub mod fs;
mod server;
pub use server::FsServer;
pub mod ninep_adapter;
pub mod policy;

/// Enforce capability checks based on the active Cohesix role.
pub fn enforce_capability(action: &str) -> Result<()> {
    let role = std::fs::read_to_string("/srv/cohrole").unwrap_or_default();
    let role = role.trim();
    if role == "QueenPrimary" {
        return Ok(());
    }
    if role == "DroneWorker" && action.contains("remote") {
        bail!("capability denied");
    }
    Ok(())
}

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

/// Convenience helper: build a server with [`FsConfig::default`] and start it.
pub fn start_server() -> Result<FsServer> {
    let mut srv = FsServer::new(FsConfig::default());
    srv.start()?;
    Ok(srv)
}

/// Parse a 9P version negotiation frame and return the version string.
pub fn parse_version_message(buf: &[u8]) -> Result<String> {
    if buf.is_empty() {
        bail!("empty message");
    }
    let s = std::str::from_utf8(buf)?.trim_end_matches('\0').to_string();
    Ok(s)
}

// ─────────────────────────────── tests ──────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use ninep::fs::{Mode, Perm};
    use std::{thread, time::Duration};

    #[test]
    fn server_starts_with_defaults() {
        let _srv = start_server().unwrap_or_else(|e| panic!("server should start: {e}"));
    }

    #[test]
    fn custom_config_propagates() {
        let cfg = FsConfig {
            root: "/tmp".into(),
            port: 9999,
            readonly: true,
        };
        let mut srv = FsServer::new(cfg.clone());
        srv.start().unwrap_or_else(|e| panic!("start failed: {e}"));
    }

    #[test]
    fn parse_version_message_ok() {
        let buf = b"9P2000.L";
        let parsed = parse_version_message(buf).unwrap_or_else(|e| panic!("parse error: {e}"));
        assert_eq!(parsed, "9P2000.L");
    }

    #[test]
    fn unix_socket_roundtrip() {
        let mut srv = FsServer::new(FsConfig {
            port: 5660,
            ..Default::default()
        });
        srv.start()
            .unwrap_or_else(|e| panic!("start socket failed: {e}"));
        thread::sleep(Duration::from_millis(100));

        let mut cli = ninep::client::TcpClient::new_tcp("tester".to_string(), "127.0.0.1:5660", "")
            .unwrap_or_else(|e| panic!("connect failed: {e}"));
        cli.create("/", "foo", Perm::OWNER_READ | Perm::OWNER_WRITE, Mode::FILE)
            .unwrap_or_else(|e| panic!("create failed: {e}"));
        let st = cli
            .stat("/foo")
            .unwrap_or_else(|e| panic!("stat failed: {e}"));
        println!("created file size {}", st.n_bytes);
    }
}
