// Author: Lukas Bower

use crate::sel4::{self, BootInfo};
use core::fmt::Write;
use heapless::String;
use sel4_sys as sys;

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
    pub fn empty_start(&self) -> sys::seL4_CPtr {
        self.bootinfo.empty.start as sys::seL4_CPtr
    }

    #[inline(always)]
    pub fn empty_end(&self) -> sys::seL4_CPtr {
        self.bootinfo.empty.end as sys::seL4_CPtr
    }

    #[inline(always)]
    pub fn root_cnode_cap(&self) -> sys::seL4_CPtr {
        sys::seL4_CapInitThreadCNode
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
    pub first_free: sys::seL4_CPtr,
    pub last_free: sys::seL4_CPtr,
    pub root_cnode_cap: sys::seL4_CPtr,
    pub root_cnode_copy_slot: sys::seL4_CPtr,
    next_slot: sys::seL4_CPtr,
}

impl CSpaceCtx {
    pub fn new(bi: BootInfoView) -> Self {
        let init_cnode_bits = bi.init_cnode_bits();
        let first_free = bi.empty_start();
        let last_free = bi.empty_end();
        let root_cnode_cap = bi.root_cnode_cap();
        Self {
            bi,
            init_cnode_bits,
            first_free,
            last_free,
            root_cnode_cap,
            root_cnode_copy_slot: sys::seL4_CapNull,
            next_slot: first_free,
        }
    }

    #[inline(always)]
    fn slot_in_bounds(&self, slot: sys::seL4_CPtr) -> bool {
        slot >= self.first_free && slot < self.last_free
    }

    #[inline(always)]
    fn assert_slot_available(&self, slot: sys::seL4_CPtr) {
        assert!(
            self.slot_in_bounds(slot),
            "allocated slot 0x{slot:04x} outside bootinfo.empty range [0x{lo:04x}..0x{hi:04x})",
            slot = slot,
            lo = self.first_free,
            hi = self.last_free,
        );
        assert!(
            !Self::is_reserved_slot(slot),
            "attempted to allocate reserved capability slot 0x{slot:04x}",
            slot = slot,
        );
    }

    pub fn alloc_slot(&mut self) -> sys::seL4_CPtr {
        let mut slot = self.next_slot;
        while slot < self.last_free && Self::is_reserved_slot(slot) {
            slot += 1;
        }
        self.assert_slot_available(slot);
        self.next_slot = slot.saturating_add(1);
        slot
    }

    #[inline(always)]
    pub fn empty_bounds(&self) -> (sys::seL4_CPtr, sys::seL4_CPtr) {
        (self.first_free, self.last_free)
    }

    pub fn mint_root_copy(&mut self) -> Result<(), sys::seL4_Error> {
        let slot = self.alloc_slot();
        let depth = self.init_cnode_bits;
        let err = sel4::cnode_mint(
            self.root_cnode_cap,
            slot,
            depth,
            self.root_cnode_cap,
            self.root_cnode_cap,
            depth,
            sys::seL4_CapRights_All,
            0,
        );
        if err != sys::seL4_NoError {
            log_cnode_mint_failure(
                err,
                slot,
                depth,
                self.root_cnode_cap,
                depth,
                sys::seL4_CapRights_All,
                0,
            );
            return Err(err);
        }
        self.root_cnode_copy_slot = slot;
        Ok(())
    }

    #[inline(always)]
    pub fn next_candidate_slot(&self) -> sys::seL4_CPtr {
        self.next_slot
    }

    #[inline(always)]
    pub fn remaining_capacity(&self) -> sys::seL4_CPtr {
        self.last_free.saturating_sub(self.next_slot)
    }

    /// Returns `true` when the provided slot index references a kernel-reserved capability.
    #[inline(always)]
    pub fn is_reserved_slot(slot: sys::seL4_CPtr) -> bool {
        matches!(
            slot,
            sys::seL4_CapNull
                | sys::seL4_CapInitThreadTCB
                | sys::seL4_CapInitThreadCNode
                | sys::seL4_CapInitThreadVSpace
                | sys::seL4_CapIRQControl
                | sys::seL4_CapASIDControl
                | sys::seL4_CapInitThreadASIDPool
                | sys::seL4_CapIOPortControl
                | sys::seL4_CapIOSpace
                | sys::seL4_CapBootInfoFrame
                | sys::seL4_CapInitThreadIPCBuffer
        )
    }
}

fn log_cnode_mint_failure(
    err: sys::seL4_Error,
    dest_slot: sys::seL4_CPtr,
    dest_depth: u8,
    src_slot: sys::seL4_CPtr,
    src_depth: u8,
    rights: sys::seL4_CapRights,
    badge: sys::seL4_Word,
) {
    let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
    let _ = write!(
        &mut line,
        "CNode_Mint err={code} dest_index=0x{dest_slot:04x} dest_depth={dest_depth} src_index=0x{src_slot:04x} src_depth={src_depth} rights=0x{rights:08x} badge=0x{badge:08x}",
        code = err,
        dest_slot = dest_slot,
        dest_depth = usize::from(dest_depth),
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
