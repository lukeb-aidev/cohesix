// CLASSIFICATION: COMMUNITY
// Filename: build.rs v1.41
// Author: Lukas Bower
// Date Modified: 2028-09-10

use std::{env, path::PathBuf};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let cflags = env::var("SEL4_SYS_CFLAGS").unwrap_or_default();
    let clang_args: Vec<&str> = cflags.split_whitespace().collect();

    let bindings = bindgen::Builder::default()
        .header("include/wrapper.h")
        .clang_args(clang_args)
        .use_core()
        .ctypes_prefix("cty")
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
