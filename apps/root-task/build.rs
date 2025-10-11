// Author: Lukas Bower
//! Build script that wires the seL4 SDK artefacts into the root-task link step.

use std::collections::VecDeque;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=SEL4_BUILD_DIR");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }

    let build_dir = env::var("SEL4_BUILD_DIR")
        .or_else(|_| env::var("SEL4_BUILD"))
        .unwrap_or_else(|_| {
            panic!(
                "The root-task build requires the SEL4_BUILD_DIR (or SEL4_BUILD) environment variable to \n\
                 point at a completed seL4 build directory containing libsel4.a."
            );
        });

    let build_path = PathBuf::from(&build_dir);
    if !build_path.is_dir() {
        panic!(
            "The provided seL4 build directory does not exist or is not a directory: {}",
            build_path.display()
        );
    }

    let libsel4 = find_libsel4(&build_path).unwrap_or_else(|err| {
        panic!(
            "Unable to locate libsel4.a inside {}: {}",
            build_path.display(),
            err
        );
    });

    let lib_dir = libsel4
        .parent()
        .expect("libsel4.a should reside inside a directory");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=sel4");
}

fn find_libsel4(root: &Path) -> Result<PathBuf, String> {
    let candidates = [
        root.join("libsel4/libsel4.a"),
        root.join("lib/libsel4.a"),
        root.join("libsel4.a"),
        root.join("sel4/libsel4.a"),
    ];

    for candidate in candidates {
        if file_matches(&candidate) {
            return Ok(candidate);
        }
    }

    breadth_first_search(root, 6)
}

fn file_matches(path: &Path) -> bool {
    match fs::metadata(path) {
        Ok(meta) => meta.is_file(),
        Err(_) => false,
    }
}

fn breadth_first_search(root: &Path, max_depth: usize) -> Result<PathBuf, String> {
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) => {
                // Ignore directories we cannot read; they might be permission restricted artefacts.
                eprintln!(
                    "cargo:warning=Skipping unreadable directory {}: {}",
                    dir.display(),
                    err
                );
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name() == Some(OsStr::new("libsel4.a")) && file_matches(&path) {
                return Ok(path);
            }

            if depth < max_depth {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_dir() {
                        queue.push_back((path, depth + 1));
                    }
                }
            }
        }
    }

    Err(format!(
        "searched up to depth {} but no libsel4.a was found",
        max_depth
    ))
}
