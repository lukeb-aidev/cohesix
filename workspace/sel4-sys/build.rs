// CLASSIFICATION: COMMUNITY
// Filename: build.rs v1.46
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

    let mut header_dirs = Vec::new();

    if let Ok(arch) = env::var("SEL4_ARCH") {
        if let Ok(alias_root) = sel4_paths::create_arch_alias(&sel4_include, &arch, &out_dir) {
            header_dirs.push(alias_root);
        }
        let arch_dir = sel4_include
            .join("libsel4")
            .join("sel4_arch")
            .join("sel4")
            .join("sel4_arch")
            .join(&arch);
        if arch_dir.exists() {
            header_dirs.push(arch_dir);
        }
    }

    header_dirs.extend(
        sel4_paths::header_dirs_from_tree(&sel4_include)
            .expect("Failed to collect seL4 header directories"),
    );
    header_dirs.push(sel4_include.join("libsel4"));
    header_dirs.push(sel4_include.join("libsel4/sel4"));
    header_dirs.push(sel4_include.join("libsel4/sel4_arch"));

    let cflags = env::var("SEL4_SYS_CFLAGS").unwrap_or_default();

    let mut builder = bindgen::Builder::default()
        .header("include/wrapper.h")
        .use_core()
        .ctypes_prefix("cty")
        .clang_args(cflags.split_whitespace());

    for dir in &header_dirs {
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
