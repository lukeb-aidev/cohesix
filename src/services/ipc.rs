// CLASSIFICATION: COMMUNITY
// Filename: ipc.rs v1.0
// Date Modified: 2025-06-02
// Author: Lukas Bower

/// IPC service exposing a 9P server stub
use super::Service;
use crate::prelude::*;
use crate::runtime::ipc::P9Server;
use crate::runtime::ipc::{P9Request, P9Response, StubP9Server};

#[derive(Default)]
pub struct IpcService {
    server: StubP9Server,
}

impl Service for IpcService {
    fn name(&self) -> &'static str {
        "IpcService"
    }

    fn init(&mut self) {
        println!("[ipc] starting stub 9P server");
    }

    fn shutdown(&mut self) {
        println!("[ipc] shutting down stub 9P server");
    }
}

impl IpcService {
    /// Forward a request to the underlying 9P server.
    pub fn handle(&self, req: P9Request) -> P9Response {
        self.server.handle(req)
    }
}
