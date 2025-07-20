// CLASSIFICATION: COMMUNITY
// Filename: sel4_paths.rs v0.2
// Author: OpenAI
// Date Modified: 2028-11-05

use std::collections::BTreeSet;
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

pub fn header_dirs_from_tree(sel4_include: &Path) -> Result<Vec<PathBuf>, String> {
    let root = sel4_include.parent().ok_or("SEL4_INCLUDE has no parent")?;
    let tree_file = root.join("sel4_tree.txt");
    if !tree_file.exists() {
        return Err(format!("{} not found", tree_file.display()));
    }

    fn collect(dir: &Path, out: &mut BTreeSet<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                collect(&path, out)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("h") {
                if let Some(parent) = path.parent() {
                    out.insert(parent.to_path_buf());
                }
            }
        }
        Ok(())
    }

    let mut dirs = BTreeSet::new();
    collect(sel4_include, &mut dirs).map_err(|e| e.to_string())?;
    if dirs.is_empty() {
        return Err("No header directories found".into());
    }
    Ok(dirs.into_iter().collect())
}
