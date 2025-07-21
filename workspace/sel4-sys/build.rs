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

    let mut builder = bindgen::Builder::default()
        .header("include/wrapper.h")
        .use_core()
        .ctypes_prefix("cty")
        .clang_arg("-Iinclude")
        .clang_args(cflags.split_whitespace());

    if let Ok(arch) = env::var("SEL4_ARCH") {
        if let Ok(alias_root) = sel4_paths::create_arch_alias(&sel4_include, &arch, &out_dir) {
            builder = builder.clang_arg(format!("-I{}", alias_root.display()));
            for dir in sel4_paths::get_all_subdirectories(&alias_root).unwrap() {
                builder = builder.clang_arg(format!("-I{}", dir.display()));
            }
        }
    }

    let header_dirs = sel4_paths::header_dirs_from_tree(&sel4_include)
        .expect("parse sel4_tree.txt");
    for dir in header_dirs {
        builder = builder.clang_arg(format!("-I{}", dir.display()));
    }

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
