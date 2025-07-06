// CLASSIFICATION: COMMUNITY
// Filename: fat.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-27

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Minimal FAT filesystem glue for the UEFI boot path.
/// This is a temporary shim that relies on the host-provided
/// FAT image supplied via QEMU `-drive format=raw,file=fat:rw:<dir>`.
//
/// Only implemented for the `minimal_uefi` feature.

#[cfg(feature = "minimal_uefi")]
use std::fs;

/// Mount the FAT partition as the root filesystem.
#[cfg(feature = "minimal_uefi")]
pub fn mount_root() {
    println!("[fat] mounted FAT partition as /");
}

/// Read a binary file from the mounted FAT filesystem into memory.
#[cfg(feature = "minimal_uefi")]
pub fn open_bin(path: &str) -> Option<&'static [u8]> {
    match fs::read(path) {
        Ok(data) => Some(Box::leak(data.into_boxed_slice())),
        Err(e) => {
            eprintln!("[fat] failed to open {}: {}", path, e);
            None
        }
    }
}
