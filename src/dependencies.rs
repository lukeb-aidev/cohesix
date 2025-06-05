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

    // Populate with a few core runtime dependencies. In a future
    // iteration this will be loaded from an SBOM manifest.
    registry.register(Dependency {
        name: "serde".into(),
        version: "1.0".into(),
        source: Some("crates.io".into()),
        license: Some("MIT OR Apache-2.0".into()),
    });

    registry.register(Dependency {
        name: "clap".into(),
        version: "4.0".into(),
        source: Some("crates.io".into()),
        license: Some("MIT OR Apache-2.0".into()),
    });

    // Example of a dependency provided by the workspace itself
    registry.register(Dependency {
        name: "anyhow".into(),
        version: "1.0".into(),
        source: Some("crates.io".into()),
        license: Some("MIT OR Apache-2.0".into()),
    });

    registry
}

