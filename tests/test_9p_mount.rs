// CLASSIFICATION: COMMUNITY
// Filename: test_9p_mount.rs v0.1
// Date Modified: 2025-06-25
// Author: Cohesix Codex

use cohesix::p9::multiplexer::Multiplexer;
use cohesix::runtime::ipc::p9::{P9Request, P9Response, P9Server};
use std::sync::Arc;

struct EchoServer;
impl P9Server for EchoServer {
    fn handle(&self, req: P9Request) -> P9Response {
        match req {
            P9Request::TRead(_) => P9Response::RRead(b"echo".to_vec()),
            _ => P9Response::RError("unsupported".into()),
        }
    }
}

#[test]
fn multiplexer_routes_services() {
    let mux = Multiplexer::new();
    mux.register_service("echo", Arc::new(EchoServer));
    let resp = mux.handle(P9Request::TRead("/echo/msg".into()));
    match resp {
        P9Response::RRead(data) => assert_eq!(data, b"echo"),
        _ => panic!("unexpected"),
    }
}
