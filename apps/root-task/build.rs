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
use serde_json::Value;
use sha2::{Digest, Sha256};

#[path = "build_support.rs"]
mod build_support;

use build_support::{
    classify_linker_script, format_build_marker, generated_artifact_is_stale, parse_timer_clock_hz,
    resolve_git_metadata_dirs, select_build_git_hash, BuildMarkerFeatures, LinkerScriptKind,
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
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
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
    if let Err(err) = emit_worker_image_identity(target_os == "none") {
        panic!("failed to bind Worker image identity: {err}");
    }
    if let Err(err) = emit_console_network_image_identity(target_os == "none") {
        panic!("failed to bind console-network runtime image identity: {err}");
    }
    if let Err(err) = emit_ninedoor_image_identity(target_os == "none") {
        panic!("failed to bind NineDoor runtime image identity: {err}");
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

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn parse_sha256(value: &str, label: &str) -> io::Result<[u8; 32]> {
    if value.len() != 64 {
        return Err(io::Error::other(format!(
            "{label} must be exactly 64 lowercase hexadecimal characters"
        )));
    }
    let mut output = [0u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = core::str::from_utf8(chunk).map_err(io::Error::other)?;
        if !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(io::Error::other(format!(
                "{label} is not lowercase hexadecimal"
            )));
        }
        output[index] = u8::from_str_radix(text, 16).map_err(io::Error::other)?;
    }
    Ok(output)
}

fn rust_digest(value: [u8; 32]) -> String {
    let bytes = value.map(|byte| format!("0x{byte:02x}"));
    format!("[{}]", bytes.join(","))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ConsoleNetworkElfIdentity {
    entry_vaddr: u64,
    load_base_vaddr: u64,
    load_limit_vaddr: u64,
    load_pages: u16,
}

fn elf_u16(bytes: &[u8], offset: usize) -> io::Result<u16> {
    let raw: [u8; 2] = bytes
        .get(offset..offset.saturating_add(2))
        .and_then(|slice| slice.try_into().ok())
        .ok_or_else(|| io::Error::other("console-network ELF u16 is truncated"))?;
    Ok(u16::from_le_bytes(raw))
}

fn elf_u32(bytes: &[u8], offset: usize) -> io::Result<u32> {
    let raw: [u8; 4] = bytes
        .get(offset..offset.saturating_add(4))
        .and_then(|slice| slice.try_into().ok())
        .ok_or_else(|| io::Error::other("console-network ELF u32 is truncated"))?;
    Ok(u32::from_le_bytes(raw))
}

fn elf_u64(bytes: &[u8], offset: usize) -> io::Result<u64> {
    let raw: [u8; 8] = bytes
        .get(offset..offset.saturating_add(8))
        .and_then(|slice| slice.try_into().ok())
        .ok_or_else(|| io::Error::other("console-network ELF u64 is truncated"))?;
    Ok(u64::from_le_bytes(raw))
}

fn elf_table_offset(base: u64, index: usize, entry_bytes: usize) -> io::Result<usize> {
    usize::try_from(base)
        .ok()
        .and_then(|base| {
            index
                .checked_mul(entry_bytes)
                .and_then(|offset| base.checked_add(offset))
        })
        .ok_or_else(|| io::Error::other("console-network ELF table offset overflows"))
}

fn elf_string(table: &[u8], offset: usize) -> io::Result<&str> {
    let tail = table
        .get(offset..)
        .ok_or_else(|| io::Error::other("console-network ELF symbol name offset is invalid"))?;
    let length = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| io::Error::other("console-network ELF symbol name is unterminated"))?;
    core::str::from_utf8(&tail[..length]).map_err(io::Error::other)
}

fn console_network_has_exact_entry_symbol(bytes: &[u8], entry: u64) -> io::Result<bool> {
    const SHT_SYMTAB: u32 = 2;
    const SHT_DYNSYM: u32 = 11;
    const SYMBOL_BYTES: usize = 24;

    let section_offset = elf_u64(bytes, 40)?;
    let section_entry_bytes = usize::from(elf_u16(bytes, 58)?);
    let section_count = usize::from(elf_u16(bytes, 60)?);
    if section_entry_bytes < 64 || section_count == 0 || section_count > 256 {
        return Ok(false);
    }
    for index in 0..section_count {
        let section = elf_table_offset(section_offset, index, section_entry_bytes)?;
        let section_type = elf_u32(bytes, section.saturating_add(4))?;
        if !matches!(section_type, SHT_SYMTAB | SHT_DYNSYM) {
            continue;
        }
        let symbol_offset = elf_u64(bytes, section.saturating_add(24))?;
        let symbol_bytes = usize::try_from(elf_u64(bytes, section.saturating_add(32))?)
            .map_err(io::Error::other)?;
        let linked_strings = usize::try_from(elf_u32(bytes, section.saturating_add(40))?)
            .map_err(io::Error::other)?;
        if linked_strings >= section_count {
            return Ok(false);
        }
        let symbol_entry_bytes = usize::try_from(elf_u64(bytes, section.saturating_add(56))?)
            .map_err(io::Error::other)?;
        if symbol_entry_bytes < SYMBOL_BYTES || symbol_bytes % symbol_entry_bytes != 0 {
            return Ok(false);
        }
        let strings_section =
            elf_table_offset(section_offset, linked_strings, section_entry_bytes)?;
        let strings_offset = usize::try_from(elf_u64(bytes, strings_section.saturating_add(24))?)
            .map_err(io::Error::other)?;
        let strings_bytes = usize::try_from(elf_u64(bytes, strings_section.saturating_add(32))?)
            .map_err(io::Error::other)?;
        let strings = bytes
            .get(strings_offset..strings_offset.saturating_add(strings_bytes))
            .ok_or_else(|| io::Error::other("console-network ELF string table is truncated"))?;
        let symbol_count = symbol_bytes / symbol_entry_bytes;
        for symbol_index in 0..symbol_count {
            let symbol = elf_table_offset(symbol_offset, symbol_index, symbol_entry_bytes)?;
            let name_offset = usize::try_from(elf_u32(bytes, symbol)?).map_err(io::Error::other)?;
            let value = elf_u64(bytes, symbol.saturating_add(8))?;
            if value == entry && elf_string(strings, name_offset)? == "_start" {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn validate_console_network_elf(bytes: &[u8]) -> io::Result<ConsoleNetworkElfIdentity> {
    const MAX_IMAGE_BYTES: usize = 512 * 1024;
    const ELF_HEADER_BYTES: usize = 64;
    const PROGRAM_HEADER_BYTES: usize = 56;
    const MAX_PROGRAM_HEADERS: usize = 16;
    const PT_LOAD: u32 = 1;
    const PF_X: u32 = 1;
    const PF_W: u32 = 2;
    const PF_R: u32 = 4;
    const PAGE_BYTES: u64 = 4096;

    if !(ELF_HEADER_BYTES..=MAX_IMAGE_BYTES).contains(&bytes.len())
        || bytes.get(..7) != Some(b"\x7fELF\x02\x01\x01")
        || elf_u16(bytes, 16)? != 2
        || elf_u16(bytes, 18)? != 183
        || elf_u32(bytes, 20)? != 1
        || elf_u16(bytes, 52)? != ELF_HEADER_BYTES as u16
    {
        return Err(io::Error::other(
            "console-network runtime must be a bounded ELF64-LE AArch64 ET_EXEC image",
        ));
    }
    let entry = elf_u64(bytes, 24)?;
    let program_offset = elf_u64(bytes, 32)?;
    let program_entry_bytes = usize::from(elf_u16(bytes, 54)?);
    let program_count = usize::from(elf_u16(bytes, 56)?);
    if entry == 0
        || program_entry_bytes < PROGRAM_HEADER_BYTES
        || program_count == 0
        || program_count > MAX_PROGRAM_HEADERS
    {
        return Err(io::Error::other(
            "console-network ELF entry or program-header bound is invalid",
        ));
    }

    let mut load_base = u64::MAX;
    let mut load_limit = 0u64;
    let mut entry_executable = false;
    let mut load_count = 0usize;
    let mut ranges = [(0u64, 0u64); MAX_PROGRAM_HEADERS];
    for index in 0..program_count {
        let header = elf_table_offset(program_offset, index, program_entry_bytes)?;
        if header.saturating_add(PROGRAM_HEADER_BYTES) > bytes.len() {
            return Err(io::Error::other(
                "console-network ELF program headers are truncated",
            ));
        }
        if elf_u32(bytes, header)? != PT_LOAD {
            continue;
        }
        let flags = elf_u32(bytes, header.saturating_add(4))?;
        let file_offset = elf_u64(bytes, header.saturating_add(8))?;
        let vaddr = elf_u64(bytes, header.saturating_add(16))?;
        let file_bytes = elf_u64(bytes, header.saturating_add(32))?;
        let memory_bytes = elf_u64(bytes, header.saturating_add(40))?;
        let file_limit = file_offset
            .checked_add(file_bytes)
            .ok_or_else(|| io::Error::other("console-network ELF file extent overflows"))?;
        let memory_limit = vaddr
            .checked_add(memory_bytes)
            .ok_or_else(|| io::Error::other("console-network ELF memory extent overflows"))?;
        if memory_bytes == 0
            || file_bytes > memory_bytes
            || file_limit > bytes.len() as u64
            || flags & !(PF_R | PF_W | PF_X) != 0
            || flags & PF_R == 0
            || flags & (PF_W | PF_X) == (PF_W | PF_X)
        {
            return Err(io::Error::other(
                "console-network ELF load segment violates bounds or W^X",
            ));
        }
        for (seen_start, seen_limit) in ranges.iter().take(load_count).copied() {
            if vaddr < seen_limit && seen_start < memory_limit {
                return Err(io::Error::other(
                    "console-network ELF load segments overlap",
                ));
            }
        }
        ranges[load_count] = (vaddr, memory_limit);
        load_count += 1;
        load_base = load_base.min(vaddr & !(PAGE_BYTES - 1));
        load_limit = load_limit.max(
            memory_limit
                .checked_add(PAGE_BYTES - 1)
                .ok_or_else(|| io::Error::other("console-network ELF page span overflows"))?
                & !(PAGE_BYTES - 1),
        );
        entry_executable |= flags & PF_X != 0 && entry >= vaddr && entry < memory_limit;
    }
    if load_count == 0 || !entry_executable || load_limit <= load_base {
        return Err(io::Error::other(
            "console-network ELF entry is not in a bounded executable load segment",
        ));
    }
    if !console_network_has_exact_entry_symbol(bytes, entry)? {
        return Err(io::Error::other(
            "console-network ELF _start symbol is missing or differs from e_entry",
        ));
    }
    let load_pages = u16::try_from((load_limit - load_base) / PAGE_BYTES)
        .map_err(|_| io::Error::other("console-network ELF load-page count overflows"))?;
    Ok(ConsoleNetworkElfIdentity {
        entry_vaddr: entry,
        load_base_vaddr: load_base,
        load_limit_vaddr: load_limit,
        load_pages,
    })
}

fn generated_service_image_pages(service_key: &str, label: &str) -> io::Result<u16> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(io::Error::other)?);
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("unable to locate repository root"))?;
    let resolved_path = repo_root.join("configs/generated/root_task_resolved.json");
    let document: Value =
        serde_json::from_slice(&fs::read(resolved_path)?).map_err(io::Error::other)?;
    let service = document
        .get(service_key)
        .ok_or_else(|| io::Error::other(format!("resolved {label} service record is missing")))?;
    let total_frames = service
        .get("objects")
        .and_then(|objects| objects.get("frames"))
        .and_then(Value::as_u64)
        .ok_or_else(|| io::Error::other(format!("resolved {label} frame inventory is missing")))?;
    let stack_pages = service
        .get("stack_pages")
        .and_then(Value::as_u64)
        .ok_or_else(|| io::Error::other(format!("resolved {label} stack inventory is missing")))?;
    let direct_dma_pages = if service
        .get("direct_virtio")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        34
    } else {
        0
    };
    let pages = total_frames
        .checked_sub(
            stack_pages
                .saturating_add(6)
                .saturating_add(direct_dma_pages),
        )
        .and_then(|pages| u16::try_from(pages).ok())
        .filter(|pages| *pages != 0)
        .ok_or_else(|| {
            io::Error::other(format!("resolved {label} image-page budget is invalid"))
        })?;
    Ok(pages)
}

fn generated_console_network_image_pages() -> io::Result<u16> {
    generated_service_image_pages("console_network_service", "console-network")
}

fn emit_console_network_image_identity(required: bool) -> io::Result<()> {
    const IMAGE_ENV: &str = "COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE";

    println!("cargo:rerun-if-env-changed={IMAGE_ENV}");
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);
    let output_path = out_dir.join("console_network_image_identity.rs");
    let Some(image_path) = env::var_os(IMAGE_ENV).map(PathBuf::from) else {
        if required {
            return Err(io::Error::other(format!(
                "target root-task builds require {IMAGE_ENV}"
            )));
        }
        fs::write(
            output_path,
            "pub const CONSOLE_NETWORK_IMAGE_IDENTITY_BOUND:bool=false;\n\
             pub static CONSOLE_NETWORK_RUNTIME_IMAGE:&[u8]=&[];\n\
             pub const CONSOLE_NETWORK_RUNTIME_SHA256:[u8;32]=[0;32];\n\
             pub const CONSOLE_NETWORK_RUNTIME_BYTES:u64=0;\n\
             pub const CONSOLE_NETWORK_RUNTIME_ENTRY_VADDR:u64=0;\n\
             pub const CONSOLE_NETWORK_RUNTIME_LOAD_BASE_VADDR:u64=0;\n\
             pub const CONSOLE_NETWORK_RUNTIME_LOAD_LIMIT_VADDR:u64=0;\n\
             pub const CONSOLE_NETWORK_RUNTIME_LOAD_PAGES:u16=0;\n",
        )?;
        return Ok(());
    };
    let image_path = fs::canonicalize(image_path)?;
    println!("cargo:rerun-if-changed={}", image_path.display());
    let image = fs::read(&image_path)?;
    let identity = validate_console_network_elf(&image)?;
    let expected_pages = generated_console_network_image_pages()?;
    if identity.load_pages != expected_pages {
        return Err(io::Error::other(format!(
            "console-network ELF uses {} pages but generated object inventory admits exactly {expected_pages}",
            identity.load_pages
        )));
    }
    let include_path = rust_string_literal(&image_path.to_string_lossy());
    let contents = format!(
        "pub const CONSOLE_NETWORK_IMAGE_IDENTITY_BOUND:bool=true;\n\
         pub static CONSOLE_NETWORK_RUNTIME_IMAGE:&[u8]=include_bytes!({include_path});\n\
         pub const CONSOLE_NETWORK_RUNTIME_SHA256:[u8;32]={};\n\
         pub const CONSOLE_NETWORK_RUNTIME_BYTES:u64={};\n\
         pub const CONSOLE_NETWORK_RUNTIME_ENTRY_VADDR:u64={};\n\
         pub const CONSOLE_NETWORK_RUNTIME_LOAD_BASE_VADDR:u64={};\n\
         pub const CONSOLE_NETWORK_RUNTIME_LOAD_LIMIT_VADDR:u64={};\n\
         pub const CONSOLE_NETWORK_RUNTIME_LOAD_PAGES:u16={};\n",
        rust_digest(sha256_bytes(&image)),
        image.len(),
        identity.entry_vaddr,
        identity.load_base_vaddr,
        identity.load_limit_vaddr,
        identity.load_pages,
    );
    fs::write(output_path, contents)
}

fn emit_ninedoor_image_identity(required: bool) -> io::Result<()> {
    const IMAGE_ENV: &str = "COHESIX_NINEDOOR_RUNTIME_IMAGE";

    println!("cargo:rerun-if-env-changed={IMAGE_ENV}");
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);
    let output_path = out_dir.join("ninedoor_image_identity.rs");
    let Some(image_path) = env::var_os(IMAGE_ENV).map(PathBuf::from) else {
        if required {
            return Err(io::Error::other(format!(
                "target root-task builds require {IMAGE_ENV}"
            )));
        }
        fs::write(
            output_path,
            "pub const NINEDOOR_IMAGE_IDENTITY_BOUND:bool=false;\n\
             pub static NINEDOOR_RUNTIME_IMAGE:&[u8]=&[];\n\
             pub const NINEDOOR_RUNTIME_SHA256:[u8;32]=[0;32];\n\
             pub const NINEDOOR_RUNTIME_BYTES:u64=0;\n\
             pub const NINEDOOR_RUNTIME_ENTRY_VADDR:u64=0;\n\
             pub const NINEDOOR_RUNTIME_LOAD_BASE_VADDR:u64=0;\n\
             pub const NINEDOOR_RUNTIME_LOAD_LIMIT_VADDR:u64=0;\n\
             pub const NINEDOOR_RUNTIME_LOAD_PAGES:u16=0;\n",
        )?;
        return Ok(());
    };
    let image_path = fs::canonicalize(image_path)?;
    println!("cargo:rerun-if-changed={}", image_path.display());
    let image = fs::read(&image_path)?;
    let identity = validate_console_network_elf(&image)?;
    let expected_pages = generated_service_image_pages("ninedoor_service", "NineDoor")?;
    if identity.load_pages != expected_pages {
        return Err(io::Error::other(format!(
            "NineDoor ELF uses {} pages but generated object inventory admits exactly {expected_pages}",
            identity.load_pages
        )));
    }
    let include_path = rust_string_literal(&image_path.to_string_lossy());
    let contents = format!(
        "pub const NINEDOOR_IMAGE_IDENTITY_BOUND:bool=true;\n\
         pub static NINEDOOR_RUNTIME_IMAGE:&[u8]=include_bytes!({include_path});\n\
         pub const NINEDOOR_RUNTIME_SHA256:[u8;32]={};\n\
         pub const NINEDOOR_RUNTIME_BYTES:u64={};\n\
         pub const NINEDOOR_RUNTIME_ENTRY_VADDR:u64={};\n\
         pub const NINEDOOR_RUNTIME_LOAD_BASE_VADDR:u64={};\n\
         pub const NINEDOOR_RUNTIME_LOAD_LIMIT_VADDR:u64={};\n\
         pub const NINEDOOR_RUNTIME_LOAD_PAGES:u16={};\n",
        rust_digest(sha256_bytes(&image)),
        image.len(),
        identity.entry_vaddr,
        identity.load_base_vaddr,
        identity.load_limit_vaddr,
        identity.load_pages,
    );
    fs::write(output_path, contents)
}

fn manifest_string<'a>(object: &'a Value, key: &str) -> io::Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("Worker manifest field {key} is invalid")))
}

fn manifest_u64(object: &Value, key: &str) -> io::Result<u64> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| io::Error::other(format!("Worker manifest field {key} is invalid")))
}

fn emit_worker_image_identity(required: bool) -> io::Result<()> {
    const MANIFEST_ENV: &str = "COHESIX_WORKER_IMAGE_MANIFEST";
    const ARCHIVE_ENV: &str = "COHESIX_WORKER_IMAGE_ARCHIVE";
    const EXPECTED: [(&str, &str); 3] = [
        ("worker-heart", "worker-heartbeat"),
        ("worker-gpu", "worker-gpu"),
        ("worker-lora", "worker-lora"),
    ];

    println!("cargo:rerun-if-env-changed={MANIFEST_ENV}");
    println!("cargo:rerun-if-env-changed={ARCHIVE_ENV}");
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);
    let output_path = out_dir.join("worker_image_identity.rs");
    let manifest_path = env::var_os(MANIFEST_ENV).map(PathBuf::from);
    let archive_path = env::var_os(ARCHIVE_ENV).map(PathBuf::from);
    let (Some(manifest_path), Some(archive_path)) = (manifest_path, archive_path) else {
        if required {
            return Err(io::Error::other(
                "target root-task builds require COHESIX_WORKER_IMAGE_MANIFEST and COHESIX_WORKER_IMAGE_ARCHIVE",
            ));
        }
        fs::write(
            output_path,
            "pub const WORKER_IMAGE_IDENTITY_BOUND:bool=false;\n\
             pub const WORKER_ARCHIVE_SHA256:[u8;32]=[0;32];\n\
             pub const WORKER_MANIFEST_SHA256:[u8;32]=[0;32];\n\
             pub static EMBEDDED_WORKER_ARCHIVE:[u8;0]=[];\n\
             pub static EMBEDDED_WORKER_MANIFEST:[u8;0]=[];\n\
             pub const WORKER_IMAGE_IDENTITIES:[super::ExpectedWorkerImage;0]=[];\n",
        )?;
        return Ok(());
    };
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    println!("cargo:rerun-if-changed={}", archive_path.display());
    let manifest_bytes = fs::read(&manifest_path)?;
    let archive_bytes = fs::read(&archive_path)?;
    let document: Value = serde_json::from_slice(&manifest_bytes).map_err(io::Error::other)?;
    if manifest_string(&document, "schema")? != "cohesix-worker-image-manifest/v1"
        || manifest_string(&document, "target")? != "aarch64-unknown-none"
    {
        return Err(io::Error::other(
            "Worker manifest schema or target differs from the root contract",
        ));
    }
    let archive = document
        .get("archive")
        .ok_or_else(|| io::Error::other("Worker manifest archive identity is missing"))?;
    let declared_archive_bytes = manifest_u64(archive, "bytes")?;
    let declared_archive_digest = parse_sha256(
        manifest_string(archive, "sha256")?,
        "Worker archive SHA-256",
    )?;
    let actual_archive_digest = sha256_bytes(&archive_bytes);
    if declared_archive_bytes != archive_bytes.len() as u64
        || declared_archive_digest != actual_archive_digest
    {
        return Err(io::Error::other(
            "Worker archive bytes differ from the target-qualified manifest",
        ));
    }
    let images = document
        .get("images")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("Worker manifest image matrix is missing"))?;
    if images.len() != EXPECTED.len() {
        return Err(io::Error::other(
            "Worker manifest must bind exactly Heartbeat, GPU, and LoRA",
        ));
    }
    let mut generated_rows = String::new();
    for (index, ((expected_name, expected_role), image)) in
        EXPECTED.iter().zip(images.iter()).enumerate()
    {
        let name = manifest_string(image, "name")?;
        let role = manifest_string(image, "role")?;
        if name != *expected_name || role != *expected_role {
            return Err(io::Error::other(format!(
                "Worker image row {index} is not the required {expected_name}/{expected_role} binding"
            )));
        }
        if manifest_u64(image, "abi_version")? != 2
            || manifest_u64(image, "entry_version")? != 2
            || manifest_string(image, "entry_symbol")? != "_start"
        {
            return Err(io::Error::other(format!(
                "Worker image row {index} has an incompatible ABI entry contract"
            )));
        }
        let digest = parse_sha256(
            manifest_string(image, "image_sha256")?,
            "Worker image SHA-256",
        )?;
        let metadata_digest = parse_sha256(
            manifest_string(image, "metadata_sha256")?,
            "Worker metadata SHA-256",
        )?;
        let name = rust_string_literal(name);
        let role = rust_string_literal(role);
        let archive_path = rust_string_literal(manifest_string(image, "archive_path")?);
        generated_rows.push_str(&format!(
            "super::ExpectedWorkerImage{{name:{name},role:{role},archive_path:{archive_path},\
             image_sha256:{},image_bytes:{},entry_vaddr:{},load_base_vaddr:{},\
             load_limit_vaddr:{},metadata_vaddr:{},metadata_sha256:{}}},\n",
            rust_digest(digest),
            manifest_u64(image, "image_bytes")?,
            manifest_u64(image, "entry_vaddr")?,
            manifest_u64(image, "load_base_vaddr")?,
            manifest_u64(image, "load_limit_vaddr")?,
            manifest_u64(image, "metadata_vaddr")?,
            rust_digest(metadata_digest),
        ));
    }
    let archive_literal = rust_string_literal(archive_path.to_string_lossy().as_ref());
    let manifest_literal = rust_string_literal(manifest_path.to_string_lossy().as_ref());
    let contents = format!(
        "pub const WORKER_IMAGE_IDENTITY_BOUND:bool=true;\n\
         pub const WORKER_ARCHIVE_SHA256:[u8;32]={};\n\
         pub const WORKER_MANIFEST_SHA256:[u8;32]={};\n\
         #[used]\n\
         #[link_section=\".cohesix_worker_image_archive\"]\n\
         pub static EMBEDDED_WORKER_ARCHIVE:[u8;include_bytes!({archive_literal}).len()]=*include_bytes!({archive_literal});\n\
         #[used]\n\
         #[link_section=\".cohesix_worker_image_manifest\"]\n\
         pub static EMBEDDED_WORKER_MANIFEST:[u8;include_bytes!({manifest_literal}).len()]=*include_bytes!({manifest_literal});\n\
         pub const WORKER_IMAGE_IDENTITIES:[super::ExpectedWorkerImage;3]=[\n{}];\n",
        rust_digest(actual_archive_digest),
        rust_digest(sha256_bytes(&manifest_bytes)),
        generated_rows,
    );
    fs::write(output_path, contents)
}

fn emit_built_info() -> io::Result<()> {
    emit_git_rerun_triggers()?;
    println!("cargo:rerun-if-env-changed=COHESIX_BUILD_STAMP");
    println!("cargo:rerun-if-env-changed=COHESIX_EXACT_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=COHESIX_EXACT_SOURCE_CLEAN");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build_support.rs");
    println!("cargo:rerun-if-changed=src");
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);
    let git_full = git_stdout(["rev-parse", "HEAD"]).unwrap_or_else(|| "nogit".to_owned());
    let git_short =
        git_stdout(["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "nogit".to_owned());
    let exact_git_commit = env::var("COHESIX_EXACT_GIT_COMMIT").ok();
    let exact_source_clean = match env::var("COHESIX_EXACT_SOURCE_CLEAN") {
        Ok(value) if value == "1" => true,
        Ok(value) => {
            return Err(io::Error::other(format!(
                "COHESIX_EXACT_SOURCE_CLEAN must be exactly 1, got {value:?}"
            )));
        }
        Err(env::VarError::NotPresent) => false,
        Err(error) => return Err(io::Error::other(error)),
    };
    if exact_git_commit.is_none() && exact_source_clean {
        return Err(io::Error::other(
            "COHESIX_EXACT_SOURCE_CLEAN requires COHESIX_EXACT_GIT_COMMIT",
        ));
    }
    let git_hash = select_build_git_hash(
        git_full.trim(),
        git_short.trim(),
        git_has_worktree_changes(),
        exact_git_commit.as_deref(),
        exact_source_clean,
    )
    .map_err(io::Error::other)?;
    let timestamp = env::var("COHESIX_BUILD_STAMP").unwrap_or_else(|_| Utc::now().to_rfc3339());
    let build_marker = format_build_marker(
        &git_hash,
        &timestamp,
        BuildMarkerFeatures {
            kernel: cargo_feature_enabled("KERNEL"),
            bootstrap_trace: cargo_feature_enabled("BOOTSTRAP_TRACE"),
            serial_console: cargo_feature_enabled("SERIAL_CONSOLE"),
            net: cargo_feature_enabled("NET"),
            net_console: cargo_feature_enabled("NET_CONSOLE"),
            qemu_driver_task_smoke: cargo_feature_enabled("QEMU_DRIVER_TASK_SMOKE"),
        },
    );
    let build_marker_bytes = build_marker.len();
    let git_hash = rust_string_literal(&git_hash);
    let timestamp = rust_string_literal(&timestamp);
    let build_marker = rust_string_literal(&build_marker);
    let contents = format!(
        "pub const GIT_HASH:&str={git_hash};\npub const BUILD_TS:&str={timestamp};\n\
         #[used]\n\
         pub static BUILD_MARKER_BYTES:[u8;{build_marker_bytes}]=*b{build_marker};\n"
    );
    fs::write(out_dir.join("built_info.rs"), contents)?;
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}

fn cargo_feature_enabled(name: &str) -> bool {
    env::var_os(format!("CARGO_FEATURE_{name}")).is_some()
}

fn emit_git_rerun_triggers() -> io::Result<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(io::Error::other)?);
    let repo_root = manifest_dir
        .parent()
        .and_then(|parent| parent.parent())
        .ok_or_else(|| io::Error::other("unable to locate repo root"))?;
    let Some(git_dirs) = resolve_git_metadata_dirs(repo_root, &repo_root.join(".git")) else {
        return Ok(());
    };

    emit_rerun_if_path_exists(&git_dirs.worktree.join("HEAD"));
    emit_rerun_if_path_exists(&git_dirs.worktree.join("index"));
    emit_rerun_if_path_exists(&git_dirs.common.join("packed-refs"));

    if let Ok(head) = fs::read_to_string(git_dirs.worktree.join("HEAD")) {
        if let Some(reference) = head.trim().strip_prefix("ref: ") {
            emit_rerun_if_path_exists(&git_dirs.common.join(reference));
        }
    }
    Ok(())
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

fn git_has_worktree_changes() -> bool {
    git_stdout(["status", "--porcelain", "--untracked-files=all"])
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
                "#[used]\n\
#[link_section = \".cohesix_driver_runtime_payload\"]\n\
pub(crate) static EMBEDDED_PI4_DRIVER_RUNTIME_PAYLOAD: [u8; include_bytes!({payload}).len()] = *include_bytes!({payload});\n",
            ));
        }
        _ => {
            contents
                .push_str("pub(crate) static EMBEDDED_PI4_DRIVER_RUNTIME_PAYLOAD: [u8; 0] = [];\n");
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
             CONFIG_EXPORT_VCNT_USER=y). Reconfigure the selected target build instead of falling \
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
                "feature `timers-arch-counter` expects the selected target profile to export only \
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
