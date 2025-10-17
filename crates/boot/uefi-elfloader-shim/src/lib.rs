// Author: Lukas Bower
#![doc = r"Helper crate that anchors documentation for the UEFI elfloader packaging.\n\n\
This crate intentionally exposes no runtime APIs. It exists so the Cargo\nworkspace can track the shim that documents how the seL4 elfloader is\nrebuilt as a UEFI application and bundled into the Cohesix ESP image."]
#![no_std]

/// Marker type ensuring the crate is not considered empty by lint
/// configurations that flag packages without public items.
#[allow(dead_code)]
pub struct UefiElfloaderShim;
