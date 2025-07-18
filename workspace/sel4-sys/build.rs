// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.4
// Author: Lukas Bower
// Date Modified: 2027-12-31

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

    let workspace_dir = std::env::var("CARGO_WORKSPACE_DIR")
        .or_else(|_| {
            std::env::var("CARGO_MANIFEST_DIR").map(|m| {
                std::path::Path::new(&m)
                    .parent()
                    .expect("CARGO_MANIFEST_DIR has no parent")
                    .to_string_lossy()
                    .into_owned()
            })
        })
        .expect("CARGO_WORKSPACE_DIR or CARGO_MANIFEST_DIR must be set");
    println!(
        "cargo:rustc-link-search=native={}/third_party/seL4/lib",
        workspace_dir
    );
    println!("cargo:rustc-link-lib=static=sel4");
}
