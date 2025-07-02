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

    if cfg!(feature = "cuda") {
        let cuda_home = env::var("CUDA_HOME").unwrap_or_else(|_| "/usr".into());
        println!("cargo:warning=Using CUDA from {}", cuda_home);

        if let Ok(p) = env::var("PATH") {
            env::set_var("PATH", format!("{}/bin:{}", cuda_home, p));
        } else {
            env::set_var("PATH", format!("{}/bin", cuda_home));
        }

        if let Ok(ld) = env::var("LD_LIBRARY_PATH") {
            env::set_var(
                "LD_LIBRARY_PATH",
                format!("{}/lib/aarch64-linux-gnu:{}", cuda_home, ld),
            );
        } else {
            env::set_var(
                "LD_LIBRARY_PATH",
                format!("{}/lib/aarch64-linux-gnu", cuda_home),
            );
        }

        if which::which("nvcc").is_ok() {
            let status = Command::new("nvcc")
                .args([
                    "-ptx",
                    "tests/gpu_demos/add.cu",
                    "-o",
                    "tests/gpu_demos/add.ptx",
                ])
                .status();
            match status {
                Ok(s) if s.success() => println!("cargo:info=PTX built"),
                _ => println!("cargo:info=nvcc failed, using prebuilt PTX"),
            }
        } else {
            println!("cargo:info=nvcc missing; using prebuilt PTX");
        }
    }
}
