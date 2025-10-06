// CLASSIFICATION: COMMUNITY
// Filename: sel4_paths.rs v0.7
// Author: OpenAI
// Date Modified: 2028-12-05
#![allow(dead_code)]

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            copy_recursive(&src_path, &dst_path)?;
        }
    } else if src.exists() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dst)?;
    }
    Ok(())
}

pub fn project_root(manifest_dir: &str) -> PathBuf {
    Path::new(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("Unexpected manifest directory structure")
        .to_path_buf()
}

pub fn sel4_include(project_root: &Path) -> PathBuf {
    project_root
        .join("third_party")
        .join("seL4")
        .join("include")
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
        let clean = raw
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
            let mut current = path.clone();
            loop {
                if current.starts_with(sel4_include) {
                    dirs.insert(current.clone());
                }
                if current == sel4_include {
                    break;
                }
                match current.parent() {
                    Some(parent) => current = parent.to_path_buf(),
                    None => break,
                }
            }
        }
    }

    if dirs.is_empty() {
        return Err("No header directories found".into());
    }

    Ok(dirs.into_iter().collect())
}

use std::io;

pub fn create_arch_alias(
    sel4_include: &Path,
    sel4_arch: &str,
    out_dir: &Path,
) -> io::Result<PathBuf> {
    let src = sel4_include
        .join("libsel4")
        .join("sel4_arch")
        .join("sel4")
        .join("sel4_arch")
        .join(sel4_arch);

    let alias_root = out_dir.join("sel4_arch_alias");
    let target_arch = alias_root.join("sel4/arch");
    let target_sel4_arch = alias_root.join("sel4/sel4_arch");
    let target_mode = alias_root.join("sel4/mode");
    if target_arch.exists() {
        fs::remove_dir_all(&target_arch)?;
    }
    if target_sel4_arch.exists() {
        fs::remove_dir_all(&target_sel4_arch)?;
    }
    if target_mode.exists() {
        fs::remove_dir_all(&target_mode)?;
    }
    fs::create_dir_all(&target_arch)?;
    fs::create_dir_all(&target_sel4_arch)?;
    fs::create_dir_all(&target_mode)?;

    crate::sel4_paths::copy_recursive(&src, &target_arch)?;

    fn generate_wrappers(
        src_base: &Path,
        dst_base: &Path,
        rel_base: &str,
    ) -> io::Result<()> {
        for entry in fs::read_dir(src_base)? {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(src_base).unwrap();
            let dst_path = dst_base.join(rel);
            if path.is_dir() {
                fs::create_dir_all(&dst_path)?;
                generate_wrappers(&path, &dst_path, rel_base)?;
            } else if path.extension().map(|e| e == "h").unwrap_or(false) {
                if let Some(parent) = dst_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(
                    &dst_path,
                    format!(
                        "#pragma once\n#include \"{}/{}\"\n",
                        rel_base,
                        rel.to_string_lossy()
                    ),
                )?;
            }
        }
        Ok(())
    }

    generate_wrappers(&target_arch, &target_sel4_arch, "../arch")?;

    if let Some(parent) = src.parent() {
        let invocation = parent.join("invocation.h");
        if invocation.exists() {
            let arch_invocation = target_arch.join("invocation.h");
            fs::copy(&invocation, &arch_invocation)?;
            fs::write(
                target_sel4_arch.join("invocation.h"),
                "#pragma once\n#include \"../arch/invocation.h\"\n",
            )?;
        }

        let types_gen = parent.join("types_gen.h");
        if types_gen.exists() {
            let arch_types_gen = target_arch.join("types_gen.h");
            fs::copy(&types_gen, &arch_types_gen)?;
            fs::write(
                target_sel4_arch.join("types_gen.h"),
                "#pragma once\n#include \"../arch/types_gen.h\"\n",
            )?;
        }
    }

    let mode_wrapper = target_mode.join("types.h");
    fs::create_dir_all(mode_wrapper.parent().unwrap())?;
    fs::write(
        &mode_wrapper,
        "#pragma once\n#include \"../sel4_arch/types.h\"\n",
    )?;

    Ok(alias_root)
}

pub fn get_all_subdirectories(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    fn walk(dir: &Path, root: &Path, set: &mut HashSet<PathBuf>) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, set)?;
            } else if path.extension().map(|e| e == "h").unwrap_or(false) {
                if let Some(mut current) = path.parent() {
                    loop {
                        if current.starts_with(root) {
                            set.insert(current.to_path_buf());
                        }
                        if current == root {
                            break;
                        }
                        match current.parent() {
                            Some(next) => current = next,
                            None => break,
                        }
                    }
                }
            }
        }
        Ok(())
    }

    let mut set = HashSet::new();
    walk(root, root, &mut set)?;
    let mut dirs: Vec<PathBuf> = set.into_iter().collect();
    dirs.sort();
    Ok(dirs)
}

/// Recursively collect all header directories under `root`.
/// Returns a sorted vector of unique parent directories containing `.h` files.
pub fn header_dirs_recursive(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    get_all_subdirectories(root)
}

/// Compute standard seL4 include flags.
pub fn default_cflags(sel4_include: &Path, project_root: &Path) -> Vec<String> {
    let mut flags = Vec::new();
    flags.push(format!("-I{}", sel4_include.display()));
    flags.push(format!("-I{}/libsel4/interfaces", sel4_include.display()));
    flags.push(format!(
        "-I{}/libsel4/sel4_arch/sel4/sel4_arch/aarch64",
        sel4_include.display()
    ));
    if let Ok(dirs) = header_dirs_recursive(sel4_include) {
        for d in dirs {
            flags.push(format!("-I{}", d.display()));
        }
    }
    if let Ok(gen_dirs) = header_dirs_recursive(&sel4_generated(project_root)) {
        for d in gen_dirs {
            flags.push(format!("-I{}", d.display()));
        }
    }
    flags
}
