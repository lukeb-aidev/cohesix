// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.5
// Author: Lukas Bower
// Date Modified: 2026-07-23

fn main() {
    use std::{env, process::Command};

    println!("cargo:rerun-if-changed=tests/gpu_demos/add.cu");

    let cuda_feature = env::var("CARGO_FEATURE_CUDA").is_ok();
    if !cuda_feature {
        println!("cargo:warning=cuda feature disabled; using prebuilt PTX");
        return;
    }

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
}
