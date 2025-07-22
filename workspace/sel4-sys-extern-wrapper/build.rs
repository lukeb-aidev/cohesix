// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.4
// Author: Lukas Bower
// Date Modified: 2025-07-22

use std::{env, fs, path::{Path, PathBuf}};
#[path = "../sel4_paths.rs"]
mod sel4_paths;

fn generate_wrapper_header(out_dir: &Path) -> PathBuf {
    let header_path = out_dir.join("sel4_wrapper.h");
    let mut content = String::new();
    content.push_str("// CLASSIFICATION: COMMUNITY\n");
    content.push_str("// Filename: sel4_wrapper.h v0.2\n");
    content.push_str("// Author: Lukas Bower\n");
    content.push_str("// Date Modified: 2025-07-22\n\n");
    content.push_str("#pragma once\n");
    content.push_str("#include <sel4/config.h>\n");
    content.push_str("#include <sel4/sel4_arch/constants.h>\n\n");
    content.push_str("typedef unsigned long seL4_ARM_VMAttributes;\n");
    content.push_str("typedef unsigned long seL4_ARM_Page;\n");
    content.push_str("typedef unsigned long seL4_ARM_PageTable;\n");
    content.push_str("typedef unsigned long seL4_ARM_VSpace;\n");
    content.push_str("typedef unsigned long seL4_ARM_ASIDControl;\n");
    content.push_str("typedef unsigned long seL4_ARM_ASIDPool;\n");
    content.push_str("typedef unsigned long seL4_ARM_VCPU;\n");
    content.push_str("typedef unsigned long seL4_ARM_IOSpace;\n");
    content.push_str("typedef unsigned long seL4_ARM_IOPageTable;\n");
    content.push_str("typedef unsigned long seL4_ARM_SMC;\n");
    content.push_str("typedef unsigned long seL4_ARM_SIDControl;\n");
    content.push_str("typedef unsigned long seL4_ARM_SID;\n");
    content.push_str("typedef unsigned long seL4_ARM_CBControl;\n");
    content.push_str("typedef unsigned long seL4_ARM_CB;\n\n");
    content.push_str("#include <sel4/sel4.h>\n");
    fs::write(&header_path, content).expect("write sel4_wrapper.h");
    header_path
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root = sel4_paths::project_root(&manifest_dir);
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let sel4_include = env::var("SEL4_INCLUDE")
        .expect("SEL4_INCLUDE environment variable not set");
    let _lib_dir = env::var("SEL4_LIB_DIR")
        .expect("SEL4_LIB_DIR environment variable not set");

    let wrapper_header = generate_wrapper_header(&out_dir);

    let mut builder = bindgen::Builder::default()
        .header(wrapper_header.to_str().unwrap())
        .use_core()
        .clang_arg(format!("-I{}", out_dir.display()))
        .ctypes_prefix("cty");

    let include_path = PathBuf::from(&sel4_include);
    builder = builder.clang_arg(format!("-I{}", include_path.display()));
    builder = builder.clang_arg(format!("-I{}/libsel4", include_path.display()));
    builder = builder.clang_arg(format!("-I{}/libsel4/sel4", include_path.display()));
    builder = builder.clang_arg(format!("-I{}/libsel4/interfaces", include_path.display()));
    builder = builder.clang_arg(format!("-I{}/generated", include_path.display()));
    if let Ok(arch) = env::var("SEL4_ARCH") {
        let alias_root = sel4_paths::create_arch_alias(&include_path, &arch, &out_dir)
            .expect("create arch alias");
        builder = builder.clang_arg(format!("-I{}", alias_root.display()));
    }
    println!("cargo:rerun-if-env-changed=SEL4_ARCH");

    if env::var("SEL4_SYS_CFLAGS").is_ok() {
        println!("cargo:warning=SEL4_SYS_CFLAGS ignored (sel4-sys removed)");
    }

    let bindings = builder.generate().expect("Unable to generate bindings");
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rustc-link-search=native={}/third_party/seL4/lib",
        project_root.display()
    );
    println!("cargo:rustc-link-lib=static=sel4");
}
