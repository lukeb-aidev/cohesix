// CLASSIFICATION: COMMUNITY
// Filename: sbom.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! SBOM Generator for Cohesix
//!
//! This module generates a Software Bill of Materials (SBOM) that lists all third-party packages,
//! licenses, versions, and source locations used in the build. This aids compliance, auditing, and secure supply chain verification.

use std::collections::HashMap;

/// Struct representing a single software component in the SBOM.
pub struct SbomEntry {
    pub name: String,
    pub version: String,
    pub license: String,
    pub source_url: Option<String>,
}

/// The SBOM document structure containing a list of entries.
pub struct SbomDocument {
    pub entries: Vec<SbomEntry>,
}

impl SbomDocument {
    /// Create a new, empty SBOM document.
    pub fn new() -> Self {
        SbomDocument { entries: Vec::new() }
    }

    /// Add a software component to the SBOM.
    pub fn add_entry(&mut self, entry: SbomEntry) {
        println!("[sbom] adding entry for '{}'", entry.name);
        self.entries.push(entry);
    }

    /// Generate a human-readable summary of the SBOM contents.
    pub fn summarize(&self) -> String {
        let mut summary = String::from("Cohesix SBOM:\n");
        for entry in &self.entries {
            summary.push_str(&format!(
                "- {} {} ({})\n",
                entry.name, entry.version, entry.license
            ));
        }
        summary
    }

    /// Optionally export the SBOM as JSON or another format.
    pub fn export(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for entry in &self.entries {
            map.insert(entry.name.clone(), entry.version.clone());
        }
        map
    }
}
