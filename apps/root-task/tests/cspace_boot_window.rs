// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::mem::{self, MaybeUninit};
use std::panic::{self, AssertUnwindSafe};

use root_task::bootstrap::cspace::CSpaceWindow;
use root_task::bootstrap::cspace_sys::{
    bits_as_u8, take_last_host_retype_trace, untyped_retype_encoded,
};
use root_task::sel4::BootInfoView;
use sel4_sys::{self, seL4_BootInfo, seL4_SlotRegion};

fn bootinfo_fixture() -> &'static seL4_BootInfo {
    static mut BOOTINFO: MaybeUninit<seL4_BootInfo> = MaybeUninit::uninit();
    unsafe {
        let ptr = BOOTINFO.as_mut_ptr();
        ptr.write(mem::zeroed());
        let bootinfo = &mut *ptr;
        bootinfo.initThreadCNodeSizeBits = 12;
        bootinfo.empty = seL4_SlotRegion {
            start: 0x40,
            end: 0x80,
        };
        bootinfo
    }
}

#[test]
fn boot_window_adapter_logs_and_bounds_check() {
    let bootinfo = bootinfo_fixture();
    let view = BootInfoView::new(bootinfo).expect("bootinfo fixture must be valid");
    let (empty_start, empty_end) = view.init_cnode_empty_range();
    let window = CSpaceWindow::new(
        view.root_cnode_cap(),
        view.canonical_root_cap(),
        bits_as_u8(usize::from(view.init_cnode_bits())),
        empty_start,
        empty_end,
        empty_start,
    );
    assert_eq!(window.empty_start, bootinfo.empty.start);
    assert_eq!(window.empty_end, bootinfo.empty.end);
    window.assert_contains(window.first_free);

    let _ = take_last_host_retype_trace();
    let err = untyped_retype_encoded(
        0x200,
        sel4_sys::seL4_EndpointObject as u32,
        0,
        window.root,
        window.first_free as u64,
        window.bits,
        1,
    );
    assert_eq!(err, sel4_sys::seL4_NoError);
    let trace = take_last_host_retype_trace().expect("host trace must be captured");
    assert_eq!(trace.node_index, 0);
    assert_eq!(trace.node_depth, 0);
    assert_eq!(trace.node_offset, window.first_free as sel4_sys::seL4_Word);

    let out_of_range = window.empty_end;
    let panic_result = panic::catch_unwind(AssertUnwindSafe(|| {
        window.assert_contains(out_of_range);
    }));
    assert!(
        panic_result.is_err(),
        "CSpaceWindow::assert_contains must reject slots beyond the boot window"
    );
}
