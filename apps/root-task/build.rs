// Author: Lukas Bower
//! Build script that wires the seL4 SDK artefacts into the root-task link step.

use std::collections::VecDeque;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_CANDIDATES: &[&str] = &[
    ".config",
    "kernel/.config",
    "KernelConfig",
    "kernel/KernelConfig",
    "kernel/gen_config/KernelConfig",
    "kernel/gen_config/KernelConfigGenerated.cmake",
    "kernel/gen_config/kernel_all.cmake",
];

fn main() {
    println!("cargo:rerun-if-env-changed=SEL4_BUILD_DIR");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_debug_build)");

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

    let libsel4 = find_library(
        &build_path,
        "libsel4.a",
        &["libsel4/libsel4.a", "lib/libsel4.a", "libsel4.a", "sel4/libsel4.a"],
    )
    .unwrap_or_else(|err| {
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

    emit_config_flags(&build_path);
}

fn find_library(root: &Path, filename: &str, primary: &[&str]) -> Result<PathBuf, String> {
    for relative in primary {
        let candidate = root.join(relative);
        if file_matches(&candidate) {
            return Ok(candidate);
        }
    }

    breadth_first_search(root, filename, 6)
}

fn file_matches(path: &Path) -> bool {
    match fs::metadata(path) {
        Ok(meta) => meta.is_file(),
        Err(_) => false,
    }
}

fn breadth_first_search(root: &Path, needle: &str, max_depth: usize) -> Result<PathBuf, String> {
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
            if path.file_name() == Some(OsStr::new(needle)) && file_matches(&path) {
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
        "searched up to depth {} but no {} was found",
        max_depth,
        needle
    ))
}

fn emit_config_flags(root: &Path) {
    if let Some(true) = probe_config_flag(root, "CONFIG_DEBUG_BUILD") {
        println!("cargo:rustc-cfg=sel4_config_debug_build");
    }
}

fn probe_config_flag(root: &Path, flag: &str) -> Option<bool> {
    for relative in CONFIG_CANDIDATES {
        let candidate = root.join(relative);
        println!("cargo:rerun-if-changed={}", candidate.display());
        let Ok(contents) = fs::read_to_string(&candidate) else {
            continue;
        };

        if let Some(value) = parse_config_flag(&contents, flag) {
            return Some(value);
        }
    }

    None
}

fn parse_config_flag(contents: &str, flag: &str) -> Option<bool> {
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(value) = parse_comment_line(line, flag) {
            return Some(value);
        }

        if let Some(value) = parse_assignment_line(line, flag) {
            return Some(value);
        }

        if let Some(value) = parse_cmake_line(line, flag) {
            return Some(value);
        }
    }

    None
}

fn parse_comment_line(line: &str, flag: &str) -> Option<bool> {
    if !line.starts_with('#') {
        return None;
    }

    if line.contains(flag) && line.contains("is not set") {
        return Some(false);
    }

    None
}

fn parse_assignment_line(line: &str, flag: &str) -> Option<bool> {
    let stripped = line.strip_prefix(flag)?;

    let remainder = stripped
        .trim_start_matches(['=', ':', '?', ' ', '\t'])
        .trim();

    if remainder.is_empty() {
        return None;
    }

    let value = remainder
        .split([' ', '\t', '#'])
        .next()
        .unwrap_or(remainder);

    parse_bool_token(value)
}

fn parse_cmake_line(line: &str, flag: &str) -> Option<bool> {
    if !(line.contains(flag) || line.starts_with("set(") || line.starts_with("option(")) {
        return None;
    }

    let normalized = line.replace(['(', ')', '"'], " ");
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();

    if tokens.len() >= 3 {
        match tokens[0] {
            "set" | "option" if tokens[1] == flag => {
                if let Some(parsed) = parse_bool_token(tokens[2]) {
                    return Some(parsed);
                }
            }
            _ => {}
        }
    }

    if let Some(idx) = tokens.iter().position(|&token| token == flag) {
        if let Some(next) = tokens.get(idx + 1) {
            if let Some(parsed) = parse_bool_token(next) {
                return Some(parsed);
            }
        }
    }

    if let Some(pos) = line.find(flag) {
        let after = &line[pos + flag.len()..];
        if let Some(eq_pos) = after.find('=') {
            let value = after[eq_pos + 1..]
                .split([' ', '\t', ')', ';'])
                .next()
                .unwrap_or("");
            if let Some(parsed) = parse_bool_token(value) {
                return Some(parsed);
            }
        }
    }

    None
}

fn parse_bool_token(token: &str) -> Option<bool> {
    let normalized = token
        .trim_matches(['"', '\'', ')', ';', ','])
        .to_ascii_uppercase();

    match normalized.as_str() {
        "Y" | "YES" | "1" | "ON" | "TRUE" => Some(true),
        "N" | "NO" | "0" | "OFF" | "FALSE" => Some(false),
        _ => None,
    }
}
