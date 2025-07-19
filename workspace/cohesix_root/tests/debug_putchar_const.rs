// CLASSIFICATION: COMMUNITY
// Filename: debug_putchar_const.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2028-02-16

#[test]
fn debug_putchar_constant() {
    let src = include_str!("../src/sys.rs");
    assert!(src.contains("const SYS_DEBUG_PUTCHAR: i64 = -9;"));
}
