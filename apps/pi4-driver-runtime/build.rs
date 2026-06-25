// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Mirror seL4 kernel configuration flags for linked Pi 4 runtimes.
// Author: Lukas Bower

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_CANDIDATES: &[&str] = &[
    "CMakeCache.txt",
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
    "libsel4/sel4_arch_include/sel4/config.h",
    "libsel4/gen_config/sel4/config.h",
    "libsel4/gen_config/sel4/gen_config.h",
    "libsel4/include/sel4/gen_config.h",
    "libsel4/include/sel4/config.h",
];

const TIMER_PLATFORM_HEADER_CANDIDATES: &[&str] = &[
    "kernel/gen_headers/plat/platform_gen.h",
    "gen_headers/plat/platform_gen.h",
];

fn main() {
    println!("cargo:rustc-check-cfg=cfg(sel4_config_kernel_mcs)");
    println!("cargo:rustc-check-cfg=cfg(sel4_config_export_vcnt_user)");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD_DIR");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let build_dir = env::var("SEL4_BUILD_DIR")
        .or_else(|_| env::var("SEL4_BUILD"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("../../seL4/build"));

    if probe_config_flag(&build_dir, "CONFIG_KERNEL_MCS") == Some(true) {
        println!("cargo:rustc-cfg=sel4_config_kernel_mcs");
    }
    validate_arch_counter_config(&build_dir);
}

fn probe_config_flag(build_dir: &Path, key: &str) -> Option<bool> {
    CONFIG_CANDIDATES.iter().find_map(|candidate| {
        let path = build_dir.join(candidate);
        let contents = fs::read_to_string(&path).ok()?;
        println!("cargo:rerun-if-changed={}", path.display());
        probe_config_contents(&contents, key)
    })
}

fn probe_config_contents(contents: &str, key: &str) -> Option<bool> {
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.starts_with('#') && line.contains(&format!("{key} is not set")) {
            return Some(false);
        }
        if let Some(value) = config_value_for_key(line, key) {
            return parse_config_bool(value);
        }
    }
    None
}

fn config_value_for_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    if let Some(rest) = line.strip_prefix("#define") {
        let mut fields = rest.split_whitespace();
        let name = fields.next()?;
        return (name == key).then(|| fields.next()).flatten();
    }
    if line.starts_with("set(") && line.ends_with(')') {
        let body = &line[4..line.len().saturating_sub(1)];
        let mut fields = body.split_whitespace();
        let name = fields.next()?;
        return (name == key).then(|| fields.next()).flatten();
    }
    let (name, value) = line.split_once('=')?;
    let name = name.split_once(':').map_or(name, |(name, _)| name);
    (name.trim() == key).then_some(value.trim())
}

fn parse_config_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "1" | "ON" | "on" | "TRUE" | "True" | "true" | "yes" | "YES" | "y" | "Y" => Some(true),
        "0" | "OFF" | "off" | "FALSE" | "False" | "false" | "no" | "NO" | "n" | "N" => Some(false),
        _ => None,
    }
}

fn validate_arch_counter_config(build_dir: &Path) {
    let timer_clock_hz = find_timer_clock_hz(build_dir).unwrap_or_else(|| {
        panic!(
            "linked Pi 4 runtimes require TIMER_CLOCK_HZ from the selected seL4 build; \
             set SEL4_BUILD_DIR to the Pi 4 U-Boot build"
        )
    });
    if timer_clock_hz == 0 {
        panic!("linked Pi 4 runtimes require nonzero TIMER_CLOCK_HZ");
    }
    println!("cargo:rustc-env=PI4_DRIVER_RUNTIME_TIMER_CLOCK_HZ={timer_clock_hz}");

    let vcnt_user = probe_any_config_flag(
        build_dir,
        &["KernelArmExportVCNTUser", "CONFIG_EXPORT_VCNT_USER"],
    );
    if vcnt_user == Some(true) {
        println!("cargo:rustc-cfg=sel4_config_export_vcnt_user");
        println!("cargo:rustc-env=PI4_DRIVER_RUNTIME_TIMER_COUNTER_KIND=virtual");
    } else {
        println!("cargo:rustc-env=PI4_DRIVER_RUNTIME_TIMER_COUNTER_KIND=iterations");
    }

    let pi4_platform = selected_pi4_platform(build_dir);
    if pi4_platform && vcnt_user != Some(true) {
        panic!(
            "linked Pi 4 runtimes require the selected seL4 build to expose \
             CNTVCT_EL0/CNTFRQ_EL0 to EL0 (KernelArmExportVCNTUser=ON or \
             CONFIG_EXPORT_VCNT_USER=y)"
        );
    }

    if pi4_platform {
        for flag in [
            ("KernelArmExportPCNTUser", "CONFIG_EXPORT_PCNT_USER"),
            ("KernelArmExportPTMRUser", "CONFIG_EXPORT_PTMR_USER"),
            ("KernelArmExportVTMRUser", "CONFIG_EXPORT_VTMR_USER"),
        ] {
            if probe_any_config_flag(build_dir, &[flag.0, flag.1]) == Some(true) {
                panic!(
                    "linked Pi 4 runtimes expect only the read-only virtual counter; \
                     disable {} / {}",
                    flag.0, flag.1
                );
            }
        }
    }
}

fn selected_pi4_platform(build_dir: &Path) -> bool {
    probe_any_config_flag(build_dir, &["CONFIG_PLAT_BCM2711", "KernelPlatformRpi4"]) == Some(true)
}

fn find_timer_clock_hz(build_dir: &Path) -> Option<u64> {
    TIMER_PLATFORM_HEADER_CANDIDATES
        .iter()
        .find_map(|candidate| {
            let path = build_dir.join(candidate);
            println!("cargo:rerun-if-changed={}", path.display());
            let contents = fs::read_to_string(path).ok()?;
            parse_timer_clock_hz(&contents)
        })
}

fn parse_timer_clock_hz(contents: &str) -> Option<u64> {
    contents.lines().find_map(|raw_line| {
        let line = raw_line.trim();
        let value = line.strip_prefix("#define TIMER_CLOCK_HZ")?.trim();
        parse_first_decimal_u64(value)
    })
}

fn parse_first_decimal_u64(value: &str) -> Option<u64> {
    let mut digits = String::new();
    let mut started = false;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            started = true;
            digits.push(ch);
            continue;
        }
        if started {
            break;
        }
    }
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn probe_any_config_flag(build_dir: &Path, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| probe_config_flag(build_dir, key))
}

#[cfg(test)]
mod tests {
    use super::probe_config_contents;

    #[test]
    fn config_probe_accepts_cmake_cache_bool() {
        assert_eq!(
            probe_config_contents(
                "KernelArmExportVCNTUser:BOOL=ON\n",
                "KernelArmExportVCNTUser"
            ),
            Some(true)
        );
        assert_eq!(
            probe_config_contents(
                "KernelArmExportPCNTUser:BOOL=OFF\n",
                "KernelArmExportPCNTUser"
            ),
            Some(false)
        );
    }

    #[test]
    fn config_probe_accepts_generated_define_spacing() {
        assert_eq!(
            probe_config_contents(
                "#define CONFIG_EXPORT_VCNT_USER  1\n",
                "CONFIG_EXPORT_VCNT_USER"
            ),
            Some(true)
        );
        assert_eq!(
            probe_config_contents(
                "#define CONFIG_EXPORT_PCNT_USER 0\n",
                "CONFIG_EXPORT_PCNT_USER"
            ),
            Some(false)
        );
    }
}
