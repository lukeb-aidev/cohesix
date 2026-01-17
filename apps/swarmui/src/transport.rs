// Author: Lukas Bower
// Purpose: Provide SwarmUI Secure9P transport implementations.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use cohsh_core::Secure9pTransport;

/// Errors returned by the SwarmUI TCP transport.
#[derive(Debug)]
pub enum TcpTransportError {
    /// TCP IO error.
    Io(std::io::Error),
    /// Frame length invalid.
    InvalidLength {
        /// Length declared in the frame header.
        declared: u32,
        /// Maximum frame length accepted by this transport.
        max: u32,
    },
    /// Response frame truncated.
    Truncated {
        /// Frame length declared by the peer.
        declared: u32,
        /// Bytes received before the stream ended.
        received: usize,
    },
}

impl std::fmt::Display for TcpTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TcpTransportError::Io(err) => write!(f, "tcp io error: {err}"),
            TcpTransportError::InvalidLength { declared, max } => {
                write!(f, "invalid frame length {declared} (max {max})")
            }
            TcpTransportError::Truncated { declared, received } => {
                write!(
                    f,
                    "truncated frame (declared {declared} bytes, got {received})"
                )
            }
        }
    }
}

impl std::error::Error for TcpTransportError {}

impl From<std::io::Error> for TcpTransportError {
    fn from(err: std::io::Error) -> Self {
        TcpTransportError::Io(err)
    }
}

/// Secure9P transport over a TCP stream.
#[derive(Debug)]
pub struct TcpTransport {
    stream: TcpStream,
    max_frame_len: u32,
}

impl TcpTransport {
    /// Connect to a Secure9P TCP endpoint.
    pub fn connect(
        host: &str,
        port: u16,
        timeout: Duration,
        max_frame_len: u32,
    ) -> Result<Self, TcpTransportError> {
        let addr = (host, port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "no socket address"))?;
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            max_frame_len,
        })
    }
}

impl Secure9pTransport for TcpTransport {
    type Error = TcpTransportError;

    fn exchange(&mut self, batch: &[u8]) -> Result<Vec<u8>, Self::Error> {
        if batch.len() < 4 {
            return Err(TcpTransportError::InvalidLength {
                declared: 0,
                max: self.max_frame_len,
            });
        }
        let declared = u32::from_le_bytes(batch[0..4].try_into().expect("len checked"));
        if declared as usize != batch.len() || declared > self.max_frame_len || declared < 4 {
            return Err(TcpTransportError::InvalidLength {
                declared,
                max: self.max_frame_len,
            });
        }
        self.stream.write_all(batch)?;
        self.stream.flush()?;

        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf)?;
        let response_len = u32::from_le_bytes(len_buf);
        if response_len < 4 || response_len > self.max_frame_len {
            return Err(TcpTransportError::InvalidLength {
                declared: response_len,
                max: self.max_frame_len,
            });
        }
        let payload_len = response_len.saturating_sub(4) as usize;
        let mut payload = vec![0u8; payload_len];
        self.stream.read_exact(&mut payload)?;
        if payload.len() != payload_len {
            return Err(TcpTransportError::Truncated {
                declared: response_len,
                received: payload.len().saturating_add(4),
            });
        }
        let mut response = Vec::with_capacity(response_len as usize);
        response.extend_from_slice(&len_buf);
        response.extend_from_slice(&payload);
        Ok(response)
    }
}

/// Factory for TCP-backed Secure9P transports.
#[derive(Debug, Clone)]
pub struct TcpTransportFactory {
    host: String,
    port: u16,
    timeout: Duration,
    max_frame_len: u32,
}

impl TcpTransportFactory {
    /// Create a new TCP transport factory.
    pub fn new(host: impl Into<String>, port: u16, timeout: Duration, max_frame_len: u32) -> Self {
        Self {
            host: host.into(),
            port,
            timeout,
            max_frame_len,
        }
    }

    /// Build a new TCP transport.
    pub fn build(&self) -> Result<TcpTransport, TcpTransportError> {
        TcpTransport::connect(&self.host, self.port, self.timeout, self.max_frame_len)
    }
}
