// CLASSIFICATION: COMMUNITY
// Filename: profile.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Sandbox Profile Module
//
/// Represents and enforces the execution constraints of sandboxed processes in Cohesix.
/// Profiles govern resource limits, syscall permissions, and namespace visibility.

/// Struct representing a sandbox profile with basic attributes.
pub struct SandboxProfile {
    pub name: String,
    pub allowed_syscalls: Vec<String>,
    pub memory_limit_mb: u32,
    pub cpu_shares: u8,
}

impl SandboxProfile {
    /// Creates a new named profile with default conservative limits.
    pub fn new(name: &str) -> Self {
        SandboxProfile {
            name: name.to_string(),
            allowed_syscalls: vec!["read".into(), "write".into(), "exit".into()],
            memory_limit_mb: 64,
            cpu_shares: 10,
        }
    }

    /// Checks if a syscall is permitted under this profile.
    pub fn is_syscall_allowed(&self, syscall: &str) -> bool {
        self.allowed_syscalls.contains(&syscall.to_string())
    }

    /// Displays a summary of the profile’s current settings.
    pub fn describe(&self) -> String {
        format!(
            "[profile: {}] syscalls={:?}, mem={}MB, cpu={}",
            self.name, self.allowed_syscalls, self.memory_limit_mb, self.cpu_shares
        )
    }
}
