// CLASSIFICATION: COMMUNITY
// Filename: observer.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
/// Role module for the Cohesix `Observer`.
/// Observers passively monitor system events, collect telemetry, and optionally record logs or state transitions.

/// Trait representing observer functionality.
pub trait ObserverRole {
    fn monitor_event(&mut self, event: &str);
    fn emit_summary(&self) -> String;
    fn reset(&mut self);
}

/// Stub implementation of the observer role.
pub struct DefaultObserver {
    pub event_log: Vec<String>,
}

impl DefaultObserver {
    pub fn new() -> Self {
        DefaultObserver {
            event_log: Vec::new(),
        }
    }
}

impl ObserverRole for DefaultObserver {
    fn monitor_event(&mut self, event: &str) {
        println!("[observer] monitoring event: {}", event);
        self.event_log.push(event.to_string());
    }

    fn emit_summary(&self) -> String {
        println!("[observer] emitting summary...");
        format!("{} events recorded", self.event_log.len())
    }

    fn reset(&mut self) {
        println!("[observer] resetting log...");
        self.event_log.clear();
    }
}
