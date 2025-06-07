// CLASSIFICATION: COMMUNITY
// Filename: multiplexer.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

//! Simple 9P multiplexer routing requests to registered services.
//! Includes an async router used by the Go helper via a channel.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::runtime::ipc::p9::{P9Request, P9Response, P9Server};

/// Multiplexer allowing multiple services to mount under `/srv/`.
pub struct Multiplexer {
    services: Mutex<HashMap<String, Arc<dyn P9Server + Send + Sync>>>,
    /// Optional channel to receive requests from external processes.
    rx: Mutex<Option<UnboundedReceiver<P9Request>>>,
}

impl Multiplexer {
    /// Create a new empty multiplexer.
    pub fn new() -> Self {
        Self { services: Mutex::new(HashMap::new()), rx: Mutex::new(None) }
    }

    /// Register a named service.
    pub fn register_service(&self, name: &str, svc: Arc<dyn P9Server + Send + Sync>) {
        self.services.lock().unwrap().insert(name.to_string(), svc);
    }

    /// Attach an incoming request channel used by the async router.
    pub fn attach_channel(&self, rx: UnboundedReceiver<P9Request>) {
        *self.rx.lock().unwrap() = Some(rx);
    }

    fn dispatch(&self, path: &str, f: impl FnOnce(&dyn P9Server, &str) -> P9Response) -> P9Response {
        let parts: Vec<&str> = path.trim_start_matches('/').splitn(2, '/').collect();
        if parts.len() < 2 {
            return P9Response::RError("invalid path".into());
        }
        let svc_name = parts[0];
        let sub = parts[1];
        let map = self.services.lock().unwrap();
        if let Some(svc) = map.get(svc_name) {
            f(svc.as_ref(), sub)
        } else {
            P9Response::RError(format!("service {} not found", svc_name))
        }
    }

    /// Handle a 9P request by routing based on the path.
    pub fn handle(&self, req: P9Request) -> P9Response {
        match req {
            P9Request::TRead(p) => self.dispatch(&p, |svc, sub| svc.handle(P9Request::TRead(sub.to_string()))),
            P9Request::TWrite(p, data) => self.dispatch(&p, |svc, sub| svc.handle(P9Request::TWrite(sub.to_string(), data))),
            P9Request::TOpen(p) => self.dispatch(&p, |svc, sub| svc.handle(P9Request::TOpen(sub.to_string()))),
            P9Request::TStat(p) => self.dispatch(&p, |svc, sub| svc.handle(P9Request::TStat(sub.to_string()))),
        }
    }

    /// Start an async router loop reading requests from the attached channel.
    pub async fn serve(&self) {
        let mut rx_opt = self.rx.lock().unwrap().take();
        if let Some(ref mut rx) = rx_opt {
            while let Some(req) = rx.recv().await {
                let _ = self.handle(req);
            }
        }
    }
}
