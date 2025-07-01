// CLASSIFICATION: COMMUNITY
// Filename: srv_registry.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-22

use crate::prelude::*;
//! Registry for Plan 9 style services mounted under `/srv`.

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;

#[derive(Clone, Debug)]
pub struct SrvEndpoint {
    pub fd: i32,
}

static REGISTRY: Lazy<Mutex<HashMap<String, SrvEndpoint>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn register_srv(name: &str, fd: i32) {
    REGISTRY
        .lock()
        .unwrap()
        .insert(name.to_string(), SrvEndpoint { fd });
}

pub fn lookup_srv(name: &str) -> Option<SrvEndpoint> {
    REGISTRY.lock().unwrap().get(name).cloned()
}
