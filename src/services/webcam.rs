// CLASSIFICATION: COMMUNITY
// Filename: webcam.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-08-17
#![cfg(not(target_os = "uefi"))]

//! USB webcam service implemented with v4l2.

use crate::cohesix_types::{Role, RoleManifest};
use crate::runtime::ServiceRegistry;
use crate::telemetry::telemetry::emit_kv;
use v4l::prelude::*;
use crate::webcam::capture;
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
            let dev_path = std::env::var("VIDEO_DEVICE").unwrap_or_else(|_| "/dev/video0".into());
            match Device::with_path(&dev_path) {
                Ok(_) => {
                    self.opened = true;
                    println!("[webcam] device {} opened", dev_path);
                }
                Err(e) => {
                    println!("[webcam] failed to open device: {e}; using simulator");
                    emit_kv("webcam", &[("status", "open_failed"), ("device", &dev_path)]);
                }
            }
            std::fs::create_dir_all("/srv/webcam").ok();
            let frame_path = "/srv/webcam/frame.jpg";
            if capture::capture_jpeg(frame_path).is_ok() {
                println!("[webcam] captured initial frame");
            }
            let _ = ServiceRegistry::register_service("webcam", "/srv/webcam");
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
