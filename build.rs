// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.6
// Author: Lukas Bower
// Date Modified: 2026-07-29

fn main() {
    use std::{env, process::Command};

    println!("cargo:rerun-if-changed=tests/gpu_demos/add.cu");

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
                .args(["-ptx", "tests/gpu_demos/add.cu", "-o", "tests/gpu_demos/add.ptx"])
                .status();
            match status {
                Ok(s) if s.success() => println!("cargo:warning=PTX built"),
                _ => println!("cargo:warning=nvcc failed, using prebuilt PTX"),
            }
        } else {
            println!("cargo:warning=nvcc missing; using prebuilt PTX");
        }
    } else {
        println!("cargo:warning=cuda feature disabled; using prebuilt PTX");
    }
}
