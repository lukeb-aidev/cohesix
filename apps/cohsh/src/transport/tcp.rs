// Author: Lukas Bower
//! TCP transport backend for the Cohesix shell console.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::Role;
use secure9p_wire::SessionId;

use crate::{Session, Transport};

/// Default TCP timeout applied to socket operations.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// TCP transport speaking the root-task console protocol.
#[derive(Debug)]
pub struct TcpTransport {
    address: String,
    port: u16,
    timeout: Duration,
    stream: Option<TcpStream>,
    reader: Option<BufReader<TcpStream>>,
    next_session_id: u64,
}

impl TcpTransport {
    /// Create a new transport targeting the provided endpoint.
    pub fn new(address: impl Into<String>, port: u16) -> Self {
        Self {
            address: address.into(),
            port,
            timeout: DEFAULT_TIMEOUT,
            stream: None,
            reader: None,
            next_session_id: 2,
        }
    }

    fn connect(&self) -> Result<TcpStream> {
        let socket_addr = (self.address.as_str(), self.port)
            .to_socket_addrs()
            .context("invalid TCP endpoint")?
            .next()
            .ok_or_else(|| anyhow!("no TCP addresses resolved"))?;
        let stream = TcpStream::connect(socket_addr).context("failed to connect to TCP console")?;
        stream
            .set_read_timeout(Some(self.timeout))
            .context("failed to configure read timeout")?;
        stream
            .set_write_timeout(Some(self.timeout))
            .context("failed to configure write timeout")?;
        Ok(stream)
    }

    fn ensure_connection(&mut self) -> Result<()> {
        if self.stream.is_none() {
            let stream = self.connect()?;
            let reader_stream = stream.try_clone().context("failed to clone TCP stream")?;
            self.reader = Some(BufReader::new(reader_stream));
            self.stream = Some(stream);
        }
        Ok(())
    }

    fn send_line(&mut self, line: &str) -> Result<()> {
        let stream = self
            .stream
            .as_mut()
            .context("attach to the TCP transport before issuing commands")?;
        stream
            .write_all(line.as_bytes())
            .context("failed to write to TCP transport")?;
        stream
            .write_all(b"\n")
            .context("failed to terminate TCP line")?;
        stream.flush().context("failed to flush TCP transport")
    }

    fn read_line(&mut self) -> Result<String> {
        let reader = self
            .reader
            .as_mut()
            .context("attach to the TCP transport before reading")?;
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .context("failed to read from TCP transport")?;
        if bytes == 0 {
            return Err(anyhow!("connection closed by peer"));
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_owned())
    }

    fn role_label(role: Role) -> &'static str {
        match role {
            Role::Queen => "queen",
            Role::WorkerHeartbeat => "worker-heartbeat",
            Role::WorkerGpu => "worker-gpu",
        }
    }
}

impl Transport for TcpTransport {
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        self.ensure_connection()?;
        let ticket_fragment = ticket.unwrap_or("");
        let command = format!("ATTACH {} {}", Self::role_label(role), ticket_fragment);
        self.send_line(&command)?;
        let response = self.read_line()?;
        if !response.starts_with("OK") {
            return Err(anyhow!("remote attach failed: {response}"));
        }
        let session = Session::new(SessionId::from_raw(self.next_session_id), role);
        self.next_session_id = self.next_session_id.wrapping_add(1);
        Ok(session)
    }

    fn tail(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        self.send_line(&format!("TAIL {path}"))?;
        let mut lines = Vec::new();
        loop {
            let line = self.read_line()?;
            if line == "END" {
                break;
            }
            lines.push(line);
        }
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn attaches_and_tails() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("ATTACH queen"));
            writeln!(stream, "OK session").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("TAIL /log/queen.log"));
            writeln!(stream, "line one").unwrap();
            writeln!(stream, "line two").unwrap();
            writeln!(stream, "END").unwrap();
        });

        let mut transport = TcpTransport::new("127.0.0.1", port);
        let session = transport.attach(Role::Queen, None).unwrap();
        let lines = transport.tail(&session, "/log/queen.log").unwrap();
        assert_eq!(lines, vec!["line one".to_owned(), "line two".to_owned()]);
    }
}
