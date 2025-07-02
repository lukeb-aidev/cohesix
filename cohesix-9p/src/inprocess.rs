// CLASSIFICATION: COMMUNITY
// Filename: inprocess.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

use crossbeam_channel::{Receiver, Sender, unbounded};
use ninep::Stream;
use std::io::{self, Read, Write};
use std::sync::Arc;

/// In-process byte stream implemented with crossbeam channels.
#[derive(Clone)]
pub struct InProcessStream {
    rx: Receiver<Vec<u8>>,
    tx: Sender<Vec<u8>>,
    buffer: Arc<std::sync::Mutex<Vec<u8>>>,
}

impl InProcessStream {
    /// Create paired streams for bidirectional communication.
    pub fn pair() -> (Self, Self) {
        let (a_tx, a_rx) = unbounded();
        let (b_tx, b_rx) = unbounded();
        (
            Self {
                rx: a_rx,
                tx: b_tx.clone(),
                buffer: Arc::new(std::sync::Mutex::new(Vec::new())),
            },
            Self {
                rx: b_rx,
                tx: a_tx.clone(),
                buffer: Arc::new(std::sync::Mutex::new(Vec::new())),
            },
        )
    }
}

impl Read for InProcessStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut local = self.buffer.lock().unwrap();
        if local.is_empty() {
            match self.rx.recv() {
                Ok(data) => *local = data,
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(local.len());
        buf[..n].copy_from_slice(&local[..n]);
        local.drain(..n);
        Ok(n)
    }
}

impl Write for InProcessStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let data = buf.to_vec();
        self.tx
            .send(data)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "closed"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Stream for InProcessStream {
    fn try_clone(&self) -> ninep::Result<Self> {
        Ok(self.clone())
    }
}
