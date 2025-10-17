// Author: Lukas Bower
//! Build script that wires the seL4 SDK artefacts into the root-task link step.

use std::collections::VecDeque;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "build_support.rs"]
mod build_support;

use build_support::{classify_linker_script, LinkerScriptKind};

const CONFIG_CANDIDATES: &[&str] = &[
    ".config",
    "kernel/.config",
    "KernelConfig",
    "kernel/KernelConfig",
    "kernel/gen_config/KernelConfig",
    "kernel/gen_config/KernelConfigGenerated.cmake",
    "kernel/gen_config/kernel_all.cmake",
];

#[derive(Clone, Copy)]
struct LinkerScriptSearchSet {
    file_name: &'static str,
    primary: &'static [&'static str],
}

const LINKER_SCRIPT_SEARCH_SETS: &[LinkerScriptSearchSet] = &[
    LinkerScriptSearchSet {
        file_name: "sel4.ld",
        primary: &[
            "sel4/sel4.ld",
            "rootserver/sel4.ld",
            "projects/sel4runtime/elf/sel4.ld",
            "projects/seL4Runtime/elf/sel4.ld",
            "linker/sel4.ld",
            "kernel/sel4.ld",
            "kernel/linker/sel4.ld",
            "sel4.ld",
        ],
    },
    LinkerScriptSearchSet {
        file_name: "linker.lds",
        primary: &[
            "sel4/linker.lds",
            "rootserver/linker.lds",
            "projects/sel4runtime/elf/linker.lds",
            "projects/seL4Runtime/elf/linker.lds",
            "linker/linker.lds",
            "kernel/linker.lds",
            "kernel/gen_config/linker.lds",
            "kernel/gen_config/kernel/linker.lds",
            "linker.lds",
        ],
    },
    LinkerScriptSearchSet {
        file_name: "linker.lds_pp",
        primary: &[
            "sel4/linker.lds_pp",
            "rootserver/linker.lds_pp",
            "projects/sel4runtime/elf/linker.lds_pp",
            "projects/seL4Runtime/elf/linker.lds_pp",
            "linker/linker.lds_pp",
            "kernel/linker.lds_pp",
            "kernel/gen_config/linker.lds_pp",
            "kernel/gen_config/kernel/linker.lds_pp",
            "linker.lds_pp",
        ],
    },
];

enum ArtifactDecision {
    Accept,
    Reject(String),
}

fn main() {
    println!("cargo:rerun-if-env-changed=SEL4_BUILD_DIR");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_debug_build)");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_printing)");

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

    let libsel4 = find_artifact(
        &build_path,
        "libsel4.a",
        &[
            "libsel4/libsel4.a",
            "lib/libsel4.a",
            "libsel4.a",
            "sel4/libsel4.a",
        ],
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

    stage_linker_script(&build_path);

    emit_config_flags(&build_path);
}

fn find_artifact(root: &Path, filename: &str, primary: &[&str]) -> Result<PathBuf, String> {
    find_artifact_with(root, filename, primary, |_| Ok(ArtifactDecision::Accept))
}

fn find_artifact_with<F>(
    root: &Path,
    filename: &str,
    primary: &[&str],
    mut filter: F,
) -> Result<PathBuf, String>
where
    F: FnMut(&Path) -> Result<ArtifactDecision, String>,
{
    let mut errors = Vec::new();

    for relative in primary {
        let candidate = root.join(relative);
        if !file_matches(&candidate) {
            continue;
        }

        match filter(&candidate) {
            Ok(ArtifactDecision::Accept) => return Ok(candidate),
            Ok(ArtifactDecision::Reject(reason)) => {
                errors.push(format!("{} rejected: {}", candidate.display(), reason))
            }
            Err(err) => errors.push(format!("{} rejected: {}", candidate.display(), err)),
        }
    }

    const MAX_DEPTH: usize = 6;
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) => {
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
            if path.file_name() == Some(OsStr::new(filename)) && file_matches(&path) {
                match filter(&path) {
                    Ok(ArtifactDecision::Accept) => return Ok(path),
                    Ok(ArtifactDecision::Reject(reason)) => {
                        errors.push(format!("{} rejected: {}", path.display(), reason))
                    }
                    Err(err) => errors.push(format!("{} rejected: {}", path.display(), err)),
                }
            }

            if depth < MAX_DEPTH {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_dir() {
                        queue.push_back((path, depth + 1));
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Err(format!(
            "searched up to depth {} but no {} satisfying the predicate was found",
            MAX_DEPTH, filename
        ))
    } else {
        Err(errors.join("; "))
    }
}

fn file_matches(path: &Path) -> bool {
    match fs::metadata(path) {
        Ok(meta) => meta.is_file(),
        Err(_) => false,
    }
}

fn stage_linker_script(build_root: &Path) {
    let mut errors = Vec::new();

    for candidate in LINKER_SCRIPT_SEARCH_SETS {
        match find_artifact_with(build_root, candidate.file_name, candidate.primary, |path| {
            let kind = classify_linker_script(path)?;
            let display = path.display().to_string();
            match kind {
                LinkerScriptKind::Kernel => Ok(ArtifactDecision::Reject(format!(
                    "detected seL4 kernel linker script: {}",
                    display
                ))),
                LinkerScriptKind::User => Ok(ArtifactDecision::Accept),
                LinkerScriptKind::Unknown => Ok(ArtifactDecision::Reject(format!(
                    "unrecognised linker script without userland markers: {}",
                    display
                ))),
            }
        }) {
            Ok(script) => {
                println!("cargo:rerun-if-changed={}", script.display());

                let out_dir =
                    PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set by cargo"));
                let staged =
                    out_dir.join(script.file_name().unwrap_or_else(|| OsStr::new("sel4.ld")));

                fs::copy(&script, &staged).unwrap_or_else(|err| {
                    panic!(
                        "Failed to stage linker script from {} to {}: {}",
                        script.display(),
                        staged.display(),
                        err
                    );
                });

                println!("cargo:rustc-env=SEL4_LD={}", staged.display());
                println!("cargo:rustc-link-arg-bin=root-task=-T{}", staged.display());
                println!("cargo:rustc-link-arg-bin=root-task=-Wl,--gc-sections");
                println!("cargo:rustc-link-arg-bin=root-task=-no-pie");
                return;
            }
            Err(err) => errors.push(format!("{}: {}", candidate.file_name, err)),
        }
    }

    let searched = LINKER_SCRIPT_SEARCH_SETS
        .iter()
        .map(|set| set.file_name)
        .collect::<Vec<_>>()
        .join(", ");

    let detail = if errors.is_empty() {
        String::from("no candidates were evaluated")
    } else {
        errors.join("; ")
    };

    panic!(
        "Unable to locate a suitable seL4 linker script inside {}. Tried [{}]. {}",
        build_root.display(),
        searched,
        detail
    );
}

fn emit_config_flags(root: &Path) {
    if let Some(true) = probe_config_flag(root, "CONFIG_DEBUG_BUILD") {
        println!("cargo:rustc-cfg=sel4_config_debug_build");
    }

    if let Some(true) = probe_config_flag(root, "CONFIG_PRINTING") {
        println!("cargo:rustc-cfg=sel4_config_printing");
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
