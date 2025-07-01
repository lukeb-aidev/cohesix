// CLASSIFICATION: COMMUNITY
// Filename: oss_manifest.rs v1.1
// Author: Lukas Bower
// Date Modified: 2025-06-08

use crate::prelude::*;
//! OSS manifest module for Cohesix.
//! Tracks open-source components and license metadata included in the kernel and userland builds.

/// Represents a third-party dependency tracked in the manifest.
pub struct Dependency {
    pub name: &'static str,
    pub version: &'static str,
    pub license: &'static str,
    pub source_url: &'static str,
}

/// Static list of known OSS dependencies.
pub static OSS_DEPENDENCIES: &[Dependency] = &[
    Dependency {
        name: "rapier3d",
        version: "0.17.2",
        license: "Apache-2.0",
        source_url: "https://github.com/dimforge/rapier",
    },
    Dependency {
        name: "clap",
        version: "4.5.4",
        license: "MIT",
        source_url: "https://github.com/clap-rs/clap",
    },
    Dependency {
        name: "p9",
        version: "0.3.2",
        license: "Apache-2.0",
        source_url: "https://github.com/wahern/p9",
    },
];

/// Display the list of OSS components used by the project.
pub fn print_manifest() {
    println!("Cohesix OSS Manifest:");
    for dep in OSS_DEPENDENCIES {
        println!(
            "• {} {} — {} ({})",
            dep.name, dep.version, dep.license, dep.source_url
        );
    }
}
