// CLASSIFICATION: COMMUNITY
// Filename: webcam.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

//! USB webcam service implemented with v4l2.

use crate::cohesix_types::{Role, RoleManifest};
use crate::runtime::ServiceRegistry;
use v4l::prelude::*;
use super::Service;

/// Webcam frame streaming service.
#[derive(Default)]
pub struct WebcamService {
    opened: bool,
}

impl Service for WebcamService {
    fn name(&self) -> &'static str { "WebcamService" }

    fn init(&mut self) {
        let role = RoleManifest::current_role();
        if matches!(role, Role::DroneWorker | Role::SensorRelay) {
            match Device::new(0) {
                Ok(_) => {
                    self.opened = true;
                    println!("[webcam] device /dev/video0 opened");
                }
                Err(e) => {
                    println!("[webcam] failed to open device: {e}; using simulator");
                }
            }
            std::fs::create_dir_all("/srv/webcam").ok();
            ServiceRegistry::register_service("webcam", "/srv/webcam");
        } else {
            println!("[webcam] service disabled for role {role:?}");
        }
    }

    fn shutdown(&mut self) {
        if self.opened {
            println!("[webcam] shutting down");
            self.opened = false;
        }
    }
}
