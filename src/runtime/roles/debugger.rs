// CLASSIFICATION: COMMUNITY
// Filename: debugger.rs v1.1
// Author: Lukas Bower
// Date Modified: 2025-06-10

use crate::prelude::*;
/// Role module for the Cohesix `Debugger`.
/// Provides diagnostic capabilities to inspect runtime state, trace execution, and emit system-level debug information.

use chrono;

/// Trait representing debugger behavior.
pub trait DebuggerRole {
    fn dump_trace(&self) -> Result<(), String>;
    fn inspect_state(&self) -> String;
    fn enable_debug_mode(&mut self, enabled: bool);
}

/// Stub implementation of the debugger role.
pub struct DefaultDebugger {
    pub debug_enabled: bool,
}

impl DefaultDebugger {
    pub fn new() -> Self {
        DefaultDebugger {
            debug_enabled: false,
        }
    }
}

impl DebuggerRole for DefaultDebugger {
    fn dump_trace(&self) -> Result<(), String> {
        if self.debug_enabled {
            println!("[debugger] dumping trace...");
            use std::fs;
            use std::path::Path;
            let src = Path::new("/srv/trace.log");
            let dst = Path::new("/history").join(format!(
                "trace_dump_{}.log",
                chrono::Utc::now().timestamp()
            ));
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            if src.exists() {
                fs::copy(src, &dst).map_err(|e| e.to_string())?;
            }
            Ok(())
        } else {
            Err("Debug mode not enabled".into())
        }
    }

    fn inspect_state(&self) -> String {
        println!("[debugger] inspecting system state...");
        std::fs::read_to_string("/srv/state.json").unwrap_or_else(|_| "unknown".to_string())
    }

    fn enable_debug_mode(&mut self, enabled: bool) {
        self.debug_enabled = enabled;
        println!("[debugger] debug mode set to {}", enabled);
    }
}
