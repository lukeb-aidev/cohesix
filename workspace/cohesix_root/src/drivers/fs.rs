// CLASSIFICATION: COMMUNITY
// Filename: fs.rs v0.2
// Author: Lukas Bower
// Date Modified: 2028-11-21
#![no_std]

use crate::coherr;

pub struct StubFs;

impl StubFs {
    pub fn mount_root() -> Result<(), ()> {
        coherr!("fs_mount_root");
        Ok(())
    }
    pub fn mount(path: &str) -> Result<(), ()> {
        coherr!("fs_mount {}", path);
        Ok(())
    }
}
