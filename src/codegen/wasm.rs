// CLASSIFICATION: COMMUNITY
// Filename: wasm.rs v1.1
// Date Modified: 2025-07-24
// Author: Lukas Bower

//! WASM backend for the Coh_CC compiler. Translates IR into WebAssembly text (WAT) format.


use crate::ir::Module;

/// Generates a WebAssembly text (WAT) representation from an IR `Module`.
pub fn generate_wasm(module: &Module) -> String {
    let mut output = String::new();
    // Module header
    output.push_str("(module\n");
    // Import printf for debug (if needed)
    output.push_str("  (import \"env\" \"printf\" (func $printf (param i32)))\n");

    // Function definitions
    for func in &module.functions {
        output.push_str(&format!(
            "  (func ${} (export \"{}\")\n",
            func.name, func.name
        ));
        // Instruction emission will be implemented in a later revision
        output.push_str("    ;; instruction emission pending\n");
        // FIXME: emit WASM instructions based on IR opcodes
        output.push_str("    ;; FIXME: instruction emission not implemented\n");
        output.push_str("  )\n");
    }

    // Optional start function
    if let Some(first) = module.functions.first() {
        output.push_str(&format!("  (start ${})\n", first.name));
    }

    output.push_str(")\n");
    output
}
