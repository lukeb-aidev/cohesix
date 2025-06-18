// CLASSIFICATION: COMMUNITY
// Filename: multiplexer.rs v0.4
// Author: Lukas Bower
// Date Modified: 2025-06-18
#![cfg(not(target_os = "uefi"))]

//! Concurrent 9P request multiplexer.
//!
//! Registered services are mounted under `/srv/<name>` and are
//! looked up by prefix when handling incoming requests. The
//! multiplexer supports both synchronous and asynchronous handling
//! and is used by the Go helper via a channel.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::runtime::ipc::p9::{P9Request, P9Response, P9Server};

/// Multiplexer routing 9P requests to named services.
pub struct Multiplexer {
    services: Arc<Mutex<HashMap<String, Arc<dyn P9Server + Send + Sync>>>>,
    rx: Mutex<Option<UnboundedReceiver<P9Request>>>,
}

impl Multiplexer {
    /// Create a new shared multiplexer.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            services: Arc::new(Mutex::new(HashMap::new())),
            rx: Mutex::new(None),
        })
    }

    /// Register a service under `/srv/<name>`.
    pub fn register_service(&self, name: &str, svc: Arc<dyn P9Server + Send + Sync>) {
        self.services.lock().unwrap().insert(name.to_string(), svc);
    }

    /// Attach an async channel used to receive requests from the Go side.
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

    /// Handle a 9P request synchronously.
    pub fn handle(&self, req: P9Request) -> P9Response {
        match req {
            P9Request::TRead(p) => self.dispatch(&p, |svc, sub| svc.handle(P9Request::TRead(sub.to_string()))),
            P9Request::TWrite(p, data) => {
                self.dispatch(&p, |svc, sub| svc.handle(P9Request::TWrite(sub.to_string(), data)))
            }
            P9Request::TOpen(p) => self.dispatch(&p, |svc, sub| svc.handle(P9Request::TOpen(sub.to_string()))),
            P9Request::TStat(p) => self.dispatch(&p, |svc, sub| svc.handle(P9Request::TStat(sub.to_string()))),
        }
    }

    /// Serve incoming requests on the attached channel.
    pub async fn serve(&self) {
        let mut rx_opt: Option<UnboundedReceiver<P9Request>> =
            self.rx.lock().unwrap().take();
        if let Some(ref mut rx) = rx_opt {
            while let Some(req) = rx.recv().await {
                let _ = self.handle(req);
            }
        }
    }

    /// Spawn a thread handling the request and return join handle.
    pub fn handle_async(self: Arc<Self>, req: P9Request) -> std::thread::JoinHandle<P9Response> {
        std::thread::spawn(move || self.handle(req))
    }
}
