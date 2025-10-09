// CLASSIFICATION: COMMUNITY
// Filename: build.rs v1.49
// Author: Lukas Bower
// Date Modified: 2029-10-08

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

fn emit_sel4_config(out_dir: &str, generated_dir: &Path) {
    let config_path = generated_dir.join("kernel/gen_config.h");
    let contents = fs::read_to_string(&config_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", config_path.display(), err));

    let mut max_untyped_caps: Option<usize> = None;
    let mut printing_enabled = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("#define CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS") {
            let value = rest.split_whitespace().next().unwrap_or_else(|| {
                panic!(
                    "missing value for CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS in {}",
                    config_path.display()
                )
            });
            max_untyped_caps = Some(value.parse().unwrap_or_else(|err| {
                panic!(
                    "invalid CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS `{}`: {}",
                    value, err
                )
            }));
            continue;
        }
        if trimmed == "#define CONFIG_PRINTING 1" {
            printing_enabled = true;
        }
    }

    let caps = max_untyped_caps.unwrap_or_else(|| {
        panic!(
            "CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS not found in {}",
            config_path.display()
        )
    });

    let out_path = Path::new(out_dir).join("sel4_config.rs");
    let mut file = fs::File::create(&out_path)
        .unwrap_or_else(|err| panic!("failed to create {}: {}", out_path.display(), err));
    writeln!(
        file,
        "// Auto-generated from {config}\npub const CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS: usize = {caps};\npub const CONFIG_PRINTING: bool = {printing};",
        config = config_path.display(),
        printing = printing_enabled
    )
    .expect("write sel4_config.rs");

    println!("cargo:rerun-if-changed={}", config_path.display());
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

fn sanitize_rel_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

fn embed_rootfs(out_dir: &str, manifest_dir: &str) {
    println!("cargo:rerun-if-env-changed=COHESIX_ROOTFS_DIR");
    let configured = env::var("COHESIX_ROOTFS_DIR").ok();
    let rootfs_path = configured
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(manifest_dir).join("../../out"));
    let rootfs_dir = match fs::canonicalize(&rootfs_path) {
        Ok(path) => path,
        Err(err) => {
            println!(
                "cargo:warning=Rootfs directory {:?} unavailable: {}",
                rootfs_path, err
            );
            return;
        }
    };
    if !rootfs_dir.is_dir() {
        println!(
            "cargo:warning=Rootfs directory {:?} missing or not a directory",
            rootfs_dir
        );
        return;
    }

    let dest_base = Path::new(out_dir).join("rootfs");
    if dest_base.exists() {
        fs::remove_dir_all(&dest_base).ok();
    }
    fs::create_dir_all(&dest_base).expect("create rootfs staging dir");

    #[derive(Clone)]
    struct Entry {
        rel: String,
        dest_rel: String,
    }

    fn collect(root: &Path, current: &Path, dest_base: &Path, entries: &mut Vec<Entry>) {
        let read_dir = match fs::read_dir(current) {
            Ok(dir) => dir,
            Err(err) => {
                println!(
                    "cargo:warning=Failed to read directory {:?}: {}",
                    current, err
                );
                return;
            }
        };
        for entry in read_dir {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    println!("cargo:warning=Dir entry error: {}", err);
                    continue;
                }
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(err) => {
                    println!("cargo:warning=Failed to stat {:?}: {}", path, err);
                    continue;
                }
            };
            if file_type.is_dir() {
                collect(root, &path, dest_base, entries);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let rel = match path.strip_prefix(root) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let rel_string = sanitize_rel_path(rel);
            if rel_string.is_empty() {
                continue;
            }
            if rel_string == "bin/cohesix_root.elf" {
                continue;
            }
            let dest_path = dest_base.join(&rel_string);
            if let Some(parent) = dest_path.parent() {
                if let Err(err) = fs::create_dir_all(parent) {
                    println!("cargo:warning=Failed creating {:?}: {}", parent, err);
                    continue;
                }
            }
            if let Err(err) = fs::copy(&path, &dest_path) {
                println!(
                    "cargo:warning=Failed to copy {:?} -> {:?}: {}",
                    path, dest_path, err
                );
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());
            entries.push(Entry {
                rel: format!("/{}", rel_string),
                dest_rel: rel_string,
            });
        }
    }

    let mut entries = Vec::new();
    collect(&rootfs_dir, &rootfs_dir, &dest_base, &mut entries);
    entries.sort_by(|a, b| a.rel.cmp(&b.rel));

    let out_file = Path::new(out_dir).join("rootfs_data.rs");
    let mut file = fs::File::create(&out_file).expect("create rootfs_data.rs");
    writeln!(
        file,
        "// Auto-generated rootfs entries from {}\npub static ROOT_FS: &[RootFsEntry] = &[",
        rootfs_dir.display()
    )
    .unwrap();
    for entry in &entries {
        let escaped_path = entry.rel.replace('\"', "\\\"");
        let escaped_dest = entry.dest_rel.replace('\"', "\\\"");
        writeln!(
            file,
            "    RootFsEntry {{ path: \"{}\", data: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/rootfs/{}\")), }},",
            escaped_path, escaped_dest
        )
        .unwrap();
    }
    writeln!(file, "];\n").unwrap();
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
        println!(
            "cargo:warning=sel4.ld not found under {}; relying on rootserver link.ld only",
            lib_dir.display()
        );
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
    let sel4_generated_dir = sel4_generated(&repo_root);
    emit_sel4_config(&out_dir, &sel4_generated_dir);
    embed_rootfs(&out_dir, &manifest_dir);

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
        sel4_generated_dir.display()
    );

    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("third_party/seL4/kernel.dts").display()
    );
}
