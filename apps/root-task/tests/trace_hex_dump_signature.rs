// Author: Lukas Bower

#[test]
fn hex_dump_slice_rejects_pointers() {
    let mut flags = std::env::var("RUSTFLAGS").unwrap_or_default();
    if !flags.is_empty() {
        flags.push(' ');
    }
    flags.push_str("--cfg feature=\"kernel\"");
    std::env::set_var("RUSTFLAGS", flags);

    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/hex_dump_slice_pointer.rs");
}
