// CLASSIFICATION: COMMUNITY
// Filename: build.rs v1.45
// Author: Lukas Bower
// Date Modified: 2028-11-08

use std::{env, fs, path::Path};
#[path = "../sel4_paths.rs"]
mod sel4_paths;
use sel4_paths::{header_dirs_from_tree, project_root, sel4_generated};
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
                    if let Some(values) =
                        reg_line.split('<').nth(1).and_then(|p| p.split('>').next())
                    {
                        let parts: Vec<&str> = values.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let hi = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16)
                                .unwrap_or(0);
                            let lo = u64::from_str_radix(parts[1].trim_start_matches("0x"), 16)
                                .unwrap_or(0);
                            uart_base = (hi << 32) | lo;
                        }
                    }
                }
            }
        }
    }

    let mut file =
        fs::File::create(format!("{}/dt_generated.rs", out_dir)).expect("create dt_generated.rs");
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
    println!("cargo:rustc-link-lib=static=sel4");
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let lib_dir = std::env::var("SEL4_LIB_DIR").unwrap_or_else(|_| {
        project_root(&manifest_dir)
            .join("third_party/seL4/lib")
            .to_string_lossy()
            .into_owned()
    });
    println!("cargo:rustc-link-search=native={}", lib_dir);
    let header_dir = format!("{}/../../third_party/seL4/include", manifest_dir);
    if fs::metadata(&header_dir).is_err() {
        panic!("seL4 headers not found at {}", header_dir);
    }
    let out_dir = env::var("OUT_DIR").unwrap();
    generate_dtb_constants(&out_dir, &manifest_dir);
    embed_sel4_spec(&out_dir, &manifest_dir);
    embed_vectors(&out_dir, &manifest_dir);
    let project_root = project_root(&manifest_dir);

    let cflags = env::var("SEL4_SYS_CFLAGS").unwrap_or_else(|_| {
        let include_root = sel4_paths::sel4_include(&project_root);
        let mut dirs = Vec::new();
        if let Ok(arch) = env::var("SEL4_ARCH") {
            if let Ok(alias_root) = sel4_paths::create_arch_alias(&include_root, &arch, Path::new(&out_dir)) {
                dirs.push(alias_root);
            }
            let arch_dir = include_root
                .join("libsel4")
                .join("sel4_arch")
                .join("sel4")
                .join("sel4_arch")
                .join(&arch);
            if arch_dir.exists() {
                dirs.push(arch_dir);
            }
        }
        dirs.extend(header_dirs_from_tree(&include_root).expect("collect seL4 header dirs"));
        dirs.push(include_root.join("libsel4"));
        dirs.push(include_root.join("libsel4/sel4"));
        dirs.push(include_root.join("libsel4/sel4_arch"));
        let mut args = String::from("--target=aarch64-unknown-none");
        for d in &dirs {
            args.push_str(&format!(" -I{}", d.display()));
        }
        args
    });
    println!(
        "cargo:rustc-env=SEL4_GEN_HDR={}",
        sel4_generated(&project_root).display()
    );
    println!("cargo:rustc-env=SEL4_SYS_CFLAGS={}", cflags);

    println!(
        "cargo:rerun-if-changed={}",
        project_root.join("third_party/seL4/kernel.dts").display()
    );
}
