// CLASSIFICATION: COMMUNITY
// Filename: queen.rs v1.0
// Author: Lukas Bower
// Date Modified: 2029-01-15
#![cfg(feature = "std")]

use crate::queen::orchestrator::{QueenOrchestrator, SchedulePolicy};
use crate::CohError;
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use std::net::SocketAddr;

/// Compatibility shim that wraps the gRPC-based queen orchestrator.
pub struct Queen {
    orchestrator: QueenOrchestrator,
}

impl Queen {
    /// Construct a queen orchestrator with the provided heartbeat timeout.
    pub fn new(timeout_secs: u64) -> Result<Self, CohError> {
        Ok(Self {
            orchestrator: QueenOrchestrator::new(timeout_secs, SchedulePolicy::RoundRobin),
        })
    }

    /// Start serving the gRPC control plane on the supplied address.
    pub async fn serve(self, addr: SocketAddr) -> Result<(), CohError> {
        self.orchestrator.clone().serve(addr).await
    }

    /// Access the underlying orchestrator state.
    pub fn inner(&self) -> QueenOrchestrator {
        self.orchestrator.clone()
    }
}
