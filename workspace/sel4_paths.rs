// CLASSIFICATION: COMMUNITY
// Filename: sel4_paths.rs v0.4
// Author: OpenAI
// Date Modified: 2028-11-07

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

    let content = std::fs::read_to_string(&tree_file)
        .map_err(|e| format!("failed to read {}: {}", tree_file.display(), e))?;

    let lines: Vec<&str> = content.lines().collect();
    let mut stack: Vec<String> = Vec::new();
    let mut dirs = BTreeSet::new();
    dirs.insert(sel4_include.to_path_buf());

    for (idx, raw) in lines.iter().enumerate() {
        let mut clean = raw
            .replace('│', " ")
            .replace('├', " ")
            .replace('└', " ")
            .replace('─', " ")
            .replace('\u{00a0}', " ");

        let indent = clean.chars().take_while(|c| *c == ' ').count();
        let mut depth = indent / 4;
        if depth > 0 {
            depth -= 1;
        }
        let name = clean.trim();
        if name.is_empty() || name == "." {
            continue;
        }
        while stack.len() > depth {
            stack.pop();
        }

        let next_indent = lines.get(idx + 1).map(|next| {
            next.replace('│', " ")
                .replace('├', " ")
                .replace('└', " ")
                .replace('─', " ")
                .replace('\u{00a0}', " ")
                .chars()
                .take_while(|c| *c == ' ')
                .count()
        });
        let is_dir = next_indent.map(|ni| ni > indent).unwrap_or(false);

        if is_dir {
            stack.push(name.to_string());
        } else if name.ends_with(".h") {
            let mut path = root.to_path_buf();
            for part in &stack {
                path.push(part);
            }
            dirs.insert(path);
        }
    }

    if dirs.is_empty() {
        return Err("No header directories found".into());
    }

    Ok(dirs.into_iter().collect())
}

use std::fs;
use std::io;

pub fn create_arch_alias(sel4_include: &Path, sel4_arch: &str, out_dir: &Path) -> io::Result<PathBuf> {
    let src = sel4_include
        .join("libsel4")
        .join("sel4_arch")
        .join("sel4")
        .join("sel4_arch")
        .join(sel4_arch);

    let alias_root = out_dir.join("sel4_arch_alias");
    let target_arch = alias_root.join("sel4/arch");
    let target_sel4_arch = alias_root.join("sel4/sel4_arch");
    if target_arch.exists() {
        fs::remove_dir_all(&target_arch)?;
    }
    if target_sel4_arch.exists() {
        fs::remove_dir_all(&target_sel4_arch)?;
    }
    fs::create_dir_all(&target_arch)?;
    fs::create_dir_all(&target_sel4_arch)?;

    for entry in fs::read_dir(&src)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "h").unwrap_or(false) {
            let fname = entry.file_name();
            fs::copy(&path, target_arch.join(&fname))?;
            let wrapper = target_sel4_arch.join(&fname);
            fs::write(&wrapper, format!("#pragma once\n#include \"../arch/{}\"\n", fname.to_string_lossy()))?;
        }
    }

    Ok(alias_root)
}
