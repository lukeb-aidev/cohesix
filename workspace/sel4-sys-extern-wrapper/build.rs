// CLASSIFICATION: COMMUNITY
// Filename: build.rs v0.7
// Author: Lukas Bower
// Date Modified: 2028-12-13

use std::{
    env, fs,
    path::{Path, PathBuf},
};
#[path = "../sel4_paths.rs"]
mod sel4_paths;
use sel4_paths::copy_recursive;

fn generate_wrapper_header(out_dir: &Path) -> PathBuf {
    let header_path = out_dir.join("sel4_wrapper.h");
    let mut content = String::new();
    content.push_str("// CLASSIFICATION: COMMUNITY\n");
    content.push_str("// Filename: sel4_wrapper.h v0.2\n");
    content.push_str("// Author: Lukas Bower\n");
    content.push_str("// Date Modified: 2025-07-22\n\n");
    content.push_str("#pragma once\n");
    content.push_str("#include <generated/kernel/gen_config.h>\n");
    content.push_str("#include <generated/sel4/gen_config.h>\n");
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
    content.push_str("typedef struct seL4_MessageInfo seL4_MessageInfo_t;\n");
    content.push_str("typedef unsigned long seL4_CPtr;\n");
    content.push_str("typedef unsigned long seL4_Word;\n");
    content.push_str("#define ARMPageTableMap 0\n");
    content.push_str("#define ARMPageTableUnmap 0\n");
    content.push_str("#define ARMPageMap 0\n");
    content.push_str("#define ARMPageUnmap 0\n");
    content.push_str("#define ARMPageClean_Data 0\n");
    content.push_str("#define ARMPageInvalidate_Data 0\n");
    content.push_str("#define ARMPageCleanInvalidate_Data 0\n");
    content.push_str("#define ARMPageUnify_Instruction 0\n");
    content.push_str("#define ARMPageGetAddress 0\n");
    content.push_str("#define ARMASIDControlMakePool 0\n");
    content.push_str("#define ARMASIDPoolAssign 0\n");
    content.push_str("#define ARMIRQIssueIRQHandlerTrigger 0\n");
    content.push_str("#define ARMIRQIssueSGISignal 0\n");
    content.push_str("seL4_MessageInfo_t seL4_CallWithMRs(seL4_CPtr dest, seL4_MessageInfo_t info, seL4_Word* mr0, seL4_Word* mr1, seL4_Word* mr2, seL4_Word* mr3);\n\n");
    content.push_str("void seL4_DebugPutChar(int c);\n");
    content.push_str("void seL4_DebugHalt(void);\n");
    content.push_str("#include <sel4/sel4.h>\n");
    fs::write(&header_path, content).expect("write sel4_wrapper.h");
    header_path
}

fn generate_extra_source(out_dir: &Path) -> PathBuf {
    let source_path = out_dir.join("cohesix_sel4_wrappers.c");
    let mut body = String::new();
    body.push_str("// CLASSIFICATION: COMMUNITY\n");
    body.push_str("// Filename: cohesix_sel4_wrappers.c v0.1\n");
    body.push_str("// Author: Lukas Bower\n");
    body.push_str("// Date Modified: 2025-10-06\n\n");
    body.push_str("#include \"sel4_wrapper.h\"\n");
    body.push_str("#include <sel4/sel4.h>\n\n");
    body.push_str("int cohesix_seL4_Untyped_Retype(seL4_Untyped ut, seL4_Word type, seL4_Word size_bits, seL4_CNode root, seL4_Word node_index, seL4_Word node_depth, seL4_Word node_offset, seL4_Word num_objects) {\n");
    body.push_str("    return seL4_Untyped_Retype(ut, type, size_bits, root, node_index, node_depth, node_offset, num_objects);\n}\n\n");
    body.push_str("int cohesix_seL4_CNode_Mint(seL4_CPtr dest_root, seL4_CPtr dest_index, seL4_Word dest_depth, seL4_CPtr src_root, seL4_CPtr src_index, seL4_Word src_depth, seL4_CapRights_t rights, seL4_CapData_t data) {\n");
    body.push_str("    return seL4_CNode_Mint(dest_root, dest_index, dest_depth, src_root, src_index, src_depth, rights, data);\n}\n\n");
    body.push_str("int cohesix_seL4_CNode_Delete(seL4_CPtr dest_root, seL4_CPtr dest_index, seL4_Word dest_depth) {\n");
    body.push_str("    return seL4_CNode_Delete(dest_root, dest_index, dest_depth);\n}\n\n");
    body.push_str("int cohesix_seL4_ARM_Page_Map(seL4_ARM_Page page, seL4_CPtr vspace, seL4_Word vaddr, seL4_CapRights_t rights, seL4_ARM_VMAttributes attr) {\n");
    body.push_str("    return seL4_ARM_Page_Map(page, vspace, vaddr, rights, attr);\n}\n\n");
    body.push_str("int cohesix_seL4_ARM_Page_Unmap(seL4_ARM_Page page) {\n");
    body.push_str("    return seL4_ARM_Page_Unmap(page);\n}\n\n");
    body.push_str("seL4_CapRights_t cohesix_seL4_CapRights_new(seL4_Uint64 grant_reply, seL4_Uint64 grant, seL4_Uint64 read, seL4_Uint64 write) {\n");
    body.push_str("    return seL4_CapRights_new(grant_reply, grant, read, write);\n}\n\n");
    body.push_str("seL4_CNode_CapData_t cohesix_seL4_CNode_CapData_new(seL4_Uint64 guard, seL4_Uint64 guard_size) {\n");
    body.push_str("    return seL4_CNode_CapData_new(guard, guard_size);\n}\n");
    fs::write(&source_path, body).expect("write cohesix_sel4_wrappers.c");
    source_path
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root = sel4_paths::project_root(&manifest_dir);
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let include_root = project_root
        .join("third_party")
        .join("seL4")
        .join("include");
    // Perform a single recursive copy of the entire seL4 include tree so that
    // all nested headers are available during bindgen and build steps.
    copy_recursive(&include_root, &out_dir).expect("copy seL4 include tree");
    if let Ok(arch) = env::var("SEL4_ARCH") {
        let alias_root = sel4_paths::create_arch_alias(&include_root, &arch, &out_dir)
            .expect("create arch alias");
        copy_recursive(&alias_root, &out_dir).expect("merge arch alias");
    }
    let simple_types = out_dir.join("sel4").join("arch").join("simple_types.h");
    if !simple_types.exists() {
        panic!("missing {}", simple_types.display());
    }

    let mut include_dirs = Vec::new();
    if let Ok(dirs) = sel4_paths::header_dirs_recursive(&out_dir) {
        include_dirs.extend(dirs);
    }

    let plat_api = out_dir.join("sel4").join("plat").join("api");
    fs::create_dir_all(&plat_api).expect("create plat/api dir");
    fs::write(plat_api.join("constants.h"), "#pragma once\n").expect("stub constants.h");

    let wrapper_header = generate_wrapper_header(&out_dir);
    let extra_source = generate_extra_source(&out_dir);

    let mut builder = bindgen::Builder::default()
        .header(wrapper_header.to_str().unwrap())
        .use_core()
        .clang_arg(format!("-I{}", out_dir.display()))
        .ctypes_prefix("cty")
        .generate_inline_functions(true);
    for dir in &include_dirs {
        builder = builder.clang_arg(format!("-I{}", dir.display()));
    }

    println!("cargo:rerun-if-env-changed=SEL4_ARCH");

    if env::var("SEL4_SYS_CFLAGS").is_ok() {
        println!("cargo:warning=SEL4_SYS_CFLAGS ignored (sel4-sys removed)");
    }

    let bindings = builder.generate().expect("Unable to generate bindings");
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    let mut cc_builder = cc::Build::new();
    cc_builder.compiler("aarch64-linux-gnu-gcc");
    cc_builder.file(&extra_source);
    cc_builder.include(&out_dir);
    for dir in &include_dirs {
        cc_builder.include(dir);
    }
    cc_builder.flag_if_supported("-Wno-unused-parameter");
    cc_builder.compile("cohesix_sel4_wrappers");

    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rustc-link-search=native={}/third_party/seL4/lib",
        project_root.display()
    );
    println!("cargo:rustc-link-lib=static=sel4");
}
