// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::mem::{self, MaybeUninit};

use root_task::bootstrap::cspace::{BootInfoView, CSpaceCtx};
use root_task::cspace::CSpace;
use root_task::sel4::{
    self, seL4_CNode_Mint, seL4_CapInitThreadCNode, seL4_CapInitThreadTCB,
    seL4_CapRights_ReadWrite, seL4_NoError, seL4_SlotRegion,
};

#[cfg(target_os = "none")]
fn bootinfo_fixture() -> &'static sel4::BootInfo {
    static mut BOOTINFO: MaybeUninit<sel4::BootInfo> = MaybeUninit::uninit();

    unsafe {
        let ptr = BOOTINFO.as_mut_ptr();
        ptr.write(mem::zeroed());
        let bootinfo = &mut *ptr;
        bootinfo.initThreadCNodeSizeBits = 12;
        bootinfo.empty = seL4_SlotRegion {
            start: 0x40,
            end: 0x80,
        };
        bootinfo.sharedFrames = seL4_SlotRegion { start: 0, end: 0 };
        bootinfo.userImageFrames = seL4_SlotRegion { start: 0, end: 0 };
        bootinfo.userImagePaging = seL4_SlotRegion { start: 0, end: 0 };
        bootinfo.ioSpaceCaps = seL4_SlotRegion { start: 0, end: 0 };
        bootinfo.extraBIPages = seL4_SlotRegion { start: 0, end: 0 };
        bootinfo.untyped = seL4_SlotRegion {
            start: 0x100,
            end: 0x180,
        };
        &*ptr
    }
}

#[cfg(target_os = "none")]
fn ctx_fixture() -> CSpaceCtx {
    let bootinfo = bootinfo_fixture();
    let cspace = CSpace::from_bootinfo(bootinfo);
    let view = BootInfoView::new(bootinfo).expect("bootinfo fixture must be valid");
    CSpaceCtx::new(view, cspace)
}

#[cfg(target_os = "none")]
#[test]
fn zero_depth_mint_is_rejected() {
    let mut ctx = ctx_fixture();
    assert_eq!(ctx.smoke_copy_init_tcb(), Ok(()));

    let err = unsafe {
        seL4_CNode_Mint(
            seL4_CapInitThreadCNode,
            ctx.first_free,
            0,
            seL4_CapInitThreadCNode,
            seL4_CapInitThreadTCB,
            0,
            seL4_CapRights_ReadWrite,
            0,
            0,
        )
    };

    assert_ne!(err, seL4_NoError);
}

#[cfg(target_os = "none")]
#[test]
fn guard_depth_mint_succeeds() {
    let mut ctx = ctx_fixture();
    assert_eq!(ctx.smoke_copy_init_tcb(), Ok(()));
    let init_bits = ctx.bi.init_cnode_bits();
    let guard_depth = ctx.cnode_invocation_depth_bits;
    let canonical_depth = sel4::word_bits() as u8;
    assert_eq!(ctx.cnode_bits(), init_bits);
    assert_eq!(guard_depth, canonical_depth);
    assert_ne!(canonical_depth, init_bits);

    let err = unsafe {
        seL4_CNode_Mint(
            seL4_CapInitThreadCNode,
            ctx.first_free.saturating_add(1),
            guard_depth,
            seL4_CapInitThreadCNode,
            seL4_CapInitThreadTCB,
            guard_depth,
            seL4_CapRights_ReadWrite,
            0,
            0,
        )
    };

    assert_eq!(err, seL4_NoError);

    let legacy_err = unsafe {
        seL4_CNode_Mint(
            seL4_CapInitThreadCNode,
            ctx.first_free.saturating_add(2),
            init_bits,
            seL4_CapInitThreadCNode,
            seL4_CapInitThreadTCB,
            init_bits,
            seL4_CapRights_ReadWrite,
            0,
            0,
        )
    };

    assert_ne!(legacy_err, seL4_NoError);
}
