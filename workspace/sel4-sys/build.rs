// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.7
// Author: Lukas Bower
// Date Modified: 2028-01-12

use std::{env, fs, path::{Path, PathBuf}};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let header = PathBuf::from(&manifest_dir).join("wrapper.h");
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

    // Determine this crate's manifest dir (workspace/sel4-sys)
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set");

    // Project root is two levels up: project_root/third_party/seL4/lib
    let project_root = Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("Unexpected manifest directory structure");

    // Verify workspace dir points to the same project root
    if let Ok(workspace) = env::var("CARGO_WORKSPACE_DIR") {
        let expected = Path::new(&workspace).join("third_party/seL4/lib/libsel4.a");
        if !expected.is_file() {
            panic!(
                "CARGO_WORKSPACE_DIR set to {} but libsel4.a not found",
                expected.display()
            );
        }
    }

    // Compose the seL4 lib directory path
    let sel4_lib_dir = project_root.join("third_party/seL4/lib");
    if !sel4_lib_dir.is_dir() {
        panic!(
            "seL4 lib directory not found at {}. \n\
             Please ensure you have fetched seL4 sources and built libsel4.a.",
            sel4_lib_dir.display()
        );
    }

    println!(
        "cargo:rustc-link-search=native={}",
        sel4_lib_dir.display()
    );
    println!("cargo:rustc-link-lib=static=sel4");
    println!("cargo:rerun-if-changed=wrapper.h");
}
