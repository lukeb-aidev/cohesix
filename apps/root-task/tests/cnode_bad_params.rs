// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::mem::{self, MaybeUninit};

use root_task::bootstrap::cspace::{BootInfoView, CSpaceCtx};
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
    CSpaceCtx::new(BootInfoView::new(bootinfo))
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
fn bootinfo_depth_mint_succeeds() {
    let mut ctx = ctx_fixture();
    assert_eq!(ctx.smoke_copy_init_tcb(), Ok(()));
    let boot_depth = ctx.cnode_invocation_depth_bits;
    assert_eq!(boot_depth, ctx.bi.init_cnode_bits());

    let err = unsafe {
        seL4_CNode_Mint(
            seL4_CapInitThreadCNode,
            ctx.first_free.saturating_add(1),
            boot_depth,
            seL4_CapInitThreadCNode,
            seL4_CapInitThreadTCB,
            boot_depth,
            seL4_CapRights_ReadWrite,
            0,
            0,
        )
    };

    assert_eq!(err, seL4_NoError);
}
