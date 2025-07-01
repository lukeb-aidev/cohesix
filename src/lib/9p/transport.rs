// Minimal in-memory transport implementation for tests and examples.

use crate::prelude::*;
// CLASSIFICATION: COMMUNITY
// Filename: transport.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! 9P transport layer for Cohesix.
//! Responsible for sending and receiving 9P messages over an abstract I/O channel.

use super::protocol::{P9Message, parse_message, serialize_message};

/// Trait defining a generic 9P transport interface.
pub trait Transport {
    fn send(&mut self, message: &P9Message) -> Result<(), String>;
    fn receive(&mut self) -> Result<P9Message, String>;
}

/// Stub transport using in-memory buffers (placeholder for testing).
pub struct InMemoryTransport {
    pub queue: Vec<Vec<u8>>, // simple FIFO of raw messages
}

impl InMemoryTransport {
    pub fn new() -> Self {
        InMemoryTransport { queue: Vec::new() }
    }
}

impl Transport for InMemoryTransport {
    fn send(&mut self, message: &P9Message) -> Result<(), String> {
        let bytes = serialize_message(message);
        self.queue.push(bytes);
        println!("[9P] Sent: {:?}", message);
        Ok(())
    }

    fn receive(&mut self) -> Result<P9Message, String> {
        if let Some(bytes) = self.queue.pop() {
            let msg = parse_message(&bytes);
            println!("[9P] Received: {:?}", msg);
            Ok(msg)
        } else {
            Err("no message".into())
        }
    }
}