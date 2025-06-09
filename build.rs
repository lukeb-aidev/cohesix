// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-20

fn main() {
    println!("cargo:rerun-if-changed=resources/add.cu");
    if which::which("nvcc").is_ok() {
        let status = std::process::Command::new("nvcc")
            .args(["-ptx", "resources/add.cu", "-o", "resources/add.ptx"])
            .status();
        match status {
            Ok(s) if s.success() => println!("cargo:warning=PTX built"),
            _ => println!("cargo:warning=nvcc failed, using prebuilt PTX"),
        }
    } else {
        println!("cargo:warning=nvcc not found, using prebuilt PTX");
    }
}
