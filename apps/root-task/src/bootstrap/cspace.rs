// Author: Lukas Bower

use crate::cspace::{cap_rights_read_write_grant, CSpace};
use crate::sel4::{self, is_boot_reserved_slot, BootInfo, BootInfoExt};
use core::fmt::Write;
use heapless::String;

use super::cspace_sys::{self, CANONICAL_CNODE_DEPTH_BITS};

const MAX_DIAGNOSTIC_LEN: usize = 224;
fn log_boot(beg: sel4::seL4_CPtr, end: sel4::seL4_CPtr, bits: u8) {
    let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
    let _ = write!(
        &mut line,
        "[boot] empty=[{:#x}..{:#x}) cnode_bits={}",
        beg, end, bits
    );
    emit_console_line(line.as_str());
}

/// Lightweight projection of [`seL4_BootInfo`] exposing capability-space fields.
#[derive(Copy, Clone)]
pub struct BootInfoView {
    bootinfo: &'static BootInfo,
}

impl BootInfoView {
    #[inline(always)]
    /// Captures the kernel-provided boot info pointer for later capability-space queries.
    pub fn new(bootinfo: &'static BootInfo) -> Self {
        Self { bootinfo }
    }

    #[inline(always)]
    /// Returns the raw bootinfo pointer backing this view.
    pub fn bootinfo(&self) -> &'static BootInfo {
        self.bootinfo
    }

    #[inline(always)]
    /// Returns the radix width of the initial thread's CNode as declared by the kernel.
    pub fn init_cnode_bits(&self) -> u8 {
        self.bootinfo.initThreadCNodeSizeBits as u8
    }

    #[inline(always)]
    /// Returns the radix width of the initial thread's CNode expressed as `usize`.
    pub fn init_cnode_size_bits(&self) -> usize {
        self.bootinfo.initThreadCNodeSizeBits as usize
    }

    #[inline(always)]
    /// Reports the inclusive-exclusive range of free slots available in the initial CNode.
    pub fn init_cnode_empty_range(&self) -> (sel4::seL4_CPtr, sel4::seL4_CPtr) {
        (
            self.bootinfo.empty.start as sel4::seL4_CPtr,
            self.bootinfo.empty.end as sel4::seL4_CPtr,
        )
    }

    #[inline(always)]
    /// Returns the capability pointer referencing the initial thread's root CNode.
    pub fn root_cnode_cap(&self) -> sel4::seL4_CPtr {
        self.bootinfo.init_cnode_cap()
    }
}

impl From<&'static BootInfo> for BootInfoView {
    fn from(value: &'static BootInfo) -> Self {
        Self::new(value)
    }
}

/// Canonical CSpace context orchestrating bootstrap-time seL4 CNode operations.
pub struct CSpaceCtx {
    /// Cached boot info projection used for runtime diagnostics.
    pub bi: BootInfoView,
    /// Capability-space allocator driving init CNode operations.
    pub cspace: CSpace,
    /// Canonical guard depth supplied to CNode invocations targeting the init CNode.
    pub cnode_invocation_depth_bits: u8,
    /// Radix width of the init thread's CNode as reported by bootinfo.
    pub init_cnode_bits: u8,
    /// First slot index in the init CNode guaranteed to be available for the root task.
    pub first_free: sel4::seL4_CPtr,
    /// End of the init CNode free window supplied by the kernel boot info.
    pub last_free: sel4::seL4_CPtr,
    /// Capability pointer for the init CNode itself.
    pub root_cnode_cap: sel4::seL4_CPtr,
    /// Slot index containing a copied init TCB capability once `smoke_copy_init_tcb` succeeds.
    pub tcb_copy_slot: sel4::seL4_CPtr,
    /// Slot index containing a copied root CNode capability when minted.
    pub root_cnode_copy_slot: sel4::seL4_CPtr,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
/// Enumerates failure cases encountered while allocating CNode slots during bootstrap.
pub enum SlotAllocError {
    /// The requested slot lies outside the kernel-advertised free window.
    OutOfBootWindow {
        /// Slot index that triggered the failure.
        candidate: sel4::seL4_CPtr,
        /// Boot info lower bound of the free slot window.
        start: sel4::seL4_CPtr,
        /// Boot info upper bound of the free slot window.
        end: sel4::seL4_CPtr,
    },
    /// The chosen slot overlaps a kernel-reserved capability.
    ReservedSlot {
        /// Slot index owned by the kernel.
        slot: sel4::seL4_CPtr,
    },
}

impl CSpaceCtx {
    /// Constructs a new capability-space context from kernel boot information.
    pub fn new(bi: BootInfoView, cspace: CSpace) -> Self {
        let init_cnode_bits = cspace.depth();
        assert!(
            init_cnode_bits > 0,
            "bootinfo reported zero-width init CNode"
        );
        let invocation_depth_bits = init_cnode_bits;
        let (first_free, last_free) = bi.init_cnode_empty_range();
        debug_assert!(
            init_cnode_bits <= CANONICAL_CNODE_DEPTH_BITS,
            "bootinfo-reported radix exceeds canonical invocation depth",
        );
        let limit = 1usize << bi.init_cnode_size_bits();
        assert!(
            first_free < limit,
            "bootinfo.empty.start exceeds init CNode size"
        );
        assert!(
            last_free <= limit,
            "bootinfo.empty.end exceeds init CNode size"
        );
        let root_cnode_cap = cspace.root();
        let ctx = Self {
            bi,
            cspace,
            cnode_invocation_depth_bits: invocation_depth_bits,
            init_cnode_bits,
            first_free,
            last_free,
            root_cnode_cap,
            tcb_copy_slot: sel4::seL4_CapNull,
            root_cnode_copy_slot: sel4::seL4_CapNull,
        };
        log_boot(first_free, last_free, init_cnode_bits);
        ctx
    }

    #[inline(always)]
    /// Returns the depth (in bits) of the init thread's root CNode.
    pub fn cnode_bits(&self) -> u8 {
        self.init_cnode_bits
    }

    #[inline(always)]
    fn slot_in_bounds(&self, slot: sel4::seL4_CPtr) -> bool {
        slot >= self.first_free && slot < self.last_free
    }

    #[inline(always)]
    fn assert_slot_available(&self, slot: sel4::seL4_CPtr) {
        cspace_sys::check_slot_in_range(self.init_cnode_bits, slot);
        assert!(
            self.slot_in_bounds(slot),
            "slot 0x{slot:04x} outside boot empty window [0x{lo:04x}..0x{hi:04x})",
            slot = slot,
            lo = self.first_free,
            hi = self.last_free,
        );
        assert!(
            !Self::is_reserved_slot(slot),
            "slot 0x{slot:04x} collides with kernel reserved capability",
            slot = slot,
        );
    }

    #[inline(always)]
    /// Attempts to reserve the next available slot while enforcing boot window and reservation checks.
    pub fn alloc_slot_checked(&mut self) -> Result<sel4::seL4_CPtr, SlotAllocError> {
        loop {
            let candidate = self.cspace.next_free_slot();
            cspace_sys::check_slot_in_range(self.init_cnode_bits, candidate);
            if !self.slot_in_bounds(candidate) {
                return Err(SlotAllocError::OutOfBootWindow {
                    candidate,
                    start: self.first_free,
                    end: self.last_free,
                });
            }

            let slot = match self.cspace.alloc_slot() {
                Ok(slot) => slot,
                Err(_) => {
                    return Err(SlotAllocError::OutOfBootWindow {
                        candidate,
                        start: self.first_free,
                        end: self.last_free,
                    });
                }
            };

            if Self::is_reserved_slot(slot) {
                continue;
            }

            self.assert_slot_available(slot);
            return Ok(slot);
        }
    }

    /// Panicking convenience wrapper around [`Self::alloc_slot_checked`].
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
    /// Returns the `[start, end)` bounds of the boot-time free slot window.
    pub fn empty_bounds(&self) -> (sel4::seL4_CPtr, sel4::seL4_CPtr) {
        (self.first_free, self.last_free)
    }

    #[inline(always)]
    /// Emits a structured diagnostic covering the supplied slot allocation failure.
    pub fn log_slot_failure(&self, err: SlotAllocError) {
        log_slot_allocation_failure(err, self.first_free, self.last_free);
    }

    /// Logs the outcome of an init CNode copy invocation.
    pub fn log_cnode_copy(
        &self,
        err: sel4::seL4_Error,
        dest_index: sel4::seL4_CPtr,
        src_index: sel4::seL4_CPtr,
    ) {
        if err != sel4::seL4_NoError {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            let _ = write!(
                &mut line,
                "[cnode] Copy err={err} root=0x{root:04x} dest(index=0x{dest_index:04x},depth={depth}) src(index=0x{src_index:04x},depth={depth})",
                root = self.root_cnode_cap,
                depth = self.cnode_invocation_depth_bits,
            );
            emit_console_line(line.as_str());
        }
    }

    /// Logs the outcome of an init CNode mint invocation.
    pub fn log_cnode_mint(
        &self,
        err: sel4::seL4_Error,
        dest_index: sel4::seL4_CPtr,
        src_index: sel4::seL4_CPtr,
        badge: sel4::seL4_Word,
    ) {
        if err != sel4::seL4_NoError {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            let _ = write!(
                &mut line,
                "[cnode] Mint err={err} root=0x{root:04x} dest(index=0x{dest_index:04x},depth={depth},offset=0) src(index=0x{src_index:04x},depth={depth}) badge={badge}",
                root = self.root_cnode_cap,
                depth = self.cnode_invocation_depth_bits,
            );
            emit_console_line(line.as_str());
        }
    }

    /// Logs the outcome of an init CNode untyped retype invocation.
    pub fn log_retype(
        &self,
        err: sel4::seL4_Error,
        untyped: sel4::seL4_CPtr,
        obj_ty: sel4::seL4_Word,
        size_bits: sel4::seL4_Word,
        dest_index: sel4::seL4_CPtr,
    ) {
        if err != sel4::seL4_NoError {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            let _ = write!(
                &mut line,
                "[retype] err={err} root=0x{root:04x} untyped_slot=0x{untyped:04x} dest(index=0x{dest_index:04x},depth={depth},offset=0) ty={obj_ty} sz={size_bits}",
                root = self.root_cnode_cap,
                depth = self.cnode_invocation_depth_bits,
            );
            emit_console_line(line.as_str());
        }
    }

    /// Copies the init thread TCB capability into the free slot window to validate CSpace operations.
    pub fn smoke_copy_init_tcb(&mut self) -> Result<(), sel4::seL4_Error> {
        let dst_slot = match self.alloc_slot_checked() {
            Ok(slot) => slot,
            Err(err) => {
                self.log_slot_failure(err);
                return Err(sel4::seL4_RangeError);
            }
        };
        let src_slot = sel4::seL4_CapInitThreadTCB;
        let err = self.copy_init_tcb_from(dst_slot, src_slot);
        if err == sel4::seL4_NoError {
            self.tcb_copy_slot = dst_slot;
            Ok(())
        } else {
            Err(err)
        }
    }

    fn copy_init_tcb_from(
        &mut self,
        dst_slot: sel4::seL4_CPtr,
        src_slot: sel4::seL4_CPtr,
    ) -> sel4::seL4_Error {
        #[cfg(all(feature = "kernel", sel4_config_debug_build))]
        {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            let _ = write!(
                &mut line,
                "[cnode] Copy attempt dest=0x{dst_slot:04x} src=0x{src_slot:04x}",
            );
            emit_console_line(line.as_str());
        }
        let rights = cap_rights_read_write_grant();
        let err = self.cspace.copy_here(dst_slot, src_slot, rights);
        self.log_cnode_copy(err, dst_slot, src_slot);
        err
    }

    /// Mints a duplicate of the root CNode capability for later management operations.
    pub fn mint_root_cnode_copy(&mut self) -> Result<(), sel4::seL4_Error> {
        let dst_slot = match self.alloc_slot_checked() {
            Ok(slot) => slot,
            Err(err) => {
                self.log_slot_failure(err);
                return Err(sel4::seL4_RangeError);
            }
        };
        let src_slot = sel4::seL4_CapInitThreadCNode;
        let rights = cap_rights_read_write_grant();
        let err = self.cspace.mint_here(dst_slot, src_slot, rights, 0);
        self.log_cnode_mint(err, dst_slot, src_slot, 0);
        if err == sel4::seL4_NoError {
            self.root_cnode_copy_slot = dst_slot;
            Ok(())
        } else {
            Err(err)
        }
    }

    /// Retypes the provided untyped capability into the destination slot.
    pub fn retype_to_slot(
        &mut self,
        untyped: sel4::seL4_CPtr,
        obj_ty: sel4::seL4_Word,
        size_bits: sel4::seL4_Word,
        dst_slot: sel4::seL4_CPtr,
    ) -> sel4::seL4_Error {
        self.assert_slot_available(dst_slot);
        let err = cspace_sys::untyped_retype_invoc(
            self.root_cnode_cap,
            self.cnode_invocation_depth_bits,
            untyped,
            obj_ty,
            size_bits,
            dst_slot,
        );
        self.log_retype(err, untyped, obj_ty, size_bits, dst_slot);
        err
    }

    #[inline(always)]
    /// Returns the slot index the allocator will consider next.
    pub fn next_candidate_slot(&self) -> sel4::seL4_CPtr {
        self.cspace.next_free_slot()
    }

    #[inline(always)]
    /// Reports the remaining number of slots available before exhausting the boot window.
    pub fn remaining_capacity(&self) -> sel4::seL4_CPtr {
        self.last_free.saturating_sub(self.cspace.next_free_slot())
    }

    /// Returns `true` when the provided slot index references a kernel-reserved capability.
    #[inline(always)]
    pub fn is_reserved_slot(slot: sel4::seL4_CPtr) -> bool {
        is_boot_reserved_slot(slot)
    }
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

    emit_console_line(line.as_str());
}

fn emit_console_line(line: &str) {
    log::info!("{}", line);
    for byte in line.as_bytes() {
        sel4::debug_put_char(*byte as i32);
    }
    sel4::debug_put_char(b'\n' as i32);
}
