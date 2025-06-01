// CLASSIFICATION: COMMUNITY
// Filename: dependencies.rs v1.0
// Date Modified: 2025-05-31
// Author: Lukas Bower

//! Dependency Tracker for Cohesix
//!
//! This module tracks runtime and build-time dependencies for key Cohesix services,
//! allowing introspection, SBOM generation, and dependency auditing.

use std::collections::HashMap;

/// Represents a single dependency with metadata.
#[derive(Debug, Clone)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub license: Option<String>,
}

/// Registry to hold known dependencies.
pub struct DependencyRegistry {
    pub entries: HashMap<String, Dependency>,
}

impl DependencyRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        DependencyRegistry {
            entries: HashMap::new(),
        }
    }

    /// Add a dependency to the registry.
    pub fn register(&mut self, dep: Dependency) {
        self.entries.insert(dep.name.clone(), dep);
    }

    /// Retrieve a dependency by name.
    pub fn get(&self, name: &str) -> Option<&Dependency> {
        self.entries.get(name)
    }

    /// Emit a summary of all registered dependencies.
    pub fn emit_summary(&self) {
        println!("[dependencies] Summary of registered dependencies:");
        for dep in self.entries.values() {
            println!(
                "- {} v{} ({:?})",
                dep.name,
                dep.version,
                dep.license.as_deref().unwrap_or("Unknown")
            );
        }
    }
}

/// Stub function to populate common system dependencies.
pub fn preload_dependencies() -> DependencyRegistry {
    let mut registry = DependencyRegistry::new();
    // TODO(cohesix): Load from SBOM or manifest
    registry
}