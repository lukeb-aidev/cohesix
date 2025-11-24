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

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let build_dir = env::var("SEL4_BUILD_DIR")
        .or_else(|_| env::var("SEL4_BUILD"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("../../seL4/build"));

    let config_sources = load_config_files(&build_dir);

    let max_bootinfo_untypeds = parse_config_usize(
        &config_sources,
        "CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS",
        &build_dir,
    )
    .unwrap_or_else(|message| panic!("{}", message));
    write_config_constants(&out_dir, max_bootinfo_untypeds)
        .unwrap_or_else(|error| panic!("{}", error));
    eprintln!(
        "sel4-sys: MAX_BOOTINFO_UNTYPEDS derived from config: {}",
        max_bootinfo_untypeds
    );

    if let Some(true) = probe_config_flag(&config_sources, "CONFIG_KERNEL_MCS") {
        println!("cargo:rustc-cfg=sel4_config_kernel_mcs");
    }

    generate_bindings(&build_dir, &config_sources);
}

fn load_config_files(root: &Path) -> Vec<(PathBuf, String)> {
    let mut sources = Vec::new();
    for relative in CONFIG_CANDIDATES {
        let candidate = root.join(relative);
        println!("cargo:rerun-if-changed={}", candidate.display());
        if let Ok(contents) = fs::read_to_string(&candidate) {
            sources.push((candidate, contents));
        }
    }

    sources
}

fn probe_config_flag(sources: &[(PathBuf, String)], flag: &str) -> Option<bool> {
    for (_path, contents) in sources {
        if let Some(value) = parse_config_flag(contents, flag) {
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

fn parse_config_value(sources: &[(PathBuf, String)], key: &str) -> Option<String> {
    for (_path, contents) in sources {
        if let Some(value) = parse_value_line(contents, key) {
            return Some(value);
        }
    }

    None
}

fn parse_value_line(contents: &str, key: &str) -> Option<String> {
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("/* disabled:") {
            continue;
        }

        if let Some(value) = parse_define_value(line, key) {
            return Some(value);
        }

        if let Some(value) = parse_assignment_value(line, key) {
            return Some(value);
        }

        if let Some(value) = parse_cmake_value(line, key) {
            return Some(value);
        }
    }

    None
}

fn parse_define_value(line: &str, key: &str) -> Option<String> {
    let line = line.strip_prefix("#define ")?;
    let mut parts = line.split_whitespace();
    let name = parts.next()?;
    if name != key {
        return None;
    }

    let value = parts.collect::<Vec<_>>().join(" ");
    Some(trim_config_value(&value))
}

fn parse_assignment_value(line: &str, key: &str) -> Option<String> {
    if !line.starts_with(key) {
        return None;
    }

    let mut parts = line.splitn(2, '=');
    let _name = parts.next()?;
    let value = parts.next()?.trim();
    Some(trim_config_value(value))
}

fn parse_cmake_value(line: &str, key: &str) -> Option<String> {
    let line = line.strip_prefix("set(")?.trim_end_matches(')');
    let mut parts = line.split_whitespace();
    let name = parts.next()?;
    if name != key {
        return None;
    }

    let value = parts.next()?;
    Some(trim_config_value(value))
}

fn trim_config_value(raw: &str) -> String {
    raw.trim_matches(&['"', '\''][..]).to_string()
}

fn parse_config_usize(
    sources: &[(PathBuf, String)],
    key: &str,
    build_dir: &Path,
) -> Result<usize, String> {
    if let Some(raw_value) = parse_config_value(sources, key) {
        raw_value.parse::<usize>().map_err(|error| {
            format!(
                "Unable to parse {} as integer (value: {}): {}",
                key, raw_value, error
            )
        })
    } else {
        Err(format!(
            "Missing {} in seL4 configuration; checked: {}",
            key,
            format_checked_paths(build_dir, sources)
        ))
    }
}

fn format_checked_paths(root: &Path, sources: &[(PathBuf, String)]) -> String {
    if sources.is_empty() {
        return format!(
            "none found under {}; ensure seL4 has been configured and built",
            root.display()
        );
    }

    sources
        .iter()
        .map(|(path, _)| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn write_config_constants(out_dir: &Path, max_bootinfo_untypeds: usize) -> Result<(), String> {
    let dest = out_dir.join("sel4_config_consts.rs");
    let contents = format!(
        "// Author: Lukas Bower\n// @generated by crates/sel4-sys/build.rs\npub const MAX_BOOTINFO_UNTYPEDS: usize = {};\n",
        max_bootinfo_untypeds
    );

    fs::write(&dest, contents)
        .map_err(|error| format!("Failed to write {}: {}", dest.display(), error))
}

fn resolve_platform(
    config_sources: &[(PathBuf, String)],
    upstream_root: &Path,
) -> Result<String, String> {
    if let Ok(value) = env::var("SEL4_PLATFORM") {
        return Ok(value);
    }

    if let Some(value) = parse_config_value(config_sources, "CONFIG_PLAT") {
        return Ok(value);
    }

    if let Some(value) = parse_config_value(config_sources, "CONFIG_ARM_PLAT") {
        return Ok(value);
    }

    if let Some(value) = parse_config_value(config_sources, "KernelPlatform") {
        return Ok(value);
    }

    if let Some(value) = parse_config_value(config_sources, "PLATFORM") {
        return Ok(value);
    }

    let plat_root = upstream_root.join("plat_include");
    let platforms: Vec<String> = fs::read_dir(&plat_root)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();

    for name in &platforms {
        let flag = format!(
            "CONFIG_PLAT_{}",
            name.replace('-', "_").to_ascii_uppercase()
        );
        if let Some(true) = probe_config_flag(config_sources, &flag) {
            return Ok(name.clone());
        }
    }

    if !platforms.is_empty() {
        if platforms.len() == 1 {
            let platform = platforms[0].clone();
            println!(
                "cargo:warning=Unable to derive seL4 platform from configuration; defaulting to vendored {}",
                platform
            );
            return Ok(platform);
        }

        return Err(format!(
            "Unable to determine seL4 platform; set SEL4_BUILD_DIR or SEL4_PLATFORM to one of: {}",
            platforms.join(", ")
        ));
    }

    Err(
        "No vendored platform headers found under upstream/libsel4/plat_include; unable to derive platform".to_string(),
    )
}

fn generate_bindings(build_dir: &Path, config_sources: &[(PathBuf, String)]) {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let upstream_root = manifest_dir.join("upstream/libsel4");

    let arch =
        parse_config_value(config_sources, "CONFIG_ARCH").unwrap_or_else(|| "arm".to_string());
    let sel4_arch = parse_config_value(config_sources, "CONFIG_SEL4_ARCH")
        .or_else(|| env::var("SEL4_ARCH").ok())
        .unwrap_or_else(|| "aarch64".to_string());
    let platform = resolve_platform(config_sources, &upstream_root).unwrap_or_else(|message| {
        panic!("{}", message);
    });
    let mode = parse_config_value(config_sources, "CONFIG_WORD_SIZE")
        .as_deref()
        .and_then(|value| value.parse::<u32>().ok())
        .map(|word_size| if word_size >= 64 { "64" } else { "32" })
        .unwrap_or_else(|| match sel4_arch.as_str() {
            "aarch64" | "x86_64" | "riscv64" => "64",
            _ => "32",
        })
        .to_string();

    let mode_include_dir = upstream_root.join(format!("mode_include/{}", mode));
    let mode_header = mode_include_dir.join("sel4/mode/types.h");
    if !mode_header.is_file() {
        panic!(
            "Could not locate libsel4 mode headers for current seL4 config (mode {}); expected {}",
            mode,
            mode_header.display()
        );
    }

    let plat_include_dir = upstream_root.join(format!("plat_include/{}", platform));
    let plat_header = plat_include_dir.join("sel4/plat/api/constants.h");
    if !plat_header.is_file() {
        panic!(
            "Could not locate libsel4 platform headers for current seL4 config (platform {}); expected {}",
            platform,
            plat_header.display()
        );
    }

    let mut include_dirs = vec![
        build_dir.join("libsel4/include"),
        build_dir.join(format!("libsel4/sel4_arch_include/{}", sel4_arch)),
        build_dir.join(format!("libsel4/arch_include/{}", arch)),
        build_dir.join("libsel4/autoconf"),
        build_dir.join("libsel4/gen_config"),
        build_dir.join("kernel/gen_config"),
    ];

    let build_mode_dir = build_dir.join(format!("libsel4/mode_include/{}", mode));
    if build_mode_dir.is_dir() {
        include_dirs.push(build_mode_dir);
    }

    let build_platform_dir = build_dir.join(format!("libsel4/sel4_plat_include/{}", platform));
    if build_platform_dir.is_dir() {
        include_dirs.push(build_platform_dir);
    }

    include_dirs.extend_from_slice(&[
        upstream_root.join("include"),
        upstream_root.join(format!("sel4_arch_include/{}", sel4_arch)),
        upstream_root.join(format!("arch_include/{}", arch)),
        mode_include_dir.clone(),
        plat_include_dir.clone(),
    ]);

    include_dirs.retain(|dir| dir.is_dir());

    let wrapper = out_dir.join("wrapper.h");
    let mut wrapper_file = fs::File::create(&wrapper).expect("create wrapper");
    writeln!(wrapper_file, "#include <sel4/sel4.h>").unwrap();
    writeln!(wrapper_file, "#include <sel4/syscalls.h>").unwrap();
    writeln!(wrapper_file, "#include <sel4/functions.h>").unwrap();
    writeln!(wrapper_file, "#include <interfaces/sel4_client.h>").unwrap();

    writeln!(wrapper_file, "#ifdef __cplusplus").unwrap();
    writeln!(wrapper_file, "extern \"C\" {{").unwrap();
    writeln!(wrapper_file, "#endif").unwrap();
    writeln!(
        wrapper_file,
        "seL4_Error seL4_CNode_Copy(seL4_CNode _service, seL4_Word dest_index, seL4_Word dest_depth, seL4_CNode src_root, seL4_Word src_index, seL4_Word src_depth, seL4_CapRights_t rights);",
    )
    .unwrap();
    writeln!(
        wrapper_file,
        "seL4_Error seL4_CNode_Mint(seL4_CNode _service, seL4_Word dest_index, seL4_Word dest_depth, seL4_CNode src_root, seL4_Word src_index, seL4_Word src_depth, seL4_CapRights_t rights, seL4_Word badge);",
    )
    .unwrap();
    writeln!(
        wrapper_file,
        "seL4_Error seL4_CNode_Move(seL4_CNode _service, seL4_Word dest_index, seL4_Word dest_depth, seL4_CNode src_root, seL4_Word src_index, seL4_Word src_depth);",
    )
    .unwrap();
    writeln!(
        wrapper_file,
        "seL4_Error seL4_CNode_Delete(seL4_CNode _service, seL4_Word index, seL4_Word depth);",
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
    writeln!(wrapper_file, "void seL4_DebugPutChar(char c);").unwrap();
    writeln!(
        wrapper_file,
        "seL4_Uint32 seL4_DebugCapIdentify(seL4_CPtr cap);",
    )
    .unwrap();
    writeln!(
        wrapper_file,
        "seL4_Error seL4_ARM_Page_Map(seL4_ARM_Page _service, seL4_CPtr vspace, seL4_Word vaddr, seL4_CapRights_t rights, seL4_ARM_VMAttributes attr);",
    )
    .unwrap();
    writeln!(
        wrapper_file,
        "seL4_Error seL4_ARM_PageTable_Map(seL4_ARM_PageTable _service, seL4_CPtr vspace, seL4_Word vaddr, seL4_ARM_VMAttributes attr);",
    )
    .unwrap();
    writeln!(wrapper_file, "#ifdef __cplusplus").unwrap();
    writeln!(wrapper_file, "}}").unwrap();
    writeln!(wrapper_file, "#endif").unwrap();

    let mut builder = bindgen::Builder::default()
        .use_core()
        .ctypes_prefix("core::ffi")
        .header(wrapper.to_string_lossy())
        .generate_inline_functions(true)
        .layout_tests(false)
        .size_t_is_usize(true)
        .allowlist_function("seL4_.*")
        .allowlist_type("seL4_.*|invocation_label|arch_invocation_label")
        .allowlist_var(
            "seL4_.*|CNode.*|UntypedRetype|ARMPageTableMap|ARMPageMap|InvalidInvocation",
        );

    for dir in include_dirs {
        builder = builder.clang_arg(format!("-I{}", dir.display()));
    }

    let bindings = builder.generate().expect("unable to generate bindings");
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("write bindings");
}
