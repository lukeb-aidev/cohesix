// Author: Lukas Bower
//! Build script that wires the seL4 SDK artefacts into the root-task link step.

use std::collections::VecDeque;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use regex::Regex;

#[path = "build_support.rs"]
mod build_support;

use build_support::{classify_linker_script, LinkerScriptKind};

const IPC_GUARD_SOURCE: &str = "apps/root-task/src";
const IPC_GUARD_ALLOW: &str = "sel4.rs";
const IPC_GUARD_PATTERN: &str = r"\bseL4_(Send|Call|ReplyRecv)\s*\(";

const CONFIG_CANDIDATES: &[&str] = &[
    ".config",
    "kernel/.config",
    "KernelConfig",
    "kernel/KernelConfig",
    "kernel/gen_config/KernelConfig",
    "kernel/gen_config/kernel/gen_config.h",
    "kernel/gen_config/kernel/KernelConfig",
    "kernel/gen_config/kernel/KernelConfigGenerated.cmake",
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
    if let Err(err) = emit_built_info() {
        panic!("failed to emit built_info.rs: {err}");
    }

    println!("cargo:rerun-if-env-changed=SEL4_LD");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD_DIR");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_debug_build)");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_printing)");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_kernel_mcs)");

    if let Err(error) = enforce_guarded_ipc() {
        panic!("failed to scan `{IPC_GUARD_SOURCE}` for direct IPC syscalls: {error}");
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }

    let explicit_linker_script = env::var("SEL4_LD").ok();
    if let Some(ref ld) = explicit_linker_script {
        println!("cargo:rustc-link-arg-bin=root-task=-T{ld}");
        println!("cargo:rustc-link-arg-bin=root-task=-gc-sections");
        println!("cargo:rustc-link-arg-bin=root-task=-no-pie");
    }

    let build_dir = env::var("SEL4_BUILD_DIR")
        .or_else(|_| env::var("SEL4_BUILD"))
        .unwrap_or_else(|_| {
            panic!(
                "The root-task build requires the SEL4_BUILD_DIR (or SEL4_BUILD) environment variable to \n\
                 point at a completed seL4 build directory containing libsel4.a.\n\
                 Export SEL4_LD to use a repository-provided linker script when the seL4 build lacks one."
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

    let debug_enabled = probe_config_flag(&build_path, "CONFIG_DEBUG_BUILD") == Some(true);
    let mut debug_syscalls_enabled = false;
    if debug_enabled {
        if let Ok(libsel4debug) = find_artifact(
            &build_path,
            "libsel4debug.a",
            &[
                "libsel4/libsel4debug.a",
                "lib/libsel4debug.a",
                "libsel4debug.a",
            ],
        ) {
            if let Some(dir) = libsel4debug.parent() {
                println!("cargo:rustc-link-search=native={}", dir.display());
            }
            println!("cargo:rustc-link-lib=static=sel4debug");
            debug_syscalls_enabled = true;
        } else {
            println!(
                "cargo:warning=CONFIG_DEBUG_BUILD enabled but libsel4debug.a not found; debug syscalls will be disabled"
            );
        }
    }

    if explicit_linker_script.is_none() {
        if let Err(err) = stage_linker_script(&build_path) {
            panic!(
                "Unable to locate a suitable seL4 linker script inside {}. {}",
                build_path.display(),
                err
            );
        }
    }

    emit_config_flags(&build_path, debug_syscalls_enabled);
}

fn emit_built_info() -> io::Result<()> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);
    let git = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| "nogit".to_owned());
    let timestamp = Utc::now().to_rfc3339();
    let contents = format!(
        "pub const GIT_HASH:&str=\"{}\";\npub const BUILD_TS:&str=\"{}\";\n",
        git.trim(),
        timestamp
    );
    fs::write(out_dir.join("built_info.rs"), contents)?;
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
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

fn stage_linker_script(build_root: &Path) -> Result<(), String> {
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
                println!("cargo:rustc-link-arg-bin=root-task=-gc-sections");
                println!("cargo:rustc-link-arg-bin=root-task=-no-pie");
                return Ok(());
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

    Err(format!("Tried [{}]. {}", searched, detail))
}

fn emit_config_flags(root: &Path, debug_syscalls_enabled: bool) {
    if debug_syscalls_enabled {
        println!("cargo:rustc-cfg=sel4_config_debug_build");
    }

    if let Some(true) = probe_config_flag(root, "CONFIG_PRINTING") {
        println!("cargo:rustc-cfg=sel4_config_printing");
    }

    if let Some(true) = probe_config_flag(root, "CONFIG_KERNEL_MCS") {
        println!("cargo:rustc-cfg=sel4_config_kernel_mcs");
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

fn enforce_guarded_ipc() -> io::Result<()> {
    let regex = Regex::new(IPC_GUARD_PATTERN).expect("valid IPC guard regex");
    scan_ipc_directory(Path::new(IPC_GUARD_SOURCE), &regex)
}

fn scan_ipc_directory(path: &Path, regex: &Regex) -> io::Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            scan_ipc_directory(&entry.path(), regex)?;
        }
        return Ok(());
    }

    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return Ok(());
    }

    if path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == IPC_GUARD_ALLOW)
        .unwrap_or(false)
    {
        return Ok(());
    }

    let contents = fs::read_to_string(path)?;
    if regex.is_match(&contents) {
        panic!(
            "Forbidden raw seL4 IPC in {} — use guarded wrapper",
            path.display()
        );
    }

    Ok(())
}
