// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the build script for root-task.
// Author: Lukas Bower
//! Build script that wires the seL4 SDK artefacts into the root-task link step.

use std::collections::VecDeque;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use cargo_build_directive::{emit_cargo_directive, rust_string_literal};
use chrono::Utc;
use regex::Regex;

#[path = "build_support.rs"]
mod build_support;

use build_support::{
    classify_linker_script, generated_artifact_is_stale, parse_timer_clock_hz, LinkerScriptKind,
};

const IPC_GUARD_SOURCE: &str = "apps/root-task/src";
const IPC_GUARD_ALLOW: &str = "sel4.rs";
const IPC_GUARD_PATTERN: &str = r"\bseL4_(Send|Call|ReplyRecv)\s*\(";

const CONFIG_CANDIDATES: &[&str] = &[
    "CMakeCache.txt",
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

const TIMER_PLATFORM_HEADER_CANDIDATES: &[&str] = &[
    "kernel/gen_headers/plat/platform_gen.h",
    "gen_headers/plat/platform_gen.h",
];

enum ArtifactDecision {
    Accept,
    Reject(String),
}

fn main() {
    if let Err(err) = emit_built_info() {
        panic!("failed to emit built_info.rs: {err}");
    }
    if let Err(err) = validate_generated_manifest() {
        panic!("generated manifest check failed: {err}");
    }
    if let Err(err) = emit_pi4_wifi_firmware() {
        panic!("failed to stage pi4 wifi firmware bundle: {err}");
    }
    if let Err(err) = emit_pi4_driver_runtime_payload() {
        panic!("failed to stage pi4 driver runtime payload: {err}");
    }

    println!("cargo:rerun-if-env-changed=SEL4_LD");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD_DIR");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD");
    println!("cargo:rerun-if-env-changed=COHESIX_BUILD_STAMP");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_debug_build)");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_printing)");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_kernel_mcs)");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_export_vcnt_user)");

    if let Err(error) = enforce_guarded_ipc() {
        panic!("failed to scan `{IPC_GUARD_SOURCE}` for direct IPC syscalls: {error}");
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }

    let explicit_linker_script = env::var("SEL4_LD").ok();
    if let Some(ref ld) = explicit_linker_script {
        emit_cargo_directive(format!("cargo:rustc-link-arg-bin=root-task=-T{ld}"));
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

    let debug_syscalls_enabled = probe_config_flag(&build_path, "CONFIG_DEBUG_BUILD") == Some(true);
    let timer_clock_hz = match emit_timer_build_metadata(&build_path) {
        Ok(freq) => freq,
        Err(err) => {
            panic!(
                "Unable to derive seL4 timer metadata from {}: {}",
                build_path.display(),
                err
            );
        }
    };
    validate_arch_counter_config(&build_path, timer_clock_hz);

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
    emit_git_rerun_triggers()?;
    println!("cargo:rerun-if-env-changed=COHESIX_BUILD_STAMP");
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);
    let git = git_stdout(["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "nogit".to_owned());
    let git_dirty_suffix = if git_has_tracked_changes() {
        "-dirty"
    } else {
        ""
    };
    let timestamp = env::var("COHESIX_BUILD_STAMP").unwrap_or_else(|_| Utc::now().to_rfc3339());
    let git_hash = format!("{}{}", git.trim(), git_dirty_suffix);
    let git_hash = rust_string_literal(&git_hash);
    let timestamp = rust_string_literal(&timestamp);
    let contents =
        format!("pub const GIT_HASH:&str={git_hash};\npub const BUILD_TS:&str={timestamp};\n");
    fs::write(out_dir.join("built_info.rs"), contents)?;
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}

fn emit_git_rerun_triggers() -> io::Result<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(io::Error::other)?);
    let repo_root = manifest_dir
        .parent()
        .and_then(|parent| parent.parent())
        .ok_or_else(|| io::Error::other("unable to locate repo root"))?;
    let Some(git_dir) = resolve_git_dir(repo_root, &repo_root.join(".git")) else {
        return Ok(());
    };

    emit_rerun_if_path_exists(&git_dir.join("HEAD"));
    emit_rerun_if_path_exists(&git_dir.join("index"));
    emit_rerun_if_path_exists(&git_dir.join("packed-refs"));

    if let Ok(head) = fs::read_to_string(git_dir.join("HEAD")) {
        if let Some(reference) = head.trim().strip_prefix("ref: ") {
            emit_rerun_if_path_exists(&git_dir.join(reference));
        }
    }
    Ok(())
}

fn resolve_git_dir(repo_root: &Path, dot_git: &Path) -> Option<PathBuf> {
    if dot_git.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    let gitdir = fs::read_to_string(dot_git).ok()?;
    let raw_path = gitdir.trim().strip_prefix("gitdir:")?.trim();
    let path = PathBuf::from(raw_path);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(repo_root.join(path))
    }
}

fn emit_rerun_if_path_exists(path: &Path) {
    if path.exists() {
        emit_cargo_directive(format!("cargo:rerun-if-changed={}", path.display()));
    }
}

fn git_stdout<const N: usize>(args: [&str; N]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

fn git_has_tracked_changes() -> bool {
    git_stdout(["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|status| !status.trim().is_empty())
}

fn validate_generated_manifest() -> io::Result<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(io::Error::other)?);
    let repo_root = manifest_dir
        .parent()
        .and_then(|parent| parent.parent())
        .ok_or_else(|| io::Error::other("unable to locate repo root"))?;
    let manifest_path = repo_root.join("configs/root_task.toml");
    let generated_dir = manifest_dir.join("src").join("generated");
    let generated_mod = generated_dir.join("mod.rs");
    let generated_bootstrap = generated_dir.join("bootstrap.rs");
    let manifest_out = repo_root.join("configs/generated/root_task_resolved.json");
    let manifest_hash = repo_root.join("configs/generated/root_task_resolved.json.sha256");
    let cli_script = repo_root.join("scripts/cohsh/boot_v0.coh");
    let doc_snippet = repo_root.join("docs/snippets/root_task_manifest.md");

    for path in [
        &manifest_path,
        &generated_mod,
        &generated_bootstrap,
        &manifest_out,
        &manifest_hash,
        &cli_script,
        &doc_snippet,
    ] {
        emit_cargo_directive(format!("cargo:rerun-if-changed={}", path.display()));
    }

    let manifest_meta = manifest_path.metadata().map_err(|err| {
        io::Error::other(format!(
            "missing configs/root_task.toml (run coh-rtc): {err}"
        ))
    })?;
    let manifest_mtime = manifest_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let manifest_has_tracked_changes = tracked_path_has_changes(repo_root, &manifest_path);

    let required = [
        generated_mod,
        generated_bootstrap,
        manifest_out,
        manifest_hash,
        cli_script,
        doc_snippet,
    ];

    for path in required {
        let meta = path.metadata().map_err(|err| {
            io::Error::other(format!(
                "missing generated artefact {} (run coh-rtc): {err}",
                path.display()
            ))
        })?;
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if generated_artifact_is_stale(manifest_has_tracked_changes, manifest_mtime, modified) {
            return Err(io::Error::other(format!(
                "generated artefact {} is stale relative to configs/root_task.toml; rerun coh-rtc",
                path.display()
            )));
        }
    }
    Ok(())
}

fn tracked_path_has_changes(repo_root: &Path, path: &Path) -> bool {
    let Ok(relative_path) = path.strip_prefix(repo_root) else {
        return false;
    };
    let Ok(status) = Command::new("git")
        .current_dir(repo_root)
        .args(["diff", "--quiet", "HEAD", "--"])
        .arg(relative_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    else {
        return false;
    };

    status.code() == Some(1)
}

fn emit_pi4_wifi_firmware() -> io::Result<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(io::Error::other)?);
    let repo_root = manifest_dir
        .parent()
        .and_then(|parent| parent.parent())
        .ok_or_else(|| io::Error::other("unable to locate repo root"))?;
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);
    let staged = out_dir.join("pi4_wifi_firmware.rs");

    let firmware = find_pi4_wifi_firmware(repo_root)
        .map_err(io::Error::other)?
        .map(|bundle| Pi4WifiFirmwareBundle {
            firmware: bundle.firmware,
            nvram: bundle.nvram,
            clm_blob: bundle.clm_blob,
            firmware_sha256: bundle.firmware_sha256,
            nvram_sha256: bundle.nvram_sha256,
            clm_blob_sha256: bundle.clm_blob_sha256,
            board_type: PI4_WIFI_BOARD_TYPE,
        });

    let mut contents = String::from(
        "// Copyright 2026 Lukas Bower\n\
// SPDX-License-Identifier: Apache-2.0\n\
// Purpose: Stages bounded Pi 4 CYW43455 firmware assets discovered at build time.\n\
// Author: Lukas Bower\n\n",
    );

    match firmware {
        Some(bundle) => {
            for path in [&bundle.firmware, &bundle.nvram, &bundle.clm_blob] {
                emit_cargo_directive(format!("cargo:rerun-if-changed={}", path.display()));
            }
            let firmware = rust_string_literal(&bundle.firmware.to_string_lossy());
            let nvram = rust_string_literal(&bundle.nvram.to_string_lossy());
            let clm_blob = rust_string_literal(&bundle.clm_blob.to_string_lossy());
            let firmware_sha256 = rust_string_literal(bundle.firmware_sha256);
            let nvram_sha256 = rust_string_literal(bundle.nvram_sha256);
            let clm_blob_sha256 = rust_string_literal(bundle.clm_blob_sha256);
            let board_type = rust_string_literal(bundle.board_type);
            contents.push_str(&format!(
                "pub(crate) static PI4_WIFI_FIRMWARE: &[u8] = include_bytes!({firmware});\n\
pub(crate) static PI4_WIFI_NVRAM: &[u8] = include_bytes!({nvram});\n\
pub(crate) static PI4_WIFI_CLM_BLOB: &[u8] = include_bytes!({clm_blob});\n\
pub(crate) const PI4_WIFI_FIRMWARE_SHA256: &str = {firmware_sha256};\n\
pub(crate) const PI4_WIFI_NVRAM_SHA256: &str = {nvram_sha256};\n\
pub(crate) const PI4_WIFI_CLM_BLOB_SHA256: &str = {clm_blob_sha256};\n\
pub(crate) const PI4_WIFI_BOARD_TYPE: &str = {board_type};\n",
            ));
        }
        None if env::var_os("CARGO_FEATURE_RELEASE_PI4").is_some() => {
            return Err(io::Error::other(format!(
                "Pi 4 release builds require a CYW43455 firmware bundle; searched {} or set {PI4_WIFI_FIRMWARE_DIR_ENV}",
                pi4_wifi_default_search_dirs(repo_root)
            )));
        }
        None => {
            emit_cargo_directive(format!(
                "cargo:warning=Pi 4 WiFi firmware bundle not found in {}; generated CYW43455 assets are empty",
                pi4_wifi_default_search_dirs(repo_root)
            ));
            contents.push_str(
                "pub(crate) static PI4_WIFI_FIRMWARE: &[u8] = &[];\n\
pub(crate) static PI4_WIFI_NVRAM: &[u8] = &[];\n\
pub(crate) static PI4_WIFI_CLM_BLOB: &[u8] = &[];\n\
pub(crate) const PI4_WIFI_FIRMWARE_SHA256: &str = \"none\";\n\
pub(crate) const PI4_WIFI_NVRAM_SHA256: &str = \"none\";\n\
pub(crate) const PI4_WIFI_CLM_BLOB_SHA256: &str = \"none\";\n\
pub(crate) const PI4_WIFI_BOARD_TYPE: &str = \"raspberrypi,4-model-b\";\n",
            );
        }
    }

    fs::write(staged, contents)?;
    Ok(())
}

struct Pi4WifiFirmwareBundle {
    firmware: PathBuf,
    nvram: PathBuf,
    clm_blob: PathBuf,
    firmware_sha256: &'static str,
    nvram_sha256: &'static str,
    clm_blob_sha256: &'static str,
    board_type: &'static str,
}

struct Pi4WifiFirmwareSearch {
    firmware: PathBuf,
    nvram: PathBuf,
    clm_blob: PathBuf,
    firmware_sha256: &'static str,
    nvram_sha256: &'static str,
    clm_blob_sha256: &'static str,
}

#[derive(Clone, Copy)]
struct Pi4WifiKnownBundle {
    dir: &'static str,
    firmware: Pi4WifiKnownArtifact,
    nvram: Pi4WifiKnownArtifact,
    clm_blob: Pi4WifiKnownArtifact,
}

#[derive(Clone, Copy)]
struct Pi4WifiKnownArtifact {
    file_name: &'static str,
    expected_len: u64,
    expected_sha256: &'static str,
}

const PI4_WIFI_FIRMWARE_DIR_ENV: &str = "COHESIX_PI4_WIFI_FIRMWARE_DIR";
const PI4_WIFI_THIRD_PARTY_BUNDLE_DIR: &str =
    "third_party/raspberry-pi-firmware/v1.50/firmware/cyw43455-linux-capture";
const PI4_WIFI_BOARD_TYPE: &str = "raspberrypi,4-model-b";
const PI4_WIFI_CAPTURE_FIRMWARE: Pi4WifiKnownArtifact = Pi4WifiKnownArtifact {
    file_name: "cyfmac43455-sdio.bin",
    expected_len: 609_309,
    expected_sha256: "d608f866582519c0a28d86db43040f4f1b98dd1d153e72e9752586546b4a36c3",
};
const PI4_WIFI_CAPTURE_NVRAM: Pi4WifiKnownArtifact = Pi4WifiKnownArtifact {
    file_name: "brcmfmac43455-sdio.raspberrypi,4-model-b.txt",
    expected_len: 2_074,
    expected_sha256: "ca709be81a78bdb6932936374f39943acbd7af07fae6151011127599a3ce9e3d",
};
const PI4_WIFI_CAPTURE_CLM: Pi4WifiKnownArtifact = Pi4WifiKnownArtifact {
    file_name: "cyfmac43455-sdio.clm_blob",
    expected_len: 2_676,
    expected_sha256: "9823842cae9fb9a5dd1e5fb31f595516ec7deee341354bef30bb3026eee29cc1",
};
const PI4_WIFI_KNOWN_BUNDLES: &[Pi4WifiKnownBundle] = &[Pi4WifiKnownBundle {
    dir: PI4_WIFI_THIRD_PARTY_BUNDLE_DIR,
    firmware: PI4_WIFI_CAPTURE_FIRMWARE,
    nvram: PI4_WIFI_CAPTURE_NVRAM,
    clm_blob: PI4_WIFI_CAPTURE_CLM,
}];
const PI4_DRIVER_RUNTIME_PAYLOAD_ENV: &str = "COHESIX_PI4_DRIVER_RUNTIME_PAYLOAD";

fn emit_pi4_driver_runtime_payload() -> io::Result<()> {
    println!("cargo:rerun-if-env-changed={PI4_DRIVER_RUNTIME_PAYLOAD_ENV}");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(io::Error::other)?);
    let repo_root = manifest_dir
        .parent()
        .and_then(|parent| parent.parent())
        .ok_or_else(|| io::Error::other("unable to locate repo root"))?;
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);
    let staged = out_dir.join("pi4_driver_runtime_payload.rs");

    let mut contents = String::from(
        "// Copyright 2026 Lukas Bower\n\
// SPDX-License-Identifier: Apache-2.0\n\
// Purpose: Stage the embedded Pi 4 driver runtime CPIO payload.\n\
// Author: Lukas Bower\n\n",
    );

    match env::var(PI4_DRIVER_RUNTIME_PAYLOAD_ENV) {
        Ok(value) if !value.trim().is_empty() => {
            let configured = PathBuf::from(value);
            let payload = if configured.is_absolute() {
                configured
            } else {
                repo_root.join(configured)
            };
            validate_pi4_driver_runtime_payload(&payload)?;
            emit_cargo_directive(format!("cargo:rerun-if-changed={}", payload.display()));
            let payload = rust_string_literal(&payload.to_string_lossy());
            contents.push_str(&format!(
                "pub(crate) static EMBEDDED_PI4_DRIVER_RUNTIME_PAYLOAD: &[u8] = include_bytes!({payload});\n",
            ));
        }
        _ => {
            contents
                .push_str("pub(crate) static EMBEDDED_PI4_DRIVER_RUNTIME_PAYLOAD: &[u8] = &[];\n");
        }
    }

    fs::write(staged, contents)?;
    Ok(())
}

fn validate_pi4_driver_runtime_payload(path: &Path) -> io::Result<()> {
    let data = fs::read(path).map_err(|err| {
        io::Error::other(format!(
            "failed to read {} from {PI4_DRIVER_RUNTIME_PAYLOAD_ENV}: {err}",
            path.display()
        ))
    })?;
    if data.is_empty() {
        return Err(io::Error::other(format!(
            "{} from {PI4_DRIVER_RUNTIME_PAYLOAD_ENV} is empty",
            path.display()
        )));
    }
    if !payload_contains_cpio_magic(&data) {
        return Err(io::Error::other(format!(
            "{} from {PI4_DRIVER_RUNTIME_PAYLOAD_ENV} does not contain a newc CPIO archive in the first 4096 bytes",
            path.display()
        )));
    }
    Ok(())
}

fn payload_contains_cpio_magic(data: &[u8]) -> bool {
    let search_len = data.len().min(4096);
    data[..search_len]
        .windows(6)
        .any(|window| window == b"070701")
}

fn find_pi4_wifi_firmware(repo_root: &Path) -> Result<Option<Pi4WifiFirmwareSearch>, String> {
    println!("cargo:rerun-if-env-changed={PI4_WIFI_FIRMWARE_DIR_ENV}");
    if let Ok(value) = env::var(PI4_WIFI_FIRMWARE_DIR_ENV) {
        if !value.trim().is_empty() {
            let configured = PathBuf::from(value);
            let firmware_dir = if configured.is_absolute() {
                configured
            } else {
                repo_root.join(configured)
            };
            if !firmware_dir.is_dir() {
                return Err(format!(
                    "{}={} is not a directory",
                    PI4_WIFI_FIRMWARE_DIR_ENV,
                    firmware_dir.display(),
                ));
            }
            return find_pi4_wifi_firmware_in_dir(&firmware_dir).map(Some);
        }
    }

    let mut invalid = Vec::new();
    for bundle in PI4_WIFI_KNOWN_BUNDLES {
        let firmware_dir = repo_root.join(bundle.dir);
        emit_cargo_directive(format!("cargo:rerun-if-changed={}", firmware_dir.display()));
        if !firmware_dir.is_dir() {
            continue;
        }
        match validate_pi4_wifi_known_bundle(&firmware_dir, bundle) {
            Ok(found) => return Ok(Some(found)),
            Err(err) => invalid.push(format!("{}: {}", firmware_dir.display(), err)),
        }
    }

    if invalid.is_empty() {
        Ok(None)
    } else {
        Err(format!(
            "Pi 4 CYW43455 firmware candidates were present but invalid: {}",
            invalid.join("; "),
        ))
    }
}

fn find_pi4_wifi_firmware_in_dir(firmware_dir: &Path) -> Result<Pi4WifiFirmwareSearch, String> {
    let mut invalid = Vec::new();
    for bundle in PI4_WIFI_KNOWN_BUNDLES {
        match validate_pi4_wifi_known_bundle(firmware_dir, bundle) {
            Ok(found) => return Ok(found),
            Err(err) => invalid.push(format!("{} layout: {}", bundle.dir, err)),
        }
    }

    Err(format!(
        "{}={} does not match a supported CYW43455 firmware bundle: {}",
        PI4_WIFI_FIRMWARE_DIR_ENV,
        firmware_dir.display(),
        invalid.join("; "),
    ))
}

fn validate_pi4_wifi_known_bundle(
    firmware_dir: &Path,
    bundle: &Pi4WifiKnownBundle,
) -> Result<Pi4WifiFirmwareSearch, String> {
    let firmware = validate_pi4_wifi_known_artifact(firmware_dir, &bundle.firmware)?;
    let nvram = validate_pi4_wifi_known_artifact(firmware_dir, &bundle.nvram)?;
    let clm_blob = validate_pi4_wifi_known_artifact(firmware_dir, &bundle.clm_blob)?;
    Ok(Pi4WifiFirmwareSearch {
        firmware,
        nvram,
        clm_blob,
        firmware_sha256: bundle.firmware.expected_sha256,
        nvram_sha256: bundle.nvram.expected_sha256,
        clm_blob_sha256: bundle.clm_blob.expected_sha256,
    })
}

fn pi4_wifi_default_search_dirs(repo_root: &Path) -> String {
    PI4_WIFI_KNOWN_BUNDLES
        .iter()
        .map(|bundle| repo_root.join(bundle.dir).display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn validate_pi4_wifi_known_artifact(
    firmware_dir: &Path,
    artifact: &Pi4WifiKnownArtifact,
) -> Result<PathBuf, String> {
    let path = firmware_dir.join(artifact.file_name);
    let meta = fs::metadata(&path).map_err(|err| format!("{} missing: {err}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.len() != artifact.expected_len {
        return Err(format!(
            "{} length mismatch: got {} expected {}",
            path.display(),
            meta.len(),
            artifact.expected_len,
        ));
    }
    let actual_sha256 = sha256_hex(&path)?;
    if actual_sha256 != artifact.expected_sha256 {
        return Err(format!(
            "{} sha256 mismatch: got {} expected {}",
            path.display(),
            actual_sha256,
            artifact.expected_sha256,
        ));
    }
    Ok(path)
}

fn sha256_hex(path: &Path) -> Result<String, String> {
    let shasum = Command::new("shasum")
        .arg("-a")
        .arg("256")
        .arg(path)
        .output();
    if let Ok(output) = shasum {
        if output.status.success() {
            return parse_sha256_output(path, &output.stdout);
        }
    }

    let sha256sum = Command::new("sha256sum").arg(path).output();
    if let Ok(output) = sha256sum {
        if output.status.success() {
            return parse_sha256_output(path, &output.stdout);
        }
    }

    Err(format!(
        "unable to compute sha256 for {}; install shasum or sha256sum",
        path.display()
    ))
}

fn parse_sha256_output(path: &Path, stdout: &[u8]) -> Result<String, String> {
    let output = std::str::from_utf8(stdout)
        .map_err(|err| format!("invalid sha256 output for {}: {err}", path.display()))?;
    output
        .split_whitespace()
        .next()
        .filter(|hash| hash.len() == 64 && hash.chars().all(|ch| ch.is_ascii_hexdigit()))
        .map(|hash| hash.to_ascii_lowercase())
        .ok_or_else(|| format!("malformed sha256 output for {}", path.display()))
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

fn stage_and_emit_linker_script(script: &Path) -> Result<(), String> {
    emit_cargo_directive(format!("cargo:rerun-if-changed={}", script.display()));

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set by cargo"));
    let staged = out_dir.join(script.file_name().unwrap_or_else(|| OsStr::new("sel4.ld")));

    let mut contents = fs::read_to_string(script).map_err(|err| {
        format!(
            "Failed to read linker script {} for staging: {}",
            script.display(),
            err
        )
    })?;

    // Guard against the precedence pitfall in the upstream elfloader
    // script where the core stack reservation expression,
    // `. = . + 1 * 1 << 12;`, expands to `(. + 1) << 12`, inflating the
    // root-task image size by ~256 GiB and pushing `.bss` symbols far
    // outside the 4 GiB range that AArch64 ADRP relocations can reach.
    // Normalise the expression so the increment is a single 4 KiB page.
    contents = contents.replace(". = . + 1 * 1 << 12;", ". = . + (1 * (1 << 12));");

    fs::write(&staged, contents).map_err(|err| {
        format!(
            "Failed to stage linker script from {} to {}: {}",
            script.display(),
            staged.display(),
            err
        )
    })?;

    emit_cargo_directive(format!("cargo:rustc-env=SEL4_LD={}", staged.display()));
    emit_cargo_directive(format!(
        "cargo:rustc-link-arg-bin=root-task=-T{}",
        staged.display()
    ));
    println!("cargo:rustc-link-arg-bin=root-task=-gc-sections");
    println!("cargo:rustc-link-arg-bin=root-task=-no-pie");
    Ok(())
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
            Ok(script) => return stage_and_emit_linker_script(&script),
            Err(err) => errors.push(format!("{}: {}", candidate.file_name, err)),
        }
    }

    let manifest_fallback = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|err| format!("CARGO_MANIFEST_DIR is unavailable: {err}"))?,
    )
    .join("sel4.ld");
    if file_matches(&manifest_fallback) {
        match classify_linker_script(&manifest_fallback) {
            Ok(LinkerScriptKind::User) => return stage_and_emit_linker_script(&manifest_fallback),
            Ok(other) => errors.push(format!(
                "manifest fallback {} rejected: classified as {:?}",
                manifest_fallback.display(),
                other
            )),
            Err(err) => errors.push(format!(
                "manifest fallback {} rejected: {}",
                manifest_fallback.display(),
                err
            )),
        }
    } else {
        errors.push(format!(
            "manifest fallback {} is missing",
            manifest_fallback.display()
        ));
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

fn emit_timer_build_metadata(build_root: &Path) -> Result<u64, String> {
    let header = find_artifact_with(
        build_root,
        "platform_gen.h",
        TIMER_PLATFORM_HEADER_CANDIDATES,
        |path| {
            let contents = fs::read_to_string(path)
                .map_err(|err| format!("failed to read {}: {}", path.display(), err))?;
            match parse_timer_clock_hz(&contents) {
                Some(_) => Ok(ArtifactDecision::Accept),
                None => Ok(ArtifactDecision::Reject(format!(
                    "missing TIMER_CLOCK_HZ: {}",
                    path.display()
                ))),
            }
        },
    )?;
    emit_cargo_directive(format!("cargo:rerun-if-changed={}", header.display()));
    let contents = fs::read_to_string(&header)
        .map_err(|err| format!("failed to read {}: {}", header.display(), err))?;
    let timer_clock_hz = parse_timer_clock_hz(&contents)
        .ok_or_else(|| format!("missing TIMER_CLOCK_HZ in {}", header.display()))?;
    println!("cargo:rustc-env=SEL4_TIMER_CLOCK_HZ={timer_clock_hz}");
    println!("cargo:rustc-env=SEL4_TIMER_COUNTER_KIND=virtual");
    Ok(timer_clock_hz)
}

fn validate_arch_counter_config(build_root: &Path, timer_clock_hz: u64) {
    if timer_clock_hz == 0 {
        panic!("seL4 TIMER_CLOCK_HZ must be nonzero");
    }

    let vcnt_user = probe_any_config_flag(
        build_root,
        &["KernelArmExportVCNTUser", "CONFIG_EXPORT_VCNT_USER"],
    );
    if vcnt_user == Some(true) {
        println!("cargo:rustc-cfg=sel4_config_export_vcnt_user");
    }

    if !feature_enabled("TIMERS_ARCH_COUNTER") {
        return;
    }

    if vcnt_user != Some(true) {
        panic!(
            "feature `timers-arch-counter` requires the selected seL4 build to expose \
             CNTVCT_EL0/CNTFRQ_EL0 to EL0 (KernelArmExportVCNTUser=ON or \
             CONFIG_EXPORT_VCNT_USER=y). Reconfigure the Pi build instead of falling \
             back to the dummy timer."
        );
    }

    for flag in [
        ("KernelArmExportPCNTUser", "CONFIG_EXPORT_PCNT_USER"),
        ("KernelArmExportPTMRUser", "CONFIG_EXPORT_PTMR_USER"),
        ("KernelArmExportVTMRUser", "CONFIG_EXPORT_VTMR_USER"),
    ] {
        if probe_any_config_flag(build_root, &[flag.0, flag.1]) == Some(true) {
            panic!(
                "feature `timers-arch-counter` expects the Pi profile to export only \
                 the read-only virtual counter; disable {} / {}",
                flag.0, flag.1
            );
        }
    }
}

fn feature_enabled(feature: &str) -> bool {
    let env_name = format!(
        "CARGO_FEATURE_{}",
        feature.replace('-', "_").to_ascii_uppercase()
    );
    env::var_os(env_name).is_some()
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

fn probe_any_config_flag(root: &Path, flags: &[&str]) -> Option<bool> {
    flags.iter().find_map(|flag| probe_config_flag(root, flag))
}

fn probe_config_flag(root: &Path, flag: &str) -> Option<bool> {
    for relative in CONFIG_CANDIDATES {
        let candidate = root.join(relative);
        emit_cargo_directive(format!("cargo:rerun-if-changed={}", candidate.display()));
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

        if let Some(value) = parse_define_line(line, flag) {
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
    if line.contains(flag) && line.contains("is not set") {
        return Some(false);
    }

    if line.contains(flag) && line.contains("disabled:") {
        return Some(false);
    }

    None
}

fn parse_define_line(line: &str, flag: &str) -> Option<bool> {
    let stripped = line.strip_prefix("#define")?.trim();
    let mut parts = stripped.split_whitespace();
    let name = parts.next()?;
    if name != flag {
        return None;
    }
    let value = parts.next().unwrap_or("1");
    parse_bool_token(value)
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
