// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.5
// Author: Lukas Bower
// Date Modified: 2028-01-10

use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let header = PathBuf::from(&manifest_dir).join("wrapper.h");
    println!("cargo:rerun-if-changed={}", header.display());
    // Rust >=1.76 forbids setting built-in cfg `panic` via flags.
    // The target JSON and RUSTFLAGS already enforce panic abort.

    fs::create_dir_all(&out_path).unwrap();

    let bindings = bindgen::Builder::default()
        .clang_arg(format!("-I{}", out_path.display()))
        .header(header.to_string_lossy())
        .use_core()
        .ctypes_prefix("cty")
        .allowlist_function("seL4_.*")
        .allowlist_type("seL4_.*")
        .allowlist_var("seL4_.*")
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    let workspace_root = env::current_dir()
        .ok()
        .and_then(|d| d.parent().map(|p| p.to_path_buf()))
        .or_else(|| {
            env::var("CARGO_MANIFEST_DIR").ok().and_then(|m| {
                PathBuf::from(m)
                    .parent()
                    .map(|p| p.to_path_buf())
            })
        })
        .expect("workspace root discovery failed");

    let sel4_lib_dir = workspace_root
        .join("third_party")
        .join("seL4")
        .join("lib")
        .canonicalize()
        .expect("canonicalize seL4 lib directory");

    println!(
        "cargo:rustc-link-search=native={}",
        sel4_lib_dir.display()
    );
    println!("cargo:rustc-link-lib=static=sel4");
}
