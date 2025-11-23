// Author: Lukas Bower
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const CONFIG_CANDIDATES: &[&str] = &[
    ".config",
    "kernel/.config",
    "KernelConfig",
    "kernel/KernelConfig",
    "kernel/gen_config/KernelConfig",
    "kernel/gen_config/kernel/gen_config.h",
    "kernel/gen_config/kernel/KernelConfig",
    "kernel/gen_config/KernelConfigGenerated.cmake",
    "kernel/gen_config/kernel/KernelConfigGenerated.cmake",
    "kernel/gen_config/kernel_all.cmake",
];

fn main() {
    println!("cargo:rustc-check-cfg=cfg(sel4_config_kernel_mcs)");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD_DIR");
    println!("cargo:rerun-if-env-changed=SEL4_BUILD");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let build_dir = env::var("SEL4_BUILD_DIR")
        .or_else(|_| env::var("SEL4_BUILD"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("../../seL4/build"));

    if let Some(true) = probe_config_flag(&build_dir, "CONFIG_KERNEL_MCS") {
        println!("cargo:rustc-cfg=sel4_config_kernel_mcs");
    }

    generate_bindings(&build_dir);
}

fn probe_config_flag(root: &Path, flag: &str) -> Option<bool> {
    for relative in CONFIG_CANDIDATES {
        let candidate = root.join(relative);
        println!("cargo:rerun-if-changed={}", candidate.display());
        let Ok(contents) = fs::read_to_string(&candidate) else {
            continue;
        };

        if let Some(value) = parse_config_flag(&contents, flag) {
            return Some(value);
        }
    }

    None
}

fn parse_config_flag(contents: &str, flag: &str) -> Option<bool> {
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(value) = parse_comment_line(line, flag) {
            return Some(value);
        }

        if let Some(value) = parse_assignment_line(line, flag) {
            return Some(value);
        }

        if let Some(value) = parse_cmake_line(line, flag) {
            return Some(value);
        }
    }

    None
}

fn parse_comment_line(line: &str, flag: &str) -> Option<bool> {
    if !line.starts_with('#') {
        return None;
    }

    if line.contains(flag) && line.contains("is not set") {
        return Some(false);
    }

    None
}

fn parse_assignment_line(line: &str, flag: &str) -> Option<bool> {
    if !line.starts_with(flag) {
        return None;
    }

    let mut parts = line.splitn(2, '=');
    let _key = parts.next()?;
    let value = parts.next()?.trim();
    match value {
        "y" | "Y" | "1" => Some(true),
        "n" | "N" | "0" => Some(false),
        _ => None,
    }
}

fn parse_cmake_line(line: &str, flag: &str) -> Option<bool> {
    let line = line.strip_prefix("set(")?.trim_end_matches(')');
    let mut parts = line.split_whitespace();
    let key = parts.next()?;
    if key != flag {
        return None;
    }

    let value = parts.next()?;
    match value {
        "ON" | "TRUE" | "YES" | "1" => Some(true),
        "OFF" | "FALSE" | "NO" | "0" => Some(false),
        _ => None,
    }
}

fn generate_bindings(build_dir: &Path) {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let shim_dir = out_dir.join("sel4_shim");
    let shim_sel4 = shim_dir.join("sel4");
    let shim_sel4_arch = shim_sel4.join("sel4_arch");
    fs::create_dir_all(&shim_sel4_arch).expect("create shim include");

    write_macros(&shim_sel4.join("macros.h"));
    write_debug_assert(&shim_sel4.join("debug_assert.h"));
    write_config(&shim_sel4.join("config.h"));
    write_simple_types(&shim_sel4.join("simple_types.h"));
    write_constants(&shim_sel4.join("constants.h"));
    write_arch_constants(&shim_sel4_arch.join("constants.h"));
    write_arch_types(&shim_sel4_arch.join("types.h"));
    write_errors(&shim_sel4.join("errors.h"));
    write_types(&shim_sel4.join("types.h"));
    write_shared_types(&shim_sel4);
    write_mode_types(&shim_sel4.join("mode"));
    write_sel4_api(&shim_sel4.join("sel4.h"));

    let wrapper = shim_dir.join("wrapper.h");
    let mut wrapper_file = fs::File::create(&wrapper).expect("create wrapper");
    writeln!(wrapper_file, "#include <sel4/sel4.h>").unwrap();
    writeln!(wrapper_file, "#include <sel4/shared_types.h>").unwrap();
    writeln!(wrapper_file, "#include <interfaces/sel4_client.h>").unwrap();

    let builder = bindgen::Builder::default()
        .use_core()
        .ctypes_prefix("core::ffi")
        .header(wrapper.to_string_lossy())
        .clang_arg(format!("-I{}", shim_dir.display()))
        .clang_arg(format!("-I{}", build_dir.join("libsel4/include").display()))
        .clang_arg(format!(
            "-I{}",
            build_dir
                .join("libsel4/sel4_arch_include/aarch64")
                .display()
        ))
        .clang_arg(format!(
            "-I{}",
            build_dir.join("libsel4/arch_include/arm").display()
        ))
        .clang_arg(format!(
            "-I{}",
            build_dir.join("libsel4/autoconf").display()
        ))
        .clang_arg(format!(
            "-I{}",
            build_dir.join("libsel4/gen_config").display()
        ))
        .clang_arg(format!(
            "-I{}",
            build_dir.join("kernel/gen_config").display()
        ))
        .generate_inline_functions(true)
        .layout_tests(false)
        .size_t_is_usize(true)
        .allowlist_function("seL4_.*")
        .allowlist_type("seL4_.*")
        .allowlist_var("seL4_.*")
        .blocklist_item("seL4_ARM_CB.*")
        .blocklist_item("seL4_ARM_SID.*")
        .blocklist_type("seL4_BootInfo")
        .blocklist_type("seL4_BootInfoHeader")
        .blocklist_type("seL4_SlotRegion")
        .blocklist_type("seL4_UntypedDesc")
        .wrap_static_fns(true);

    let bindings = builder.generate().expect("unable to generate bindings");
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("write bindings");
}

fn write_macros(path: &Path) {
    let mut file = fs::File::create(path).expect("create macros.h");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#define LIBSEL4_INLINE static inline").unwrap();
    writeln!(file, "#define LIBSEL4_INLINE_FUNC static inline").unwrap();
    writeln!(file, "#define LIBSEL4_INLINE_FUNC_WARN_UNUSED_RESULT static inline __attribute__((warn_unused_result))").unwrap();
    writeln!(file, "#define LIBSEL4_INLINE_FUNC_DEPRECATED(msg) static inline __attribute__((deprecated(msg)))").unwrap();
    writeln!(
        file,
        "#define LIBSEL4_WARN_UNUSED_RESULT __attribute__((warn_unused_result))"
    )
    .unwrap();
    writeln!(
        file,
        "#define LIBSEL4_DEPRECATED(msg) __attribute__((deprecated(msg)))"
    )
    .unwrap();
    writeln!(file, "#define LIBSEL4_ALIGN(x) __attribute__((aligned(x)))").unwrap();
    writeln!(file, "#define LIBSEL4_BIT(x) (1ull << (x))").unwrap();
    writeln!(
        file,
        "#define SEL4_COMPILE_ASSERT(name, expr) typedef char name[(expr) ? 1 : -1]"
    )
    .unwrap();
    writeln!(file, "#define SEL4_INLINE static inline").unwrap();
    writeln!(file, "#define SEL4_CONST __attribute__((const))").unwrap();
    writeln!(
        file,
        "#define SEL4_FORCE_INLINE static inline __attribute__((always_inline))"
    )
    .unwrap();
    writeln!(file, "#define PURE __attribute__((pure))").unwrap();
    writeln!(file, "#define CONST __attribute__((const))").unwrap();
}

fn write_debug_assert(path: &Path) {
    let mut file = fs::File::create(path).expect("create debug_assert.h");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#define seL4_DebugAssert(cond) ((void)0)").unwrap();
}

fn write_config(path: &Path) {
    let mut file = fs::File::create(path).expect("create config.h");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#include <kernel/gen_config.h>").unwrap();
    writeln!(file, "#include <sel4/gen_config.h>").unwrap();
}

fn write_simple_types(path: &Path) {
    let mut file = fs::File::create(path).expect("create simple_types.h");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#include <sel4/macros.h>").unwrap();
    writeln!(file, "typedef signed char int8_t;").unwrap();
    writeln!(file, "typedef short int16_t;").unwrap();
    writeln!(file, "typedef int int32_t;").unwrap();
    writeln!(file, "typedef long int64_t;").unwrap();
    writeln!(file, "typedef unsigned char uint8_t;").unwrap();
    writeln!(file, "typedef unsigned short uint16_t;").unwrap();
    writeln!(file, "typedef unsigned int uint32_t;").unwrap();
    writeln!(file, "typedef unsigned long uint64_t;").unwrap();
    writeln!(file, "typedef unsigned long size_t;").unwrap();
    writeln!(file, "typedef int8_t seL4_Int8;").unwrap();
    writeln!(file, "typedef int16_t seL4_Int16;").unwrap();
    writeln!(file, "typedef int32_t seL4_Int32;").unwrap();
    writeln!(file, "typedef int64_t seL4_Int64;").unwrap();
    writeln!(file, "typedef uint8_t seL4_Uint8;").unwrap();
    writeln!(file, "typedef uint16_t seL4_Uint16;").unwrap();
    writeln!(file, "typedef uint32_t seL4_Uint32;").unwrap();
    writeln!(file, "typedef uint64_t seL4_Uint64;").unwrap();
    writeln!(file, "typedef uint8_t seL4_Bool;").unwrap();
    writeln!(file, "typedef unsigned long seL4_Word;").unwrap();
    writeln!(file, "typedef signed long seL4_SWord;").unwrap();
    writeln!(file, "typedef seL4_Word seL4_CPtr;").unwrap();
    writeln!(file, "typedef seL4_Word seL4_UintPtr;").unwrap();
    writeln!(file, "typedef seL4_Word seL4_PAddr;").unwrap();
}

fn write_constants(path: &Path) {
    let mut file = fs::File::create(path).expect("create constants.h");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#include <sel4/macros.h>").unwrap();
    writeln!(file, "#define seL4_WordBits 64").unwrap();
    writeln!(file, "#define seL4_PageBits 12").unwrap();
    writeln!(file, "#define seL4_MsgLengthBits 7").unwrap();
    writeln!(file, "#define seL4_MsgExtraCapBits 2").unwrap();
    writeln!(file, "#define seL4_MsgMaxLength 120").unwrap();
}

fn write_arch_constants(path: &Path) {
    let mut file = fs::File::create(path).expect("create arch constants");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#include <sel4/invocation.h>").unwrap();
    writeln!(file, "#include <sel4/config.h>").unwrap();
    writeln!(file, "#include <sel4/sel4_arch/invocation.h>").unwrap();
    writeln!(file, "#include <sel4/constants.h>").unwrap();
}

fn write_arch_types(path: &Path) {
    let mut file = fs::File::create(path).expect("create sel4_arch/types.h");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#include <sel4/simple_types.h>").unwrap();
    writeln!(file, "#include <sel4/sel4_arch/types_gen.h>").unwrap();
}

fn write_errors(path: &Path) {
    let mut file = fs::File::create(path).expect("create errors.h");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "typedef int seL4_Error;").unwrap();
    writeln!(file, "#define seL4_NoError ((seL4_Error)0)").unwrap();
    writeln!(file, "#define seL4_InvalidArgument ((seL4_Error)1)").unwrap();
    writeln!(file, "#define seL4_InvalidCapability ((seL4_Error)2)").unwrap();
    writeln!(file, "#define seL4_IllegalOperation ((seL4_Error)3)").unwrap();
    writeln!(file, "#define seL4_RangeError ((seL4_Error)4)").unwrap();
    writeln!(file, "#define seL4_AlignmentError ((seL4_Error)5)").unwrap();
    writeln!(file, "#define seL4_FailedLookup ((seL4_Error)6)").unwrap();
    writeln!(file, "#define seL4_TruncatedMessage ((seL4_Error)7)").unwrap();
    writeln!(file, "#define seL4_DeleteFirst ((seL4_Error)8)").unwrap();
    writeln!(file, "#define seL4_RevokeFirst ((seL4_Error)9)").unwrap();
}

fn write_types(path: &Path) {
    let mut file = fs::File::create(path).expect("create types.h");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#include <sel4/config.h>").unwrap();
    writeln!(file, "#include <sel4/simple_types.h>").unwrap();
    writeln!(file, "#include <sel4/macros.h>").unwrap();
    writeln!(file, "#include <sel4/sel4_arch/types.h>").unwrap();
    writeln!(file, "#include <sel4/errors.h>").unwrap();
    writeln!(file, "#include <sel4/shared_types_gen.h>").unwrap();
    writeln!(file, "typedef seL4_Word seL4_NodeId;").unwrap();
    writeln!(file, "typedef seL4_Word seL4_Domain;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_CNode;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_IRQHandler;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_IRQControl;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_TCB;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_Untyped;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_DomainSet;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_SchedContext;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_SchedControl;").unwrap();
    writeln!(file, "typedef seL4_Uint64 seL4_Time;").unwrap();
    writeln!(file, "typedef seL4_Word seL4_ARM_VMAttributes;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_Page;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_PageTable;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_VSpace;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_ASIDControl;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_ASIDPool;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_VCPU;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_IOSpace;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_IOPageTable;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_SMC;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_SIDControl;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_SID;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_CBControl;").unwrap();
    writeln!(file, "typedef seL4_CPtr seL4_ARM_CB;").unwrap();
    writeln!(file, "typedef seL4_Word seL4_VCPUReg;").unwrap();
    writeln!(file, "#define seL4_NilData 0").unwrap();

    writeln!(file, "typedef struct seL4_UserContext {{").unwrap();
    writeln!(file, "    seL4_Word pc;").unwrap();
    writeln!(file, "    seL4_Word sp;").unwrap();
    writeln!(file, "    seL4_Word spsr;").unwrap();
    for reg in 0..31 {
        writeln!(file, "    seL4_Word x{};", reg).unwrap();
    }
    writeln!(file, "    seL4_Word tpidr_el0;").unwrap();
    writeln!(file, "    seL4_Word tpidrro_el0;").unwrap();
    writeln!(file, "}} seL4_UserContext;").unwrap();

    writeln!(file, "typedef struct seL4_ARM_SMCContext {{").unwrap();
    for idx in 0..8 {
        writeln!(file, "    seL4_Word x{};", idx).unwrap();
    }
    writeln!(file, "}} seL4_ARM_SMCContext;").unwrap();
}

fn write_shared_types(dir: &Path) {
    fs::create_dir_all(dir).expect("create shared_types dir");
    let file_path = dir.join("shared_types.h");
    let mut file = fs::File::create(file_path).expect("create shared_types.h");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#include <sel4/constants.h>").unwrap();
    writeln!(file, "#include <sel4/shared_types_gen.h>").unwrap();

    writeln!(file, "typedef struct seL4_IPCBuffer_ {{").unwrap();
    writeln!(file, "    seL4_MessageInfo_t tag;").unwrap();
    writeln!(file, "    seL4_Word msg[seL4_MsgMaxLength];").unwrap();
    writeln!(file, "    seL4_Word userData;").unwrap();
    writeln!(file, "    seL4_Word caps_or_badges[((1ul << (seL4_MsgExtraCapBits)) - 1)];").unwrap();
    writeln!(file, "    seL4_CPtr receiveCNode;").unwrap();
    writeln!(file, "    seL4_CPtr receiveIndex;").unwrap();
    writeln!(file, "    seL4_Word receiveDepth;").unwrap();
    writeln!(file, "}} seL4_IPCBuffer __attribute__((__aligned__(sizeof(struct seL4_IPCBuffer_))));").unwrap();
}

fn write_mode_types(path: &Path) {
    fs::create_dir_all(path).expect("create mode dir");
    let mut file = fs::File::create(path.join("types.h")).expect("create mode/types.h");
    writeln!(file, "#pragma once").unwrap();
}

fn write_sel4_api(path: &Path) {
    let mut file = fs::File::create(path).expect("create sel4.h");
    writeln!(file, "#pragma once").unwrap();
    writeln!(file, "#include <sel4/macros.h>").unwrap();
    writeln!(file, "#include <sel4/config.h>").unwrap();
    writeln!(file, "#include <sel4/constants.h>").unwrap();
    writeln!(file, "#include <sel4/types.h>").unwrap();
    writeln!(file, "#include <sel4/invocation.h>").unwrap();
    writeln!(file, "#include <sel4/sel4_arch/invocation.h>").unwrap();
    writeln!(file, "#include <sel4/shared_types.h>").unwrap();
    writeln!(file, "#ifndef SEL4_FORCE_LONG_ENUM").unwrap();
    writeln!(file, "#define SEL4_FORCE_LONG_ENUM(type) type##_FORCE_LONG_ENUM = 0x7fffffff").unwrap();
    writeln!(file, "#endif").unwrap();
    writeln!(file, "#include <sel4/syscall.h>").unwrap();

    writeln!(file, "typedef struct seL4_BootInfo seL4_BootInfo;").unwrap();

    writeln!(
        file,
        "seL4_MessageInfo_t seL4_CallWithMRs(seL4_CPtr, seL4_MessageInfo_t, seL4_Word*, seL4_Word*, seL4_Word*, seL4_Word*);",
    )
    .unwrap();
    writeln!(file, "void seL4_SetMR(seL4_Word, seL4_Word);").unwrap();
    writeln!(file, "seL4_Word seL4_GetMR(seL4_Word);").unwrap();
    writeln!(file, "void seL4_SetCap(seL4_Word, seL4_CPtr);").unwrap();
    writeln!(file, "seL4_CPtr seL4_GetCap(seL4_Word);").unwrap();
    writeln!(file, "const seL4_BootInfo* seL4_GetBootInfo(void);").unwrap();
    writeln!(file, "void seL4_SetIPCBuffer(seL4_IPCBuffer*);").unwrap();
    writeln!(file, "seL4_IPCBuffer* seL4_GetIPCBuffer(void);").unwrap();
    writeln!(file, "void seL4_DebugPutChar(char);").unwrap();
    writeln!(file, "seL4_Word seL4_DebugCapIdentify(seL4_CPtr);").unwrap();
}
