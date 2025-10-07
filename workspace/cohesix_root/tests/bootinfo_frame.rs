// CLASSIFICATION: COMMUNITY
// Filename: bootinfo_frame.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-03-21

#[test]
fn bootinfo_frame_base_is_preserved() {
    let src = include_str!("../src/bootinfo.rs");
    assert!(
        src.contains("static mut BOOTINFO_FRAME_BASE"),
        "bootinfo frame base pointer must be preserved"
    );
    assert!(
        src.contains("BOOTINFO_FRAME_BASE = ptr as *const u8;"),
        "copy_bootinfo must record the kernel-provided frame base"
    );
    assert!(
        src.contains("let base_ptr = match bootinfo_frame_base()"),
        "dtb_slice should consult the bootinfo frame base"
    );
    assert!(
        src.contains("cmp::min(bootinfo_ref.extra_len, available_bytes)"),
        "dtb_slice must clamp the readable length to the available mapped bytes"
    );
}
