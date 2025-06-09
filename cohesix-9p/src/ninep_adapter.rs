// CLASSIFICATION: COMMUNITY
// Filename: ninep_adapter.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-16

//! Compatibility helpers for the `ninep` crate.
//!
//! The upstream library lacks a few convenience methods that our server
//! implementation relies on. This module provides small wrappers around the
//! public API to keep `server.rs` readable and isolated from version churn.

use ninep::{client::TcpClient, fs::Mode};
use std::io;

/// Read a slice of bytes from `path` using the provided client.
///
/// The upstream client only exposes full-file reads, so this helper
/// fetches the entire file and then returns the requested subset.
pub fn read_slice(
    client: &mut TcpClient,
    path: &str,
    offset: usize,
    count: usize,
) -> io::Result<Vec<u8>> {
    let data = client.read(path.to_string())?;
    Ok(data.into_iter().skip(offset).take(count).collect())
}

/// Ensure a file exists by walking and stat-ing it.
pub fn verify_open(client: &mut TcpClient, path: &str, _mode: Mode) -> io::Result<()> {
    let _ = client.walk(path.to_string())?;
    let _ = client.stat(path.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FsConfig, server::FsServer};
    use ninep::fs::{Perm, Mode};
    use ninep::client::TcpClient;
    use serial_test::serial;

    fn start() -> (FsServer, TcpClient) {
        let mut srv = FsServer::new(FsConfig { root: "/".into(), port: 5655, readonly: false });
        srv.start().unwrap_or_else(|e| panic!("server start failed: {e}"));
        std::thread::sleep(std::time::Duration::from_millis(100));
        let client = TcpClient::new_tcp("tester".to_string(), "127.0.0.1:5655", "")
            .unwrap_or_else(|e| panic!("client connect failed: {e}"));
        (srv, client)
    }

    #[test]
    #[serial]
    fn slice_read() {
        let (mut srv, mut cli) = start();
        cli.create("/", "file", Perm::OWNER_READ | Perm::OWNER_WRITE, Mode::FILE)
            .unwrap_or_else(|e| panic!("create failed: {e}"));
        cli.write("/file", 0, b"abcdef")
            .unwrap_or_else(|e| panic!("write failed: {e}"));
        let slice = read_slice(&mut cli, "/file", 2, 3)
            .unwrap_or_else(|e| panic!("read slice failed: {e}"));
        assert_eq!(slice, b"cde");
        let _ = srv; // drop after test
    }
}

