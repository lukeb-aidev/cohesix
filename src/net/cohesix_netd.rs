// CLASSIFICATION: COMMUNITY
// Filename: cohesix_netd.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-13

//! Network daemon providing 9P over TCP with discovery and HTTP fallback.

use crate::validator::{self, RuleViolation};
use chrono::Utc;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};

/// Cohesix network daemon.
pub struct CohesixNetd {
    /// TCP port to listen on for 9P.
    pub port: u16,
    /// UDP port used for discovery broadcasts.
    pub discovery_port: u16,
}

impl CohesixNetd {
    /// Create a new daemon with default ports.
    pub fn new() -> Self {
        Self { port: 564, discovery_port: 9864 }
    }

    /// Run the daemon indefinitely.
    pub fn run(&self) -> anyhow::Result<()> {
        self.broadcast_presence()?;
        match TcpListener::bind(("0.0.0.0", self.port)) {
            Ok(listener) => {
                log_event("tcp_listen")?;
                for stream in listener.incoming() {
                    match stream {
                        Ok(mut s) => {
                            log_event("tcp_accept")?;
                            if let Err(e) = self.handle_stream(&mut s) {
                                log_event(&format!("tcp_handle_error {e}"))?;
                            }
                        }
                        Err(e) => {
                            log_event(&format!("tcp_accept_error {e}"))?;
                        }
                    }
                }
            }
            Err(e) => {
                log_event(&format!("tcp_bind_failed {e}"))?;
                validator::log_violation(RuleViolation {
                    type_: "net",
                    file: "cohesix_netd.rs".into(),
                    agent: "cohesix_netd".into(),
                    time: validator::timestamp(),
                });
                self.http_fallback("http://127.0.0.1:8064")?;
            }
        }
        Ok(())
    }

    /// Handle a single TCP stream carrying a 9P message.
    pub fn handle_stream(&self, stream: &mut TcpStream) -> anyhow::Result<()> {
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        stream.write_all(&buf[..n])?;
        Ok(())
    }

    /// Broadcast presence for discovery.
    pub fn broadcast_presence(&self) -> std::io::Result<()> {
        let socket = UdpSocket::bind(("127.0.0.1", 0))?;
        socket.set_broadcast(true)?;
        let msg = b"cohesix_netd_discovery";
        socket.send_to(msg, ("127.0.0.1", self.discovery_port))?;
        log_event("broadcast_presence")?;
        Ok(())
    }

    /// Listen for one discovery packet.
    pub fn listen_discovery_once(&self) -> std::io::Result<Vec<u8>> {
        let socket = UdpSocket::bind(("127.0.0.1", self.discovery_port))?;
        let mut buf = [0u8; 512];
        let (n, _) = socket.recv_from(&mut buf)?;
        log_event("discovery_recv")?;
        Ok(buf[..n].to_vec())
    }

    /// Send an HTTP POST as fallback if TCP fails.
    pub fn http_fallback(&self, url: &str) -> anyhow::Result<()> {
        log_event("http_fallback")?;
        let resp = ureq::post(url).send_string("fallback")?;
        if !(200..300).contains(&resp.status()) {
            validator::log_violation(RuleViolation {
                type_: "net",
                file: "cohesix_netd.rs".into(),
                agent: "cohesix_netd".into(),
                time: validator::timestamp(),
            });
        }
        Ok(())
    }
}

fn log_event(msg: &str) -> std::io::Result<()> {
    let ts = Utc::now().to_rfc3339();
    std::fs::create_dir_all("/srv/network")?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/network/events.log")?;
    writeln!(f, "[{}] {}", ts, msg)
}
