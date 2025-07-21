// CLASSIFICATION: COMMUNITY
// Filename: build.rs v1.52
// Author: Lukas Bower
// Date Modified: 2028-11-12

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
    let sel4_root = sel4_include
        .to_str()
        .expect("SEL4_INCLUDE invalid utf8");
    println!("cargo:warning=SEL4_INCLUDE={}", sel4_root);
    let interfaces = format!("{}/libsel4/interfaces", sel4_root);
    let arch = format!("{}/libsel4/sel4_arch/sel4/sel4_arch/aarch64", sel4_root);

    if !sel4_include.exists() {
        panic!("SEL4_INCLUDE path {} not found", sel4_include.display());
    }

    let cflags = env::var("SEL4_SYS_CFLAGS").unwrap_or_default();

    let mut builder = bindgen::Builder::default()
        .header("include/wrapper.h")
        .use_core()
        .ctypes_prefix("cty")
        .clang_args(cflags.split_whitespace());
    // ensure architecture headers are first
    builder = builder.clang_arg(format!("-I{}/libsel4/sel4_arch/sel4/sel4_arch/aarch64", sel4_root));
    builder = builder.clang_arg(format!("-I{}", interfaces));
    println!("cargo:warning=INCLUDE DIR {}", interfaces);
    builder = builder.clang_arg(format!("-I{}", arch));
    println!("cargo:warning=INCLUDE DIR {}", arch);
    builder = builder.clang_arg(format!("-I{}/generated", sel4_root));
    println!("cargo:warning=INCLUDE DIR {}/generated", sel4_root);

    if let Ok(arch) = env::var("SEL4_ARCH") {
        if let Ok(alias_root) = sel4_paths::create_arch_alias(&sel4_include, &arch, &out_dir) {
            for dir in sel4_paths::header_dirs_recursive(&alias_root).unwrap() {
                println!("cargo:warning=INCLUDE DIR {}", dir.display());
                builder = builder.clang_arg(format!("-I{}", dir.display()));
            }
        }
    }

    for dir in sel4_paths::header_dirs_recursive(&sel4_include).unwrap() {
        println!("cargo:warning=INCLUDE DIR {}", dir.display());
        builder = builder.clang_arg(format!("-I{}", dir.display()));
    }

    for dir in sel4_paths::header_dirs_recursive(&sel4_paths::sel4_generated(&project_root)).unwrap() {
        println!("cargo:warning=INCLUDE GEN {}", dir.display());
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
