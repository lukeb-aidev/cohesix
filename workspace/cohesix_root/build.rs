// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

use std::{env, fs};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let header_dir = format!("{}/../../third_party/seL4/include", manifest_dir);
    if fs::metadata(&header_dir).is_err() {
        panic!("seL4 headers not found at {}", header_dir);
    }
    println!("cargo:rerun-if-changed=../../third_party/seL4/lib/libsel4.a");
    println!("cargo:rustc-link-search=../../third_party/seL4/lib");
    println!("cargo:rustc-link-lib=static=sel4");
}
