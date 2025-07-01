
use crate::prelude::*;
// CLASSIFICATION: COMMUNITY
// Filename: net.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

/// Network driver interface for Cohesix kernel runtime.
/// Provides initialization and basic transmit/receive capabilities, to be extended for real NICs.

/// Enumeration of supported network interfaces.
#[derive(Debug, Clone, Copy)]
pub enum NetInterfaceType {
    VirtIO,
    Loopback,
    None,
}

/// Represents the kernel network driver state.
pub struct NetDriver {
    pub interface: NetInterfaceType,
    pub initialized: bool,
    queue: std::collections::VecDeque<Vec<u8>>, // simple loopback buffer
}

impl NetDriver {
    /// Initialize the network interface and driver state.
    pub fn initialize() -> Self {
        use std::env;
        println!("[Net] Initializing network driver...");
        let interface = match env::var("COHESIX_NET_IFACE").as_deref() {
            Ok("virtio") => NetInterfaceType::VirtIO,
            Ok("loopback") => NetInterfaceType::Loopback,
            _ => NetInterfaceType::Loopback,
        };
        NetDriver {
            interface,
            initialized: true,
            queue: std::collections::VecDeque::new(),
        }
    }

    /// Transmit a packet (stub).
    pub fn transmit(&mut self, packet: &[u8]) {
        if !self.initialized {
            println!("[Net] driver not initialized");
            return;
        }
        println!("[Net] Transmitting {} bytes", packet.len());
        if matches!(self.interface, NetInterfaceType::Loopback) {
            self.queue.push_back(packet.to_vec());
        }
    }

    /// Receive a packet (stub).
    pub fn receive(&mut self) -> Option<Vec<u8>> {
        if !self.initialized {
            println!("[Net] driver not initialized");
            return None;
        }
        if let Some(pkt) = self.queue.pop_front() {
            println!("[Net] Received {} bytes", pkt.len());
            Some(pkt)
        } else {
            println!("[Net] no packet available");
            None
        }
    }
}

