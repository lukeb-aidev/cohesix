// CLASSIFICATION: COMMUNITY
// Filename: devd.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-17
#![cfg(not(target_os = "uefi"))]

//! Device hotplug detection service using inotify.

use crate::runtime::ServiceRegistry;
use crate::services::Service;
use crate::validator::{self, RuleViolation};
use inotify::{EventMask, Inotify, WatchMask};
use std::env;
use std::thread;

#[derive(Default)]
pub struct DevdService;

impl Service for DevdService {
    fn name(&self) -> &'static str { "DevdService" }

    fn init(&mut self) {
        let root = env::var("COH_DEV_ROOT").unwrap_or_else(|_| "/dev".into());
        let mut inotify = Inotify::init().expect("inotify init");
        if inotify.watches().add(&root, WatchMask::CREATE | WatchMask::DELETE).is_err() {
            return;
        }
        println!("[devd] watching {}", root);
        thread::spawn(move || {
            let mut buffer = [0u8; 1024];
            loop {
                let events = match inotify.read_events_blocking(&mut buffer) {
                    Ok(ev) => ev,
                    Err(_) => break,
                };
                for event in events {
                    if let Some(name) = event.name {
                        let dev_name = name.to_string_lossy();
                        let path = format!("{}/{}", root, dev_name);
                        if event.mask.contains(EventMask::CREATE) {
                            if is_device_allowed(&path) {
                                let _ = ServiceRegistry::register_service(&dev_name, &path);
                            } else {
                                validator::log_violation(RuleViolation {
                                    type_: "device_violation",
                                    file: path,
                                    agent: "cohdevd".into(),
                                    time: validator::timestamp(),
                                });
                            }
                        } else if event.mask.contains(EventMask::DELETE) {
                            let _ = ServiceRegistry::unregister_service(&dev_name);
                        }
                    }
                }
            }
        });
    }

    fn shutdown(&mut self) {
        println!("[devd] shutdown");
    }
}

fn is_device_allowed(path: &str) -> bool {
    matches!(std::path::Path::new(path).file_name().and_then(|n| n.to_str()),
        Some("null") | Some("zero") | Some("video0"))
}
