// Author: Lukas Bower

use crate::sel4::{self, BootInfo};
use core::fmt::Write;
use heapless::String;
use sel4_sys::seL4_CapRights_new;

use super::cspace_sys;

const MAX_DIAGNOSTIC_LEN: usize = 192;

/// Lightweight projection of [`seL4_BootInfo`] exposing capability-space fields.
#[derive(Copy, Clone)]
pub struct BootInfoView {
    bootinfo: &'static BootInfo,
}

impl BootInfoView {
    #[inline(always)]
    pub fn new(bootinfo: &'static BootInfo) -> Self {
        Self { bootinfo }
    }

    #[inline(always)]
    pub fn init_cnode_bits(&self) -> u8 {
        self.bootinfo.initThreadCNodeSizeBits as u8
    }

    #[inline(always)]
    pub fn empty_start(&self) -> sel4::seL4_CPtr {
        self.bootinfo.empty.start as sel4::seL4_CPtr
    }

    #[inline(always)]
    pub fn empty_end(&self) -> sel4::seL4_CPtr {
        self.bootinfo.empty.end as sel4::seL4_CPtr
    }

    #[inline(always)]
    pub fn root_cnode_cap(&self) -> sel4::seL4_CPtr {
        sel4::seL4_CapInitThreadCNode
    }
}

impl From<&'static BootInfo> for BootInfoView {
    fn from(value: &'static BootInfo) -> Self {
        Self::new(value)
    }
}

/// Canonical CSpace context using direct addressing for all seL4 CNode operations.
pub struct CSpaceCtx {
    pub bi: BootInfoView,
    pub init_cnode_bits: u8,
    pub first_free: sel4::seL4_CPtr,
    pub last_free: sel4::seL4_CPtr,
    pub root_cnode_cap: sel4::seL4_CPtr,
    pub root_cnode_copy_slot: sel4::seL4_CPtr,
    next_slot: sel4::seL4_CPtr,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SlotAllocError {
    OutOfBootWindow {
        candidate: sel4::seL4_CPtr,
        start: sel4::seL4_CPtr,
        end: sel4::seL4_CPtr,
    },
    ReservedSlot {
        slot: sel4::seL4_CPtr,
    },
}

impl CSpaceCtx {
    pub fn new(bi: BootInfoView) -> Self {
        let init_cnode_bits = bi.init_cnode_bits();
        let first_free = bi.empty_start();
        let last_free = bi.empty_end();
        let root_cnode_cap = bi.root_cnode_cap();
        let ctx = Self {
            bi,
            init_cnode_bits,
            first_free,
            last_free,
            root_cnode_cap,
            root_cnode_copy_slot: sel4::seL4_CapNull,
            next_slot: first_free,
        };
        ctx.log_boot_window();
        ctx
    }

    fn log_boot_window(&self) {
        let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
        let _ = write!(
            &mut line,
            "[boot] init_cnode_bits={bits} empty=[0x{lo:04x}..0x{hi:04x})",
            bits = self.init_cnode_bits,
            lo = self.first_free,
            hi = self.last_free,
        );
        for byte in line.as_bytes() {
            sel4::debug_put_char(*byte as i32);
        }
        sel4::debug_put_char(b'\n' as i32);
    }

    #[inline(always)]
    fn slot_in_bounds(&self, slot: sel4::seL4_CPtr) -> bool {
        slot >= self.first_free && slot < self.last_free
    }

    pub fn alloc_slot_checked(&mut self) -> Result<sel4::seL4_CPtr, SlotAllocError> {
        let mut slot = self.next_slot;
        while slot < self.last_free && Self::is_reserved_slot(slot) {
            slot += 1;
        }

        if !self.slot_in_bounds(slot) {
            return Err(SlotAllocError::OutOfBootWindow {
                candidate: slot,
                start: self.first_free,
                end: self.last_free,
            });
        }

        if Self::is_reserved_slot(slot) {
            return Err(SlotAllocError::ReservedSlot { slot });
        }

        self.next_slot = slot.saturating_add(1);
        Ok(slot)
    }

    pub fn alloc_slot(&mut self) -> sel4::seL4_CPtr {
        match self.alloc_slot_checked() {
            Ok(slot) => slot,
            Err(err) => {
                self.log_slot_failure(err);
                panic!("boot CSpace exhausted while allocating slot: {:?}", err);
            }
        }
    }

    #[inline(always)]
    pub fn empty_bounds(&self) -> (sel4::seL4_CPtr, sel4::seL4_CPtr) {
        (self.first_free, self.last_free)
    }

    #[inline(always)]
    pub fn log_slot_failure(&self, err: SlotAllocError) {
        log_slot_allocation_failure(err, self.first_free, self.last_free);
    }

    pub fn mint_root_cnode_copy(&mut self) -> Result<(), sel4::seL4_Error> {
        let slot = match self.alloc_slot_checked() {
            Ok(slot) => slot,
            Err(err) => {
                self.log_slot_failure(err);
                return Err(sel4_sys::seL4_RangeError);
            }
        };
        let rights = seL4_CapRights_new(1, 1, 1);
        let src_slot = sel4::seL4_CapInitThreadCNode;
        let err = cspace_sys::cnode_mint_invocation(
            self.root_cnode_cap,
            slot,
            self.root_cnode_cap,
            src_slot,
            rights,
            0,
        );
        if err != sel4::seL4_NoError {
            log_cnode_mint_failure(
                err,
                slot,
                0,
                slot,
                self.root_cnode_cap,
                src_slot,
                0,
                rights,
                0,
            );
            return Err(err);
        }
        self.root_cnode_copy_slot = slot;
        Ok(())
    }
    pub fn retype_to_slot(
        &self,
        untyped: sel4::seL4_CPtr,
        obj_ty: sel4::seL4_Word,
        size_bits: sel4::seL4_Word,
        dst_slot: sel4::seL4_CPtr,
    ) -> sel4::seL4_Error {
        cspace_sys::untyped_retype_invocation(
            untyped,
            obj_ty,
            size_bits,
            self.root_cnode_cap,
            dst_slot,
        )
    }

    #[inline(always)]
    pub fn next_candidate_slot(&self) -> sel4::seL4_CPtr {
        self.next_slot
    }

    #[inline(always)]
    pub fn remaining_capacity(&self) -> sel4::seL4_CPtr {
        self.last_free.saturating_sub(self.next_slot)
    }

    /// Returns `true` when the provided slot index references a kernel-reserved capability.
    #[inline(always)]
    pub fn is_reserved_slot(slot: sel4::seL4_CPtr) -> bool {
        matches!(
            slot,
            sel4::seL4_CapNull
                | sel4::seL4_CapInitThreadTCB
                | sel4::seL4_CapInitThreadCNode
                | sel4::seL4_CapInitThreadVSpace
                | sel4::seL4_CapIRQControl
                | sel4::seL4_CapASIDControl
                | sel4::seL4_CapInitThreadASIDPool
                | sel4::seL4_CapIOPortControl
                | sel4::seL4_CapIOSpace
                | sel4::seL4_CapBootInfoFrame
                | sel4::seL4_CapInitThreadIPCBuffer
        )
    }
}

fn log_cnode_mint_failure(
    err: sel4::seL4_Error,
    dest_index: sel4::seL4_CPtr,
    dest_depth: u8,
    dest_offset: sel4::seL4_CPtr,
    _src_root: sel4::seL4_CNode,
    src_slot: sel4::seL4_CPtr,
    src_depth: u8,
    rights: sel4::seL4_CapRights,
    badge: sel4::seL4_Word,
) {
    let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
    let _ = write!(
        &mut line,
        "[cnode] op=Mint err={code} dest_index={dest_index} dest_depth={dest_depth} dest_offset=0x{dest_offset:04x} src_index=0x{src_slot:04x} src_depth={src_depth} rights=0x{rights:08x} badge=0x{badge:08x}",
        code = err,
        dest_index = dest_index,
        dest_depth = usize::from(dest_depth),
        dest_offset = dest_offset,
        src_slot = src_slot,
        src_depth = usize::from(src_depth),
        rights = rights.raw(),
        badge = badge,
    );
    for byte in line.as_bytes() {
        sel4::debug_put_char(*byte as i32);
    }
    sel4::debug_put_char(b'\n' as i32);
}

fn log_slot_allocation_failure(
    err: SlotAllocError,
    empty_start: sel4::seL4_CPtr,
    empty_end: sel4::seL4_CPtr,
) {
    let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
    match err {
        SlotAllocError::OutOfBootWindow {
            candidate,
            start,
            end,
        } => {
            let _ = write!(
                &mut line,
                "[cnode] op=SlotAlloc err=out_of_boot_window candidate=0x{candidate:04x} declared.empty=[0x{start:04x}..0x{end:04x}) runtime.empty=[0x{lo:04x}..0x{hi:04x})",
                candidate = candidate,
                start = start,
                end = end,
                lo = empty_start,
                hi = empty_end,
            );
        }
        SlotAllocError::ReservedSlot { slot } => {
            let _ = write!(
                &mut line,
                "[cnode] op=SlotAlloc err=reserved_slot slot=0x{slot:04x} runtime.empty=[0x{lo:04x}..0x{hi:04x})",
                slot = slot,
                lo = empty_start,
                hi = empty_end,
            );
        }
    }

    for byte in line.as_bytes() {
        sel4::debug_put_char(*byte as i32);
    }
    sel4::debug_put_char(b'\n' as i32);
}
