// CLASSIFICATION: COMMUNITY
// Filename: initfs.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
/// InitFS — a static, read-only filesystem embedded into the Cohesix kernel.
/// Used for boot-time resources such as init scripts, config files, and fallback binaries.

/// A simple in-memory representation of an InitFS file entry.
pub struct InitFile {
    pub name: &'static str,
    pub contents: &'static [u8],
}

/// Static list of embedded files (to be populated at link time).
// Example embedded files for early boot. In a real build these would be
// populated via a linker script or generated constants.
static INIT_FILES: &[InitFile] = &[
    InitFile {
        name: "init.rc",
        contents: b"echo booting Cohesix\n",
    },
    InitFile {
        name: "config.txt",
        contents: b"BOOT_VERBOSE=1\n",
    },
    InitFile {
        name: "README",
        contents: b"Cohesix InitFS\n",
    },
];

/// Attempt to retrieve a file by name.
pub fn get_file(name: &str) -> Option<&'static InitFile> {
    INIT_FILES.iter().find(|f| f.name == name)
}

/// List all embedded file names.
pub fn list_files() -> impl Iterator<Item = &'static str> {
    INIT_FILES.iter().map(|f| f.name)
}

