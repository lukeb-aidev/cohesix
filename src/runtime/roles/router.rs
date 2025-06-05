// CLASSIFICATION: COMMUNITY
// Filename: router.rs v1.1
// Author: Lukas Bower
// Date Modified: 2025-06-08

//! Role module for the Cohesix `Router`.
//! The router handles message routing, inter-process dispatch, and namespace resolution for worker services.

/// Trait representing router responsibilities.
pub trait RouterRole {
    fn route_message(&self, src: &str, dest: &str, payload: &[u8]) -> Result<(), String>;
    fn resolve_namespace(&self, path: &str) -> Option<String>;
    fn register_service(&mut self, name: &str, endpoint: &str);
}

use std::collections::HashMap;

/// Simple router implementation holding an in-memory service table.
pub struct DefaultRouter {
    routes: HashMap<String, String>,
}

impl Default for DefaultRouter {
    fn default() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }
}

impl RouterRole for DefaultRouter {
    fn route_message(&self, src: &str, dest: &str, payload: &[u8]) -> Result<(), String> {
        println!(
            "[router] routing message from '{}' to '{}': {} bytes",
            src,
            dest,
            payload.len()
        );
        if let Some(endpoint) = self.routes.get(dest) {
            println!(
                "[router] delivered {} bytes from '{}' to endpoint '{}'",
                payload.len(),
                src,
                endpoint
            );
        } else {
            println!("[router] no route for '{}'; dropping payload", dest);
        }
        Ok(())
    }

    fn resolve_namespace(&self, path: &str) -> Option<String> {
        println!("[router] resolving namespace for '{}'", path);
        if let Some(endpoint) = self.routes.get(path) {
            Some(endpoint.clone())
        } else {
            Some(format!("/srv/{}", path))
        }
    }

    fn register_service(&mut self, name: &str, endpoint: &str) {
        println!("[router] registering service '{}' at '{}'", name, endpoint);
        self.routes.insert(name.to_string(), endpoint.to_string());
    }
}
