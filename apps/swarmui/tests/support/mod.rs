// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide lightweight test transports for SwarmUI integration tests.
// Author: Lukas Bower
#![allow(dead_code)]

/// In-process NineDoor transport used by SwarmUI integration tests.
pub struct TestInProcessTransport {
    connection: nine_door::InProcessConnection,
}

impl TestInProcessTransport {
    /// Wrap a NineDoor in-process connection.
    pub fn new(connection: nine_door::InProcessConnection) -> Self {
        Self { connection }
    }
}

impl cohsh_core::Secure9pTransport for TestInProcessTransport {
    type Error = nine_door::NineDoorError;

    fn exchange(&mut self, batch: &[u8]) -> Result<Vec<u8>, Self::Error> {
        self.connection.exchange_batch(batch)
    }
}

/// Transport placeholder for tests that assert connect is never reached.
pub struct RejectTransport;

impl cohsh_core::Secure9pTransport for RejectTransport {
    type Error = std::convert::Infallible;

    fn exchange(&mut self, _batch: &[u8]) -> Result<Vec<u8>, Self::Error> {
        unreachable!("RejectTransport must not exchange Secure9P frames")
    }
}
