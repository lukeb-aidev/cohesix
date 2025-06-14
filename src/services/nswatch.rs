// CLASSIFICATION: COMMUNITY
// Filename: nswatch.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-23

//! Namespace hotplug watcher service

use super::Service;
use crate::validator::{self, RuleViolation};
use inotify::{EventMask, Inotify, WatchMask};
use std::env;
use std::thread;

#[derive(Default)]
pub struct NsWatchService;

impl Service for NsWatchService {
    fn name(&self) -> &'static str {
        "NsWatchService"
    }

    fn init(&mut self) {
        let root = env::var("NS_HOTPLUG_ROOT").unwrap_or_else(|_| "/mnt".into());
        let mut inotify = match Inotify::init() {
            Ok(v) => v,
            Err(_) => return,
        };
        if inotify.watches().add(&root, WatchMask::CREATE).is_err() {
            return;
        }
        println!("[nswatch] watching {}", root);
        thread::spawn(move || {
            let mut buffer = [0u8; 1024];
            loop {
                let events = match inotify.read_events_blocking(&mut buffer) {
                    Ok(ev) => ev,
                    Err(_) => break,
                };
                for event in events {
                    if let Some(name) = event.name {
                        if event.mask.contains(EventMask::CREATE) {
                            let path = format!("{}/{}", root, name.to_string_lossy());
                            if !is_ns_allowed(&path) {
                                validator::log_violation(RuleViolation {
                                    type_: "ns_hotplug",
                                    file: path,
                                    agent: "nswatch".into(),
                                    time: validator::timestamp(),
                                });
                            }
                        }
                    }
                }
            }
        });
    }

    fn shutdown(&mut self) {
        println!("[nswatch] shutdown");
    }
}

fn is_ns_allowed(path: &str) -> bool {
    let prefix = env::var("NS_ALLOW_PREFIX").unwrap_or_else(|_| "/mnt/allowed".into());
    path.starts_with(&prefix)
}
