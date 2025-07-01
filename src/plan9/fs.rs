// CLASSIFICATION: COMMUNITY
// Filename: fs.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
/// Plan 9 filesystem abstraction for Cohesix.
/// Provides in-memory structures and trait scaffolding for 9P-like filesystem operations.

/// Represents a file or directory node in the Plan 9 FS tree.
#[derive(Debug)]
pub struct FsNode {
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<FsNode>,
}

impl FsNode {
    pub fn new(name: &str, is_dir: bool) -> Self {
        FsNode {
            name: name.to_string(),
            is_dir,
            children: Vec::new(),
        }
    }

    pub fn add_child(&mut self, child: FsNode) {
        if self.is_dir {
            self.children.push(child);
        } else {
            println!("[fs] Cannot add child to non-directory: {}", self.name);
        }
    }

    pub fn list(&self) -> Vec<String> {
        self.children.iter().map(|c| c.name.clone()).collect()
    }
}

/// Trait representing a 9P-compatible FS interface.
pub trait Plan9FS {
    fn read(&self, path: &str) -> Result<String, String>;
    fn write(&mut self, path: &str, contents: &str) -> Result<(), String>;
    fn list_dir(&self, path: &str) -> Result<Vec<String>, String>;
}
