// CLASSIFICATION: COMMUNITY
// Filename: build.rs v1.48
// Author: Lukas Bower
// Date Modified: 2028-11-08

use std::{env, path::PathBuf};
#[path = "../sel4_paths.rs"]
mod sel4_paths;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root = sel4_paths::project_root(&manifest_dir);

    let sel4_include = env::var("SEL4_INCLUDE")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| sel4_paths::sel4_include(&project_root));

    if !sel4_include.exists() {
        panic!("SEL4_INCLUDE path {} not found", sel4_include.display());
    }

    let cflags = env::var("SEL4_SYS_CFLAGS").unwrap_or_default();
    let sel4 = std::env::var("SEL4_INCLUDE").unwrap();

    let mut builder = bindgen::Builder::default()
        .header("include/wrapper.h")
        .use_core()
        .ctypes_prefix("cty")
        .clang_arg(format!("-I{}", sel4))
        .clang_arg(format!("-I{}/generated", sel4))
        .clang_arg("-Iinclude")
        .clang_args(cflags.split_whitespace());

    let bindings = builder.generate().expect("Unable to generate bindings");

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    println!("cargo:rerun-if-changed=include/wrapper.h");
    println!("cargo:rustc-link-lib=static=sel4");
    let lib_dir = env::var("SEL4_LIB_DIR").unwrap_or_else(|_| {
        project_root
            .join("third_party/seL4/lib")
            .to_string_lossy()
            .into_owned()
    });
    println!("cargo:rustc-link-search=native={}", lib_dir);
}
