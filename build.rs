// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-02-16

fn main() {
    use std::process::Command;

    println!("cargo:rerun-if-changed=tests/gpu_demos/add.cu");
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
