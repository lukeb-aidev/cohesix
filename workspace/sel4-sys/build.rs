// CLASSIFICATION: COMMUNITY
// Filename: build.rs v1.42
// Author: Lukas Bower
// Date Modified: 2028-11-05

use std::{env, path::PathBuf};
#[path = "../sel4_paths.rs"]
mod sel4_paths;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root = sel4_paths::project_root(&manifest_dir);

    let sel4_include = env::var("SEL4_INCLUDE").unwrap_or_else(|_| {
        sel4_paths::sel4_include(&project_root)
            .to_string_lossy()
            .into_owned()
    });
    let sel4_arch = env::var("SEL4_ARCH").ok();

    let mut include_dirs = vec![
        format!("{}/libsel4/interfaces", sel4_include),
        format!("{}/libsel4/sel4", sel4_include),
        format!("{}/kernel/api", sel4_include),
        format!("{}/kernel/arch/api", sel4_include),
        format!("{}/generated", sel4_include),
    ];
    if let Some(ref arch) = sel4_arch {
        include_dirs.push(format!(
            "{}/libsel4/sel4_arch/sel4/sel4_arch/{}",
            sel4_include, arch
        ));
    }

    let cflags = env::var("SEL4_SYS_CFLAGS").unwrap_or_default();

    let mut builder = bindgen::Builder::default()
        .header("include/wrapper.h")
        .use_core()
        .ctypes_prefix("cty")
        .clang_args(cflags.split_whitespace());

    for dir in include_dirs {
        builder = builder.clang_arg(format!("-I{}", dir));
    }

    let bindings = builder
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    println!("cargo:rerun-if-changed=include/wrapper.h");
    println!("cargo:rustc-link-lib=static=sel4");
    println!(
        "cargo:rustc-link-search=native={}",
        env::var("SEL4_LIB_DIR").unwrap()
    );
}
