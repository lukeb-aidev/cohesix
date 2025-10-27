// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::bootstrap::cspace_sys::RetypeArgs;
use root_task::caps::render_retype_log_line;
use sel4_sys::{seL4_Error, seL4_ObjectType, seL4_Word};

#[test]
fn retype_log_line_is_numeric() {
    let args = RetypeArgs::new(
        0x91,
        seL4_ObjectType::seL4_CapTableObject as seL4_Word,
        12,
        0x2,
        0x1,
        0,
        0x20,
        1,
    );
    let line = render_retype_log_line("pre", &args, Some(seL4_Error::seL4_InvalidArgument));
    let text = line.as_str();
    assert!(text.starts_with("[retype:pre]"));
    assert!(text.contains("ut=0x0000000000000091"));
    assert!(text.contains("type=0x00000002"));
    assert!(text.contains("root=0x0000000000000002"));
    assert!(text.contains("idx=0x00000001"));
    assert!(text.contains("depth=0"));
    assert!(text.contains("off=0x00000020"));
    assert!(text.contains("n=1"));
    assert!(text.contains("err=0x00000003"));
    assert!(!text.contains('%'));
    assert!(!text.contains("Cap"));
}
