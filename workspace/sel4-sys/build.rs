// CLASSIFICATION: COMMUNITY
// Filename: build.rs v1.39
// Author: Lukas Bower
// Date Modified: 2028-09-07

use std::{env, fs, path::{Path, PathBuf}};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let header = PathBuf::from(&manifest_dir).join("wrapper.h");
    // Rust >=1.76 forbids setting built-in cfg `panic` via flags.
    // The target JSON and RUSTFLAGS already enforce panic abort.

    fs::create_dir_all(&out_path).unwrap();

    // Provide minimal autoconf.h so the seL4 headers compile without the
    // full kernel build system. The real seL4 build generates this file with
    // numerous configuration options. For binding generation we only need a
    // handful of definitions, so create a lightweight version in OUT_DIR.
    let autoconf_path = out_path.join("autoconf.h");
    fs::write(
        &autoconf_path,
        "#pragma once\n\
         #define CONFIG_KERNEL_MCS 0\n\
         #define CONFIG_BENCHMARK_TRACEPOINTS 0\n\
         #define CONFIG_BENCHMARK_TRACK_UTILISATION 0\n\
         #define CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS 256\n\
         #define CONFIG_NUM_PRIORITIES 256\n",
    )
    .expect("write autoconf.h");

    // Provide minimal arch/simple_types.h for 64-bit builds
    let arch_dir = out_path.join("sel4/arch");
    fs::create_dir_all(&arch_dir).unwrap();
    let sel4_arch_dir = out_path.join("sel4/sel4_arch");
    fs::create_dir_all(&sel4_arch_dir).unwrap();
    fs::write(
        arch_dir.join("simple_types.h"),
        "#pragma once\n\
         #define SEL4_INT64_IS_LONG_LONG 1\n\
         #define SEL4_WORD_IS_UINT64 1\n",
    )
    .expect("write simple_types.h");

    fs::write(
        arch_dir.join("types.h"),
        "#pragma once\n\
         typedef unsigned long long seL4_Word;\n\
         typedef seL4_Word seL4_CPtr;\n\
         typedef seL4_Word seL4_PAddr;\n\
         typedef seL4_Word seL4_NodeId;\n\
         typedef seL4_Word seL4_Domain;\n",
    )
    .expect("write types.h");

    fs::write(
        arch_dir.join("objecttype.h"),
        "#pragma once\ntypedef unsigned long long arch_object_type_t;\n",
    )
    .expect("write objecttype.h");

    fs::write(
        arch_dir.join("syscalls.h"),
        "#pragma once\n",
    )
    .expect("write arch syscalls.h");

    let sel4_arch_dir = out_path.join("sel4/sel4_arch");
    fs::create_dir_all(&sel4_arch_dir).unwrap();
    fs::write(
        sel4_arch_dir.join("objecttype.h"),
        "#pragma once\ntypedef unsigned long long sel4_arch_object_type_t;\n",
    )
    .expect("write sel4_arch objecttype.h");

    fs::write(
        sel4_arch_dir.join("syscalls.h"),
        "#pragma once\n",
    )
    .expect("write sel4_arch syscalls.h");

    let mode_dir = out_path.join("sel4/mode");
    fs::create_dir_all(&mode_dir).unwrap();
    fs::write(
        mode_dir.join("types.h"),
        "#pragma once\n",
    )
    .expect("write mode types.h");

    fs::write(
        arch_dir.join("constants.h"),
        "#pragma once\n\
         #define seL4_WordSizeBits 3\n\
         #define seL4_WordBits 64\n",
    )
    .expect("write constants.h");

    let sel4_include_root = Path::new(&manifest_dir)
        .join("../../third_party/seL4/include");
    let sel4_include = sel4_include_root.join("libsel4");
    let include_dirs = [
        sel4_include_root.clone(),
        sel4_include.clone(),
        sel4_include.join("sel4"),
        sel4_include.join("sel4_arch"),
        sel4_include.join("sel4_arch/sel4"),
    ];

    // Export CFLAGS for dependents such as cohesix_root
    let cflags = format!(
        "--target=aarch64-unknown-none -I{} -I{} -I{} -I{} -I{}",
        sel4_include_root.display(),
        sel4_include.display(),
        sel4_include.join("sel4").display(),
        sel4_include.join("sel4_arch").display(),
        sel4_include.join("sel4_arch/sel4").display()
    );
    println!("cargo:rustc-env=SEL4_SYS_CFLAGS={}", cflags);

    let mut builder = bindgen::Builder::default();
    builder = builder
        .clang_arg("--target=aarch64-unknown-none")
        .clang_arg("-DSEL4_INT64_IS_LONG_LONG")
        .clang_arg("-DSEL4_WORD_IS_UINT64")
        .clang_arg(format!("-I{}", out_path.display()))
        .header(header.to_string_lossy())
        .use_core()
        .ctypes_prefix("cty");
    for dir in &include_dirs {
        builder = builder.clang_arg(format!("-I{}", dir.display()));
    }
    let bindings = builder
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
