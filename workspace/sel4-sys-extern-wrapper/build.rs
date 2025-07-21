// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.1
// Author: Lukas Bower
// Date Modified: 2028-11-21

use std::{env, path::PathBuf};
#[path = "../sel4_paths.rs"]
mod sel4_paths;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root = sel4_paths::project_root(&manifest_dir);
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let sel4_include = env::var("SEL4_INCLUDE").unwrap_or_else(|_| {
        sel4_paths::sel4_include(&project_root)
            .to_string_lossy()
            .into_owned()
    });

    let mut builder = bindgen::Builder::default()
        .header("include/sel4_wrapper.h")
        .use_core()
        .ctypes_prefix("cty");

    let include_path = PathBuf::from(&sel4_include);
    for flag in sel4_paths::default_cflags(&include_path, &project_root) {
        builder = builder.clang_arg(flag);
    }
    if let Ok(arch) = env::var("SEL4_ARCH") {
        if let Ok(alias_root) = sel4_paths::create_arch_alias(&include_path, &arch, &out_dir) {
            if let Ok(alias_dirs) = sel4_paths::header_dirs_recursive(&alias_root) {
                for dir in alias_dirs {
                    builder = builder.clang_arg(format!("-I{}", dir.display()));
                }
            }
        }
    }

    if let Ok(extra) = env::var("SEL4_SYS_CFLAGS") {
        for arg in extra.split_whitespace() {
            builder = builder.clang_arg(arg);
        }
    }

    let bindings = builder.generate().expect("Unable to generate bindings");
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    println!("cargo:rerun-if-changed=include/sel4_wrapper.h");
    println!("cargo:rustc-link-lib=static=sel4");
    let lib_dir = env::var("SEL4_LIB_DIR").unwrap_or_else(|_| {
        project_root
            .join("third_party/seL4/lib")
            .to_string_lossy()
            .into_owned()
    });
    println!("cargo:rustc-link-search=native={}", lib_dir);
}
