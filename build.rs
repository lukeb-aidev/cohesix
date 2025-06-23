// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.4
// Author: Lukas Bower
// Date Modified: 2026-07-22

fn main() {
    use std::{env, process::Command};

    println!("cargo:rerun-if-changed=tests/gpu_demos/add.cu");
    env::set_var("CUDA_HOME", "/usr");
    if let Ok(p) = env::var("PATH") {
        env::set_var("PATH", format!("/usr/bin:{}", p));
    } else {
        env::set_var("PATH", "/usr/bin");
    }
    if let Ok(ld) = env::var("LD_LIBRARY_PATH") {
        env::set_var(
            "LD_LIBRARY_PATH",
            format!("/usr/lib/aarch64-linux-gnu:{}", ld),
        );
    } else {
        env::set_var("LD_LIBRARY_PATH", "/usr/lib/aarch64-linux-gnu");
    }
    let coh_gpu = std::env::var("COH_GPU").unwrap_or_default();

    if coh_gpu == "1" {
        if which::which("nvcc").is_ok() {
            let status = Command::new("nvcc")
                .args(["-ptx", "tests/gpu_demos/add.cu", "-o", "tests/gpu_demos/add.ptx"])
                .status();
            match status {
                Ok(s) if s.success() => println!("cargo:warning=PTX built"),
                _ => println!("cargo:warning=nvcc failed, using prebuilt PTX (COH_GPU=0)"),
            }
        } else {
            println!("cargo:warning=using prebuilt PTX (COH_GPU=0)");
        }
    } else {
        println!("cargo:warning=using prebuilt PTX (COH_GPU=0)");
    }
}
