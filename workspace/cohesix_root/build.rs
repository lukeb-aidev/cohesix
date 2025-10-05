// CLASSIFICATION: COMMUNITY
// Filename: build.rs v1.48
// Author: Lukas Bower
// Date Modified: 2028-11-21

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
#[path = "../sel4_paths.rs"]
mod sel4_paths;
use sel4_paths::{project_root, sel4_generated};
use std::io::Write;

fn generate_dtb_constants(out_dir: &str, manifest_dir: &str) {
    let dts_path = format!("{}/../../third_party/seL4/kernel.dts", manifest_dir);
    let mut uart_base = 0x0900_0000u64;
    if let Ok(dts) = fs::read_to_string(&dts_path) {
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
    } else {
        println!(
            "cargo:warning=kernel.dts not found at {}; using default UART base 0x{:x}",
            dts_path, uart_base
        );
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

fn rust_sysroot() -> String {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(&rustc)
        .arg("--print")
        .arg("sysroot")
        .output()
        .expect("invoke rustc to determine sysroot");
    if !output.status.success() {
        panic!(
            "failed to compute rustc sysroot using `{rustc}`: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let sysroot = String::from_utf8(output.stdout)
        .expect("rustc sysroot output should be valid UTF-8")
        .trim()
        .to_owned();
    if sysroot.is_empty() {
        panic!("rustc reported an empty sysroot path");
    }
    sysroot
}

fn main() {
    println!("cargo:rustc-link-lib=static=sel4");
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let sysroot = rust_sysroot();
    println!("cargo:rustc-link-arg=--sysroot={}", sysroot);
    println!("cargo:rustc-env=COHESIX_RUST_SYSROOT={}", sysroot);
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!("cargo:rerun-if-env-changed=SEL4_LIB_DIR");
    let repo_root = project_root(&manifest_dir);
    let lib_dir = env::var("SEL4_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("third_party/seL4/lib"));
    let lib_dir = fs::canonicalize(&lib_dir)
        .expect("failed to resolve third_party/seL4/lib; run fetch_sel4.sh");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());

    let root_linker = fs::canonicalize(Path::new(&manifest_dir).join("link.ld"))
        .expect("missing link.ld in cohesix_root crate");
    println!("cargo:rustc-link-arg=-T{}", root_linker.display());
    println!("cargo:rerun-if-changed={}", root_linker.display());

    let sel4_linker_path = lib_dir.join("sel4.ld");
    if let Ok(sel4_linker) = fs::canonicalize(&sel4_linker_path) {
        println!("cargo:rustc-link-arg=-T{}", sel4_linker.display());
        println!("cargo:rerun-if-changed={}", sel4_linker.display());
    } else {
        println!("cargo:warning=sel4.ld not found under {}; relying on rootserver link.ld only", lib_dir.display());
    }

    let libsel4 = fs::canonicalize(lib_dir.join("libsel4.a"))
        .expect("missing libsel4.a under third_party/seL4/lib");
    println!("cargo:rustc-link-arg=--whole-archive");
    println!("cargo:rustc-link-arg={}", libsel4.display());
    println!("cargo:rustc-link-arg=--no-whole-archive");
    println!("cargo:rerun-if-changed={}", libsel4.display());
    let out_dir = env::var("OUT_DIR").unwrap();
    generate_dtb_constants(&out_dir, &manifest_dir);
    embed_sel4_spec(&out_dir, &manifest_dir);
    embed_vectors(&out_dir, &manifest_dir);

    let sel4 = env::var("SEL4_INCLUDE").unwrap_or_else(|_| {
        sel4_paths::sel4_include(&repo_root)
            .to_string_lossy()
            .into_owned()
    });
    let sel4_include = Path::new(&sel4);
    let mut flags = sel4_paths::default_cflags(sel4_include, &repo_root);
    if env::var("SEL4_SYS_CFLAGS").is_ok() {
        println!("cargo:warning=SEL4_SYS_CFLAGS no longer used");
    }
    if let Ok(extra) = env::var("CFLAGS") {
        if !extra.is_empty() {
            flags.push(extra);
        }
    }
    let cflags = flags.join(" ");
    println!("cargo:rustc-env=CFLAGS={}", cflags);
    println!(
        "cargo:rustc-env=SEL4_GEN_HDR={}",
        sel4_generated(&repo_root).display()
    );

    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("third_party/seL4/kernel.dts").display()
    );
}
