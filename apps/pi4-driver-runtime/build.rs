// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Mirror seL4 kernel configuration flags for linked Pi 4 runtimes.
// Author: Lukas Bower

use std::env;
use std::fs;
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
    "libsel4/sel4_arch_include/sel4/config.h",
    "libsel4/gen_config/sel4/config.h",
    "libsel4/gen_config/sel4/gen_config.h",
    "libsel4/include/sel4/gen_config.h",
    "libsel4/include/sel4/config.h",
];

fn main() {
    println!("cargo:rustc-check-cfg=cfg(sel4_config_kernel_mcs)");
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
        if line == format!("{key}=y")
            || line == format!("{key}=ON")
            || line == format!("set({key} ON)")
            || line == format!("#define {key} 1")
            || line == format!("#define {key} true")
        {
            return Some(true);
        }
        if line == format!("{key}=n")
            || line == format!("{key}=OFF")
            || line == format!("set({key} OFF)")
            || line == format!("#define {key} 0")
            || line == format!("#define {key} false")
        {
            return Some(false);
        }
    }
    None
}
