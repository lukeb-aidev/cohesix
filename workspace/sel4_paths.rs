// CLASSIFICATION: COMMUNITY
// Filename: sel4_paths.rs v0.1
// Author: OpenAI
// Date Modified: 2028-09-10

use std::path::{Path, PathBuf};

pub fn project_root(manifest_dir: &str) -> PathBuf {
    Path::new(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("Unexpected manifest directory structure")
        .to_path_buf()
}

pub fn sel4_include(project_root: &Path) -> PathBuf {
    project_root.join("third_party/seL4/include")
}

pub fn sel4_generated(project_root: &Path) -> PathBuf {
    sel4_include(project_root).join("generated")
}
