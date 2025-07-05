// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.8
// Author: Lukas Bower
// Date Modified: 2026-08-21

fn main() {
    use std::{env, path::Path, process::Command};

    println!("cargo:rerun-if-changed=tests/gpu_demos/add.cu");

    let home = env::var("HOME").unwrap_or_default();
    let target = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    let sel4_kernel = match target.as_str() {
        "aarch64" => Path::new(&home).join("sel4_workspace/build_qemu_arm/kernel/kernel.elf"),
        "x86_64" => Path::new(&home).join("sel4_workspace/build_pc99/kernel/kernel.elf"),
        _ => return,
    };

    if !sel4_kernel.exists() && env::var_os("SKIP_SEL4_KERNEL_CHECK").is_none() {
        eprintln!("sel4 kernel.elf not found at {}", sel4_kernel.display());
        eprintln!("Try running `ninja` in ~/sel4_workspace/");
    }

    let _ = Command::new("true");
}
