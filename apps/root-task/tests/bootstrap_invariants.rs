// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::mem;
use std::boxed::Box;

use root_task::bootstrap::cspace::CSpaceCtx;
use root_task::bootstrap::phases::{self, BootstrapPhase, BootstrapSequencer};
use root_task::bootstrap::state;
use root_task::cspace::CSpace;
use root_task::sel4::BootInfoView;
use sel4_sys::{self, seL4_BootInfo, seL4_SlotRegion};

fn bootinfo_fixture(bits: u8, empty_start: u32, empty_end: u32) -> &'static seL4_BootInfo {
    let mut bootinfo: seL4_BootInfo = unsafe { mem::zeroed() };
    bootinfo.initThreadCNodeSizeBits = bits as sel4_sys::seL4_Word;
    bootinfo.empty = seL4_SlotRegion {
        start: empty_start,
        end: empty_end,
    };
    Box::leak(Box::new(bootinfo))
}

#[test]
fn sequencer_matches_bootstrap_order() {
    state::reset_for_tests();
    let mut sequencer = BootstrapSequencer::new();
    for phase in phases::ordering() {
        sequencer
            .advance(*phase)
            .unwrap_or_else(|err| panic!("phase {phase:?} failed: {err}"));
    }
    let overflow = sequencer.advance(BootstrapPhase::UserlandHandoff);
    assert!(overflow.is_err(), "sequencer should reject extra phases");
}

#[test]
fn sequencer_rejects_reordering() {
    state::reset_for_tests();
    let mut sequencer = BootstrapSequencer::new();
    sequencer
        .advance(BootstrapPhase::CSpaceCanonicalise)
        .expect("first phase must succeed");
    let out_of_order = sequencer.advance(BootstrapPhase::IPCInstall);
    assert!(out_of_order.is_err(), "out-of-order advance must fail");
}

#[test]
fn bootinfo_validation_rejects_bad_windows() {
    let bootinfo = bootinfo_fixture(4, 0x40, 0x60);
    let view = BootInfoView::new(bootinfo).expect("bootinfo fixture must be valid");
    state::reset_for_tests();
    let mut sequencer = BootstrapSequencer::new();
    sequencer
        .advance(BootstrapPhase::CSpaceCanonicalise)
        .expect("phase CSpaceCanonicalise must succeed");
    let err = sequencer.validate_bootinfo(&view).unwrap_err();
    assert!(
        err.message().contains("empty window exceeds"),
        "expected empty-window capacity failure"
    );
}

#[test]
fn bootinfo_validation_bounds_init_bits() {
    let bootinfo = bootinfo_fixture(sel4_sys::seL4_WordBits as u8 + 1, 0x40, 0x44);
    let view_result = BootInfoView::new(bootinfo);
    assert!(
        view_result.is_err(),
        "init bits beyond word width must fail"
    );

    let zero_bits = bootinfo_fixture(0, 0x10, 0x20);
    let view = BootInfoView::new(zero_bits).expect("bootinfo view must construct");
    state::reset_for_tests();
    let mut sequencer = BootstrapSequencer::new();
    sequencer
        .advance(BootstrapPhase::CSpaceCanonicalise)
        .expect("phase CSpaceCanonicalise must succeed");
    let err = sequencer
        .validate_bootinfo(&view)
        .expect_err("zero init bits must be rejected at validation");
    assert!(
        err.message().contains("non-zero"),
        "expected zero init bits to be rejected with a clear message"
    );
}

#[test]
fn slot_allocator_skips_reserved_range() {
    let reserved_end = sel4_sys::seL4_NumInitialCaps as u32;
    let bootinfo = bootinfo_fixture(12, reserved_end - 1, reserved_end + 4);
    let view = BootInfoView::new(bootinfo).expect("bootinfo fixture must be valid");
    let cspace = CSpace::from_bootinfo(bootinfo);
    let mut ctx = CSpaceCtx::new(view, cspace);
    let slot = ctx.alloc_slot_checked().expect("first slot must allocate");
    assert!(
        slot >= sel4_sys::seL4_NumInitialCaps,
        "allocator must not return kernel-reserved slots"
    );
}

#[test]
fn ipc_install_allows_retypes_to_proceed() {
    let (empty_start, empty_end) = (0x20, 0x28);
    let bootinfo = bootinfo_fixture(12, empty_start, empty_end);
    let view = BootInfoView::new(bootinfo).expect("bootinfo fixture must be valid");

    state::reset_for_tests();
    let mut sequencer = BootstrapSequencer::new();
    sequencer
        .advance(BootstrapPhase::CSpaceCanonicalise)
        .expect("phase CSpaceCanonicalise must succeed");
    sequencer
        .validate_bootinfo(&view)
        .expect("bootinfo must validate");
    sequencer
        .advance(BootstrapPhase::MemoryLayoutBuild)
        .expect("MemoryLayoutBuild must follow BootInfoValidate");
    sequencer
        .advance(BootstrapPhase::CSpaceRecord)
        .expect("CSpaceRecord must follow MemoryLayoutBuild");

    let mut cspace = CSpace::from_bootinfo(bootinfo);
    let ipc_slot = cspace
        .alloc_slot()
        .expect("IPC install must allocate a slot");
    sequencer
        .advance(BootstrapPhase::IPCInstall)
        .expect("IPCInstall must precede retype planning");

    assert_eq!(
        ipc_slot, empty_start,
        "IPC install should consume the first advertised empty slot"
    );
    assert!(
        cspace.next_free_slot() < empty_end,
        "remaining CSpace capacity must still be available for retype plan"
    );

    sequencer
        .advance(BootstrapPhase::UntypedPlan)
        .expect("UntypedPlan must follow IPCInstall");
    sequencer
        .advance(BootstrapPhase::RetypeCommit)
        .expect("RetypeCommit must follow UntypedPlan");
}
