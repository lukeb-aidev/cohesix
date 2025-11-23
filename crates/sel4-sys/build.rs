// Author: Lukas Bower
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const CONFIG_CANDIDATES: &[&str] = &[
    ".config",
    "kernel/.config",
    "KernelConfig",
    "kernel/KernelConfig",
    "kernel/gen_config/KernelConfig",
    "kernel/gen_config/kernel/gen_config.h",
    "kernel/gen_config/kernel/KernelConfig",
    "kernel/gen_config/KernelConfigGenerated.cmake",
    "kernel/gen_config/kernel/KernelConfigGenerated.cmake",
    "kernel/gen_config/kernel_all.cmake",
];

fn main() {
    println!("cargo:rustc-check-cfg=cfg(sel4_config_kernel_mcs)");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD_DIR");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let build_dir = env::var("SEL4_BUILD_DIR")
        .or_else(|_| env::var("SEL4_BUILD"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("../../seL4/build"));

    if let Some(true) = probe_config_flag(&build_dir, "CONFIG_KERNEL_MCS") {
        println!("cargo:rustc-cfg=sel4_config_kernel_mcs");
    }

    generate_bindings(&build_dir);
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
    if !line.starts_with(flag) {
        return None;
    }

    let mut parts = line.splitn(2, '=');
    let _key = parts.next()?;
    let value = parts.next()?.trim();
    match value {
        "y" | "Y" | "1" => Some(true),
        "n" | "N" | "0" => Some(false),
        _ => None,
    }
}

fn parse_cmake_line(line: &str, flag: &str) -> Option<bool> {
    let line = line.strip_prefix("set(")?.trim_end_matches(')');
    let mut parts = line.split_whitespace();
    let key = parts.next()?;
    if key != flag {
        return None;
    }

    let value = parts.next()?;
    match value {
        "ON" | "TRUE" | "YES" | "1" => Some(true),
        "OFF" | "FALSE" | "NO" | "0" => Some(false),
        _ => None,
    }
}

fn generate_bindings(build_dir: &Path) {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let upstream_root = manifest_dir.join("upstream/libsel4");

    let wrapper = out_dir.join("wrapper.h");
    let mut wrapper_file = fs::File::create(&wrapper).expect("create wrapper");
    writeln!(wrapper_file, "#include <sel4/sel4.h>").unwrap();
    writeln!(wrapper_file, "#include <sel4/syscalls.h>").unwrap();

    writeln!(
        wrapper_file,
        "seL4_Error seL4_CNode_Copy(seL4_CNode _service, seL4_Word dest_index, seL4_Uint8 dest_depth, seL4_CNode src_root, seL4_Word src_index, seL4_Uint8 src_depth, seL4_CapRights_t rights);",
    )
    .unwrap();
    writeln!(
        wrapper_file,
        "seL4_Error seL4_CNode_Mint(seL4_CNode _service, seL4_Word dest_index, seL4_Uint8 dest_depth, seL4_CNode src_root, seL4_Word src_index, seL4_Uint8 src_depth, seL4_CapRights_t rights, seL4_Word badge);",
    )
    .unwrap();
    writeln!(
        wrapper_file,
        "seL4_Error seL4_Untyped_Retype(seL4_Untyped _service, seL4_Word type, seL4_Word size_bits, seL4_CNode root, seL4_Word node_index, seL4_Word node_depth, seL4_Word node_offset, seL4_Word num_objects);",
    )
    .unwrap();
    writeln!(
        wrapper_file,
        "seL4_Error seL4_TCB_SetIPCBuffer(seL4_TCB _service, seL4_Word buffer, seL4_CPtr bufferFrame);",
    )
    .unwrap();
    writeln!(
        wrapper_file,
        "seL4_Error seL4_TCB_SetFaultHandler(seL4_TCB _service, seL4_CPtr faultEP);",
    )
    .unwrap();
    writeln!(
        wrapper_file,
        "seL4_Uint32 seL4_DebugCapIdentify(seL4_CPtr cap);",
    )
    .unwrap();

    let builder = bindgen::Builder::default()
        .use_core()
        .ctypes_prefix("core::ffi")
        .header(wrapper.to_string_lossy())
        .clang_arg(format!("-I{}", build_dir.join("libsel4/include").display()))
        .clang_arg(format!(
            "-I{}",
            build_dir
                .join("libsel4/sel4_arch_include/aarch64")
                .display()
        ))
        .clang_arg(format!(
            "-I{}",
            build_dir.join("libsel4/arch_include/arm").display()
        ))
        .clang_arg(format!(
            "-I{}",
            build_dir.join("libsel4/autoconf").display()
        ))
        .clang_arg(format!(
            "-I{}",
            build_dir.join("libsel4/gen_config").display()
        ))
        .clang_arg(format!(
            "-I{}",
            build_dir.join("kernel/gen_config").display()
        ))
        .clang_arg(format!("-I{}", upstream_root.join("include").display()))
        .clang_arg(format!(
            "-I{}",
            upstream_root.join("sel4_arch_include/aarch64").display()
        ))
        .clang_arg(format!(
            "-I{}",
            upstream_root.join("arch_include/arm").display()
        ))
        .generate_inline_functions(false)
        .layout_tests(false)
        .size_t_is_usize(true)
        .allowlist_function("seL4_.*")
        .allowlist_type("seL4_.*")
        .allowlist_var("seL4_.*");

    let bindings = builder.generate().expect("unable to generate bindings");
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("write bindings");
}
