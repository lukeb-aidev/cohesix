// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.2
// Author: Lukas Bower
// Date Modified: 2027-12-31

use std::{env, fs, path::Path};
use std::io::Write;

fn generate_dtb_constants(out_dir: &str, manifest_dir: &str) {
    let dts_path = format!("{}/../../third_party/seL4/kernel.dts", manifest_dir);
    let dts = fs::read_to_string(&dts_path).expect("read kernel.dts");

    let mut uart_base = 0x0900_0000u64;
    if let Some(stdout_line) = dts.lines().find(|l| l.contains("stdout-path")) {
        if let Some(path) = stdout_line.split('"').nth(1) {
            let node = path.trim_start_matches('/');
            if let Some(idx) = dts.find(&format!("{} {{", node)) {
                if let Some(reg_line) = dts[idx..].lines().find(|l| l.contains("reg =")) {
                    if let Some(values) = reg_line.split('<').nth(1).and_then(|p| p.split('>').next()) {
                        let parts: Vec<&str> = values.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let hi = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16).unwrap_or(0);
                            let lo = u64::from_str_radix(parts[1].trim_start_matches("0x"), 16).unwrap_or(0);
                            uart_base = (hi << 32) | lo;
                        }
                    }
                }
            }
        }
    }

    let mut file = fs::File::create(format!("{}/dt_generated.rs", out_dir)).expect("create dt_generated.rs");
    writeln!(file, "pub const UART_BASE: usize = 0x{uart_base:x};").unwrap();
}

fn embed_sel4_spec(out_dir: &str, manifest_dir: &str) {
    let spec_path = format!("{}/sel4-aarch64.json", manifest_dir);
    let dest = format!("{}/sel4_spec.json", out_dir);
    fs::copy(&spec_path, &dest).expect("copy sel4 spec");
    println!("cargo:rerun-if-changed={spec_path}");
}

fn embed_vectors(out_dir: &str, manifest_dir: &str) {
    let vec_path = format!("{}/mmu_vectors.bin", manifest_dir);
    if Path::new(&vec_path).exists() {
        let dest = format!("{}/mmu_vectors.bin", out_dir);
        fs::copy(&vec_path, &dest).expect("copy mmu vectors");
        println!("cargo:rerun-if-changed={vec_path}");
    }
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let header_dir = format!("{}/../../third_party/seL4/include", manifest_dir);
    if fs::metadata(&header_dir).is_err() {
        panic!("seL4 headers not found at {}", header_dir);
    }
    let out_dir = env::var("OUT_DIR").unwrap();
    generate_dtb_constants(&out_dir, &manifest_dir);
    embed_sel4_spec(&out_dir, &manifest_dir);
    embed_vectors(&out_dir, &manifest_dir);
    println!("cargo:rerun-if-changed=../../third_party/seL4/lib/libsel4.a");
    println!("cargo:rerun-if-changed=../../third_party/seL4/kernel.dts");
    println!("cargo:rustc-link-search=../../third_party/seL4/lib");
    println!("cargo:rustc-link-lib=static=sel4");
}
