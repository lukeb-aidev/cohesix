// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::mem;
use std::boxed::Box;
use std::panic::{self, AssertUnwindSafe};

use root_task::bootstrap::cspace::CSpaceCtx;
use root_task::bootstrap::phases::{BootstrapPhase, BootstrapSequencer};
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
    for phase in [
        BootstrapPhase::CSpaceCanonicalise,
        BootstrapPhase::BootInfoValidate,
        BootstrapPhase::MemoryLayoutBuild,
        BootstrapPhase::CSpaceRecord,
        BootstrapPhase::IPCInstall,
        BootstrapPhase::UntypedPlan,
        BootstrapPhase::RetypeCommit,
        BootstrapPhase::UserlandHandoff,
    ] {
        sequencer
            .advance(phase)
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
    let panic_result = panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = BootInfoView::new(zero_bits);
    }));
    assert!(
        panic_result.is_err(),
        "zero init bits should trip debug assertions"
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
