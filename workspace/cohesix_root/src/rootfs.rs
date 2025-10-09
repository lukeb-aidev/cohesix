// CLASSIFICATION: COMMUNITY
// Filename: rootfs.rs v0.1
// Author: Lukas Bower
// Date Modified: 2030-03-12
#![allow(dead_code)]

pub struct RootFsEntry {
    pub path: &'static str,
    pub data: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/rootfs_data.rs"));

pub fn lookup(path: &[u8]) -> Option<&'static [u8]> {
    for entry in ROOT_FS {
        if entry.path.as_bytes() == path {
            return Some(entry.data);
        }
    }
    None
}
